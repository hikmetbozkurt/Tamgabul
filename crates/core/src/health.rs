//! App health derivation (docs/09: global health = required embedding + DB +
//! scope readiness, NOT "all models present").

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::{ModelCategory, ModelStatus, StatusTone};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub enum HealthLevel {
    /// Semantic search usable.
    Ready,
    /// App works, semantic search not usable yet (model on disk but not loaded,
    /// or no folder granted).
    Degraded,
    /// Mandatory embedding missing/invalid or DB unavailable.
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub struct ModelSummary {
    pub category: ModelCategory,
    pub status: ModelStatus,
    pub tone: StatusTone,
    pub required: bool,
    /// User-facing name if a model is registered; never a file path.
    pub display_name: Option<String>,
}

impl ModelSummary {
    pub fn new(category: ModelCategory, status: ModelStatus, display_name: Option<String>) -> Self {
        Self {
            category,
            status,
            tone: status.tone(category),
            required: category.is_required(),
            display_name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub struct AppHealth {
    pub api_version: u32,
    pub level: HealthLevel,
    pub db_ok: bool,
    pub semantic_search_enabled: bool,
    pub active_scope_count: u32,
    /// Always 4 entries in fixed order: embedding, llm, stt, tts.
    pub models: Vec<ModelSummary>,
}

/// Pure inputs so the rule can be unit-tested without a database.
#[derive(Debug, Clone)]
pub struct HealthInputs {
    pub db_ok: bool,
    pub active_scope_count: u32,
    pub embedding: ModelSummary,
    pub llm: ModelSummary,
    pub stt: ModelSummary,
    pub tts: ModelSummary,
}

pub fn derive_health(inputs: HealthInputs) -> AppHealth {
    let emb = inputs.embedding.status;
    let semantic = inputs.db_ok && emb.is_loaded();

    let level = if !inputs.db_ok || inputs.embedding.tone == StatusTone::Red {
        HealthLevel::Blocked
    } else if semantic && inputs.active_scope_count > 0 {
        HealthLevel::Ready
    } else {
        HealthLevel::Degraded
    };

    AppHealth {
        api_version: crate::IPC_API_VERSION,
        level,
        db_ok: inputs.db_ok,
        semantic_search_enabled: semantic,
        active_scope_count: inputs.active_scope_count,
        models: vec![inputs.embedding, inputs.llm, inputs.stt, inputs.tts],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(emb: ModelStatus, scopes: u32, db_ok: bool) -> HealthInputs {
        HealthInputs {
            db_ok,
            active_scope_count: scopes,
            embedding: ModelSummary::new(ModelCategory::Embedding, emb, None),
            llm: ModelSummary::new(ModelCategory::Llm, ModelStatus::Missing, None),
            stt: ModelSummary::new(ModelCategory::Stt, ModelStatus::Missing, None),
            tts: ModelSummary::new(ModelCategory::Tts, ModelStatus::Missing, None),
        }
    }

    #[test]
    fn zero_models_is_blocked_and_search_disabled() {
        let h = derive_health(inputs(ModelStatus::Missing, 0, true));
        assert_eq!(h.level, HealthLevel::Blocked);
        assert!(!h.semantic_search_enabled);
        assert_eq!(h.models[0].tone, StatusTone::Red);
        assert!(h.models[1..].iter().all(|m| m.tone == StatusTone::Neutral));
    }

    #[test]
    fn available_embedding_is_degraded_not_ready() {
        let h = derive_health(inputs(ModelStatus::Available, 3, true));
        assert_eq!(h.level, HealthLevel::Degraded);
        assert!(!h.semantic_search_enabled);
    }

    #[test]
    fn ready_embedding_without_scope_is_degraded() {
        let h = derive_health(inputs(ModelStatus::Ready, 0, true));
        assert_eq!(h.level, HealthLevel::Degraded);
        assert!(h.semantic_search_enabled);
    }

    #[test]
    fn ready_embedding_and_scope_is_ready() {
        let h = derive_health(inputs(ModelStatus::Idle, 1, true));
        assert_eq!(h.level, HealthLevel::Ready);
    }

    #[test]
    fn optional_models_missing_do_not_block() {
        let h = derive_health(inputs(ModelStatus::Ready, 1, true));
        assert_eq!(h.level, HealthLevel::Ready);
    }

    #[test]
    fn db_failure_blocks_even_with_ready_embedding() {
        let h = derive_health(inputs(ModelStatus::Ready, 1, false));
        assert_eq!(h.level, HealthLevel::Blocked);
        assert!(!h.semantic_search_enabled);
    }

    #[test]
    fn incompatible_embedding_blocks() {
        let h = derive_health(inputs(ModelStatus::Incompatible, 1, true));
        assert_eq!(h.level, HealthLevel::Blocked);
    }
}
