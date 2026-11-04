// Release builds on Windows: no extra console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod hotkey;
mod models;
mod scopes;
mod search_cmd;
mod system;
mod tray;
mod watch;
mod windows;

use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde::Serialize;
use tamgabul_core::{db, model::ModelStatus, settings, settings::AppSettings};
use tamgabul_runtime::{manager::Listener, ModelManager, ModelSnapshot};
use tauri::{AppHandle, Emitter, Manager, RunEvent};
use tauri_plugin_window_state::StateFlags;

pub struct AppState {
    /// `None` when the DB failed to open/migrate; health then reports Blocked.
    pub db: Mutex<Option<Connection>>,
    pub settings: Mutex<AppSettings>,
    pub hotkey_active: Mutex<bool>,
    /// Single lifecycle authority for model workers (docs/05). Nothing is
    /// started here: workers launch on demand and stop when idle.
    pub models: Arc<ModelManager>,
    /// Add-model validation in progress / waiting for Save.
    pub model_ops: models::ModelsState,
    /// Database file, for background jobs that open their own connection.
    pub db_path: Option<std::path::PathBuf>,
    /// Never scanned as a folder (docs/07).
    pub app_data_dir: Option<std::path::PathBuf>,
    pub scopes: scopes::ScopesState,
    pub watch: watch::WatchState,
}

/// `model_status` event payload. No paths, no model input.
#[derive(Serialize, Clone)]
struct ModelStatusEvent {
    model_id: String,
    status: ModelStatus,
    active: u32,
    unload_in_secs: Option<u64>,
    error_code: Option<tamgabul_core::error::ErrorCode>,
}

fn model_listener(app: AppHandle) -> Listener {
    Arc::new(move |s: &ModelSnapshot| {
        let _ = app.emit(
            "model_status",
            ModelStatusEvent {
                model_id: s.id.clone(),
                status: s.status,
                active: s.active,
                unload_in_secs: s.unload_in.map(|d| d.as_secs()),
                error_code: s.last_error.as_ref().map(|e| e.code()),
            },
        );
    })
}

/// Stop every model worker and wait until the processes are gone.
pub fn stop_models(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.models.shutdown();
    }
}

fn open_db(dir: Option<&std::path::Path>) -> Option<Connection> {
    let dir = dir?;
    std::fs::create_dir_all(dir).ok()?;
    match db::open_and_migrate(&dir.join("tamgabul.db")) {
        Ok(conn) => Some(conn),
        Err(e) => {
            // Code only: no paths or user content in logs (AGENTS.md).
            eprintln!("database unavailable: {:?}", e.code);
            None
        }
    }
}

fn main() {
    tauri::Builder::default()
        // Must be the first plugin: a second launch focuses the existing UI.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            windows::show_widget(app);
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_window_state::Builder::new()
                // The widget is always re-anchored; only the app window keeps its frame.
                .with_denylist(&[windows::WIDGET])
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED)
                .build(),
        )
        .setup(|app| {
            let data_dir = app.path().app_data_dir().ok();
            let conn = open_db(data_dir.as_deref());
            let db_path = conn
                .as_ref()
                .and(data_dir.as_ref())
                .map(|d| d.join("tamgabul.db"));
            let loaded = conn
                .as_ref()
                .and_then(|c| settings::load(c).ok())
                .unwrap_or_default();
            app.manage(AppState {
                db: Mutex::new(conn),
                settings: Mutex::new(loaded.clone()),
                hotkey_active: Mutex::new(false),
                models: Arc::new(ModelManager::new(Some(model_listener(
                    app.handle().clone(),
                )))),
                model_ops: models::ModelsState::default(),
                db_path,
                app_data_dir: data_dir,
                scopes: scopes::ScopesState::default(),
                watch: watch::WatchState::default(),
            });
            // Make registered models known to the manager. Nothing is loaded.
            models::init(&app.state::<AppState>());
            // Finish interrupted purges first, then resume interrupted scans.
            scopes::resume(app.handle());
            search_cmd::init(&app.state::<AppState>());
            // Watched folders: follow changes from now on.
            watch::start(app.handle());

            let active = hotkey::register(app.handle(), &loaded.hotkey).is_ok();
            *app.state::<AppState>().hotkey_active.lock().unwrap() = active;

            tray::install(app.handle(), loaded.language)?;
            windows::install_close_to_hide(app.handle());
            windows::show_widget(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::get_app_health,
            commands::get_settings,
            commands::set_settings,
            commands::show_app,
            commands::show_widget,
            commands::hide_window,
            commands::quit_app,
            commands::not_implemented,
            models::list_models,
            models::validate_model,
            models::cancel_validation,
            models::register_model,
            models::load_model,
            models::unload_model,
            models::unload_all_models,
            models::set_model_ttl,
            models::remove_model,
            scopes::list_scopes,
            scopes::index_stats,
            scopes::preview_scope,
            scopes::grant_scope,
            scopes::start_index,
            scopes::cancel_index,
            scopes::set_watching,
            scopes::relink_scope,
            scopes::revoke_scope,
            search_cmd::search,
            search_cmd::preview_result,
            search_cmd::open_result,
            search_cmd::list_history,
            search_cmd::session_turns,
            search_cmd::delete_session,
            search_cmd::clear_history,
            system::resource_usage,
            system::export_diagnostics,
            system::open_data_folder,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Tamgabul")
        .run(|app, event| {
            // Any exit path (tray Quit, OS logoff, last-resort exit) ends the
            // workers. On Windows the Job Object also covers crashes.
            if let RunEvent::Exit = event {
                stop_models(app);
            }
        });
}
