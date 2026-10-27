# 12 — SQLite Schema, Index Generation & Data Retention

## Source of truth
One per-user SQLite database in OS Application Support directory; versioned migrations with PRAGMA foreign_keys=ON and WAL. `database/001_initial.sql` is a **proposed migration** validated for SQL syntax, not final app code. Schema review must include benchmarks and privacy.

## Conceptual tables
- `app_settings(key, value_json)` — non-secret preferences.
- `model_registry` — model registration, category, adapter/path, validation, status snapshot (runtime live status is in process, persisted last-known status must not be shown green after restart).
- `source_scopes` — grant paths and state, filters, revocation, last scan.
- `documents` — dedup source identity, canonical path, file metadata, content hash, parse status.
- `document_scopes` — independent document↔grant association.
- `chunks` — text/OCR, file location, embedding/index generation, stale flag.
- `chunk_fts` — FTS5 searchable derived text.
- `embedding_records` — vector storage metadata with generation, dimension, model fingerprint and BLOB. For larger scale, use versioned sqlite-vec or approved sidecar index; never mix spaces.
- `sessions`, `turns`, `search_events`, `result_interactions`, `memory_notes` — conversational and episodic memory, referring to file IDs not embedding pointers.
- `index_jobs`, `purge_jobs`, `audit_events` — recovery, resume and privacy operations.

## Index generation identity
Fingerprint MUST cover: `embedding_model_hash/revision`, `adapter_version`, `preprocessing_pipeline`, `query/document instructions`, `dimension`, `normalization`, `modality`. All vectors carry this ID. A model swap/LoRA update builds a **new generation** in parallel/staged and switches atomically after validation. Old generation is purged only when new generation healthy and storage policy allows.

## Storage boundaries
- Content index only for authorized scopes; scoped ACL applied in `SELECT` and again before open. Cached result objects invalidated on scope revocation.
- User can choose to keep history; if disabled, no persistent raw turns. Raw audio not persisted by default.
- Local migrations backup first, transactional when possible; interrupted destructive schema migration must offer recoverable state. No silent DB reset.
- Old model status snapshots are not runtime health truths; after reboot `READY` becomes `AVAILABLE` pending runner startup health check.

## Purge
Immediately set scope revoked; detach document grants; delete exclusive chunks/embeddings/FTS, snippet-based notes, thumbnails; redact prior event details. `purge_jobs` assures restart continuation. Database may retain de-identified audit record. Issue WAL checkpoint/vacuum per policy; backups age out. Define `History OFF` data-minimization separately.

## Proposed SQL schema
See [`../database/001_initial.sql`](database/001_initial.sql). Vector rows store BLOB for schema bootstrapping; indexed ANN choice is deferred until performance spike. Never claim plain BLOB supports efficient similarity search without vector/index adapter.
