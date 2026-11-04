//! Settings → Performance and Privacy commands: measured resource use, a
//! redacted diagnostics file, and the app data folder.
//!
//! The diagnostics file is for bug reports. It holds versions, statuses,
//! counts and error codes only: no paths, file names, queries, model hashes
//! or conversation text (AGENTS.md "Do not publish private samples...").

use std::path::PathBuf;

use serde_json::{json, Value};
use tamgabul_core::dto::{GpuUsage, ResourceUsage};
use tamgabul_core::error::{ErrorCode, IpcError};
use tamgabul_core::registry;
use tamgabul_platform::{usage, PlatformInfo};
use tamgabul_runtime::locate;
use tauri::{AppHandle, Manager, State};

use crate::AppState;

fn lock_err() -> IpcError {
    IpcError::new(ErrorCode::Internal, "state lock poisoned")
}

#[tauri::command]
pub async fn resource_usage(app: AppHandle) -> Result<ResourceUsage, IpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let pids: Vec<u32> = state
            .models
            .snapshots()
            .into_iter()
            .filter(|s| s.status.is_loaded())
            .filter_map(|s| s.pid)
            .collect();
        let measured: Vec<u64> = pids
            .iter()
            .filter_map(|p| usage::process_memory_mb(*p))
            .collect();
        ResourceUsage {
            models_ram_mb: (!pids.is_empty() && measured.len() == pids.len())
                .then(|| measured.iter().sum::<u64>() as u32),
            running_models: pids.len() as u32,
            gpu: usage::gpu_memory().map(|g| GpuUsage {
                name: g.name,
                used_mb: g.used_mb as u32,
                total_mb: g.total_mb as u32,
            }),
        }
    })
    .await
    .map_err(|_| IpcError::new(ErrorCode::Internal, "usage task failed"))
}

fn code_str(v: impl serde::Serialize) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// Everything in the file, built from live state. Pure data for review.
fn diagnostics(app: &AppHandle) -> Result<Value, IpcError> {
    let state = app.state::<AppState>();
    let p = PlatformInfo::current();
    let settings = state.settings.lock().map_err(|_| lock_err())?.clone();
    let runtime = locate::find_llama_server(None)
        .ok()
        .map(|f| locate::version(&f.path).unwrap_or_else(|_| "unknown".into()));

    let guard = state.db.lock().map_err(|_| lock_err())?;
    let (models, scopes, db_version) = match guard.as_ref() {
        Some(conn) => {
            let models: Vec<Value> = registry::active(conn)?
                .iter()
                .map(|r| {
                    let snap = state.models.snapshot(&r.model_id);
                    json!({
                        "category": r.category,
                        "name": r.display_name,
                        "source": r.provider,
                        "architecture": r.architecture,
                        "runtime": format!("{} {}", r.runtime_adapter, r.adapter_version),
                        "capabilities": r.capabilities,
                        "ttl": r.ttl,
                        "status": snap.as_ref().map(|s| code_str(s.status)).unwrap_or_else(|| code_str(r.status)),
                        "last_error": snap.as_ref().and_then(|s| s.last_error.as_ref()).map(|e| code_str(e.code())),
                        "loads_this_session": snap.as_ref().map(|s| s.loads).unwrap_or(0),
                    })
                })
                .collect();
            let mut st = conn
                .prepare(
                    "SELECT state, COUNT(*), SUM(file_count), SUM(error_summary IS NOT NULL)
                     FROM source_scopes GROUP BY state",
                )
                .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
            let scopes: Vec<Value> = st
                .query_map([], |r| {
                    Ok(json!({
                        "state": r.get::<_, String>(0)?,
                        "folders": r.get::<_, i64>(1)?,
                        "files": r.get::<_, Option<i64>>(2)?,
                        "with_error": r.get::<_, i64>(3)?,
                    }))
                })
                .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
                .filter_map(Result::ok)
                .collect();
            let v: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            (models, scopes, Some(v))
        }
        None => (Vec::new(), Vec::new(), None),
    };
    drop(guard);

    let docs = state.db_path.as_deref().and_then(|p| {
        let conn = tamgabul_core::db::open_worker(p).ok()?;
        let mut st = conn
            .prepare(
                "SELECT file_type, parse_status, COALESCE(parse_error, ''), COUNT(*)
                 FROM documents GROUP BY 1, 2, 3 ORDER BY 1, 2, 3",
            )
            .ok()?;
        let rows: Vec<Value> = st
            .query_map([], |r| {
                Ok(json!({
                    "type": r.get::<_, String>(0)?,
                    "status": r.get::<_, String>(1)?,
                    "reason": r.get::<_, String>(2)?,
                    "count": r.get::<_, i64>(3)?,
                }))
            })
            .ok()?
            .filter_map(Result::ok)
            .collect();
        Some(rows)
    });

    // What each kind of file was indexed into (captions, OCR, vectors).
    let chunks = state.db_path.as_deref().and_then(|p| {
        let conn = tamgabul_core::db::open_worker(p).ok()?;
        let mut st = conn
            .prepare(
                "SELECT d.file_type, c.modality, COUNT(*), COUNT(DISTINCT c.document_id),
                        SUM(EXISTS(SELECT 1 FROM embedding_records e WHERE e.chunk_id = c.chunk_id))
                 FROM chunks c JOIN documents d ON d.document_id = c.document_id
                 GROUP BY 1, 2 ORDER BY 1, 2",
            )
            .ok()?;
        let rows: Vec<Value> = st
            .query_map([], |r| {
                Ok(json!({
                    "type": r.get::<_, String>(0)?,
                    "modality": r.get::<_, String>(1)?,
                    "chunks": r.get::<_, i64>(2)?,
                    "files": r.get::<_, i64>(3)?,
                    "with_vector": r.get::<_, i64>(4)?,
                }))
            })
            .ok()?
            .filter_map(Result::ok)
            .collect();
        Some(rows)
    });

    Ok(json!({
        "tamgabul_diagnostics": 1,
        "note": "No file paths, file names, queries or conversation text are included.",
        "app_version": env!("CARGO_PKG_VERSION"),
        "os": p.os,
        "arch": p.arch,
        "llama_server": runtime,
        "db_schema_version": db_version,
        "settings": {
            "language": settings.language,
            "history_enabled": settings.history_enabled,
            "history_retention": settings.history_retention,
            "launch_at_startup": settings.launch_at_startup,
            "hotkey_active": *state.hotkey_active.lock().map_err(|_| lock_err())?,
        },
        "ocr_available": tamgabul_platform::media::ocr_supported(),
        "models": models,
        "folders": scopes,
        "documents": docs,
        "chunks": chunks,
        "generation": state.db_path.as_deref().and_then(|p| {
            let conn = tamgabul_core::db::open_worker(p).ok()?;
            tamgabul_core::index::store::current_generation(&conn).ok().flatten().map(|(id, _)| id)
        }),
    }))
}

/// Write the diagnostics JSON to a file the user picked in a save dialog.
#[tauri::command]
pub async fn export_diagnostics(app: AppHandle, path: String) -> Result<(), IpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(path);
        if path.as_os_str().is_empty() {
            return Err(IpcError::new(ErrorCode::Internal, "no file chosen"));
        }
        let v = diagnostics(&app)?;
        let text = serde_json::to_string_pretty(&v)
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
        std::fs::write(&path, text)
            .map_err(|_| IpcError::new(ErrorCode::Internal, "could not write the file"))
    })
    .await
    .map_err(|_| IpcError::new(ErrorCode::Internal, "diagnostics task failed"))?
}

/// Show the folder that holds the index, history and settings.
#[tauri::command]
pub fn open_data_folder(state: State<'_, AppState>) -> Result<(), IpcError> {
    let dir = state
        .app_data_dir
        .clone()
        .ok_or_else(|| IpcError::new(ErrorCode::Internal, "no data folder"))?;
    let fail = |_| IpcError::new(ErrorCode::Internal, "could not open the folder");
    #[cfg(windows)]
    std::process::Command::new("explorer.exe")
        .arg(&dir)
        .spawn()
        .map_err(fail)?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&dir)
        .spawn()
        .map_err(fail)?;
    #[cfg(all(unix, not(target_os = "macos")))]
    std::process::Command::new("xdg-open")
        .arg(&dir)
        .spawn()
        .map_err(fail)?;
    Ok(())
}
