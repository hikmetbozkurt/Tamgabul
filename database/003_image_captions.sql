-- 003: image descriptions.
--
-- chunks.modality gains 'caption': a short description of an image, written
-- once at scan time by the optional local vision LLM (what the picture is,
-- important visible text, search words in English and Turkish). It is text,
-- so it is matched by words (FTS) and embedded like any other text chunk.
-- Rebuilt table-by-copy (SQLite cannot alter a CHECK). chunk_fts keeps its
-- rows: chunk ids do not change.

CREATE TABLE chunks_v3 (
    chunk_id       INTEGER PRIMARY KEY,
    document_id    TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
    generation_id  TEXT NOT NULL REFERENCES index_generations(generation_id) ON DELETE CASCADE,
    ordinal        INTEGER NOT NULL,
    modality       TEXT NOT NULL CHECK (modality IN ('text','ocr','caption','image','audio')),
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

INSERT INTO chunks_v3 SELECT chunk_id, document_id, generation_id, ordinal, modality, text, page,
       line_start, line_end, time_start_ms, time_end_ms, bbox_json, is_stale
FROM chunks;

DROP TRIGGER chunks_ai;
DROP TRIGGER chunks_ad;
DROP TRIGGER chunks_au;
DROP TABLE chunks;
ALTER TABLE chunks_v3 RENAME TO chunks;
CREATE INDEX ix_chunks_generation ON chunks(generation_id);
CREATE INDEX ix_chunks_document ON chunks(document_id);

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
