//! User preferences (non-secret), persisted as one JSON value in
//! `app_settings` under the key `ui`. Unknown/missing fields fall back to
//! defaults so older or newer builds never fail to start over settings.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{ErrorCode, IpcError};

pub const SETTINGS_KEY: &str = "ui";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub enum Language {
    En,
    Tr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub enum MicMode {
    PushToTalk,
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub enum WidgetPosition {
    /// Bottom-right on Windows, top-center on macOS.
    PlatformDefault,
    LastPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub enum HistoryRetention {
    Days30,
    Days90,
    Forever,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub enum IndexingMode {
    Manual,
    WhenIdle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub struct AppSettings {
    pub language: Language,
    /// Accelerator string, e.g. "Ctrl+Shift+Space".
    pub hotkey: String,
    pub widget_position: WidgetPosition,
    pub mic_mode: MicMode,
    pub launch_at_startup: bool,
    pub history_enabled: bool,
    pub history_retention: HistoryRetention,
    pub indexing_mode: IndexingMode,
    /// LLM jobs, each on/off (used only when an LLM is set up): understand
    /// the request, pick and answer from the results, describe images
    /// while scanning.
    /// (Named `llm_understand`, not `llm_plan`, so the new default
    /// applies to settings saved before the evaluation.)
    pub llm_understand: bool,
    pub llm_pick: bool,
    pub llm_caption: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: Language::En,
            hotkey: "Ctrl+Shift+Space".into(),
            widget_position: WidgetPosition::PlatformDefault,
            mic_mode: MicMode::PushToTalk,
            launch_at_startup: false,
            history_enabled: true,
            history_retention: HistoryRetention::Days90,
            indexing_mode: IndexingMode::Manual,
            // Off by default: in the evaluation it did not raise accuracy
            // and added ~1 s per search (ADR 025).
            llm_understand: false,
            llm_pick: true,
            llm_caption: true,
        }
    }
}

/// What `get_settings` returns: preferences plus live state the UI must show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub struct SettingsSnapshot {
    pub settings: AppSettings,
    /// False when the OS refused the shortcut (another app owns it).
    pub hotkey_active: bool,
    pub os: String,
}

impl AppSettings {
    pub fn validate(&self) -> Result<(), IpcError> {
        let hk = self.hotkey.trim();
        let parts: Vec<&str> = hk.split('+').map(str::trim).collect();
        let has_key = parts.last().is_some_and(|k| !k.is_empty());
        let has_modifier = parts.len() >= 2;
        if !has_key || !has_modifier {
            return Err(IpcError::new(
                ErrorCode::HotkeyConflict,
                "shortcut needs a modifier and a key",
            ));
        }
        Ok(())
    }
}

fn db_err(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Internal, e.to_string())
}

pub fn load(conn: &Connection) -> Result<AppSettings, IpcError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value_json FROM app_settings WHERE key = ?1",
            params![SETTINGS_KEY],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_err)?;
    Ok(raw
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default())
}

pub fn save(conn: &Connection, settings: &AppSettings) -> Result<(), IpcError> {
    settings.validate()?;
    let json = serde_json::to_string(settings).map_err(db_err)?;
    conn.execute(
        "INSERT INTO app_settings(key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        params![SETTINGS_KEY, json],
    )
    .map_err(db_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_and_migrate;

    #[test]
    fn defaults_are_english() {
        let s = AppSettings::default();
        assert_eq!(s.language, Language::En);
        assert_eq!(s.hotkey, "Ctrl+Shift+Space");
    }

    #[test]
    fn round_trip_through_db() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_and_migrate(&dir.path().join("t.db")).unwrap();
        assert_eq!(load(&conn).unwrap(), AppSettings::default());
        let s = AppSettings {
            language: Language::Tr,
            mic_mode: MicMode::Toggle,
            ..AppSettings::default()
        };
        save(&conn, &s).unwrap();
        assert_eq!(load(&conn).unwrap(), s);
    }

    #[test]
    fn settings_from_older_builds_still_load() {
        // Fields removed later (e.g. old demo flags) are ignored, not fatal.
        let s: AppSettings =
            serde_json::from_str(r#"{"language":"tr","demo_data":true,"demo_scenario":"ready"}"#)
                .unwrap();
        assert_eq!(s.language, Language::Tr);
    }

    #[test]
    fn partial_or_corrupt_json_falls_back_to_defaults() {
        let s: AppSettings = serde_json::from_str(r#"{"language":"tr"}"#).unwrap();
        assert_eq!(s.language, Language::Tr);
        assert_eq!(s.hotkey, "Ctrl+Shift+Space");

        let dir = tempfile::tempdir().unwrap();
        let conn = open_and_migrate(&dir.path().join("t.db")).unwrap();
        conn.execute(
            "INSERT INTO app_settings(key, value_json) VALUES ('ui', '{\"language\":42}')",
            [],
        )
        .unwrap();
        assert_eq!(load(&conn).unwrap(), AppSettings::default());
    }

    #[test]
    fn invalid_hotkey_is_rejected() {
        let s = AppSettings {
            hotkey: "Space".into(),
            ..AppSettings::default()
        };
        assert_eq!(s.validate().unwrap_err().code, ErrorCode::HotkeyConflict);
        let s = AppSettings {
            hotkey: "Ctrl+".into(),
            ..AppSettings::default()
        };
        assert!(s.validate().is_err());
    }
}
