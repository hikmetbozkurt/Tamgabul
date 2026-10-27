-- 001_initial.sql — PROPOSED migration (docs/12_DATA_SCHEMA.md)
-- Status: draft 0.1. Syntax-smoke-tested only; not final app code.
-- Vector rows are plain BLOBs for bootstrapping. ANN index choice (sqlite-vec or
-- sidecar) is deferred to spike V03. A BLOB column alone is NOT efficient search.

PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;

CREATE TABLE schema_migrations (
    version     INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    applied_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

-- Non-secret preferences only.
CREATE TABLE app_settings (
    key         TEXT PRIMARY KEY,
    value_json  TEXT NOT NULL CHECK (json_valid(value_json)),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

-- ---------------------------------------------------------------------------
-- Model registry (docs/04). Persisted status is a last-known SNAPSHOT.
-- After restart READY/IDLE/BUSY/LOADING must be shown as AVAILABLE until the
-- runner health check passes (docs/12 Storage boundaries).
-- ---------------------------------------------------------------------------
CREATE TABLE model_registry (
    model_id                  TEXT PRIMARY KEY,             -- UUID
    category                  TEXT NOT NULL CHECK (category IN ('embedding','llm','stt','tts')),
    display_name              TEXT NOT NULL,
    provider                  TEXT,
    path                      TEXT NOT NULL,                -- user-selected, never hardcoded
    format                    TEXT NOT NULL,                -- gguf, safetensors, ...
    architecture              TEXT,
    weights_sha256            TEXT,
    model_revision            TEXT,
    license_name              TEXT,
    license_url               TEXT,
    runtime_adapter           TEXT NOT NULL,
    adapter_version           TEXT NOT NULL,
    capabilities_json         TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(capabilities_json)),
    language_tags_json        TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(language_tags_json)),
    embedding_dim             INTEGER CHECK (embedding_dim IS NULL OR embedding_dim IN (128,256,512,768)),
    preprocessing_fingerprint TEXT,
    is_active                 INTEGER NOT NULL DEFAULT 0 CHECK (is_active IN (0,1)),
    status                    TEXT NOT NULL DEFAULT 'REGISTERED' CHECK (status IN
        ('MISSING','REGISTERED','VALIDATING','AVAILABLE','LOADING','READY','BUSY',
         'IDLE','UNLOADING','ERROR','INCOMPATIBLE')),
    last_error_code           TEXT,                         -- error taxonomy, docs/13
    last_error_message        TEXT,                         -- no model input content
    validated_at              TEXT,
    created_at                TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at                TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
-- At most one active model per category.
CREATE UNIQUE INDEX ux_model_active_per_category
    ON model_registry(category) WHERE is_active = 1;

-- Per-model lifecycle policy (docs/05). ttl_seconds NULL = keep loaded (opt-in).
CREATE TABLE model_policies (
    model_id          TEXT PRIMARY KEY REFERENCES model_registry(model_id) ON DELETE CASCADE,
    ttl_mode          TEXT NOT NULL DEFAULT 'ttl' CHECK (ttl_mode IN ('ttl','keep_loaded','unload_after_job')),
    ttl_seconds       INTEGER CHECK (ttl_seconds IS NULL OR ttl_seconds >= 0),
    prewarm_on_open   INTEGER NOT NULL DEFAULT 0 CHECK (prewarm_on_open IN (0,1)),
    updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

-- ---------------------------------------------------------------------------
-- Index generation identity (docs/12). Vectors are never mixed across generations.
-- ---------------------------------------------------------------------------
CREATE TABLE index_generations (
    generation_id             TEXT PRIMARY KEY,
    embedding_model_id        TEXT REFERENCES model_registry(model_id) ON DELETE SET NULL,
    embedding_model_hash      TEXT NOT NULL,
    model_revision            TEXT NOT NULL,
    adapter_version           TEXT NOT NULL,
    preprocessing_pipeline    TEXT NOT NULL,
    query_instruction         TEXT NOT NULL,
    document_instruction      TEXT NOT NULL,
    dimension                 INTEGER NOT NULL CHECK (dimension IN (128,256,512,768)),
    normalization             TEXT NOT NULL,
    modalities_json           TEXT NOT NULL CHECK (json_valid(modalities_json)),
    fingerprint               TEXT NOT NULL UNIQUE,         -- hash of all fields above
    state                     TEXT NOT NULL DEFAULT 'BUILDING' CHECK (state IN
        ('BUILDING','VALIDATING','CURRENT','RETIRED','PURGING')),
    created_at                TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    activated_at              TEXT
);
CREATE UNIQUE INDEX ux_generation_current
    ON index_generations(state) WHERE state = 'CURRENT';

-- ---------------------------------------------------------------------------
-- Source scopes / grants (docs/07)
-- ---------------------------------------------------------------------------
CREATE TABLE source_scopes (
    scope_id            TEXT PRIMARY KEY,                    -- immutable
    canonical_path      TEXT NOT NULL,
    display_path        TEXT NOT NULL,
    volume_id           TEXT,
    is_drive_root       INTEGER NOT NULL DEFAULT 0 CHECK (is_drive_root IN (0,1)),
    include_json        TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(include_json)),
    exclude_json        TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(exclude_json)),
    max_file_bytes      INTEGER,
    scan_mode           TEXT NOT NULL DEFAULT 'manual' CHECK (scan_mode IN ('manual','idle_sync')),
    state               TEXT NOT NULL DEFAULT 'PENDING_PREVIEW' CHECK (state IN
        ('PENDING_PREVIEW','AWAIT_CONFIRM','ACTIVE','PAUSED','REVOKING','REVOKED','ERROR')),
    last_generation_id  TEXT REFERENCES index_generations(generation_id) ON DELETE SET NULL,
    file_count          INTEGER NOT NULL DEFAULT 0,
    error_summary       TEXT,
    last_scan_at        TEXT,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    revoked_at          TEXT
);
CREATE INDEX ix_scopes_state ON source_scopes(state);

-- Persistent deny rules, e.g. child revoked under an active parent (docs/07 Root overlap).
CREATE TABLE scope_exclusions (
    exclusion_id     TEXT PRIMARY KEY,
    parent_scope_id  TEXT REFERENCES source_scopes(scope_id) ON DELETE CASCADE,  -- NULL = global
    canonical_path   TEXT NOT NULL,
    reason           TEXT NOT NULL CHECK (reason IN ('child_revoked','user_exclusion','protected_path')),
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX ix_exclusions_path ON scope_exclusions(canonical_path);

-- ---------------------------------------------------------------------------
-- Documents and chunks
-- ---------------------------------------------------------------------------
CREATE TABLE documents (
    document_id      TEXT PRIMARY KEY,
    canonical_path   TEXT NOT NULL UNIQUE,
    platform_file_id TEXT,
    file_type        TEXT NOT NULL,
    size_bytes       INTEGER NOT NULL,
    mtime            TEXT NOT NULL,
    content_hash     TEXT,                                   -- BLAKE3; never authorizes access
    parse_status     TEXT NOT NULL DEFAULT 'PENDING' CHECK (parse_status IN
        ('PENDING','PARSED','SKIPPED','FAILED','STALE')),
    parse_error      TEXT,
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX ix_documents_hash ON documents(content_hash);

-- Independent document <-> grant association.
CREATE TABLE document_scopes (
    document_id  TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
    scope_id     TEXT NOT NULL REFERENCES source_scopes(scope_id) ON DELETE CASCADE,
    PRIMARY KEY (document_id, scope_id)
);
CREATE INDEX ix_document_scopes_scope ON document_scopes(scope_id);

CREATE TABLE chunks (
    chunk_id       INTEGER PRIMARY KEY,                      -- rowid, used by FTS5
    document_id    TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
    generation_id  TEXT NOT NULL REFERENCES index_generations(generation_id) ON DELETE CASCADE,
    ordinal        INTEGER NOT NULL,
    modality       TEXT NOT NULL CHECK (modality IN ('text','ocr','image')),
    text           TEXT,                                     -- NULL for pure image chunk
    page           INTEGER,
    line_start     INTEGER,
    line_end       INTEGER,
    bbox_json      TEXT CHECK (bbox_json IS NULL OR json_valid(bbox_json)),
    is_stale       INTEGER NOT NULL DEFAULT 0 CHECK (is_stale IN (0,1)),
    UNIQUE (document_id, generation_id, ordinal)
);
CREATE INDEX ix_chunks_generation ON chunks(generation_id);

-- FTS5 over derived text (external content). Trigram tokenizer helps TR/EN
-- mixed and partial identifiers; final tokenizer choice pending spike V03.
CREATE VIRTUAL TABLE chunk_fts USING fts5(
    text,
    content='chunks',
    content_rowid='chunk_id',
    tokenize='trigram'
);
CREATE TRIGGER chunks_ai AFTER INSERT ON chunks WHEN new.text IS NOT NULL BEGIN
    INSERT INTO chunk_fts(rowid, text) VALUES (new.chunk_id, new.text);
END;
CREATE TRIGGER chunks_ad AFTER DELETE ON chunks WHEN old.text IS NOT NULL BEGIN
    INSERT INTO chunk_fts(chunk_fts, rowid, text) VALUES ('delete', old.chunk_id, old.text);
END;
CREATE TRIGGER chunks_au AFTER UPDATE OF text ON chunks BEGIN
    INSERT INTO chunk_fts(chunk_fts, rowid, text)
        SELECT 'delete', old.chunk_id, old.text WHERE old.text IS NOT NULL;
    INSERT INTO chunk_fts(rowid, text)
        SELECT new.chunk_id, new.text WHERE new.text IS NOT NULL;
END;

CREATE TABLE embedding_records (
    chunk_id       INTEGER PRIMARY KEY REFERENCES chunks(chunk_id) ON DELETE CASCADE,
    generation_id  TEXT NOT NULL REFERENCES index_generations(generation_id) ON DELETE CASCADE,
    dimension      INTEGER NOT NULL CHECK (dimension IN (128,256,512,768)),
    vector         BLOB NOT NULL CHECK (length(vector) = dimension * 4)  -- float32 LE
);
CREATE INDEX ix_embeddings_generation ON embedding_records(generation_id);

-- ---------------------------------------------------------------------------
-- Conversation & episodic memory (docs/06). References file IDs, never vectors.
-- ---------------------------------------------------------------------------
CREATE TABLE sessions (
    session_id     TEXT PRIMARY KEY,
    started_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    last_active_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    active_slots_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(active_slots_json)),
    persisted      INTEGER NOT NULL DEFAULT 1 CHECK (persisted IN (0,1))  -- 0 = History OFF
);

CREATE TABLE turns (
    turn_id          TEXT PRIMARY KEY,
    session_id       TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    ordinal          INTEGER NOT NULL,
    role             TEXT NOT NULL CHECK (role IN ('user','assistant','system_note')),
    input_mode       TEXT CHECK (input_mode IN ('typed','voice')),
    text             TEXT,                                   -- raw only if history ON
    intent           TEXT CHECK (intent IS NULL OR intent IN
        ('search_current','lookup_prior_result','open_result','refine_search','list_sources',
         'model_control','cancel_action','clarify','unknown')),
    slots_json       TEXT CHECK (slots_json IS NULL OR json_valid(slots_json)),
    supersedes_turn_id TEXT REFERENCES turns(turn_id) ON DELETE SET NULL,  -- corrections as revisions
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),  -- UTC
    UNIQUE (session_id, ordinal)
);

CREATE TABLE search_events (
    event_id       TEXT PRIMARY KEY,
    session_id     TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    turn_id        TEXT REFERENCES turns(turn_id) ON DELETE SET NULL,
    event_type     TEXT NOT NULL CHECK (event_type IN
        ('asked','clarified','results_shown','file_opened','search_failed','scope_revoked')),
    query_terms    TEXT,                                     -- for episodic FTS lookup
    filters_json   TEXT CHECK (filters_json IS NULL OR json_valid(filters_json)),
    generation_id  TEXT REFERENCES index_generations(generation_id) ON DELETE SET NULL,
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX ix_search_events_time ON search_events(created_at);

CREATE VIRTUAL TABLE search_event_fts USING fts5(
    query_terms,
    content='search_events',
    content_rowid='rowid',
    tokenize='trigram'
);
CREATE TRIGGER search_events_ai AFTER INSERT ON search_events WHEN new.query_terms IS NOT NULL BEGIN
    INSERT INTO search_event_fts(rowid, query_terms) VALUES (new.rowid, new.query_terms);
END;
CREATE TRIGGER search_events_ad AFTER DELETE ON search_events WHEN old.query_terms IS NOT NULL BEGIN
    INSERT INTO search_event_fts(search_event_fts, rowid, query_terms)
        VALUES ('delete', old.rowid, old.query_terms);
END;
CREATE TRIGGER search_events_au AFTER UPDATE OF query_terms ON search_events BEGIN
    INSERT INTO search_event_fts(search_event_fts, rowid, query_terms)
        SELECT 'delete', old.rowid, old.query_terms WHERE old.query_terms IS NOT NULL;
    INSERT INTO search_event_fts(rowid, query_terms)
        SELECT new.rowid, new.query_terms WHERE new.query_terms IS NOT NULL;
END;

-- Result rows reference document_id; after revoke, document_id is NULLed and
-- the row becomes a redacted tombstone (never opens).
CREATE TABLE result_interactions (
    interaction_id  TEXT PRIMARY KEY,
    event_id        TEXT NOT NULL REFERENCES search_events(event_id) ON DELETE CASCADE,
    document_id     TEXT REFERENCES documents(document_id) ON DELETE SET NULL,
    chunk_id        INTEGER REFERENCES chunks(chunk_id) ON DELETE SET NULL,
    scope_id        TEXT REFERENCES source_scopes(scope_id) ON DELETE SET NULL,
    rank            INTEGER,
    score           REAL,                                    -- ranking statistic, not probability
    action          TEXT NOT NULL CHECK (action IN ('shown','opened','previewed','rejected')),
    title_snapshot  TEXT,                                    -- redacted on revoke
    is_tombstone    INTEGER NOT NULL DEFAULT 0 CHECK (is_tombstone IN (0,1)),
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX ix_interactions_document ON result_interactions(document_id);

CREATE TABLE memory_notes (
    note_id              TEXT PRIMARY KEY,
    summary              TEXT NOT NULL,
    tags_json            TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(tags_json)),
    source_turn_ids_json TEXT NOT NULL CHECK (json_valid(source_turn_ids_json)),  -- mandatory provenance
    source_document_ids_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(source_document_ids_json)),
    version              INTEGER NOT NULL DEFAULT 1,
    created_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

-- ---------------------------------------------------------------------------
-- Durable jobs and audit (recovery, resume, privacy)
-- ---------------------------------------------------------------------------
CREATE TABLE index_jobs (
    job_id          TEXT PRIMARY KEY,
    scope_id        TEXT NOT NULL REFERENCES source_scopes(scope_id) ON DELETE CASCADE,
    generation_id   TEXT NOT NULL REFERENCES index_generations(generation_id) ON DELETE CASCADE,
    state           TEXT NOT NULL DEFAULT 'QUEUED' CHECK (state IN
        ('QUEUED','RUNNING','PAUSED','CANCELLED','FAILED','DONE')),
    checkpoint_json TEXT CHECK (checkpoint_json IS NULL OR json_valid(checkpoint_json)),
    processed       INTEGER NOT NULL DEFAULT 0,
    errors          INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

-- scope_id is NOT a FK on purpose: purge must survive scope row changes.
CREATE TABLE purge_jobs (
    purge_id        TEXT PRIMARY KEY,
    scope_id        TEXT NOT NULL,
    policy          TEXT NOT NULL CHECK (policy IN ('purge_exclusive','purge_all_related_turns')),
    state           TEXT NOT NULL DEFAULT 'QUEUED' CHECK (state IN
        ('QUEUED','RUNNING','VERIFYING','DONE','FAILED')),
    step            TEXT,
    verified_at     TEXT,                                    -- set only after SQL proof of zero leftovers
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

-- De-identified: no paths, snippets or query text.
CREATE TABLE audit_events (
    audit_id     INTEGER PRIMARY KEY,
    kind         TEXT NOT NULL,                              -- scope_granted, scope_revoked, model_registered, ...
    subject_id   TEXT,
    detail_json  TEXT CHECK (detail_json IS NULL OR json_valid(detail_json)),
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

INSERT INTO schema_migrations(version, name) VALUES (1, '001_initial');
