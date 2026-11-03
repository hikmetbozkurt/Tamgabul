//! Tamgabul core library.
//!
//! Model status vocabulary, app health derivation, typed IPC DTOs, the SQLite
//! migration harness and the model registry. The model runtime lives in
//! `tamgabul-runtime`.

pub mod db;
pub mod describe;
pub mod dto;
pub mod error;
pub mod health;
pub mod history;
pub mod index;
pub mod ipc;
pub mod model;
pub mod registry;
pub mod rerank;
pub mod search;
pub mod settings;
pub mod understand;

/// Version of the UI <-> Rust IPC contract (docs/13_API_CONTRACTS.md).
/// Bump on any breaking change to a DTO in [`ipc`].
pub const IPC_API_VERSION: u32 = 1;
