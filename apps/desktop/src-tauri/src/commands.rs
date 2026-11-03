//! Typed IPC commands (docs/13). Each returns `Result<_, IpcError>`.

use tamgabul_core::error::{ErrorCode, IpcError};
use tamgabul_core::health::AppHealth;
use tamgabul_core::ipc::{self, PingRequest, PingResponse};
use tamgabul_core::settings::{self, AppSettings, SettingsSnapshot};
use tamgabul_platform::PlatformInfo;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;

use crate::{hotkey, tray, windows, AppState};

fn lock_err() -> IpcError {
    IpcError::new(ErrorCode::Internal, "state lock poisoned")
}

#[tauri::command]
pub fn ping(request: PingRequest) -> PingResponse {
    let p = PlatformInfo::current();
    ipc::ping(request, env!("CARGO_PKG_VERSION"), p.os, p.arch)
}

#[tauri::command]
pub fn get_app_health(state: State<'_, AppState>) -> Result<AppHealth, IpcError> {
    crate::models::health(&state)
}

fn snapshot(state: &AppState) -> Result<SettingsSnapshot, IpcError> {
    Ok(SettingsSnapshot {
        settings: state.settings.lock().map_err(|_| lock_err())?.clone(),
        hotkey_active: *state.hotkey_active.lock().map_err(|_| lock_err())?,
        os: PlatformInfo::current().os.to_string(),
    })
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<SettingsSnapshot, IpcError> {
    snapshot(&state)
}

/// Validates, applies side effects (shortcut, autostart, tray language),
/// persists, and broadcasts `settings_changed` to both windows.
#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    next: AppSettings,
) -> Result<SettingsSnapshot, IpcError> {
    next.validate()?;
    let prev = state.settings.lock().map_err(|_| lock_err())?.clone();

    if next.hotkey != prev.hotkey {
        hotkey::unregister_all(&app);
        if let Err(e) = hotkey::register(&app, &next.hotkey) {
            // Restore the previous binding so the user is never left without one.
            let restored = hotkey::register(&app, &prev.hotkey).is_ok();
            *state.hotkey_active.lock().map_err(|_| lock_err())? = restored;
            return Err(IpcError::new(ErrorCode::HotkeyConflict, e));
        }
        *state.hotkey_active.lock().map_err(|_| lock_err())? = true;
    }

    if next.launch_at_startup != prev.launch_at_startup {
        let al = app.autolaunch();
        let r = if next.launch_at_startup {
            al.enable()
        } else {
            al.disable()
        };
        r.map_err(|e| IpcError::new(ErrorCode::Internal, format!("autostart: {e}")))?;
    }

    if next.language != prev.language {
        tray::relabel(&app, next.language);
    }

    {
        let guard = state.db.lock().map_err(|_| lock_err())?;
        if let Some(conn) = guard.as_ref() {
            settings::save(conn, &next)?;
        }
    }
    *state.settings.lock().map_err(|_| lock_err())? = next;

    let snap = snapshot(&state)?;
    let _ = app.emit("settings_changed", snap.clone());
    Ok(snap)
}

#[tauri::command]
pub fn show_app(app: AppHandle, route: Option<String>) {
    windows::show_app(&app, route);
}

#[tauri::command]
pub fn show_widget(app: AppHandle) {
    windows::show_widget(&app);
}

#[tauri::command]
pub fn hide_window(app: AppHandle, label: String) {
    windows::hide(&app, &label);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    tray::quit(&app);
}

/// Contract placeholder for commands whose backend phase has not landed
/// (search, models, scopes, history). The UI shows this state honestly.
#[tauri::command]
pub fn not_implemented(command: String) -> Result<(), IpcError> {
    Err(IpcError::new(
        ErrorCode::NotImplemented,
        format!("{command} arrives with its backend phase"),
    ))
}
