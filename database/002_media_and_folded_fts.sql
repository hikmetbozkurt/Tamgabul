-- 002: audio chunks + time ranges, accent/case-insensitive word search.
--
-- * chunks.modality gains 'audio'; audio segments carry time_start_ms/time_end_ms.
-- * chunk_fts becomes contentless and stores tg_fold(text): lower case,
--   Turkish dotless/dotted i unified, diacritics removed — so "toplanti"
--   finds "Toplantı" and "surum" finds "sürüm". tg_fold is registered by the
--   app on every connection (crates/core/src/db.rs).
-- Rebuilt table-by-copy (SQLite cannot alter a CHECK); the migration runner
-- turns foreign keys off for the copy and verifies them afterwards.

CREATE TABLE chunks_v2 (
    chunk_id       INTEGER PRIMARY KEY,
    document_id    TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
    generation_id  TEXT NOT NULL REFERENCES index_generations(generation_id) ON DELETE CASCADE,
    ordinal        INTEGER NOT NULL,
    modality       TEXT NOT NULL CHECK (modality IN ('text','ocr','image','audio')),
    text           TEXT,
    page           INTEGER,
    line_start     INTEGER,
    line_end       INTEGER,
    time_start_ms  INTEGER,
    time_end_ms    INTEGER,
    bbox_json      TEXT CHECK (bbox_json IS NULL OR json_valid(bbox_json)),
    is_stale       INTEGER NOT NULL DEFAULT 0 CHECK (is_stale IN (0,1)),
    UNIQUE (document_id, generation_id, ordinal)
);

INSERT INTO chunks_v2(chunk_id, document_id, generation_id, ordinal, modality, text, page,
                      line_start, line_end, bbox_json, is_stale)
SELECT chunk_id, document_id, generation_id, ordinal, modality, text, page,
       line_start, line_end, bbox_json, is_stale
FROM chunks;

DROP TRIGGER chunks_ai;
DROP TRIGGER chunks_ad;
DROP TRIGGER chunks_au;
DROP TABLE chunk_fts;
DROP TABLE chunks;
ALTER TABLE chunks_v2 RENAME TO chunks;
CREATE INDEX ix_chunks_generation ON chunks(generation_id);
CREATE INDEX ix_chunks_document ON chunks(document_id);

CREATE VIRTUAL TABLE chunk_fts USING fts5(
    text,
    content='',
    contentless_delete=1,
    tokenize='trigram'
);
INSERT INTO chunk_fts(rowid, text)
    SELECT chunk_id, tg_fold(text) FROM chunks WHERE text IS NOT NULL;

CREATE TRIGGER chunks_ai AFTER INSERT ON chunks WHEN new.text IS NOT NULL BEGIN
    INSERT INTO chunk_fts(rowid, text) VALUES (new.chunk_id, tg_fold(new.text));
END;
CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN
    DELETE FROM chunk_fts WHERE rowid = old.chunk_id;
END;
CREATE TRIGGER chunks_au AFTER UPDATE OF text ON chunks BEGIN
    DELETE FROM chunk_fts WHERE rowid = old.chunk_id;
    INSERT INTO chunk_fts(rowid, text) SELECT new.chunk_id, tg_fold(new.text) WHERE new.text IS NOT NULL;
END;
