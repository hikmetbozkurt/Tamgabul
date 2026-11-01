//! Two surfaces (docs/24 D1): compact `widget` and expanded `app`.
//! Only one is visible at a time; closing either hides it to the tray (ADR 010).

use tamgabul_core::settings::WidgetPosition;
use tamgabul_platform::{PlatformInfo, WidgetAnchor};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow, WindowEvent};

use crate::AppState;

pub const WIDGET: &str = "widget";
pub const APP: &str = "app";

const MARGIN: i32 = 16;

fn window(app: &AppHandle, label: &str) -> Option<WebviewWindow> {
    app.get_webview_window(label)
}

/// Place the widget bottom-right (Windows) or top-center (macOS) of the
/// monitor work area, unless the user chose "last position".
fn anchor_widget(app: &AppHandle, w: &WebviewWindow) {
    let keep_last = app
        .try_state::<AppState>()
        .map(|s| s.settings.lock().unwrap().widget_position == WidgetPosition::LastPosition)
        .unwrap_or(false);
    if keep_last {
        return;
    }
    let Ok(Some(monitor)) = w.primary_monitor() else {
        return;
    };
    let Ok(size) = w.outer_size() else { return };
    let area = monitor.work_area();
    let (ax, ay) = (area.position.x, area.position.y);
    let (aw, ah) = (area.size.width as i32, area.size.height as i32);
    let (ww, wh) = (size.width as i32, size.height as i32);
    let pos = match PlatformInfo::current().default_widget_anchor() {
        WidgetAnchor::BottomRight => {
            PhysicalPosition::new(ax + aw - ww - MARGIN, ay + ah - wh - MARGIN)
        }
        WidgetAnchor::TopCenter => PhysicalPosition::new(ax + (aw - ww) / 2, ay + MARGIN),
    };
    let _ = w.set_position(pos);
}

pub fn show_widget(app: &AppHandle) {
    if let Some(a) = window(app, APP) {
        let _ = a.hide();
    }
    if let Some(w) = window(app, WIDGET) {
        anchor_widget(app, &w);
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Opens the expanded app (optionally on a route) and hides the widget (docs/24 decision).
pub fn show_app(app: &AppHandle, route: Option<String>) {
    if let Some(w) = window(app, WIDGET) {
        let _ = w.hide();
    }
    if let Some(a) = window(app, APP) {
        if let Some(r) = route {
            let _ = a.emit_to(APP, "navigate", r);
        }
        let _ = a.show();
        let _ = a.unminimize();
        let _ = a.set_focus();
    }
}

pub fn toggle_widget(app: &AppHandle) {
    let app_visible = window(app, APP)
        .and_then(|a| a.is_visible().ok())
        .unwrap_or(false);
    if app_visible {
        if let Some(a) = window(app, APP) {
            let _ = a.set_focus();
        }
        return;
    }
    match window(app, WIDGET) {
        Some(w) if w.is_visible().unwrap_or(false) => {
            let _ = w.hide();
        }
        _ => show_widget(app),
    }
}

pub fn hide(app: &AppHandle, label: &str) {
    if let Some(w) = window(app, label) {
        let _ = w.hide();
    }
}

/// X on any window hides it; only tray "Quit" exits.
pub fn install_close_to_hide(app: &AppHandle) {
    for label in [WIDGET, APP] {
        if let Some(w) = window(app, label) {
            let handle = w.clone();
            w.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = handle.hide();
                }
            });
        }
    }
}
