//! IPC data shapes for search, models, folders and history (docs/13, docs/24 §7).
//!
//! These are the contract the UI is built against. Until a backend phase
//! (P1–P4) lands, its Rust commands answer `NOT_IMPLEMENTED` and the UI shows
//! that state honestly — there is no demo or sample data. Field names are the
//! wire format.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ErrorCode;
use crate::model::{ModelCategory, ModelStatus, StatusTone};
use crate::understand::{DateFilter, FileTypeFilter};

macro_rules! ts_export {
    ($($item:item)*) => {
        $(
            #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
            #[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
            $item
        )*
    };
}

// ---------------------------------------------------------------- search

ts_export! {
    #[serde(rename_all = "snake_case")]
    pub enum SearchKind { Results, Clarify, NotFound }

    #[serde(rename_all = "snake_case")]
    pub enum FileKind { Markdown, Text, Pdf, Document, Image, Audio, Other }

    pub struct SearchRequest {
        pub session_id: Option<String>,
        pub text: String,
        /// Set when the user picks a clarification choice instead of typing.
        pub choice_id: Option<String>,
        /// Minutes east of UTC on the user's clock, for "today"/"last week".
        #[serde(default)]
        pub utc_offset_min: Option<i32>,
    }

    /// What the request was understood as, shown under the answer so the
    /// user can see (and correct) filters and added words.
    pub struct Understood {
        pub file_type: FileTypeFilter,
        pub date: DateFilter,
        pub exclude: Vec<String>,
        /// Words the LLM added (base forms, synonyms, translations).
        pub added_terms: Vec<String>,
        pub by_llm: bool,
    }

    pub struct Choice {
        pub id: String,
        pub label: String,
        pub count: u32,
    }

    pub struct ResultCard {
        /// Opaque; resolved and re-authorized server-side before opening.
        pub result_ref: String,
        pub title: String,
        pub display_path: String,
        pub project: Option<String>,
        pub file_kind: FileKind,
        pub snippet: Option<String>,
        /// Substrings of `snippet` to emphasise.
        pub highlights: Vec<String>,
        /// RFC 3339.
        pub modified_at: String,
        /// Why it matched, e.g. "previous_result", "semantic", "ocr".
        pub match_reason: String,
    }

    pub struct SearchResponse {
        pub session_id: String,
        pub turn_id: String,
        pub kind: SearchKind,
        pub count: u32,
        pub choices: Vec<Choice>,
        pub results: Vec<ResultCard>,
        /// Clarifying question (kind = clarify), in the user's language.
        pub question: Option<String>,
        pub understood: Option<Understood>,
        /// True when no LLM helped with this request (none registered, or
        /// it failed); rules-only understanding was used.
        pub degraded_llm: bool,
        /// Folders searched, for the "not found" message.
        pub searched_scope_count: u32,
    }

    pub struct ResultPreview {
        pub result: ResultCard,
        /// Plain-text excerpt; never rendered as HTML.
        pub body: String,
    }
}

// ---------------------------------------------------------------- models

ts_export! {
    #[serde(rename_all = "snake_case")]
    /// How long a loaded model may stay idle. `Never` = keep loaded (opt-in),
    /// `Immediately` = stop right after each job (docs/05).
    pub enum TtlPolicy { Immediately, Minutes5, Minutes10, Minutes30, Hours1, Never }

    #[serde(rename_all = "snake_case")]
    pub enum StepState { Pending, Running, Ok, Failed }

    pub struct ModelEntry {
        pub model_id: String,
        pub category: ModelCategory,
        pub display_name: String,
        /// e.g. "google/embeddinggemma-2" or "Q4_K_M · GGUF"
        pub source_label: Option<String>,
        pub status: ModelStatus,
        pub tone: StatusTone,
        pub required: bool,
        pub runtime: Option<String>,
        pub dimension: Option<u32>,
        /// Measured resident memory; `None` = not measured (never estimated).
        pub memory_mb: Option<f64>,
        pub validated_at: Option<String>,
        pub ttl: TtlPolicy,
        pub error_code: Option<ErrorCode>,
        /// i18n key with the specific reason, e.g. "model.err.voice_profile_missing"
        pub error_key: Option<String>,
    }

    pub struct RegisterModelRequest {
        pub category: ModelCategory,
        pub path: String,
    }

    pub struct ValidationStep {
        pub id: String,
        pub label_key: String,
        pub state: StepState,
        pub detail: Option<String>,
    }
}

// ---------------------------------------------------------------- folders

ts_export! {
    #[serde(rename_all = "snake_case")]
    pub enum ScopeStatus { UpToDate, Scanning, Paused, Unreachable, Revoking, Error }

    #[serde(rename_all = "snake_case")]
    pub enum RevokeMode {
        /// Keep the index, stop rescanning.
        StopWatching,
        /// Child under an authorized parent: block it and purge (privacy default).
        BlockAndPurge,
        /// Remove this grant and purge everything derived from it.
        RemoveAndPurge,
        /// Child under parent: drop only the separate grant.
        RemoveGrantOnly,
    }

    pub struct ScopeSummary {
        pub scope_id: String,
        pub display_path: String,
        pub status: ScopeStatus,
        pub watching: bool,
        pub file_count: Option<u32>,
        pub last_scan_at: Option<String>,
        /// 0..1 while scanning.
        pub progress: Option<f64>,
        pub include_label: Option<String>,
        pub exclusion_count: u32,
        pub is_drive_root: bool,
        pub parent_scope_id: Option<String>,
        /// Why the last scan stopped (status `error`); the index stays searchable.
        pub error_code: Option<ErrorCode>,
    }

    pub struct IndexStats {
        pub scope_count: u32,
        pub file_count: Option<u32>,
        pub index_mb: Option<f64>,
    }

    pub struct ScopePreview {
        pub preview_token: String,
        pub display_path: String,
        pub is_drive_root: bool,
        pub file_count: u32,
        pub est_index_mb: f64,
        pub cap_files: u32,
        pub auto_excludes: Vec<String>,
        pub parent_display_path: Option<String>,
        /// False when the preview hit its file/time cap: `file_count` is a minimum.
        pub complete: bool,
    }

    pub struct RevokeRequest {
        pub scope_id: String,
        pub mode: RevokeMode,
        pub purge_history: bool,
    }
}

// ---------------------------------------------------------------- history

ts_export! {
    #[serde(rename_all = "snake_case")]
    pub enum TurnRole { User, Assistant, Result }

    pub struct HistorySession {
        pub session_id: String,
        pub title: String,
        pub started_at: String,
        pub result_count: u32,
        pub opened_count: u32,
    }

    pub struct HistoryTurn {
        pub turn_id: String,
        pub role: TurnRole,
        pub at: String,
        pub text: Option<String>,
        /// For assistant turns: the response kind and count, rendered by the UI.
        pub kind: Option<SearchKind>,
        pub count: Option<u32>,
        pub choices: Vec<Choice>,
        pub results: Vec<ResultCard>,
        pub opened: bool,
        pub question: Option<String>,
        pub understood: Option<Understood>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_use_snake_case_on_the_wire() {
        assert_eq!(
            serde_json::to_string(&SearchKind::NotFound).unwrap(),
            "\"not_found\""
        );
        assert_eq!(
            serde_json::to_string(&TtlPolicy::Minutes30).unwrap(),
            "\"minutes30\""
        );
        assert_eq!(
            serde_json::to_string(&RevokeMode::BlockAndPurge).unwrap(),
            "\"block_and_purge\""
        );
        assert_eq!(
            serde_json::to_string(&ScopeStatus::UpToDate).unwrap(),
            "\"up_to_date\""
        );
    }

    #[test]
    fn search_request_accepts_missing_session() {
        let r: SearchRequest =
            serde_json::from_str(r#"{"session_id":null,"text":"timer","choice_id":null}"#).unwrap();
        assert_eq!(r.utc_offset_min, None);
        assert_eq!(r.text, "timer");
    }
}
