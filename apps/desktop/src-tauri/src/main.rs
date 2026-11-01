// Release builds on Windows: no extra console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod hotkey;
mod tray;
mod windows;

use std::sync::Mutex;

use rusqlite::Connection;
use tamgabul_core::{db, settings, settings::AppSettings};
use tauri::Manager;
use tauri_plugin_window_state::StateFlags;

pub struct AppState {
    /// `None` when the DB failed to open/migrate; health then reports Blocked.
    pub db: Mutex<Option<Connection>>,
    pub settings: Mutex<AppSettings>,
    pub hotkey_active: Mutex<bool>,
}

fn open_db(app: &tauri::App) -> Option<Connection> {
    let dir = app.path().app_data_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
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
            let conn = open_db(app);
            let loaded = conn
                .as_ref()
                .and_then(|c| settings::load(c).ok())
                .unwrap_or_default();
            app.manage(AppState {
                db: Mutex::new(conn),
                settings: Mutex::new(loaded.clone()),
                hotkey_active: Mutex::new(false),
            });

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
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Tamgabul");
}
