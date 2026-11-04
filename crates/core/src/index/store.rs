//! SQLite side of folder grants and the index (docs/07, docs/12).
//!
//! Safety rules implemented here:
//! - A document is committed only while its scope is ACTIVE, checked inside
//!   the same transaction, so nothing new lands after a revoke starts.
//! - Revoke first flips the scope to REVOKING (search must deny it at once),
//!   then a durable purge job removes everything derived from it and proves
//!   with SQL counts that nothing is left before marking it DONE.
//! - Source files are never opened for writing anywhere in this crate.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use crate::dto::RevokeMode;
use crate::error::{ErrorCode, IpcError};
use crate::index::walk::{is_within, SourceKind};

fn err(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Internal, format!("index store: {e}"))
}

pub fn new_id(prefix: &str, seed: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let h = blake3::hash(format!("{seed}|{nanos}|{}", std::process::id()).as_bytes());
    format!("{prefix}-{}", &h.to_hex()[..24])
}

/// Stable per-path id; a document is unique by canonical path.
pub fn document_id(path: &Path) -> String {
    format!(
        "d-{}",
        &blake3::hash(path.to_string_lossy().as_bytes()).to_hex()[..32]
    )
}

pub fn mtime_str(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}.{:09}", d.as_secs(), d.subsec_nanos())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeState {
    Active,
    Paused,
    Revoking,
    Revoked,
    Error,
}

impl ScopeState {
    pub fn as_db(self) -> &'static str {
        match self {
            ScopeState::Active => "ACTIVE",
            ScopeState::Paused => "PAUSED",
            ScopeState::Revoking => "REVOKING",
            ScopeState::Revoked => "REVOKED",
            ScopeState::Error => "ERROR",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "ACTIVE" => ScopeState::Active,
            "PAUSED" => ScopeState::Paused,
            "REVOKING" => ScopeState::Revoking,
            "REVOKED" => ScopeState::Revoked,
            _ => ScopeState::Error,
        }
    }
    /// May its content appear in results?
    pub fn is_authorized(self) -> bool {
        matches!(self, ScopeState::Active | ScopeState::Paused)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scope {
    pub scope_id: String,
    pub canonical_path: PathBuf,
    pub display_path: String,
    pub is_drive_root: bool,
    pub state: ScopeState,
    pub file_count: u32,
    pub last_scan_at: Option<String>,
    pub error_summary: Option<String>,
    pub max_file_bytes: Option<u64>,
}

const SCOPE_COLS: &str = "scope_id, canonical_path, display_path, is_drive_root, state, file_count,
    last_scan_at, error_summary, max_file_bytes";

fn scope_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Scope> {
    let p: String = r.get(1)?;
    let st: String = r.get(4)?;
    let max: Option<i64> = r.get(8)?;
    Ok(Scope {
        scope_id: r.get(0)?,
        canonical_path: PathBuf::from(p),
        display_path: r.get(2)?,
        is_drive_root: r.get::<_, i64>(3)? != 0,
        state: ScopeState::parse(&st),
        file_count: r.get::<_, i64>(5)?.max(0) as u32,
        last_scan_at: r.get(6)?,
        error_summary: r.get(7)?,
        max_file_bytes: max.map(|m| m.max(0) as u64),
    })
}

pub fn insert_scope(conn: &Connection, s: &Scope) -> Result<(), IpcError> {
    conn.execute(
        "INSERT INTO source_scopes(scope_id, canonical_path, display_path, is_drive_root, state, max_file_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            s.scope_id,
            s.canonical_path.to_string_lossy(),
            s.display_path,
            s.is_drive_root as i64,
            s.state.as_db(),
            s.max_file_bytes.map(|m| m as i64),
        ],
    )
    .map_err(err)?;
    audit(conn, "scope_granted", &s.scope_id)?;
    Ok(())
}

pub fn get_scope(conn: &Connection, id: &str) -> Result<Option<Scope>, IpcError> {
    conn.query_row(
        &format!("SELECT {SCOPE_COLS} FROM source_scopes WHERE scope_id = ?1"),
        [id],
        scope_row,
    )
    .optional()
    .map_err(err)
}

/// Every scope that is not fully revoked, oldest first.
pub fn visible_scopes(conn: &Connection) -> Result<Vec<Scope>, IpcError> {
    let mut st = conn
        .prepare(&format!(
            "SELECT {SCOPE_COLS} FROM source_scopes WHERE state NOT IN ('REVOKED','PENDING_PREVIEW','AWAIT_CONFIRM')
             ORDER BY created_at, scope_id"
        ))
        .map_err(err)?;
    let v = st
        .query_map([], scope_row)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(v)
}

pub fn set_scope_state(
    conn: &Connection,
    id: &str,
    state: ScopeState,
    error: Option<&str>,
) -> Result<(), IpcError> {
    conn.execute(
        "UPDATE source_scopes SET state = ?2, error_summary = ?3 WHERE scope_id = ?1",
        params![id, state.as_db(), error],
    )
    .map_err(err)?;
    Ok(())
}

/// Nearest authorized ancestor scope (for overlap semantics).
pub fn parent_of<'a>(scopes: &'a [Scope], s: &Scope) -> Option<&'a Scope> {
    scopes
        .iter()
        .filter(|p| {
            p.scope_id != s.scope_id
                && p.state.is_authorized()
                && p.canonical_path != s.canonical_path
                && is_within(&s.canonical_path, &p.canonical_path)
        })
        .max_by_key(|p| p.canonical_path.as_os_str().len())
}

/// Paths a scan of `scope` must not enter: global exclusions and those
/// recorded under this scope (e.g. a child revoked with "block").
pub fn exclusions_for(conn: &Connection, scope_id: &str) -> Result<Vec<PathBuf>, IpcError> {
    let mut st = conn
        .prepare("SELECT canonical_path FROM scope_exclusions WHERE parent_scope_id IS NULL OR parent_scope_id = ?1")
        .map_err(err)?;
    let v = st
        .query_map([scope_id], |r| r.get::<_, String>(0))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(v.into_iter().map(PathBuf::from).collect())
}

pub fn exclusion_count(conn: &Connection, scope_id: &str) -> Result<u32, IpcError> {
    conn.query_row(
        "SELECT COUNT(*) FROM scope_exclusions WHERE parent_scope_id = ?1",
        [scope_id],
        |r| r.get(0),
    )
    .map_err(err)
}

/// May content at `path` be shown? Some authorized scope must contain it and
/// no exclusion of that scope (or global) may cover it. Checked at search
/// and open time, not only at scan time (docs/07 §5).
pub fn authorized(conn: &Connection, path: &Path) -> Result<bool, IpcError> {
    for s in visible_scopes(conn)? {
        if !s.state.is_authorized() || !is_within(path, &s.canonical_path) {
            continue;
        }
        let ex = exclusions_for(conn, &s.scope_id)?;
        if !ex.iter().any(|e| is_within(path, e)) {
            return Ok(true);
        }
    }
    Ok(false)
}

// ---------------------------------------------------------------- watching

/// Folder watching (rescan on change) is `scan_mode = 'idle_sync'`.
pub fn set_watching(conn: &Connection, scope_id: &str, on: bool) -> Result<(), IpcError> {
    let n = conn
        .execute(
            "UPDATE source_scopes SET scan_mode = ?2 WHERE scope_id = ?1 AND state IN ('ACTIVE','PAUSED','ERROR')",
            params![scope_id, if on { "idle_sync" } else { "manual" }],
        )
        .map_err(err)?;
    if n == 0 {
        return Err(IpcError::new(ErrorCode::ScopeRevoked, "unknown folder"));
    }
    audit(conn, if on { "watch_on" } else { "watch_off" }, scope_id)
}

pub fn is_watching(conn: &Connection, scope_id: &str) -> Result<bool, IpcError> {
    conn.query_row(
        "SELECT scan_mode = 'idle_sync' FROM source_scopes WHERE scope_id = ?1",
        [scope_id],
        |r| r.get::<_, bool>(0),
    )
    .optional()
    .map(|v| v.unwrap_or(false))
    .map_err(err)
}

/// Active folders with watching on.
pub fn watched_scopes(conn: &Connection) -> Result<Vec<Scope>, IpcError> {
    let mut st = conn
        .prepare(&format!(
            "SELECT {SCOPE_COLS} FROM source_scopes WHERE state = 'ACTIVE' AND scan_mode = 'idle_sync'
             ORDER BY created_at, scope_id"
        ))
        .map_err(err)?;
    let v = st
        .query_map([], scope_row)
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(v)
}

// ---------------------------------------------------------------- relink

/// Share of sampled indexed files that must be found at the new place
/// (same relative path and size) to accept it as the same folder.
pub const RELINK_MIN_MATCH: f64 = 0.5;
const RELINK_SAMPLE: usize = 300;

#[derive(Debug, Clone, PartialEq)]
pub struct RelinkCheck {
    pub sampled: u32,
    pub matched: u32,
}

impl RelinkCheck {
    pub fn accepted(&self) -> bool {
        self.matched > 0 && self.matched as f64 >= self.sampled as f64 * RELINK_MIN_MATCH
    }
}

/// How many of the folder's indexed files exist under `new_root` with the
/// same relative path and size. Reads metadata only.
pub fn relink_check(
    conn: &Connection,
    scope: &Scope,
    new_root: &Path,
) -> Result<RelinkCheck, IpcError> {
    let mut st = conn
        .prepare(
            "SELECT d.canonical_path, d.size_bytes FROM documents d
             JOIN document_scopes s ON s.document_id = d.document_id
             WHERE s.scope_id = ?1 AND d.parse_status = 'PARSED' ORDER BY d.document_id LIMIT ?2",
        )
        .map_err(err)?;
    let rows: Vec<(String, i64)> = st
        .query_map(params![scope.scope_id, RELINK_SAMPLE as i64], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut c = RelinkCheck {
        sampled: 0,
        matched: 0,
    };
    for (p, size) in rows {
        let Ok(rel) = Path::new(&p).strip_prefix(&scope.canonical_path) else {
            continue;
        };
        c.sampled += 1;
        if std::fs::metadata(new_root.join(rel))
            .is_ok_and(|m| m.is_file() && m.len() as i64 == size)
        {
            c.matched += 1;
        }
    }
    Ok(c)
}

/// Point a moved folder at its new place, keeping its index: the folder
/// grant, its documents and its exclusions move to `new_root` in one
/// transaction (chunks and vectors stay as they are). The next scan only
/// processes what really changed. The caller has checked `relink_check`
/// and that the user chose `new_root`.
pub fn relink_scope(
    conn: &mut Connection,
    scope_id: &str,
    new_root: &Path,
    new_display: &str,
) -> Result<u32, IpcError> {
    let scope = get_scope(conn, scope_id)?
        .ok_or_else(|| IpcError::new(ErrorCode::ScopeRevoked, "unknown folder"))?;
    if !matches!(
        scope.state,
        ScopeState::Active | ScopeState::Paused | ScopeState::Error
    ) {
        return Err(IpcError::new(ErrorCode::ScopeRevoked, "folder was removed"));
    }
    let old = scope.canonical_path.clone();
    let tx = conn.transaction().map_err(err)?;
    let docs: Vec<(String, String)> = {
        let mut st = tx
            .prepare(
                "SELECT d.document_id, d.canonical_path FROM documents d
                 JOIN document_scopes s ON s.document_id = d.document_id WHERE s.scope_id = ?1",
            )
            .map_err(err)?;
        let v = st
            .query_map([scope_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        v
    };
    let mut moved = 0u32;
    for (id, p) in docs {
        let Ok(rel) = Path::new(&p).strip_prefix(&old) else {
            continue;
        };
        let np = new_root.join(rel);
        // A file already indexed at the new path (another folder) wins;
        // the old entry is dropped from this folder.
        let taken: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM documents WHERE canonical_path = ?1)",
                [np.to_string_lossy()],
                |r| r.get(0),
            )
            .map_err(err)?;
        if taken {
            tx.execute(
                "DELETE FROM document_scopes WHERE document_id = ?1 AND scope_id = ?2",
                params![id, scope_id],
            )
            .map_err(err)?;
            continue;
        }
        tx.execute(
            "UPDATE documents SET canonical_path = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE document_id = ?1",
            params![id, np.to_string_lossy()],
        )
        .map_err(err)?;
        moved += 1;
    }
    let ex: Vec<(String, String)> = {
        let mut st = tx
            .prepare("SELECT exclusion_id, canonical_path FROM scope_exclusions WHERE parent_scope_id = ?1")
            .map_err(err)?;
        let v = st
            .query_map([scope_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        v
    };
    for (id, p) in ex {
        if let Ok(rel) = Path::new(&p).strip_prefix(&old) {
            tx.execute(
                "UPDATE scope_exclusions SET canonical_path = ?2 WHERE exclusion_id = ?1",
                params![id, new_root.join(rel).to_string_lossy()],
            )
            .map_err(err)?;
        }
    }
    tx.execute(
        "UPDATE source_scopes SET canonical_path = ?2, display_path = ?3, error_summary = NULL,
             state = CASE WHEN state = 'ERROR' THEN 'ACTIVE' ELSE state END
         WHERE scope_id = ?1",
        params![scope_id, new_root.to_string_lossy(), new_display],
    )
    .map_err(err)?;
    audit(&tx, "scope_relinked", scope_id)?;
    delete_orphans(&tx)?;
    tx.commit().map_err(err)?;
    Ok(moved)
}

fn audit(conn: &Connection, kind: &str, subject: &str) -> Result<(), IpcError> {
    conn.execute(
        "INSERT INTO audit_events(kind, subject_id) VALUES (?1, ?2)",
        params![kind, subject],
    )
    .map_err(err)?;
    Ok(())
}

// ---------------------------------------------------------------- generations

/// Identity of the vectors in the index (docs/12): vectors from a different
/// model, revision, prompt or dimension are never mixed.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerationSpec {
    pub embedding_model_id: String,
    pub model_hash: String,
    pub model_revision: String,
    pub adapter_version: String,
    pub preprocessing: String,
    pub query_instruction: String,
    pub document_instruction: String,
    pub dimension: u32,
    pub normalization: String,
    pub modalities: Vec<String>,
}

impl GenerationSpec {
    pub fn fingerprint(&self) -> String {
        // adapter_version is recorded but not part of the identity: a newer
        // llama.cpp build computing the same model does not force a rebuild.
        let s = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.model_hash,
            self.model_revision,
            self.preprocessing,
            self.query_instruction,
            self.document_instruction,
            self.dimension,
            self.normalization,
            self.modalities.join(",")
        );
        blake3::hash(s.as_bytes()).to_hex()[..32].to_string()
    }
}

pub fn current_generation(conn: &Connection) -> Result<Option<(String, String)>, IpcError> {
    conn.query_row(
        "SELECT generation_id, fingerprint FROM index_generations WHERE state = 'CURRENT'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(err)
}

/// The CURRENT generation for `spec`, creating it (and retiring the old one)
/// when the embedding identity changed.
pub fn ensure_current_generation(
    conn: &mut Connection,
    spec: &GenerationSpec,
) -> Result<String, IpcError> {
    let fp = spec.fingerprint();
    if let Some((id, cur)) = current_generation(conn)? {
        if cur == fp {
            return Ok(id);
        }
    }
    let tx = conn.transaction().map_err(err)?;
    tx.execute(
        "UPDATE index_generations SET state = 'RETIRED' WHERE state = 'CURRENT'",
        [],
    )
    .map_err(err)?;
    let id = format!("g-{fp}");
    tx.execute(
        "INSERT INTO index_generations(generation_id, embedding_model_id, embedding_model_hash, model_revision,
            adapter_version, preprocessing_pipeline, query_instruction, document_instruction, dimension,
            normalization, modalities_json, fingerprint, state, activated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'CURRENT', strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(fingerprint) DO UPDATE SET state = 'CURRENT', activated_at = excluded.activated_at",
        params![
            id,
            spec.embedding_model_id,
            spec.model_hash,
            spec.model_revision,
            spec.adapter_version,
            spec.preprocessing,
            spec.query_instruction,
            spec.document_instruction,
            spec.dimension,
            spec.normalization,
            serde_json::to_string(&spec.modalities).map_err(err)?,
            fp,
        ],
    )
    .map_err(err)?;
    let id: String = tx
        .query_row(
            "SELECT generation_id FROM index_generations WHERE fingerprint = ?1",
            [&fp],
            |r| r.get(0),
        )
        .map_err(err)?;
    tx.commit().map_err(err)?;
    Ok(id)
}

// ---------------------------------------------------------------- documents

#[derive(Debug, Clone, PartialEq)]
pub struct DocState {
    pub document_id: String,
    pub size: u64,
    pub mtime: String,
    pub content_hash: Option<String>,
    pub parse_status: String,
    pub parse_error: Option<String>,
    /// Chunks with vectors in `generation`.
    pub indexed_in_generation: bool,
}

impl DocState {
    /// A skip that will not change unless the file changes.
    pub fn permanently_skipped(&self) -> bool {
        self.parse_status == "SKIPPED"
            && matches!(
                self.parse_error.as_deref(),
                Some("binary" | "pdf_no_text" | "pdf_unreadable" | "empty")
            )
    }
}

pub fn doc_by_path(
    conn: &Connection,
    path: &Path,
    generation: &str,
) -> Result<Option<DocState>, IpcError> {
    conn.query_row(
        "SELECT d.document_id, d.size_bytes, d.mtime, d.content_hash, d.parse_status, d.parse_error,
            EXISTS(SELECT 1 FROM chunks c JOIN embedding_records e ON e.chunk_id = c.chunk_id
                   WHERE c.document_id = d.document_id AND c.generation_id = ?2)
         FROM documents d WHERE d.canonical_path = ?1",
        params![path.to_string_lossy(), generation],
        |r| {
            Ok(DocState {
                document_id: r.get(0)?,
                size: r.get::<_, i64>(1)?.max(0) as u64,
                mtime: r.get(2)?,
                content_hash: r.get(3)?,
                parse_status: r.get(4)?,
                parse_error: r.get(5)?,
                indexed_in_generation: r.get::<_, i64>(6)? != 0,
            })
        },
    )
    .optional()
    .map_err(err)
}

/// The document already has an image description in this generation.
pub fn has_caption(
    conn: &Connection,
    document_id: &str,
    generation: &str,
) -> Result<bool, IpcError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM chunks WHERE document_id = ?1 AND generation_id = ?2 AND modality = 'caption')",
        params![document_id, generation],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n != 0)
    .map_err(err)
}

/// Marker: this image was read with sideways OCR (rotation retry).
const OCR_V2: &str = "ocr_v2";

/// An image read before sideways OCR existed and left without OCR text:
/// read once more.
pub fn needs_ocr_retry(
    conn: &Connection,
    document_id: &str,
    generation: &str,
) -> Result<bool, IpcError> {
    conn.query_row(
        "SELECT NOT EXISTS(SELECT 1 FROM chunks c
                 WHERE c.document_id = ?1 AND c.generation_id = ?2 AND c.modality = 'ocr')
            AND NOT EXISTS(SELECT 1 FROM audit_events WHERE kind = ?3 AND subject_id = ?1)",
        params![document_id, generation, OCR_V2],
        |r| r.get::<_, bool>(0),
    )
    .map_err(err)
}

pub fn mark_ocr_v2(conn: &Connection, document_id: &str) -> Result<(), IpcError> {
    let done: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM audit_events WHERE kind = ?1 AND subject_id = ?2)",
            params![OCR_V2, document_id],
            |r| r.get(0),
        )
        .map_err(err)?;
    if !done {
        audit(conn, OCR_V2, document_id)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewChunk {
    pub ordinal: u32,
    /// "text" or "image".
    pub modality: &'static str,
    /// `None` for an image chunk.
    pub text: Option<String>,
    pub page: Option<u32>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    /// Audio segment range.
    pub time_ms: Option<(u64, u64)>,
    pub vector: Vec<f32>,
}

pub struct DocMeta<'a> {
    pub path: &'a Path,
    pub kind: SourceKind,
    pub size: u64,
    pub mtime: &'a str,
    pub content_hash: Option<&'a str>,
}

fn scope_active(conn: &Connection, scope_id: &str) -> Result<bool, IpcError> {
    let s: Option<String> = conn
        .query_row(
            "SELECT state FROM source_scopes WHERE scope_id = ?1",
            [scope_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    Ok(s.as_deref() == Some("ACTIVE"))
}

/// The id of the document at `path`: the stored one when the path is
/// already indexed (its id may come from an earlier path, e.g. after a
/// folder relink), else derived from the path.
pub fn document_id_at(conn: &Connection, path: &Path) -> Result<Option<String>, IpcError> {
    conn.query_row(
        "SELECT document_id FROM documents WHERE canonical_path = ?1",
        [path.to_string_lossy()],
        |r| r.get(0),
    )
    .optional()
    .map_err(err)
}

fn doc_id_for(conn: &Connection, path: &Path) -> Result<String, IpcError> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT document_id FROM documents WHERE canonical_path = ?1",
            [path.to_string_lossy()],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    Ok(existing.unwrap_or_else(|| document_id(path)))
}

fn upsert_doc(
    conn: &Connection,
    id: &str,
    m: &DocMeta<'_>,
    status: &str,
    error: Option<&str>,
) -> Result<(), IpcError> {
    conn.execute(
        "INSERT INTO documents(document_id, canonical_path, file_type, size_bytes, mtime, content_hash, parse_status, parse_error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(canonical_path) DO UPDATE SET file_type = excluded.file_type, size_bytes = excluded.size_bytes,
            mtime = excluded.mtime, content_hash = excluded.content_hash, parse_status = excluded.parse_status,
            parse_error = excluded.parse_error, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        params![
            id,
            m.path.to_string_lossy(),
            m.kind.as_db_str(),
            m.size as i64,
            m.mtime,
            m.content_hash,
            status,
            error
        ],
    )
    .map_err(err)?;
    Ok(())
}

fn link(conn: &Connection, doc: &str, scope: &str) -> Result<(), IpcError> {
    conn.execute(
        "INSERT OR IGNORE INTO document_scopes(document_id, scope_id) VALUES (?1, ?2)",
        params![doc, scope],
    )
    .map_err(err)?;
    Ok(())
}

pub fn vector_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn vector_from_bytes(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// Atomically replace a document's chunks and vectors. Returns `false`
/// (and writes nothing) if the scope stopped being ACTIVE.
pub fn commit_document(
    conn: &mut Connection,
    scope_id: &str,
    generation: &str,
    m: &DocMeta<'_>,
    chunks: &[NewChunk],
) -> Result<bool, IpcError> {
    let tx = conn.transaction().map_err(err)?;
    if !scope_active(&tx, scope_id)? {
        return Ok(false);
    }
    let id = doc_id_for(&tx, m.path)?;
    upsert_doc(&tx, &id, m, "PARSED", None)?;
    tx.execute("DELETE FROM chunks WHERE document_id = ?1", [&id])
        .map_err(err)?;
    {
        let mut ins = tx
            .prepare(
                "INSERT INTO chunks(document_id, generation_id, ordinal, modality, text, page, line_start, line_end,
                    time_start_ms, time_end_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )
            .map_err(err)?;
        let mut emb = tx
            .prepare("INSERT INTO embedding_records(chunk_id, generation_id, dimension, vector) VALUES (?1, ?2, ?3, ?4)")
            .map_err(err)?;
        for c in chunks {
            ins.execute(params![
                id,
                generation,
                c.ordinal,
                c.modality,
                c.text,
                c.page,
                c.line_start,
                c.line_end,
                c.time_ms.map(|t| t.0 as i64),
                c.time_ms.map(|t| t.1 as i64)
            ])
            .map_err(err)?;
            let chunk_id = tx.last_insert_rowid();
            emb.execute(params![
                chunk_id,
                generation,
                c.vector.len() as i64,
                vector_bytes(&c.vector)
            ])
            .map_err(err)?;
        }
    }
    link(&tx, &id, scope_id)?;
    tx.commit().map_err(err)?;
    Ok(true)
}

/// Record a file that was seen but not indexed (too big, binary, failed).
/// It is linked so revoke still cleans it up; it has no chunks.
pub fn record_skipped(
    conn: &mut Connection,
    scope_id: &str,
    m: &DocMeta<'_>,
    status: &str,
    error: &str,
) -> Result<bool, IpcError> {
    let tx = conn.transaction().map_err(err)?;
    if !scope_active(&tx, scope_id)? {
        return Ok(false);
    }
    let id = doc_id_for(&tx, m.path)?;
    upsert_doc(&tx, &id, m, status, Some(error))?;
    tx.execute("DELETE FROM chunks WHERE document_id = ?1", [&id])
        .map_err(err)?;
    link(&tx, &id, scope_id)?;
    tx.commit().map_err(err)?;
    Ok(true)
}

/// Unchanged file: keep its chunks, refresh stat, make sure it is linked.
pub fn keep_document(
    conn: &mut Connection,
    scope_id: &str,
    doc_id: &str,
    mtime: &str,
) -> Result<bool, IpcError> {
    let tx = conn.transaction().map_err(err)?;
    if !scope_active(&tx, scope_id)? {
        return Ok(false);
    }
    tx.execute(
        "UPDATE documents SET mtime = ?2 WHERE document_id = ?1",
        params![doc_id, mtime],
    )
    .map_err(err)?;
    link(&tx, doc_id, scope_id)?;
    tx.commit().map_err(err)?;
    Ok(true)
}

/// After a full scan: drop links to files that no longer exist (or are now
/// excluded) and delete documents no scope references any more.
pub fn unlink_missing(
    conn: &mut Connection,
    scope_id: &str,
    seen: &HashSet<String>,
) -> Result<u32, IpcError> {
    let tx = conn.transaction().map_err(err)?;
    let linked: Vec<String> = {
        let mut st = tx
            .prepare("SELECT document_id FROM document_scopes WHERE scope_id = ?1")
            .map_err(err)?;
        let v = st
            .query_map([scope_id], |r| r.get(0))
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        v
    };
    let mut n = 0;
    for d in linked.iter().filter(|d| !seen.contains(*d)) {
        tx.execute(
            "DELETE FROM document_scopes WHERE document_id = ?1 AND scope_id = ?2",
            params![d, scope_id],
        )
        .map_err(err)?;
        n += 1;
    }
    delete_orphans(&tx)?;
    tx.commit().map_err(err)?;
    Ok(n)
}

/// Documents with no grant left: tombstone their history rows, then delete
/// (chunks, FTS rows and vectors go with them via FK/trigger).
fn delete_orphans(conn: &Connection) -> Result<u32, IpcError> {
    conn.execute(
        "UPDATE result_interactions SET is_tombstone = 1, title_snapshot = NULL, document_id = NULL, chunk_id = NULL
         WHERE document_id IN (SELECT document_id FROM documents d
             WHERE NOT EXISTS (SELECT 1 FROM document_scopes s WHERE s.document_id = d.document_id))",
        [],
    )
    .map_err(err)?;
    let n = conn
        .execute(
            "DELETE FROM documents WHERE NOT EXISTS
                (SELECT 1 FROM document_scopes s WHERE s.document_id = documents.document_id)",
            [],
        )
        .map_err(err)?;
    Ok(n as u32)
}

pub fn scope_doc_count(conn: &Connection, scope_id: &str) -> Result<u32, IpcError> {
    conn.query_row(
        "SELECT COUNT(*) FROM document_scopes s JOIN documents d ON d.document_id = s.document_id
         WHERE s.scope_id = ?1 AND d.parse_status = 'PARSED'",
        [scope_id],
        |r| r.get(0),
    )
    .map_err(err)
}

pub fn finish_scan(conn: &Connection, scope_id: &str, generation: &str) -> Result<(), IpcError> {
    let n = scope_doc_count(conn, scope_id)?;
    conn.execute(
        "UPDATE source_scopes SET file_count = ?2, last_generation_id = ?3, error_summary = NULL,
            last_scan_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE scope_id = ?1",
        params![scope_id, n, generation],
    )
    .map_err(err)?;
    Ok(())
}

/// Distinct searchable documents across authorized scopes.
pub fn indexed_file_count(conn: &Connection) -> Result<u32, IpcError> {
    conn.query_row(
        "SELECT COUNT(DISTINCT d.document_id) FROM documents d
         JOIN document_scopes s ON s.document_id = d.document_id
         JOIN source_scopes sc ON sc.scope_id = s.scope_id
         WHERE d.parse_status = 'PARSED' AND sc.state IN ('ACTIVE','PAUSED')",
        [],
        |r| r.get(0),
    )
    .map_err(err)
}

// ---------------------------------------------------------------- index jobs

pub fn create_job(conn: &Connection, scope_id: &str, generation: &str) -> Result<String, IpcError> {
    let id = new_id("j", scope_id);
    conn.execute(
        "INSERT INTO index_jobs(job_id, scope_id, generation_id, state) VALUES (?1, ?2, ?3, 'RUNNING')",
        params![id, scope_id, generation],
    )
    .map_err(err)?;
    Ok(id)
}

pub fn update_job(
    conn: &Connection,
    job_id: &str,
    state: &str,
    processed: u32,
    errors: u32,
) -> Result<(), IpcError> {
    conn.execute(
        "UPDATE index_jobs SET state = ?2, processed = ?3, errors = ?4,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE job_id = ?1",
        params![job_id, state, processed, errors],
    )
    .map_err(err)?;
    Ok(())
}

/// Scopes whose last job did not finish (app quit or crashed mid-scan).
pub fn interrupted_scopes(conn: &Connection) -> Result<Vec<String>, IpcError> {
    let mut st = conn
        .prepare(
            "SELECT DISTINCT j.scope_id FROM index_jobs j JOIN source_scopes s ON s.scope_id = j.scope_id
             WHERE j.state IN ('QUEUED','RUNNING') AND s.state = 'ACTIVE'",
        )
        .map_err(err)?;
    let v = st
        .query_map([], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    conn.execute(
        "UPDATE index_jobs SET state = 'CANCELLED' WHERE state IN ('QUEUED','RUNNING')",
        [],
    )
    .map_err(err)?;
    Ok(v)
}

// ---------------------------------------------------------------- revoke & purge

/// Step 1 of revoke: deny at once. Returns the purge job to run (if any).
pub fn begin_revoke(
    conn: &mut Connection,
    scope_id: &str,
    mode: RevokeMode,
    purge_history: bool,
) -> Result<Option<String>, IpcError> {
    let scope = get_scope(conn, scope_id)?
        .ok_or_else(|| IpcError::new(ErrorCode::ScopeRevoked, "unknown folder"))?;
    if matches!(scope.state, ScopeState::Revoked | ScopeState::Revoking) {
        return Ok(None);
    }
    if mode == RevokeMode::StopWatching {
        set_scope_state(conn, scope_id, ScopeState::Paused, None)?;
        return Ok(None);
    }
    let all = visible_scopes(conn)?;
    let parent = parent_of(&all, &scope).map(|p| p.scope_id.clone());
    let tx = conn.transaction().map_err(err)?;
    tx.execute(
        "UPDATE source_scopes SET state = 'REVOKING' WHERE scope_id = ?1",
        [scope_id],
    )
    .map_err(err)?;
    if mode == RevokeMode::BlockAndPurge {
        if let Some(parent) = &parent {
            tx.execute(
                "INSERT INTO scope_exclusions(exclusion_id, parent_scope_id, canonical_path, reason)
                 VALUES (?1, ?2, ?3, 'child_revoked')",
                params![new_id("x", scope_id), parent, scope.canonical_path.to_string_lossy()],
            )
            .map_err(err)?;
        }
    }
    let purge_id = new_id("p", scope_id);
    tx.execute(
        "INSERT INTO purge_jobs(purge_id, scope_id, policy, state) VALUES (?1, ?2, ?3, 'QUEUED')",
        params![
            purge_id,
            scope_id,
            if purge_history {
                "purge_all_related_turns"
            } else {
                "purge_exclusive"
            }
        ],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;
    audit(conn, "scope_revoked", scope_id)?;
    Ok(Some(purge_id))
}

fn set_purge(conn: &Connection, id: &str, state: &str, step: &str) -> Result<(), IpcError> {
    conn.execute(
        "UPDATE purge_jobs SET state = ?2, step = ?3, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE purge_id = ?1",
        params![id, state, step],
    )
    .map_err(err)?;
    Ok(())
}

/// Step 2: remove everything derived from the scope, then prove it with SQL.
/// Idempotent, so an interrupted purge simply runs again at next start.
pub fn run_purge(conn: &mut Connection, purge_id: &str) -> Result<(), IpcError> {
    let (scope_id, policy, state): (String, String, String) = conn
        .query_row(
            "SELECT scope_id, policy, state FROM purge_jobs WHERE purge_id = ?1",
            [purge_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(err)?;
    if state == "DONE" {
        return Ok(());
    }
    let scope = get_scope(conn, &scope_id)?;
    // Block mode left an exclusion for this path under its parent.
    let blocked_path: Option<PathBuf> = match &scope {
        Some(s) => conn
            .query_row(
                "SELECT canonical_path FROM scope_exclusions WHERE canonical_path = ?1 AND reason = 'child_revoked'",
                [s.canonical_path.to_string_lossy()],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(err)?
            .map(PathBuf::from),
        None => None,
    };
    set_purge(conn, purge_id, "RUNNING", "unlink")?;

    let tx = conn.transaction().map_err(err)?;
    if let Some(bp) = &blocked_path {
        // Drop every grant's link to documents under the blocked path.
        let docs: Vec<(String, String)> = {
            let mut st = tx
                .prepare("SELECT document_id, canonical_path FROM documents")
                .map_err(err)?;
            let v = st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(err)?;
            v
        };
        for (id, p) in docs {
            if is_within(Path::new(&p), bp) {
                tx.execute("DELETE FROM document_scopes WHERE document_id = ?1", [&id])
                    .map_err(err)?;
            }
        }
    }
    if policy == "purge_all_related_turns" {
        tx.execute(
            "DELETE FROM result_interactions WHERE scope_id = ?1 OR document_id IN
                (SELECT document_id FROM document_scopes WHERE scope_id = ?1)",
            [&scope_id],
        )
        .map_err(err)?;
    } else {
        tx.execute(
            "UPDATE result_interactions SET is_tombstone = 1, title_snapshot = NULL, document_id = NULL, chunk_id = NULL
             WHERE scope_id = ?1",
            [&scope_id],
        )
        .map_err(err)?;
    }
    tx.execute(
        "DELETE FROM document_scopes WHERE scope_id = ?1",
        [&scope_id],
    )
    .map_err(err)?;
    delete_orphans(&tx)?;
    tx.execute(
        "UPDATE purge_jobs SET state = 'VERIFYING', step = 'verify' WHERE purge_id = ?1",
        [purge_id],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;

    // Proof: nothing linked to the scope, no orphan documents, nothing under
    // a blocked path, no history row still pointing at removed content.
    let linked: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM document_scopes WHERE scope_id = ?1",
            [&scope_id],
            |r| r.get(0),
        )
        .map_err(err)?;
    let orphans: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM documents d WHERE NOT EXISTS
                (SELECT 1 FROM document_scopes s WHERE s.document_id = d.document_id)",
            [],
            |r| r.get(0),
        )
        .map_err(err)?;
    let history: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM result_interactions WHERE scope_id = ?1 AND is_tombstone = 0",
            [&scope_id],
            |r| r.get(0),
        )
        .map_err(err)?;
    let mut under_block = 0;
    if let Some(bp) = &blocked_path {
        let mut st = conn
            .prepare("SELECT canonical_path FROM documents")
            .map_err(err)?;
        let paths = st
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        under_block = paths.iter().filter(|p| is_within(Path::new(p), bp)).count();
    }
    if linked != 0 || orphans != 0 || history != 0 || under_block != 0 {
        set_purge(conn, purge_id, "FAILED", "verify")?;
        return Err(IpcError::new(
            ErrorCode::Internal,
            "purge verification failed",
        ));
    }
    let tx = conn.transaction().map_err(err)?;
    tx.execute(
        "UPDATE purge_jobs SET state = 'DONE', step = 'done', verified_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE purge_id = ?1",
        [purge_id],
    )
    .map_err(err)?;
    tx.execute(
        "UPDATE source_scopes SET state = 'REVOKED', file_count = 0,
            revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE scope_id = ?1",
        [&scope_id],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;
    // Push deleted pages out of the WAL file.
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    Ok(())
}

pub fn pending_purges(conn: &Connection) -> Result<Vec<String>, IpcError> {
    let mut st = conn
        .prepare("SELECT purge_id FROM purge_jobs WHERE state != 'DONE' ORDER BY created_at")
        .map_err(err)?;
    let v = st
        .query_map([], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(v)
}
