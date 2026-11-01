//! Global shortcut that toggles the widget. Registration failure means another
//! app owns the combination; the UI then shows a conflict and offers rebinding.

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::windows;

/// "Ctrl+Shift+Space" → plugin accelerator ("Control+Shift+Space").
pub fn to_accelerator(hotkey: &str) -> String {
    hotkey
        .split('+')
        .map(|p| match p.trim() {
            "Ctrl" | "ctrl" => "Control".to_string(),
            "Cmd" | "cmd" | "Win" | "win" => "Super".to_string(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

pub fn register(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let shortcut: Shortcut = to_accelerator(hotkey)
        .parse()
        .map_err(|e| format!("invalid shortcut: {e}"))?;
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _sc, event| {
            if event.state() == ShortcutState::Pressed {
                windows::toggle_widget(app);
            }
        })
        .map_err(|e| e.to_string())
}

pub fn unregister_all(app: &AppHandle) {
    let _ = app.global_shortcut().unregister_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_ui_names_to_accelerators() {
        assert_eq!(to_accelerator("Ctrl+Shift+Space"), "Control+Shift+Space");
        assert_eq!(to_accelerator("Win + K"), "Super+K");
        assert!(to_accelerator("Ctrl+Shift+Space")
            .parse::<Shortcut>()
            .is_ok());
    }
}
