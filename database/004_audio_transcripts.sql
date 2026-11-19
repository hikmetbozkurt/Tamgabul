-- 004: spoken transcripts from local STT (P6).
--
-- chunks.modality gains 'transcript': text transcribed from an audio file
-- or video audio track by local Whisper STT. Like text/ocr/caption, it is
-- indexed in FTS5 (chunk_fts) and embedded for hybrid search. Rebuilt by
-- table-copy (SQLite cannot alter a CHECK), preserving chunks and vectors.

CREATE TABLE chunks_v4 (
    chunk_id       INTEGER PRIMARY KEY,
    document_id    TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
    generation_id  TEXT NOT NULL REFERENCES index_generations(generation_id) ON DELETE CASCADE,
    ordinal        INTEGER NOT NULL,
    modality       TEXT NOT NULL CHECK (modality IN ('text','ocr','caption','transcript','image','audio')),
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

INSERT INTO chunks_v4 SELECT chunk_id, document_id, generation_id, ordinal, modality, text, page,
       line_start, line_end, time_start_ms, time_end_ms, bbox_json, is_stale
FROM chunks;

DROP TRIGGER chunks_ai;
DROP TRIGGER chunks_ad;
DROP TRIGGER chunks_au;
DROP TABLE chunks;
ALTER TABLE chunks_v4 RENAME TO chunks;
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
