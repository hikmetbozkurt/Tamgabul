//! Platform adapters.
//!
//! P0-01 ships only traits and a placeholder tray host. The real Windows
//! notification-area icon, close-to-hide and global hotkey are P5-01
//! (`agent_tasks/06_WINDOWS_UI.md`); macOS menu bar comes after that.

use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlatformInfo {
    pub os: &'static str,
    pub arch: &'static str,
}

impl PlatformInfo {
    pub fn current() -> Self {
        Self {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        }
    }

    /// Default widget corner (docs: bottom-right on Windows, top-center on macOS).
    pub fn default_widget_anchor(&self) -> WidgetAnchor {
        match self.os {
            "macos" => WidgetAnchor::TopCenter,
            _ => WidgetAnchor::BottomRight,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetAnchor {
    BottomRight,
    TopCenter,
}

/// Tray menu commands fixed by docs/09 (functional contract, ADR 010).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    OpenWidget,
    Settings,
    ToggleIndexing,
    UnloadModels,
    Quit,
}

impl TrayCommand {
    pub const MENU_ORDER: [TrayCommand; 5] = [
        TrayCommand::OpenWidget,
        TrayCommand::Settings,
        TrayCommand::ToggleIndexing,
        TrayCommand::UnloadModels,
        TrayCommand::Quit,
    ];

    /// i18n key used by the UI / native menu label.
    pub fn label_key(self) -> &'static str {
        match self {
            TrayCommand::OpenWidget => "tray.open_widget",
            TrayCommand::Settings => "tray.settings",
            TrayCommand::ToggleIndexing => "tray.toggle_indexing",
            TrayCommand::UnloadModels => "tray.unload_models",
            TrayCommand::Quit => "tray.quit",
        }
    }
}

/// Abstraction over Windows notification area / macOS menu bar.
pub trait TrayHost: Send + Sync {
    fn install(&self) -> Result<(), String>;
    fn is_installed(&self) -> bool;
    fn remove(&self);
}

/// Placeholder used until P5. Records state only; draws nothing.
#[derive(Default)]
pub struct PlaceholderTray {
    installed: Mutex<bool>,
}

impl TrayHost for PlaceholderTray {
    fn install(&self) -> Result<(), String> {
        *self.installed.lock().map_err(|e| e.to_string())? = true;
        Ok(())
    }
    fn is_installed(&self) -> bool {
        self.installed.lock().map(|g| *g).unwrap_or(false)
    }
    fn remove(&self) {
        if let Ok(mut g) = self.installed.lock() {
            *g = false;
        }
    }
}

pub mod media;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_menu_has_contract_order_and_quit_last() {
        assert_eq!(TrayCommand::MENU_ORDER.len(), 5);
        assert_eq!(*TrayCommand::MENU_ORDER.last().unwrap(), TrayCommand::Quit);
        assert_eq!(TrayCommand::Quit.label_key(), "tray.quit");
    }

    #[test]
    fn placeholder_tray_install_remove() {
        let t = PlaceholderTray::default();
        assert!(!t.is_installed());
        t.install().unwrap();
        assert!(t.is_installed());
        t.remove();
        assert!(!t.is_installed());
    }

    #[test]
    fn anchors_follow_platform() {
        let win = PlatformInfo {
            os: "windows",
            arch: "x86_64",
        };
        let mac = PlatformInfo {
            os: "macos",
            arch: "aarch64",
        };
        assert_eq!(win.default_widget_anchor(), WidgetAnchor::BottomRight);
        assert_eq!(mac.default_widget_anchor(), WidgetAnchor::TopCenter);
    }
}
