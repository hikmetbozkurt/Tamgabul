//! Platform adapters (docs/10): Windows-first, macOS-parity traits and info.

use std::path::{Path, PathBuf};
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

    pub fn is_macos(&self) -> bool {
        self.os == "macos"
    }

    /// Default widget corner (docs: bottom-right on Windows, top-center on macOS).
    pub fn default_widget_anchor(&self) -> WidgetAnchor {
        match self.os {
            "macos" => WidgetAnchor::TopCenter,
            _ => WidgetAnchor::BottomRight,
        }
    }

    pub fn default_hotkey(&self) -> &'static str {
        if self.is_macos() {
            "Cmd+Shift+Space"
        } else {
            "Ctrl+Shift+Space"
        }
    }

    /// Candidate shortcuts when the primary default is already owned by another app.
    pub fn fallback_hotkeys(&self) -> &'static [&'static str] {
        if self.is_macos() {
            &["Cmd+Shift+Space", "Ctrl+Option+Space", "Cmd+Option+Space"]
        } else {
            &["Ctrl+Shift+Space", "Alt+Shift+Space", "Ctrl+Alt+Space"]
        }
    }

    /// Standard per-user app data folder for SQLite, indexes, and settings (docs/10).
    pub fn app_data_dir_for(&self, home: &Path, app_id: &str) -> PathBuf {
        match self.os {
            "macos" => home
                .join("Library")
                .join("Application Support")
                .join(app_id),
            "windows" => home.join("AppData").join("Roaming").join(app_id),
            _ => home.join(".local").join("share").join(app_id),
        }
    }

    /// Standard per-user diagnostics log directory (docs/10).
    pub fn log_dir_for(&self, home: &Path, app_id: &str) -> PathBuf {
        match self.os {
            "macos" => home.join("Library").join("Logs").join(app_id),
            "windows" => home.join("AppData").join("Local").join(app_id).join("logs"),
            _ => home.join(".local").join("state").join(app_id).join("logs"),
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
pub mod usage;

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
        assert!(!win.is_macos());
        assert!(mac.is_macos());
    }

    #[test]
    fn hotkeys_and_directories_follow_platform() {
        let win = PlatformInfo {
            os: "windows",
            arch: "x86_64",
        };
        let mac = PlatformInfo {
            os: "macos",
            arch: "aarch64",
        };
        let lin = PlatformInfo {
            os: "linux",
            arch: "x86_64",
        };
        assert_eq!(win.default_hotkey(), "Ctrl+Shift+Space");
        assert_eq!(mac.default_hotkey(), "Cmd+Shift+Space");
        assert!(mac.fallback_hotkeys().contains(&"Ctrl+Option+Space"));
        assert!(win.fallback_hotkeys().contains(&"Alt+Shift+Space"));

        let home = Path::new("/home/user");
        assert_eq!(
            mac.app_data_dir_for(home, "dev.tamgabul.app"),
            home.join("Library/Application Support/dev.tamgabul.app")
        );
        assert_eq!(
            mac.log_dir_for(home, "dev.tamgabul.app"),
            home.join("Library/Logs/dev.tamgabul.app")
        );
        assert_eq!(
            win.app_data_dir_for(home, "dev.tamgabul.app"),
            home.join("AppData/Roaming/dev.tamgabul.app")
        );
        assert_eq!(
            lin.app_data_dir_for(home, "dev.tamgabul.app"),
            home.join(".local/share/dev.tamgabul.app")
        );
    }
}
