//! Search, preview, open and history commands (P2/P3, docs/08, docs/13).
//!
//! Queries run on a separate SQLite connection so the UI never waits on a
//! scan. The query is embedded with EmbeddingGemma 2 (started on demand,
//! ADR 019); if the model can't run, word + file-name search still answers.
//! Every result is re-authorized before it is shown or opened.

use std::path::Path;
use std::time::Duration;

use tamgabul_core::db;
use tamgabul_core::dto::{
    HistorySession, HistoryTurn, ResultPreview, SearchKind, SearchRequest, SearchResponse,
};
use tamgabul_core::error::{ErrorCode, IpcError};
use tamgabul_core::history::{self, ShownResult};
use tamgabul_core::index::store;
use tamgabul_core::model::{ModelCategory, ModelStatus};
use tamgabul_core::registry;
use tamgabul_core::search::{self, Acl};
use tamgabul_runtime::client::Client;
use tamgabul_runtime::eg2;
use tauri::{AppHandle, Manager, State};

use crate::AppState;

const RESULT_LIMIT: usize = 10;

fn no_db() -> IpcError {
    IpcError::new(ErrorCode::DbMigrationFailed, "database unavailable")
}

fn worker_conn(state: &AppState) -> Result<rusqlite::Connection, IpcError> {
    db::open_worker(state.db_path.as_deref().ok_or_else(no_db)?)
}

/// Query vector, or `None` when no usable embedding model is registered or
/// it failed to start (search then uses words and file names only).
fn embed_query(state: &AppState, conn: &rusqlite::Connection, text: &str) -> Option<Vec<f32>> {
    let model = registry::active(conn)
        .ok()?
        .into_iter()
        .find(|m| m.category == ModelCategory::Embedding)
        .filter(|m| m.status.from_persisted_snapshot() == ModelStatus::Available)?;
    if state.models.snapshot(&model.model_id).is_none() {
        crate::models::attach(&state.models, &model);
    }
    let lease = state.models.acquire(&model.model_id).ok()?;
    let c = Client::new(lease.endpoint().clone(), Duration::from_secs(60));
    let mut v = c
        .embed_texts(&[eg2::query_prompt(eg2::Task::SearchResult, text)])
        .ok()?
        .pop()?;
    if v.len() != eg2::NATIVE_DIM || !eg2::is_valid_vector(&v) {
        return None;
    }
    eg2::normalize(&mut v);
    Some(v)
}

fn run_search(app: &AppHandle, req: SearchRequest) -> Result<SearchResponse, IpcError> {
    let state = app.state::<AppState>();
    let text = req.text.trim().to_string();
    if text.is_empty() {
        return Err(IpcError::new(ErrorCode::Internal, "empty query"));
    }
    let history_on = state
        .settings
        .lock()
        .map(|s| s.history_enabled)
        .unwrap_or(true);
    let mut conn = worker_conn(&state)?;
    let generation = store::current_generation(&conn)?.map(|(id, _)| id);
    let acl = Acl::load(&conn)?;
    let scope_count = store::visible_scopes(&conn)?
        .iter()
        .filter(|s| s.state.is_authorized())
        .count() as u32;

    let qv = if generation.is_some() && !acl.is_empty() {
        embed_query(&state, &conn, &text)
    } else {
        None
    };
    let hits = search::search(
        &conn,
        generation.as_deref(),
        qv.as_deref(),
        &text,
        RESULT_LIMIT,
    )?;
    let terms = search::terms(&text);
    let mut cards = Vec::new();
    let mut shown = Vec::new();
    for h in &hits {
        if let Some(r) = search::resolve(
            &conn,
            &acl,
            &h.document_id,
            h.chunk_id,
            h.reason.as_str(),
            &terms,
        )? {
            shown.push(ShownResult {
                hit: h,
                scope_id: acl.scope_of(&r.path).map(|s| s.scope_id.clone()),
                title: r.card.title.clone(),
            });
            cards.push(r.card);
        }
    }
    let kind = if cards.is_empty() {
        SearchKind::NotFound
    } else {
        SearchKind::Results
    };
    let session = history::ensure_session(&conn, req.session_id.as_deref(), history_on)?;
    let turn = history::record_search(
        &mut conn,
        &session,
        &text,
        kind.clone(),
        &shown,
        generation.as_deref(),
    )?;
    Ok(SearchResponse {
        session_id: session,
        turn_id: turn,
        kind,
        count: cards.len() as u32,
        choices: Vec::new(),
        results: cards,
        // No LLM yet: answers come from retrieval only.
        degraded_llm: true,
        searched_scope_count: scope_count,
    })
}

#[tauri::command]
pub async fn search(app: AppHandle, req: SearchRequest) -> Result<SearchResponse, IpcError> {
    tauri::async_runtime::spawn_blocking(move || run_search(&app, req))
        .await
        .map_err(|_| IpcError::new(ErrorCode::Internal, "search task failed"))?
}

fn resolve_ref(state: &AppState, result_ref: &str) -> Result<search::Resolved, IpcError> {
    let (doc, chunk) = search::parse_ref(result_ref)
        .ok_or_else(|| IpcError::new(ErrorCode::SourceChanged, "unknown result"))?;
    let conn = worker_conn(state)?;
    let acl = Acl::load(&conn)?;
    search::resolve(&conn, &acl, &doc, chunk, "semantic", &[])?
        .ok_or_else(|| IpcError::new(ErrorCode::ScopeRevoked, "result is no longer available"))
}

#[tauri::command]
pub async fn preview_result(app: AppHandle, result_ref: String) -> Result<ResultPreview, IpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let r = resolve_ref(&state, &result_ref)?;
        Ok(ResultPreview {
            body: r.chunk_text.unwrap_or_default(),
            result: r.card,
        })
    })
    .await
    .map_err(|_| IpcError::new(ErrorCode::Internal, "preview task failed"))?
}

fn open_path(path: &Path, reveal: bool) -> Result<(), IpcError> {
    let fail = |_| IpcError::new(ErrorCode::Internal, "could not open");
    #[cfg(windows)]
    {
        let mut c = std::process::Command::new("explorer.exe");
        if reveal {
            let mut arg = std::ffi::OsString::from("/select,");
            arg.push(path.as_os_str());
            c.arg(arg);
        } else {
            c.arg(path);
        }
        c.spawn().map_err(fail)?;
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = std::process::Command::new("open");
        if reveal {
            c.arg("-R");
        }
        c.arg(path).spawn().map_err(fail)?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let target = if reveal {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map_err(fail)?;
    }
    Ok(())
}

/// Open (or show in folder) after re-checking the grant and the file.
#[tauri::command]
pub fn open_result(
    state: State<'_, AppState>,
    result_ref: String,
    reveal: bool,
) -> Result<(), IpcError> {
    let r = resolve_ref(&state, &result_ref)?;
    if !r.path.exists() {
        return Err(IpcError::new(
            ErrorCode::SourceChanged,
            "file moved or deleted",
        ));
    }
    open_path(&r.path, reveal)?;
    if let Some((doc, _)) = search::parse_ref(&result_ref) {
        let conn = worker_conn(&state)?;
        let _ = history::mark_opened(&conn, &doc);
    }
    Ok(())
}

#[tauri::command]
pub fn list_history(state: State<'_, AppState>) -> Result<Vec<HistorySession>, IpcError> {
    history::list_sessions(&worker_conn(&state)?)
}

#[tauri::command]
pub async fn session_turns(
    app: AppHandle,
    session_id: String,
) -> Result<Vec<HistoryTurn>, IpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        history::session_turns(&worker_conn(&state)?, &session_id)
    })
    .await
    .map_err(|_| IpcError::new(ErrorCode::Internal, "history task failed"))?
}

#[tauri::command]
pub fn delete_session(state: State<'_, AppState>, session_id: String) -> Result<(), IpcError> {
    history::delete_session(&worker_conn(&state)?, &session_id)
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) -> Result<(), IpcError> {
    history::clear_all(&worker_conn(&state)?)
}

/// App start: forget sessions recorded while history was off.
pub fn init(state: &AppState) {
    if let Ok(conn) = worker_conn(state) {
        let _ = history::purge_unpersisted(&conn);
    }
}
