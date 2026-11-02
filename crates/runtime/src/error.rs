//! Runtime errors. Messages are developer hints: they never contain model
//! input (queries, file text, image bytes). Paths are redacted by callers
//! before anything reaches the UI.

use tamgabul_core::error::{ErrorCode, IpcError};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RuntimeError {
    #[error("model file is missing")]
    ModelMissing,
    #[error("invalid model file: {0}")]
    InvalidModelFile(String),
    #[error("model is not compatible: {0}")]
    Incompatible(String),
    #[error("runtime executable not found")]
    RuntimeNotFound,
    #[error("worker failed to start: {0}")]
    LoadFailed(String),
    #[error("worker did not become ready in time")]
    LoadTimeout,
    #[error("worker exited unexpectedly: {0}")]
    WorkerExited(String),
    #[error("request failed: {0}")]
    Request(String),
    #[error("request timed out")]
    Timeout,
    #[error("unexpected response: {0}")]
    BadResponse(String),
    #[error("model is not registered")]
    UnknownModel,
    #[error("runtime is shutting down")]
    ShuttingDown,
    #[error("load was cancelled")]
    Cancelled,
    #[error("io: {0}")]
    Io(String),
}

impl RuntimeError {
    pub fn code(&self) -> ErrorCode {
        match self {
            RuntimeError::ModelMissing | RuntimeError::UnknownModel => ErrorCode::ModelMissing,
            RuntimeError::InvalidModelFile(_) | RuntimeError::Incompatible(_) => {
                ErrorCode::ModelIncompatible
            }
            RuntimeError::RuntimeNotFound
            | RuntimeError::LoadFailed(_)
            | RuntimeError::LoadTimeout
            | RuntimeError::WorkerExited(_) => ErrorCode::RuntimeLoadFailed,
            RuntimeError::Timeout => ErrorCode::InferenceTimeout,
            RuntimeError::Cancelled | RuntimeError::ShuttingDown => ErrorCode::UserCancelled,
            RuntimeError::Request(_) | RuntimeError::BadResponse(_) | RuntimeError::Io(_) => {
                ErrorCode::Internal
            }
        }
    }
}

impl From<RuntimeError> for IpcError {
    fn from(e: RuntimeError) -> Self {
        IpcError::new(e.code(), e.to_string())
    }
}
