//! Model registry in SQLite (docs/04): one active model per category, its
//! files referenced in place, and its idle policy (docs/05).
//!
//! The persisted `status` is a snapshot. Live states (LOADING/READY/BUSY/…)
//! come from the runtime manager and are never written here.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::dto::TtlPolicy;
use crate::error::{ErrorCode, IpcError};
use crate::model::{ModelCategory, ModelStatus};

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRecord {
    pub model_id: String,
    pub category: ModelCategory,
    pub display_name: String,
    pub provider: Option<String>,
    /// File or folder the user chose. Never shown in the UI.
    pub path: String,
    pub format: String,
    pub architecture: Option<String>,
    pub weights_sha256: Option<String>,
    pub model_revision: Option<String>,
    pub license_name: Option<String>,
    pub runtime_adapter: String,
    pub adapter_version: String,
    /// e.g. ["text","image","audio"]
    pub capabilities: Vec<String>,
    pub embedding_dim: Option<u32>,
    pub status: ModelStatus,
    pub last_error_code: Option<String>,
    pub validated_at: Option<String>,
    pub ttl: TtlPolicy,
}

fn err(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Internal, format!("registry: {e}"))
}

fn category_from_db(s: &str) -> Option<ModelCategory> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
}

/// (ttl_mode, ttl_seconds) as stored in `model_policies`.
pub fn ttl_to_db(t: &TtlPolicy) -> (&'static str, Option<i64>) {
    match t {
        TtlPolicy::Immediately => ("unload_after_job", Some(0)),
        TtlPolicy::Minutes5 => ("ttl", Some(300)),
        TtlPolicy::Minutes10 => ("ttl", Some(600)),
        TtlPolicy::Minutes30 => ("ttl", Some(1800)),
        TtlPolicy::Hours1 => ("ttl", Some(3600)),
        TtlPolicy::Never => ("keep_loaded", None),
    }
}

pub fn ttl_from_db(mode: Option<&str>, secs: Option<i64>) -> TtlPolicy {
    match (mode, secs) {
        (Some("unload_after_job"), _) => TtlPolicy::Immediately,
        (Some("keep_loaded"), _) => TtlPolicy::Never,
        (_, Some(s)) if s <= 300 => TtlPolicy::Minutes5,
        (_, Some(s)) if s <= 600 => TtlPolicy::Minutes10,
        (_, Some(s)) if s <= 1800 => TtlPolicy::Minutes30,
        (_, Some(_)) => TtlPolicy::Hours1,
        // docs/05 default.
        _ => TtlPolicy::Minutes5,
    }
}

const SELECT: &str = "SELECT r.model_id, r.category, r.display_name, r.provider, r.path, r.format,
        r.architecture, r.weights_sha256, r.model_revision, r.license_name, r.runtime_adapter,
        r.adapter_version, r.capabilities_json, r.embedding_dim, r.status, r.last_error_code,
        r.validated_at, p.ttl_mode, p.ttl_seconds
    FROM model_registry r LEFT JOIN model_policies p ON p.model_id = r.model_id";

fn from_row(r: &Row<'_>) -> rusqlite::Result<ModelRecord> {
    let category: String = r.get(1)?;
    let caps: String = r.get(12)?;
    let status: String = r.get(14)?;
    let mode: Option<String> = r.get(17)?;
    Ok(ModelRecord {
        model_id: r.get(0)?,
        category: category_from_db(&category).unwrap_or(ModelCategory::Embedding),
        display_name: r.get(2)?,
        provider: r.get(3)?,
        path: r.get(4)?,
        format: r.get(5)?,
        architecture: r.get(6)?,
        weights_sha256: r.get(7)?,
        model_revision: r.get(8)?,
        license_name: r.get(9)?,
        runtime_adapter: r.get(10)?,
        adapter_version: r.get(11)?,
        capabilities: serde_json::from_str(&caps).unwrap_or_default(),
        embedding_dim: r.get(13)?,
        status: ModelStatus::parse_db(&status).unwrap_or(ModelStatus::Error),
        last_error_code: r.get(15)?,
        validated_at: r.get(16)?,
        ttl: ttl_from_db(mode.as_deref(), r.get(18)?),
    })
}

/// Active model per category, in category order.
pub fn active(conn: &Connection) -> Result<Vec<ModelRecord>, IpcError> {
    let mut st = conn
        .prepare(&format!("{SELECT} WHERE r.is_active = 1"))
        .map_err(err)?;
    let mut v: Vec<ModelRecord> = st
        .query_map([], from_row)
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let order = |c: ModelCategory| match c {
        ModelCategory::Embedding => 0,
        ModelCategory::Llm => 1,
        ModelCategory::Stt => 2,
        ModelCategory::Tts => 3,
    };
    v.sort_by_key(|m| order(m.category));
    Ok(v)
}

pub fn get(conn: &Connection, model_id: &str) -> Result<Option<ModelRecord>, IpcError> {
    conn.query_row(
        &format!("{SELECT} WHERE r.model_id = ?1"),
        [model_id],
        from_row,
    )
    .optional()
    .map_err(err)
}

/// Save a validated model as the active one of its category. The previous
/// active model stays in the table, inactive (its index generation may still
/// reference it until a rebuild).
pub fn activate(conn: &mut Connection, m: &ModelRecord) -> Result<(), IpcError> {
    let tx = conn.transaction().map_err(err)?;
    tx.execute(
        "UPDATE model_registry SET is_active = 0, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE category = ?1 AND is_active = 1",
        [m.category.as_db_str()],
    )
    .map_err(err)?;
    tx.execute(
        "INSERT INTO model_registry(model_id, category, display_name, provider, path, format,
            architecture, weights_sha256, model_revision, license_name, runtime_adapter,
            adapter_version, capabilities_json, embedding_dim, is_active, status, validated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,1,?15,
                 COALESCE(?16, strftime('%Y-%m-%dT%H:%M:%SZ','now')))",
        params![
            m.model_id,
            m.category.as_db_str(),
            m.display_name,
            m.provider,
            m.path,
            m.format,
            m.architecture,
            m.weights_sha256,
            m.model_revision,
            m.license_name,
            m.runtime_adapter,
            m.adapter_version,
            serde_json::to_string(&m.capabilities).map_err(err)?,
            m.embedding_dim,
            serde_json::to_value(m.status)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| "REGISTERED".into()),
            m.validated_at,
        ],
    )
    .map_err(err)?;
    let (mode, secs) = ttl_to_db(&m.ttl);
    tx.execute(
        "INSERT INTO model_policies(model_id, ttl_mode, ttl_seconds) VALUES (?1, ?2, ?3)",
        params![m.model_id, mode, secs],
    )
    .map_err(err)?;
    tx.commit().map_err(err)
}

pub fn set_ttl(conn: &Connection, model_id: &str, ttl: &TtlPolicy) -> Result<(), IpcError> {
    let (mode, secs) = ttl_to_db(ttl);
    let n = conn
        .execute(
            "INSERT INTO model_policies(model_id, ttl_mode, ttl_seconds) VALUES (?1, ?2, ?3)
             ON CONFLICT(model_id) DO UPDATE SET ttl_mode = excluded.ttl_mode,
                ttl_seconds = excluded.ttl_seconds,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
            params![model_id, mode, secs],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                IpcError::new(ErrorCode::ModelMissing, "unknown model")
            }
            e => err(e),
        })?;
    if n == 0 {
        return Err(IpcError::new(ErrorCode::ModelMissing, "unknown model"));
    }
    Ok(())
}

/// Persist a terminal snapshot status (AVAILABLE, ERROR, INCOMPATIBLE…).
pub fn set_status(
    conn: &Connection,
    model_id: &str,
    status: ModelStatus,
    error_code: Option<ErrorCode>,
) -> Result<(), IpcError> {
    let status = status.from_persisted_snapshot();
    let s = serde_json::to_value(status).map_err(err)?;
    let code = error_code
        .and_then(|c| serde_json::to_value(c).ok())
        .and_then(|v| v.as_str().map(str::to_owned));
    conn.execute(
        "UPDATE model_registry SET status = ?2, last_error_code = ?3,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE model_id = ?1",
        params![model_id, s.as_str(), code],
    )
    .map_err(err)?;
    Ok(())
}

/// Forget a model. Its files are never touched (AGENTS.md).
pub fn remove(conn: &Connection, model_id: &str) -> Result<bool, IpcError> {
    let n = conn
        .execute("DELETE FROM model_registry WHERE model_id = ?1", [model_id])
        .map_err(err)?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_and_migrate;

    fn rec(id: &str, cat: ModelCategory) -> ModelRecord {
        ModelRecord {
            model_id: id.into(),
            category: cat,
            display_name: "EmbeddingGemma 2".into(),
            provider: Some("ggml-org/embeddinggemma-2-GGUF".into()),
            path: "/models/eg2".into(),
            format: "gguf".into(),
            architecture: Some("gemma-embedding2".into()),
            weights_sha256: Some("ab".into()),
            model_revision: Some("6ea3e3a".into()),
            license_name: Some("apache-2.0".into()),
            runtime_adapter: "llama.cpp".into(),
            adapter_version: "b12024".into(),
            capabilities: vec!["text".into(), "image".into(), "audio".into()],
            embedding_dim: Some(768),
            status: ModelStatus::Available,
            last_error_code: None,
            validated_at: Some("2026-11-03T00:00:00Z".into()),
            ttl: TtlPolicy::Minutes5,
        }
    }

    #[test]
    fn activate_round_trips_and_replaces_previous() {
        let d = tempfile::tempdir().unwrap();
        let mut c = open_and_migrate(&d.path().join("t.db")).unwrap();
        assert!(active(&c).unwrap().is_empty());

        activate(&mut c, &rec("a", ModelCategory::Embedding)).unwrap();
        let v = active(&c).unwrap();
        assert_eq!(v, vec![rec("a", ModelCategory::Embedding)]);

        let mut b = rec("b", ModelCategory::Embedding);
        b.ttl = TtlPolicy::Never;
        activate(&mut c, &b).unwrap();
        let v = active(&c).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].model_id, "b");
        assert_eq!(v[0].ttl, TtlPolicy::Never);
        // Old row kept, inactive.
        assert!(get(&c, "a").unwrap().is_some());
    }

    #[test]
    fn categories_are_independent_and_ordered() {
        let d = tempfile::tempdir().unwrap();
        let mut c = open_and_migrate(&d.path().join("t.db")).unwrap();
        activate(&mut c, &rec("l", ModelCategory::Llm)).unwrap();
        activate(&mut c, &rec("e", ModelCategory::Embedding)).unwrap();
        let ids: Vec<_> = active(&c)
            .unwrap()
            .into_iter()
            .map(|m| m.model_id)
            .collect();
        assert_eq!(ids, ["e", "l"]);
    }

    #[test]
    fn ttl_status_and_remove() {
        let d = tempfile::tempdir().unwrap();
        let mut c = open_and_migrate(&d.path().join("t.db")).unwrap();
        activate(&mut c, &rec("a", ModelCategory::Embedding)).unwrap();

        set_ttl(&c, "a", &TtlPolicy::Immediately).unwrap();
        assert_eq!(get(&c, "a").unwrap().unwrap().ttl, TtlPolicy::Immediately);
        assert_eq!(
            set_ttl(&c, "nope", &TtlPolicy::Minutes5).unwrap_err().code,
            ErrorCode::ModelMissing
        );

        // Live states are never persisted as live.
        set_status(&c, "a", ModelStatus::Busy, None).unwrap();
        assert_eq!(
            get(&c, "a").unwrap().unwrap().status,
            ModelStatus::Available
        );
        set_status(
            &c,
            "a",
            ModelStatus::Error,
            Some(ErrorCode::RuntimeLoadFailed),
        )
        .unwrap();
        let m = get(&c, "a").unwrap().unwrap();
        assert_eq!(m.status, ModelStatus::Error);
        assert_eq!(m.last_error_code.as_deref(), Some("RUNTIME_LOAD_FAILED"));

        assert!(remove(&c, "a").unwrap());
        assert!(!remove(&c, "a").unwrap());
        assert!(active(&c).unwrap().is_empty());
    }

    #[test]
    fn ttl_mapping_round_trips() {
        for t in [
            TtlPolicy::Immediately,
            TtlPolicy::Minutes5,
            TtlPolicy::Minutes10,
            TtlPolicy::Minutes30,
            TtlPolicy::Hours1,
            TtlPolicy::Never,
        ] {
            let (m, s) = ttl_to_db(&t);
            assert_eq!(ttl_from_db(Some(m), s), t);
        }
        assert_eq!(ttl_from_db(None, None), TtlPolicy::Minutes5);
    }
}
