//! Error taxonomy from docs/13_API_CONTRACTS.md.
//!
//! Messages shown to the user are localized in the UI from `code`; the Rust
//! side never sends raw file paths or user content in an error.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub enum ErrorCode {
    ModelMissing,
    ModelIncompatible,
    RuntimeLoadFailed,
    InferenceTimeout,
    EmbeddingGenerationStale,
    ScopeNotAuthorized,
    ScopeRevoked,
    SourceChanged,
    IndexUnavailable,
    MicPermissionDenied,
    DbMigrationFailed,
    UserCancelled,
    /// Not in the docs/13 list: the global shortcut is taken by another app.
    HotkeyConflict,
    /// Not in the docs/13 list: the command exists in the contract but its
    /// backend phase (P1–P4) has not landed yet. The UI shows this honestly.
    NotImplemented,
    /// Not in the docs/13 list: generic internal failure with no user content.
    Internal,
}

/// Error returned by every IPC command. `detail` is a short, redacted,
/// developer-facing hint (never a path, prompt or snippet).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, thiserror::Error)]
#[error("{code:?}: {detail}")]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub struct IpcError {
    pub code: ErrorCode,
    pub detail: String,
}

impl IpcError {
    pub fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_serializes_as_docs_spelling() {
        let json = serde_json::to_string(&ErrorCode::EmbeddingGenerationStale).unwrap();
        assert_eq!(json, "\"EMBEDDING_GENERATION_STALE\"");
        let json = serde_json::to_string(&ErrorCode::DbMigrationFailed).unwrap();
        assert_eq!(json, "\"DB_MIGRATION_FAILED\"");
    }
}
