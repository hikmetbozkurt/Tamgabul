//! Search, preview, open and history commands (P2/P3, docs/08, docs/13).
//!
//! Queries run on a separate SQLite connection so the UI never waits on a
//! scan. A request is first understood (rules, plus the optional small LLM:
//! docs/08), then embedded with EmbeddingGemma 2 (both started on demand,
//! ADR 019). If a model can't run, the rest still answers. Every result is
//! re-authorized before it is shown or opened.

use std::path::Path;
use std::time::Duration;

use tamgabul_core::db;
use tamgabul_core::dto::{
    HistorySession, HistoryTurn, ResultPreview, SearchRequest, SearchResponse,
};
use tamgabul_core::engine::{self, Query, QueryModels};
use tamgabul_core::error::{ErrorCode, IpcError};
use tamgabul_core::history::{self, Answer, ShownResult};
use tamgabul_core::index::store;
use tamgabul_core::model::{ModelCategory, ModelStatus};
use tamgabul_core::registry::{self, ModelRecord};
use tamgabul_core::search::{self, Acl};
use tamgabul_runtime::client::{ChatMessage, Client};
use tamgabul_runtime::eg2;
use tauri::{AppHandle, Manager, State};

use crate::AppState;

fn no_db() -> IpcError {
    IpcError::new(ErrorCode::DbMigrationFailed, "database unavailable")
}

fn worker_conn(state: &AppState) -> Result<rusqlite::Connection, IpcError> {
    db::open_worker(state.db_path.as_deref().ok_or_else(no_db)?)
}

fn active_model(conn: &rusqlite::Connection, category: ModelCategory) -> Option<ModelRecord> {
    registry::active(conn)
        .ok()?
        .into_iter()
        .find(|m| m.category == category)
        .filter(|m| m.status.from_persisted_snapshot() == ModelStatus::Available)
}

/// Query vector, or `None` when the embedding model can't run (search then
/// uses words and file names only).
fn embed_query(state: &AppState, model: &ModelRecord, text: &str) -> Option<Vec<f32>> {
    if state.models.snapshot(&model.model_id).is_none() {
        crate::models::attach(&state.models, model);
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

/// The registered LLM, unless its last start failed: a broken runtime must
/// not add a failed load to every search. "Retry" on the Models page clears
/// that.
fn usable_llm(state: &AppState, conn: &rusqlite::Connection) -> Option<ModelRecord> {
    let m = active_model(conn, ModelCategory::Llm)?;
    if !Path::new(&m.path).exists() {
        return None;
    }
    match state.models.snapshot(&m.model_id) {
        Some(s) if s.status == ModelStatus::Error => None,
        Some(_) => Some(m),
        None => {
            crate::models::attach(&state.models, &m);
            Some(m)
        }
    }
}

const LLM_TIMEOUT: Duration = Duration::from_secs(20);

/// The app's models for one search: started on demand through the
/// ModelManager (ADR 019); any failure answers `None` and the search goes
/// on without that model.
struct AppModels<'a> {
    state: &'a AppState,
    embedding: Option<ModelRecord>,
    llm: Option<ModelRecord>,
}

impl QueryModels for AppModels<'_> {
    fn embed_query(&self, text: &str) -> Option<Vec<f32>> {
        embed_query(self.state, self.embedding.as_ref()?, text)
    }
    fn has_llm(&self) -> bool {
        self.llm.is_some()
    }
    fn llm_json(
        &self,
        system: &str,
        user: &str,
        schema: &serde_json::Value,
        max_tokens: u32,
    ) -> Option<serde_json::Value> {
        let m = self.llm.as_ref()?;
        let lease = self.state.models.acquire(&m.model_id).ok()?;
        let c = Client::new(lease.endpoint().clone(), LLM_TIMEOUT);
        c.chat_json(
            &[ChatMessage::system(system), ChatMessage::user(user)],
            schema,
            max_tokens,
        )
        .ok()
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
    apply_retention(&state, &conn);
    let generation = store::current_generation(&conn)?.map(|(id, _)| id);
    let acl = Acl::load(&conn)?;
    let scope_count = store::visible_scopes(&conn)?
        .iter()
        .filter(|s| s.state.is_authorized())
        .count() as u32;
    let ctx = history::context(&conn, req.session_id.as_deref())?;

    // A picked clarification option completes the request it answered, and
    // never triggers a second question in a row.
    let picked = req
        .choice_id
        .as_deref()
        .is_some_and(|c| c.starts_with("opt:"));
    let query = match (&ctx.last_user, picked) {
        (Some(prev), true) => format!("{prev} {text}"),
        _ => text.clone(),
    };
    let searchable = !acl.is_empty();
    let models = AppModels {
        state: &state,
        embedding: generation
            .as_ref()
            .filter(|_| searchable)
            .and_then(|_| active_model(&conn, ModelCategory::Embedding)),
        llm: if searchable {
            usable_llm(&state, &conn)
        } else {
            None
        },
    };
    let out = engine::run(
        &conn,
        generation.as_deref(),
        &models,
        &Query {
            text: &query,
            ctx: &ctx,
            allow_clarify: !picked && !ctx.last_was_clarify,
            now: now_unix(),
            utc_offset: req.utc_offset_min.unwrap_or(0).clamp(-14 * 60, 14 * 60) as i64 * 60,
        },
    )?;

    let shown: Vec<ShownResult> = out
        .found
        .iter()
        .map(|f| ShownResult {
            hit: &f.hit,
            scope_id: acl.scope_of(&f.resolved.path).map(|s| s.scope_id.clone()),
            title: f.resolved.card.title.clone(),
        })
        .collect();
    let session = history::ensure_session(&conn, req.session_id.as_deref(), history_on)?;
    let turn = history::record_search(
        &mut conn,
        &session,
        &text,
        &Answer {
            kind: out.kind.clone(),
            results: &shown,
            plan: &out.plan,
            choices: &out.choices,
            direct: out.direct.as_ref(),
            summary: out.summary.as_ref(),
            picked_by_llm: out.picked_by_llm,
            notice: out.notice.as_deref(),
        },
        generation.as_deref(),
    )?;
    let cards: Vec<_> = out.found.into_iter().map(|f| f.resolved.card).collect();
    Ok(SearchResponse {
        session_id: session,
        turn_id: turn,
        kind: out.kind,
        count: cards.len() as u32,
        choices: out.choices,
        results: cards,
        question: out.plan.question.clone(),
        understood: out.plan.understood(),
        answer: out.direct,
        summary: out.summary,
        picked_by_llm: out.picked_by_llm,
        notice: out.notice,
        degraded_llm: !out.plan.by_llm,
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

/// App start: forget sessions recorded while history was off, and apply
/// the retention window.
pub fn init(state: &AppState) {
    if let Ok(conn) = worker_conn(state) {
        let _ = history::purge_unpersisted(&conn);
        apply_retention(state, &conn);
    }
}

/// Settings → Privacy → keep history for N days. Runs at start, when the
/// setting changes and before each search (the app can stay open for weeks).
pub(crate) fn apply_retention(state: &AppState, conn: &rusqlite::Connection) {
    let retention = state.settings.lock().map(|s| s.history_retention);
    if let Ok(r) = retention {
        let _ = history::apply_retention(conn, r);
    }
}
