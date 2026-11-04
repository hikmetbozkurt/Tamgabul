//! Folder commands (P1, docs/07, docs/13): preview → grant → scan; revoke.
//!
//! Scans run on their own thread with their own SQLite connection, so the UI
//! never waits on them. EmbeddingGemma 2 is held only for the duration of a
//! scan; afterwards it follows its idle policy (ADR 019). Errors leaving here
//! carry a code and a fixed hint, never a path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tamgabul_core::db;
use tamgabul_core::describe::{self, Describer};
use tamgabul_core::dto::{IndexStats, RevokeRequest, ScopePreview, ScopeStatus, ScopeSummary};
use tamgabul_core::error::{ErrorCode, IpcError};
use tamgabul_core::index::job::{self, Embedder, JobInput, Outcome, Progress};
use tamgabul_core::index::media::Platform;
use tamgabul_core::index::store::{self, GenerationSpec, Scope, ScopeState};
use tamgabul_core::index::walk::{self, Limits};
use tamgabul_core::model::{ModelCategory, ModelStatus};
use tamgabul_core::registry::{self, ModelRecord};
use tamgabul_runtime::client::{ChatMessage, Client};
use tamgabul_runtime::{eg2, Lease};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::AppState;

/// Files per folder scan (docs/07 caps). Preview stops earlier.
pub const SCAN_CAP: usize = 200_000;
const PREVIEW_CAP: usize = 10_000;
const PREVIEW_TIME: Duration = Duration::from_secs(4);
const PREVIEW_TTL: Duration = Duration::from_secs(15 * 60);
const EMBED_BATCH: usize = 16;

struct Preview {
    canonical: PathBuf,
    display: String,
    is_drive_root: bool,
    at: Instant,
}

#[derive(Default)]
pub struct ScopesState {
    previews: Mutex<HashMap<String, Preview>>,
    jobs: Mutex<HashMap<String, Arc<AtomicBool>>>,
    progress: Mutex<HashMap<String, Progress>>,
}

fn lock_err() -> IpcError {
    IpcError::new(ErrorCode::Internal, "state lock poisoned")
}

fn no_db() -> IpcError {
    IpcError::new(ErrorCode::DbMigrationFailed, "database unavailable")
}

fn with_db<T>(
    state: &AppState,
    f: impl FnOnce(&mut rusqlite::Connection) -> Result<T, IpcError>,
) -> Result<T, IpcError> {
    let mut g = state.db.lock().map_err(|_| lock_err())?;
    f(g.as_mut().ok_or_else(no_db)?)
}

fn notify(app: &AppHandle) {
    let _ = app.emit("scope_status", ());
}

/// Folders a scan never enters: the app's own data and model folders.
fn deny_paths(state: &AppState, conn: &rusqlite::Connection) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = &state.app_data_dir {
        if let Ok(c) = walk::canonical(d) {
            v.push(c);
        }
    }
    for m in registry::active(conn).unwrap_or_default() {
        let p = Path::new(&m.path);
        let dir = if p.is_file() { p.parent() } else { Some(p) };
        if let Some(c) = dir.and_then(|d| walk::canonical(d).ok()) {
            v.push(c);
        }
    }
    v
}

fn summary(
    state: &AppState,
    conn: &rusqlite::Connection,
    all: &[Scope],
    s: &Scope,
) -> ScopeSummary {
    let running = state
        .scopes
        .jobs
        .lock()
        .map(|j| j.contains_key(&s.scope_id))
        .unwrap_or(false);
    let prog = state
        .scopes
        .progress
        .lock()
        .ok()
        .and_then(|p| p.get(&s.scope_id).copied());
    let error_code: Option<ErrorCode> = s
        .error_summary
        .as_deref()
        .and_then(|e| serde_json::from_value(serde_json::Value::String(e.into())).ok());
    let status = match s.state {
        ScopeState::Revoking | ScopeState::Revoked => ScopeStatus::Revoking,
        _ if running => ScopeStatus::Scanning,
        ScopeState::Paused => ScopeStatus::Paused,
        _ if !s.canonical_path.exists() => ScopeStatus::Unreachable,
        ScopeState::Error => ScopeStatus::Error,
        _ if error_code.is_some() => ScopeStatus::Error,
        _ => ScopeStatus::UpToDate,
    };
    ScopeSummary {
        scope_id: s.scope_id.clone(),
        display_path: s.display_path.clone(),
        status,
        watching: store::is_watching(conn, &s.scope_id).unwrap_or(false),
        file_count: Some(
            prog.filter(|_| running)
                .map(|p| p.processed)
                .unwrap_or(s.file_count),
        ),
        last_scan_at: s.last_scan_at.clone(),
        progress: prog.filter(|_| running).map(|p| {
            if p.total == 0 {
                0.0
            } else {
                p.processed as f64 / p.total as f64
            }
        }),
        include_label: None,
        exclusion_count: store::exclusion_count(conn, &s.scope_id).unwrap_or(0),
        is_drive_root: s.is_drive_root,
        parent_scope_id: store::parent_of(all, s).map(|p| p.scope_id.clone()),
        error_code,
    }
}

#[tauri::command]
pub fn list_scopes(state: State<'_, AppState>) -> Result<Vec<ScopeSummary>, IpcError> {
    with_db(&state, |conn| {
        let all = store::visible_scopes(conn)?;
        Ok(all.iter().map(|s| summary(&state, conn, &all, s)).collect())
    })
}

#[tauri::command]
pub fn index_stats(state: State<'_, AppState>) -> Result<IndexStats, IpcError> {
    with_db(&state, |conn| {
        let scopes = store::visible_scopes(conn)?
            .iter()
            .filter(|s| s.state.is_authorized())
            .count() as u32;
        let bytes: i64 = conn
            .query_row(
                "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        Ok(IndexStats {
            scope_count: scopes,
            file_count: Some(store::indexed_file_count(conn)?),
            index_mb: Some(bytes as f64 / (1024.0 * 1024.0)),
        })
    })
}

// ---------------------------------------------------------------- preview / grant

#[tauri::command]
pub async fn preview_scope(app: AppHandle, path: String) -> Result<ScopePreview, IpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let raw = PathBuf::from(path.trim());
        if walk::is_network_path(&raw) {
            return Err(IpcError::new(
                ErrorCode::ScopeNotAuthorized,
                "network folders are not supported yet",
            ));
        }
        let canonical = walk::canonical(&raw)?;
        if !canonical.is_dir() {
            return Err(IpcError::new(ErrorCode::ScopeNotAuthorized, "not a folder"));
        }
        let (deny, parent, existing) = with_db(&state, |conn| {
            let all = store::visible_scopes(conn)?;
            let existing = all
                .iter()
                .any(|s| s.state.is_authorized() && s.canonical_path == canonical);
            let probe = Scope {
                scope_id: String::new(),
                canonical_path: canonical.clone(),
                display_path: String::new(),
                is_drive_root: false,
                state: ScopeState::Active,
                file_count: 0,
                last_scan_at: None,
                error_summary: None,
                max_file_bytes: None,
            };
            let parent = store::parent_of(&all, &probe).map(|p| p.display_path.clone());
            Ok((deny_paths(&state, conn), parent, existing))
        })?;
        if existing {
            return Err(IpcError::new(
                ErrorCode::ScopeNotAuthorized,
                "folder already added",
            ));
        }
        if deny.iter().any(|d| walk::is_within(&canonical, d)) {
            return Err(IpcError::new(
                ErrorCode::ScopeNotAuthorized,
                "app data or model folder",
            ));
        }
        let report = walk::walk(
            &canonical,
            &deny,
            Limits {
                max_files: PREVIEW_CAP,
                deadline: Some(Instant::now() + PREVIEW_TIME),
                ..Limits::default()
            },
            &AtomicBool::new(false),
        );
        let mut auto = report.excluded_names.clone();
        if report.skipped_links > 0 {
            auto.push("links / junctions".into());
        }
        if report.skipped_cloud > 0 {
            auto.push("online-only files".into());
        }
        let token = store::new_id("t", &canonical.to_string_lossy());
        let display = walk::display(&canonical);
        let is_drive_root = walk::is_drive_root(&canonical);
        {
            let mut p = state.scopes.previews.lock().map_err(|_| lock_err())?;
            p.retain(|_, v| v.at.elapsed() < PREVIEW_TTL);
            p.insert(
                token.clone(),
                Preview {
                    canonical,
                    display: display.clone(),
                    is_drive_root,
                    at: Instant::now(),
                },
            );
        }
        // Text + FTS trigram + one 768-float vector per ~1000 characters.
        let est = report.total_bytes as f64 * 3.0
            + (report.total_bytes as f64 / 1000.0 + report.files.len() as f64) * 3072.0;
        Ok(ScopePreview {
            preview_token: token,
            display_path: display,
            is_drive_root,
            file_count: report.files.len() as u32,
            est_index_mb: est / (1024.0 * 1024.0),
            cap_files: SCAN_CAP as u32,
            auto_excludes: auto,
            parent_display_path: parent,
            complete: report.complete,
        })
    })
    .await
    .map_err(|_| IpcError::new(ErrorCode::Internal, "preview task failed"))?
}

#[tauri::command]
pub fn grant_scope(
    app: AppHandle,
    state: State<'_, AppState>,
    token: String,
) -> Result<ScopeSummary, IpcError> {
    let p = state
        .scopes
        .previews
        .lock()
        .map_err(|_| lock_err())?
        .remove(&token)
        .filter(|p| p.at.elapsed() < PREVIEW_TTL)
        .ok_or_else(|| {
            IpcError::new(
                ErrorCode::ScopeNotAuthorized,
                "preview expired, preview again",
            )
        })?;
    let scope = Scope {
        scope_id: store::new_id("s", &p.canonical.to_string_lossy()),
        canonical_path: p.canonical,
        display_path: p.display,
        is_drive_root: p.is_drive_root,
        state: ScopeState::Active,
        file_count: 0,
        last_scan_at: None,
        error_summary: None,
        max_file_bytes: None,
    };
    // Settings "Indexing: watch new folders" turns watching on at grant.
    let watch_new = state
        .settings
        .lock()
        .map(|s| s.indexing_mode == tamgabul_core::settings::IndexingMode::WhenIdle)
        .unwrap_or(false);
    let s = with_db(&state, |conn| {
        store::insert_scope(conn, &scope)?;
        if watch_new {
            store::set_watching(conn, &scope.scope_id, true)?;
        }
        let all = store::visible_scopes(conn)?;
        Ok(summary(&state, conn, &all, &scope))
    })?;
    if watch_new {
        crate::watch::sync(&app);
    }
    // The user confirmed "Allow and scan".
    start_job(&app, &scope.scope_id)?;
    notify(&app);
    Ok(s)
}

#[tauri::command]
pub fn start_index(
    app: AppHandle,
    state: State<'_, AppState>,
    scope_id: String,
) -> Result<(), IpcError> {
    with_db(&state, |conn| {
        let s = store::get_scope(conn, &scope_id)?
            .ok_or_else(|| IpcError::new(ErrorCode::ScopeRevoked, "unknown folder"))?;
        match s.state {
            ScopeState::Active => Ok(()),
            // "Scan now" on a paused folder resumes it.
            ScopeState::Paused | ScopeState::Error => {
                store::set_scope_state(conn, &scope_id, ScopeState::Active, None)
            }
            _ => Err(IpcError::new(ErrorCode::ScopeRevoked, "folder was removed")),
        }
    })?;
    start_job(&app, &scope_id)?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub fn cancel_index(state: State<'_, AppState>, scope_id: String) -> Result<(), IpcError> {
    if let Some(c) = state
        .scopes
        .jobs
        .lock()
        .map_err(|_| lock_err())?
        .get(&scope_id)
    {
        c.store(true, Ordering::SeqCst);
    }
    Ok(())
}

pub(crate) fn is_scanning(state: &AppState, scope_id: &str) -> bool {
    state
        .scopes
        .jobs
        .lock()
        .map(|j| j.contains_key(scope_id))
        .unwrap_or(false)
}

/// Watching on: changes in the folder are picked up by a delta scan after a
/// quiet period (`watch.rs`). Turning it on also scans once to catch up.
#[tauri::command]
pub fn set_watching(
    app: AppHandle,
    state: State<'_, AppState>,
    scope_id: String,
    on: bool,
) -> Result<(), IpcError> {
    with_db(&state, |conn| store::set_watching(conn, &scope_id, on))?;
    crate::watch::sync(&app);
    if on {
        start_job(&app, &scope_id)?;
    }
    notify(&app);
    Ok(())
}

/// A folder that was moved or renamed: point it at its new place and keep
/// its index. Only for a folder whose old place is gone, only to a folder
/// the user picked, and only when most indexed files are found there with
/// the same relative path and size. Then a delta scan runs.
#[tauri::command]
pub async fn relink_scope(
    app: AppHandle,
    scope_id: String,
    path: String,
) -> Result<ScopeSummary, IpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let raw = PathBuf::from(path.trim());
        if walk::is_network_path(&raw) {
            return Err(IpcError::new(
                ErrorCode::ScopeNotAuthorized,
                "network folders are not supported yet",
            ));
        }
        let new_root = walk::canonical(&raw)?;
        if !new_root.is_dir() {
            return Err(IpcError::new(ErrorCode::ScopeNotAuthorized, "not a folder"));
        }
        let summary_out = with_db(&state, |conn| {
            let scope = store::get_scope(conn, &scope_id)?
                .ok_or_else(|| IpcError::new(ErrorCode::ScopeRevoked, "unknown folder"))?;
            if scope.canonical_path.exists() {
                return Err(IpcError::new(
                    ErrorCode::ScopeNotAuthorized,
                    "the folder is still at its old place",
                ));
            }
            let deny = deny_paths(&state, conn);
            if deny.iter().any(|d| walk::is_within(&new_root, d)) {
                return Err(IpcError::new(
                    ErrorCode::ScopeNotAuthorized,
                    "app data or model folder",
                ));
            }
            let all = store::visible_scopes(conn)?;
            let overlaps = all.iter().any(|o| {
                o.scope_id != scope_id
                    && o.state.is_authorized()
                    && (walk::is_within(&new_root, &o.canonical_path)
                        || walk::is_within(&o.canonical_path, &new_root)
                        || walk::is_within(&o.canonical_path, &scope.canonical_path))
            });
            if overlaps {
                return Err(IpcError::new(
                    ErrorCode::ScopeNotAuthorized,
                    "overlaps another folder; remove and add it instead",
                ));
            }
            let check = store::relink_check(conn, &scope, &new_root)?;
            if !check.accepted() {
                return Err(IpcError::new(
                    ErrorCode::SourceChanged,
                    "this does not look like the same folder",
                ));
            }
            store::relink_scope(conn, &scope_id, &new_root, &walk::display(&new_root))?;
            let all = store::visible_scopes(conn)?;
            let s = store::get_scope(conn, &scope_id)?
                .ok_or_else(|| IpcError::new(ErrorCode::ScopeRevoked, "unknown folder"))?;
            Ok(summary(&state, conn, &all, &s))
        })?;
        crate::watch::sync(&app);
        start_job(&app, &scope_id)?;
        notify(&app);
        Ok(summary_out)
    })
    .await
    .map_err(|_| IpcError::new(ErrorCode::Internal, "relink task failed"))?
}

// ---------------------------------------------------------------- revoke

#[tauri::command]
pub fn revoke_scope(
    app: AppHandle,
    state: State<'_, AppState>,
    req: RevokeRequest,
) -> Result<(), IpcError> {
    if let Some(c) = state
        .scopes
        .jobs
        .lock()
        .map_err(|_| lock_err())?
        .get(&req.scope_id)
    {
        c.store(true, Ordering::SeqCst);
    }
    // Deny first, synchronously: from here on nothing of it is shown.
    let purge = with_db(&state, |conn| {
        store::begin_revoke(conn, &req.scope_id, req.mode, req.purge_history)
    })?;
    crate::watch::sync(&app);
    notify(&app);
    if let Some(purge_id) = purge {
        let app = app.clone();
        std::thread::spawn(move || {
            run_purges(&app, vec![purge_id]);
        });
    }
    Ok(())
}

fn run_purges(app: &AppHandle, ids: Vec<String>) {
    let state = app.state::<AppState>();
    let Some(path) = state.db_path.clone() else {
        return;
    };
    let Ok(mut conn) = db::open_worker(&path) else {
        return;
    };
    for id in ids {
        if let Err(e) = store::run_purge(&mut conn, &id) {
            eprintln!("purge failed: {:?}", e.code);
        }
    }
    notify(app);
}

// ---------------------------------------------------------------- scan job

/// EmbeddingGemma 2 through the lifecycle manager. The lease is taken on the
/// first batch (the model starts then) and dropped with the embedder.
struct Eg2Embedder {
    app: AppHandle,
    model_id: String,
    lease: Option<Lease>,
    client: Option<Client>,
    props: Option<tamgabul_runtime::client::Props>,
}

impl Eg2Embedder {
    fn client(&mut self) -> Result<&Client, IpcError> {
        if self.client.is_none() {
            let state = self.app.state::<AppState>();
            let lease = state
                .models
                .acquire(&self.model_id)
                .map_err(crate::models::ipc)?;
            let c = Client::new(lease.endpoint().clone(), Duration::from_secs(180));
            self.props = Some(c.props().map_err(crate::models::ipc)?);
            self.client = Some(c);
            self.lease = Some(lease);
        }
        Ok(self.client.as_ref().expect("client set above"))
    }
}

impl Embedder for Eg2Embedder {
    fn embed_documents(
        &mut self,
        title: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, IpcError> {
        if self.client.is_none() {
            let state = self.app.state::<AppState>();
            let lease = state
                .models
                .acquire(&self.model_id)
                .map_err(crate::models::ipc)?;
            self.client = Some(Client::new(
                lease.endpoint().clone(),
                Duration::from_secs(180),
            ));
            self.lease = Some(lease);
        }
        let c = self.client.as_ref().expect("client set above");
        let inputs: Vec<String> = texts
            .iter()
            .map(|t| eg2::document_prompt(Some(title), t))
            .collect();
        let mut out = c.embed_texts(&inputs).map_err(crate::models::ipc)?;
        for v in &mut out {
            if v.len() != eg2::NATIVE_DIM || !eg2::is_valid_vector(v) {
                return Err(IpcError::new(ErrorCode::Internal, "invalid embedding"));
            }
            eg2::normalize(v);
        }
        Ok(out)
    }

    fn supports_vision(&mut self) -> bool {
        self.client().is_ok() && self.props.as_ref().is_some_and(|p| p.vision)
    }

    fn supports_audio(&mut self) -> bool {
        self.client().is_ok() && self.props.as_ref().is_some_and(|p| p.audio)
    }

    fn embed_media(&mut self, bytes: &[u8]) -> Result<Vec<f32>, IpcError> {
        let marker = self
            .props
            .as_ref()
            .map(|p| p.media_marker.clone())
            .unwrap_or_else(|| tamgabul_runtime::client::DEFAULT_MEDIA_MARKER.into());
        let c = self.client()?;
        match c.embed_media(&marker, &marker, &[bytes]) {
            Ok(mut v) => {
                if v.len() != eg2::NATIVE_DIM || !eg2::is_valid_vector(&v) {
                    return Err(IpcError::new(
                        ErrorCode::ModelIncompatible,
                        "invalid image embedding",
                    ));
                }
                eg2::normalize(&mut v);
                Ok(v)
            }
            // The server answered but could not use this file (corrupt,
            // unsupported variant): skip the image, keep scanning.
            Err(tamgabul_runtime::RuntimeError::Request(m)) if m.starts_with("HTTP ") => Err(
                IpcError::new(ErrorCode::ModelIncompatible, "image not decodable"),
            ),
            Err(e) => Err(crate::models::ipc(e)),
        }
    }
}

/// OCR, extra image codecs and PDF page rendering from the OS
/// (`tamgabul_platform::media`; Windows today).
struct OsMedia;

impl Platform for OsMedia {
    fn decode_to_png(&self, bytes: &[u8], max_side: u32) -> Option<Vec<u8>> {
        tamgabul_platform::media::decode_to_png(bytes, max_side).ok()
    }
    fn ocr(&self, png: &[u8]) -> Option<Result<String, String>> {
        match tamgabul_platform::media::ocr(png) {
            Ok(t) => Some(Ok(t.text())),
            Err(tamgabul_platform::media::MediaError::Unsupported) => None,
            Err(e) => Some(Err(e.to_string())),
        }
    }
    fn ocr_available(&self) -> bool {
        tamgabul_platform::media::ocr_supported()
    }
    fn video_frame(&self, path: &Path) -> Option<Vec<u8>> {
        tamgabul_platform::media::video_frame(path, 1536).ok()
    }
    fn render_pdf_pages(
        &self,
        pdf: &[u8],
        pages: &[u32],
        width: u32,
    ) -> Option<Vec<(u32, Vec<u8>)>> {
        let mut out = Vec::new();
        tamgabul_platform::media::pdf_render_pages(pdf, pages, width, &mut |i, png| {
            out.push((i, png))
        })
        .ok()?;
        Some(out)
    }
}

pub fn generation_spec(m: &ModelRecord) -> GenerationSpec {
    GenerationSpec {
        embedding_model_id: m.model_id.clone(),
        model_hash: m.weights_sha256.clone().unwrap_or_else(|| "unknown".into()),
        model_revision: m.model_revision.clone().unwrap_or_else(|| "local".into()),
        adapter_version: format!("{} {}", m.runtime_adapter, m.adapter_version),
        preprocessing: "v3: text/markup/office/rtf chunks 1000/1600 chars with lines; pdf text per page + OCR of scanned pages; images normalized to PNG ≤1536 px + OCR; audio 16 kHz mono 30 s segments; tg_fold word index".into(),
        query_instruction: eg2::query_prompt(eg2::Task::SearchResult, "{query}"),
        document_instruction: eg2::document_prompt(Some("{title}"), "{content}"),
        dimension: eg2::NATIVE_DIM as u32,
        normalization: "l2".into(),
        modalities: {
            let mut v = vec!["text".to_string()];
            for k in ["image", "audio"] {
                if m.capabilities.iter().any(|c| c == k) {
                    v.push(k.to_string());
                }
            }
            v
        },
    }
}

/// Image descriptions with the registered LLM when its vision projector is
/// present (ADR 021). The worker is started on the first image and held for
/// the scan, then follows its idle policy.
struct LlmDescriber {
    app: AppHandle,
    model: Option<registry::ModelRecord>,
    vision: Option<bool>,
    lease: Option<Lease>,
    client: Option<Client>,
}

impl LlmDescriber {
    fn new(app: &AppHandle, conn: &rusqlite::Connection) -> Self {
        let model = registry::active(conn)
            .ok()
            .and_then(|v| v.into_iter().find(|m| m.category == ModelCategory::Llm))
            .filter(|m| m.status.from_persisted_snapshot() == ModelStatus::Available)
            // Settings → Models: image descriptions can be turned off.
            .filter(|_| {
                app.state::<AppState>()
                    .settings
                    .lock()
                    .map(|s| s.llm_caption)
                    .unwrap_or(true)
            });
        Self {
            app: app.clone(),
            model,
            vision: None,
            lease: None,
            client: None,
        }
    }
}

impl Describer for LlmDescriber {
    fn available(&mut self) -> bool {
        if self.vision.is_none() {
            // Decided from the files on disk: no load needed to know.
            self.vision = Some(self.model.as_ref().is_some_and(|m| {
                tamgabul_runtime::llm::locate(Path::new(&m.path)).is_ok_and(|f| f.mmproj.is_some())
            }));
        }
        self.vision == Some(true)
    }

    fn describe_image(&mut self, png: &[u8]) -> Result<Option<String>, IpcError> {
        let Some(m) = &self.model else {
            return Ok(None);
        };
        if self.client.is_none() {
            let state = self.app.state::<AppState>();
            if state.models.snapshot(&m.model_id).is_none() {
                crate::models::attach(&state.models, m);
            }
            let lease = state
                .models
                .acquire(&m.model_id)
                .map_err(crate::models::ipc)?;
            self.client = Some(Client::new(
                lease.endpoint().clone(),
                Duration::from_secs(90),
            ));
            self.lease = Some(lease);
        }
        let c = self.client.as_ref().expect("client set above");
        let v = c
            .chat_json(
                &[
                    ChatMessage::system(describe::CAPTION_SYSTEM),
                    ChatMessage::user_with_png(describe::CAPTION_USER, png.to_vec()),
                ],
                &describe::caption_schema(),
                400,
            )
            .map_err(crate::models::ipc)?;
        Ok(describe::parse_caption(&v))
    }
}

fn set_error(app: &AppHandle, scope_id: &str, code: ErrorCode) {
    let state = app.state::<AppState>();
    let code = serde_json::to_value(code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned));
    let _ = with_db(&state, |conn| {
        conn.execute(
            "UPDATE source_scopes SET error_summary = ?2 WHERE scope_id = ?1",
            rusqlite::params![scope_id, code],
        )
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
        Ok(())
    });
}

fn scan(app: &AppHandle, scope_id: &str, cancel: &AtomicBool) -> Result<Outcome, IpcError> {
    let state = app.state::<AppState>();
    let path = state.db_path.clone().ok_or_else(no_db)?;
    let mut conn = db::open_worker(&path)?;
    let model = registry::active(&conn)?
        .into_iter()
        .find(|m| m.category == ModelCategory::Embedding)
        .filter(|m| m.status.from_persisted_snapshot() == ModelStatus::Available)
        .ok_or_else(|| IpcError::new(ErrorCode::ModelMissing, "add EmbeddingGemma 2 first"))?;
    if state.models.snapshot(&model.model_id).is_none() {
        crate::models::attach(&state.models, &model);
    }
    let generation = store::ensure_current_generation(&mut conn, &generation_spec(&model))?;
    let deny = deny_paths(&state, &conn);
    let mut embedder = Eg2Embedder {
        app: app.clone(),
        model_id: model.model_id.clone(),
        lease: None,
        client: None,
        props: None,
    };
    let mut describer = LlmDescriber::new(app, &conn);
    let mut last = Instant::now();
    let (outcome, _) = job::run(
        &mut conn,
        JobInput {
            scope_id,
            generation: &generation,
            deny: &deny,
            limits: Limits {
                max_files: SCAN_CAP,
                ..Limits::default()
            },
            batch: EMBED_BATCH,
            platform: &OsMedia,
        },
        &mut embedder,
        &mut describer,
        cancel,
        &mut |p| {
            if let Ok(mut m) = state.scopes.progress.lock() {
                m.insert(scope_id.to_string(), p);
            }
            if last.elapsed() > Duration::from_millis(400) {
                notify(app);
                last = Instant::now();
            }
        },
    )?;
    Ok(outcome)
}

/// Start a scan unless one is already running for this folder.
pub fn start_job(app: &AppHandle, scope_id: &str) -> Result<(), IpcError> {
    let state = app.state::<AppState>();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = state.scopes.jobs.lock().map_err(|_| lock_err())?;
        if jobs.contains_key(scope_id) {
            return Ok(());
        }
        jobs.insert(scope_id.to_string(), cancel.clone());
    }
    let app = app.clone();
    let id = scope_id.to_string();
    std::thread::Builder::new()
        .name("tamgabul-index".into())
        .spawn(move || {
            set_error_clear(&app, &id);
            let result = scan(&app, &id, &cancel);
            if let Err(e) = &result {
                set_error(&app, &id, e.code);
            }
            let state = app.state::<AppState>();
            if let Ok(mut j) = state.scopes.jobs.lock() {
                j.remove(&id);
            }
            if let Ok(mut p) = state.scopes.progress.lock() {
                p.remove(&id);
            }
            notify(&app);
        })
        .map_err(|_| IpcError::new(ErrorCode::Internal, "cannot start scan"))?;
    Ok(())
}

fn set_error_clear(app: &AppHandle, scope_id: &str) {
    let state = app.state::<AppState>();
    let _ = with_db(&state, |conn| {
        conn.execute(
            "UPDATE source_scopes SET error_summary = NULL WHERE scope_id = ?1",
            [scope_id],
        )
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
        Ok(())
    });
}

/// App start: finish interrupted purges, then resume interrupted scans.
pub fn resume(app: &AppHandle) {
    let state = app.state::<AppState>();
    let pending = with_db(&state, |conn| {
        // Watched folders also scan once: they may have changed while the
        // app was closed.
        let mut scans = store::interrupted_scopes(conn)?;
        for s in store::watched_scopes(conn)? {
            if !scans.contains(&s.scope_id) {
                scans.push(s.scope_id);
            }
        }
        Ok((store::pending_purges(conn)?, scans))
    });
    let Ok((purges, scans)) = pending else { return };
    let app = app.clone();
    std::thread::spawn(move || {
        if !purges.is_empty() {
            run_purges(&app, purges);
        }
        for id in scans {
            let _ = start_job(&app, &id);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tamgabul_core::dto::TtlPolicy;

    #[test]
    fn generation_identity_uses_model_card_prompts() {
        let m = ModelRecord {
            model_id: "m".into(),
            category: ModelCategory::Embedding,
            display_name: "EmbeddingGemma 2".into(),
            provider: None,
            path: "x".into(),
            format: "gguf".into(),
            architecture: None,
            weights_sha256: Some("abc".into()),
            model_revision: None,
            license_name: None,
            runtime_adapter: "llama.cpp".into(),
            adapter_version: "b1".into(),
            capabilities: vec![],
            embedding_dim: Some(768),
            status: ModelStatus::Available,
            last_error_code: None,
            validated_at: None,
            ttl: TtlPolicy::Minutes5,
        };
        let g = generation_spec(&m);
        assert_eq!(g.query_instruction, "task: search result | query: {query}");
        assert_eq!(g.document_instruction, "title: {title} | text: {content}");
        assert_eq!(g.dimension, 768);
        let mut m2 = m.clone();
        m2.adapter_version = "b2".into();
        assert_eq!(
            g.fingerprint(),
            generation_spec(&m2).fingerprint(),
            "runtime upgrade keeps the index"
        );
    }
}
