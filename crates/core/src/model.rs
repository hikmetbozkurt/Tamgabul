//! Model categories and lifecycle status (AGENTS.md, docs/04, docs/05).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub enum ModelCategory {
    Embedding,
    Llm,
    Stt,
    Tts,
}

impl ModelCategory {
    /// Only the embedding model (EmbeddingGemma 2) is mandatory (ADR 004).
    pub fn is_required(self) -> bool {
        matches!(self, ModelCategory::Embedding)
    }

    pub fn as_db_str(self) -> &'static str {
        match self {
            ModelCategory::Embedding => "embedding",
            ModelCategory::Llm => "llm",
            ModelCategory::Stt => "stt",
            ModelCategory::Tts => "tts",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub enum ModelStatus {
    Missing,
    Registered,
    Validating,
    Available,
    Loading,
    Ready,
    Busy,
    Idle,
    Unloading,
    Error,
    Incompatible,
}

/// Visual tone of a status. Always paired with a text label in the UI
/// (status is never color-only — docs/09).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub enum StatusTone {
    /// Loaded and healthy.
    Green,
    /// On disk / in progress, not running.
    Amber,
    /// Mandatory model missing, invalid, or a real runtime error.
    Red,
    /// Optional model not configured.
    Neutral,
}

impl ModelStatus {
    pub fn parse_db(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
    }

    /// Loaded in a runner and healthy.
    pub fn is_loaded(self) -> bool {
        matches!(
            self,
            ModelStatus::Ready | ModelStatus::Busy | ModelStatus::Idle
        )
    }

    /// Validated and on disk, loaded or not. Models start on demand (ADR 019),
    /// so an AVAILABLE embedding model can serve the next query.
    pub fn is_usable(self) -> bool {
        matches!(
            self,
            ModelStatus::Available
                | ModelStatus::Loading
                | ModelStatus::Ready
                | ModelStatus::Busy
                | ModelStatus::Idle
                | ModelStatus::Unloading
        )
    }

    /// Persisted status is only a snapshot. After restart nothing is loaded,
    /// so live states fall back to AVAILABLE until a runner health check
    /// passes (docs/12 "Storage boundaries", J10).
    pub fn from_persisted_snapshot(self) -> Self {
        match self {
            ModelStatus::Loading
            | ModelStatus::Ready
            | ModelStatus::Busy
            | ModelStatus::Idle
            | ModelStatus::Unloading => ModelStatus::Available,
            other => other,
        }
    }

    pub fn tone(self, category: ModelCategory) -> StatusTone {
        match self {
            s if s.is_loaded() => StatusTone::Green,
            ModelStatus::Registered
            | ModelStatus::Validating
            | ModelStatus::Available
            | ModelStatus::Loading
            | ModelStatus::Unloading => StatusTone::Amber,
            ModelStatus::Error | ModelStatus::Incompatible => StatusTone::Red,
            ModelStatus::Missing if category.is_required() => StatusTone::Red,
            _ => StatusTone::Neutral,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [ModelStatus; 11] = [
        ModelStatus::Missing,
        ModelStatus::Registered,
        ModelStatus::Validating,
        ModelStatus::Available,
        ModelStatus::Loading,
        ModelStatus::Ready,
        ModelStatus::Busy,
        ModelStatus::Idle,
        ModelStatus::Unloading,
        ModelStatus::Error,
        ModelStatus::Incompatible,
    ];

    #[test]
    fn db_strings_round_trip_for_every_status() {
        for s in ALL {
            let text = serde_json::to_value(s).unwrap();
            assert_eq!(ModelStatus::parse_db(text.as_str().unwrap()), Some(s));
        }
        assert_eq!(ModelStatus::parse_db("ready"), None);
    }

    #[test]
    fn no_green_after_restart() {
        for s in ALL {
            let after = s.from_persisted_snapshot();
            assert_ne!(
                after.tone(ModelCategory::Embedding),
                StatusTone::Green,
                "{s:?}"
            );
        }
    }

    #[test]
    fn missing_required_is_red_missing_optional_is_neutral() {
        assert_eq!(
            ModelStatus::Missing.tone(ModelCategory::Embedding),
            StatusTone::Red
        );
        for c in [ModelCategory::Llm, ModelCategory::Stt, ModelCategory::Tts] {
            assert_eq!(ModelStatus::Missing.tone(c), StatusTone::Neutral);
        }
    }

    #[test]
    fn available_is_never_green() {
        assert_eq!(
            ModelStatus::Available.tone(ModelCategory::Llm),
            StatusTone::Amber
        );
        assert_eq!(
            ModelStatus::Registered.tone(ModelCategory::Embedding),
            StatusTone::Amber
        );
        assert_eq!(
            ModelStatus::Incompatible.tone(ModelCategory::Tts),
            StatusTone::Red
        );
        assert_eq!(
            ModelStatus::Ready.tone(ModelCategory::Llm),
            StatusTone::Green
        );
    }
}
