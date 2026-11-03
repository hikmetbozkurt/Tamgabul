//! SQLite open + versioned migration harness (docs/12).
//!
//! Rules implemented here:
//! - migrations are embedded, ordered by version, applied in one transaction each;
//! - an existing database file is copied to `<name>.bak-v<N>` before any
//!   pending migration runs (no silent reset of user history);
//! - a failed migration rolls back and returns `DB_MIGRATION_FAILED`.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{ErrorCode, IpcError};
use crate::health::{derive_health, AppHealth, HealthInputs, ModelSummary};
use crate::model::{ModelCategory, ModelStatus};

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// Single source of truth for the schema: the proposed migration in `database/`.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "001_initial",
        sql: include_str!("../../../database/001_initial.sql"),
    },
    Migration {
        version: 2,
        name: "002_media_and_folded_fts",
        sql: include_str!("../../../database/002_media_and_folded_fts.sql"),
    },
];

fn db_err(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::DbMigrationFailed, e.to_string())
}

/// Connection-level pragmas. They live here (not in a transaction) because
/// `journal_mode=WAL` cannot be changed inside a transaction.
/// `tg_fold(text)`: the folding used by the word index (see
/// `index::text::fold`). Must exist on every connection that writes chunks.
fn register_functions(conn: &Connection) -> Result<(), IpcError> {
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function(
        "tg_fold",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let s: Option<String> = ctx.get(0)?;
            Ok(s.map(|s| crate::index::text::fold(&s)))
        },
    )
    .map_err(db_err)
}

fn configure(conn: &Connection) -> Result<(), IpcError> {
    register_functions(conn)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(db_err)?;
    // The UI connection and background jobs write concurrently (WAL).
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(db_err)?;
    let _: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
        .map_err(db_err)?;
    Ok(())
}

/// Strip top-level PRAGMA lines from a migration; [`configure`] owns them.
fn migration_body(sql: &str) -> String {
    sql.lines()
        .filter(|l| !l.trim_start().to_ascii_uppercase().starts_with("PRAGMA"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn current_version(conn: &Connection) -> Result<u32, IpcError> {
    let has_table: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_err)?;
    if has_table.is_none() {
        return Ok(0);
    }
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )
    .map_err(db_err)
}

pub fn migrate(conn: &mut Connection, migrations: &[Migration]) -> Result<u32, IpcError> {
    let start = current_version(conn)?;
    let mut version = start;
    // Table rebuilds (copy → drop → rename) must not cascade deletes, so
    // foreign keys are off while migrating and checked before each commit.
    // (The pragma is a no-op inside a transaction, hence out here.)
    conn.pragma_update(None, "foreign_keys", "OFF")
        .map_err(db_err)?;
    let result = run_migrations(conn, migrations, start, &mut version);
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(db_err)?;
    result.map(|_| version)
}

fn run_migrations(
    conn: &mut Connection,
    migrations: &[Migration],
    start: u32,
    version: &mut u32,
) -> Result<(), IpcError> {
    for m in migrations.iter().filter(|m| m.version > start) {
        let tx = conn.transaction().map_err(db_err)?;
        tx.execute_batch(&migration_body(m.sql))
            .map_err(|e| db_err(format!("migration {} failed: {e}", m.name)))?;
        let broken: i64 = tx
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .map_err(db_err)?;
        if broken > 0 {
            return Err(db_err(format!(
                "migration {} broke {broken} references",
                m.name
            )));
        }
        tx.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, name) VALUES (?1, ?2)",
            params![m.version, m.name],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        *version = m.version;
    }
    Ok(())
}

fn backup_path(db_path: &Path, from_version: u32) -> PathBuf {
    let mut name = db_path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".bak-v{from_version}"));
    db_path.with_file_name(name)
}

/// Open (or create) the app database and bring it to the latest schema.
pub fn open_and_migrate(db_path: &Path) -> Result<Connection, IpcError> {
    let existed = db_path.exists();
    let mut conn = Connection::open(db_path).map_err(db_err)?;
    configure(&conn)?;

    let from = current_version(&conn)?;
    let latest = MIGRATIONS.iter().map(|m| m.version).max().unwrap_or(0);
    if existed && from < latest {
        // Flush WAL into the main file so the copy is complete.
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .map_err(db_err)?;
        std::fs::copy(db_path, backup_path(db_path, from)).map_err(db_err)?;
    }
    migrate(&mut conn, MIGRATIONS)?;
    Ok(conn)
}

/// Extra connection for a background job on an already-migrated database.
pub fn open_worker(db_path: &Path) -> Result<Connection, IpcError> {
    let conn = Connection::open(db_path).map_err(db_err)?;
    configure(&conn)?;
    Ok(conn)
}

/// Active model per category, with the persisted status normalized so nothing
/// shows as running right after startup.
pub fn model_summary(conn: &Connection, category: ModelCategory) -> Result<ModelSummary, IpcError> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT display_name, status FROM model_registry
             WHERE category = ?1 AND is_active = 1",
            params![category.as_db_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_err)?;
    Ok(match row {
        None => ModelSummary::new(category, ModelStatus::Missing, None),
        Some((name, status)) => {
            let status = ModelStatus::parse_db(&status)
                .unwrap_or(ModelStatus::Error)
                .from_persisted_snapshot();
            ModelSummary::new(category, status, Some(name))
        }
    })
}

pub fn active_scope_count(conn: &Connection) -> Result<u32, IpcError> {
    conn.query_row(
        "SELECT COUNT(*) FROM source_scopes WHERE state = 'ACTIVE'",
        [],
        |r| r.get(0),
    )
    .map_err(db_err)
}

/// Health snapshot for `get_app_health`. `conn = None` means the database
/// could not be opened: the app still answers, reports `db_ok = false` and
/// stays Blocked instead of crashing.
pub fn read_health(conn: Option<&Connection>) -> AppHealth {
    let missing = |c| ModelSummary::new(c, ModelStatus::Missing, None);
    let read = |conn: &Connection| -> Result<HealthInputs, IpcError> {
        Ok(HealthInputs {
            db_ok: true,
            active_scope_count: active_scope_count(conn)?,
            embedding: model_summary(conn, ModelCategory::Embedding)?,
            llm: model_summary(conn, ModelCategory::Llm)?,
            stt: model_summary(conn, ModelCategory::Stt)?,
            tts: model_summary(conn, ModelCategory::Tts)?,
        })
    };
    let inputs = conn
        .and_then(|c| read(c).ok())
        .unwrap_or_else(|| HealthInputs {
            db_ok: false,
            active_scope_count: 0,
            embedding: missing(ModelCategory::Embedding),
            llm: missing(ModelCategory::Llm),
            stt: missing(ModelCategory::Stt),
            tts: missing(ModelCategory::Tts),
        });
    derive_health(inputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::HealthLevel;

    #[test]
    fn health_without_db_is_blocked_not_panicking() {
        let h = read_health(None);
        assert!(!h.db_ok);
        assert_eq!(h.level, HealthLevel::Blocked);
        assert_eq!(h.models.len(), 4);
    }

    #[test]
    fn health_on_fresh_db_is_blocked_by_missing_embedding() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_and_migrate(&dir.path().join("t.db")).unwrap();
        let h = read_health(Some(&conn));
        assert!(h.db_ok);
        assert_eq!(h.level, HealthLevel::Blocked);
        assert!(!h.semantic_search_enabled);
    }

    fn tables(conn: &Connection) -> Vec<String> {
        let mut st = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        st.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn fresh_db_migrates_to_latest_with_all_tables() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_and_migrate(&dir.path().join("t.db")).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 2);
        let t = tables(&conn);
        for want in [
            "schema_migrations",
            "app_settings",
            "model_registry",
            "model_policies",
            "index_generations",
            "source_scopes",
            "scope_exclusions",
            "documents",
            "document_scopes",
            "chunks",
            "embedding_records",
            "sessions",
            "turns",
            "search_events",
            "result_interactions",
            "memory_notes",
            "index_jobs",
            "purge_jobs",
            "audit_events",
        ] {
            assert!(
                t.iter().any(|x| x == want),
                "missing table {want}; have {t:?}"
            );
        }
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn reopening_is_idempotent_and_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.db");
        {
            let conn = open_and_migrate(&p).unwrap();
            conn.execute(
                "INSERT INTO app_settings(key, value_json) VALUES ('lang', '\"tr\"')",
                [],
            )
            .unwrap();
        }
        let conn = open_and_migrate(&p).unwrap();
        let v: String = conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key='lang'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, "\"tr\"");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
        // Nothing pending on the second open, so no backup is written.
        assert!(!backup_path(&p, 2).exists());
    }

    #[test]
    fn failing_migration_rolls_back_and_reports_code() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        migrate(&mut conn, MIGRATIONS).unwrap();
        let bad = [Migration {
            version: 3,
            name: "003_bad",
            sql: "CREATE TABLE ok_t(x); SELEC nonsense;",
        }];
        let err = migrate(&mut conn, &bad).unwrap_err();
        assert_eq!(err.code, ErrorCode::DbMigrationFailed);
        assert_eq!(current_version(&conn).unwrap(), 2);
        assert!(
            !tables(&conn).iter().any(|t| t == "ok_t"),
            "partial migration leaked"
        );
    }

    #[test]
    fn pending_migration_on_existing_file_writes_backup() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.db");
        // An existing v0 file (e.g. created by an older build).
        Connection::open(&p)
            .unwrap()
            .execute_batch("CREATE TABLE legacy(x);")
            .unwrap();
        open_and_migrate(&p).unwrap();
        assert!(backup_path(&p, 0).exists());
    }

    #[test]
    fn empty_registry_reports_missing_and_persisted_ready_is_not_live() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_and_migrate(&dir.path().join("t.db")).unwrap();
        let emb = model_summary(&conn, ModelCategory::Embedding).unwrap();
        assert_eq!(emb.status, ModelStatus::Missing);

        conn.execute(
            "INSERT INTO model_registry(model_id, category, display_name, path, format,
                runtime_adapter, adapter_version, is_active, status)
             VALUES ('m1','embedding','EmbeddingGemma 2','x','gguf','llama.cpp','0',1,'READY')",
            [],
        )
        .unwrap();
        let emb = model_summary(&conn, ModelCategory::Embedding).unwrap();
        assert_eq!(emb.status, ModelStatus::Available);
        assert_eq!(emb.display_name.as_deref(), Some("EmbeddingGemma 2"));
        assert_eq!(active_scope_count(&conn).unwrap(), 0);
    }

    #[test]
    fn v1_to_v2_keeps_chunks_vectors_and_rebuilds_folded_fts() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        migrate(&mut conn, &MIGRATIONS[..1]).unwrap();
        conn.execute_batch(
            "INSERT INTO model_registry(model_id, category, display_name, path, format, runtime_adapter, adapter_version)
                VALUES ('m','embedding','E','x','gguf','l','1');
             INSERT INTO index_generations(generation_id, embedding_model_hash, model_revision, adapter_version,
                preprocessing_pipeline, query_instruction, document_instruction, dimension, normalization,
                modalities_json, fingerprint) VALUES ('g','h','r','a','p','q','d',128,'l2','[]','f');
             INSERT INTO documents(document_id, canonical_path, file_type, size_bytes, mtime) VALUES ('d','/x.md','markdown',1,'0');
             INSERT INTO chunks(chunk_id, document_id, generation_id, ordinal, modality, text) VALUES (7,'d','g',0,'text','Toplantı sürümü');
             INSERT INTO embedding_records(chunk_id, generation_id, dimension, vector) VALUES (7,'g',128,zeroblob(512));",
        )
        .unwrap();
        migrate(&mut conn, MIGRATIONS).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 2);
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM embedding_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "vectors survive the table rebuild");
        let hit: i64 = conn
            .query_row(
                "SELECT rowid FROM chunk_fts WHERE chunk_fts MATCH '\"toplanti\" AND \"surumu\"'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, 7);
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
        // Audio chunks are now allowed; deleting a chunk clears its FTS row.
        conn.execute(
            "INSERT INTO chunks(document_id, generation_id, ordinal, modality, time_start_ms, time_end_ms)
             VALUES ('d','g',1,'audio',0,30000)",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM chunks WHERE chunk_id = 7", [])
            .unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunk_fts WHERE chunk_fts MATCH 'toplanti'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
        let emb: i64 = conn
            .query_row("SELECT COUNT(*) FROM embedding_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(emb, 0, "cascade still works after the rebuild");
    }

    #[test]
    fn fts5_and_json1_are_available_in_bundled_sqlite() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE VIRTUAL TABLE f USING fts5(body);")
            .unwrap();
        let ok: i64 = conn
            .query_row("SELECT json_valid('[1]')", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ok, 1);
    }
}
