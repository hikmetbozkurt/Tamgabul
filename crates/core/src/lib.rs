//! Tamgabul core library.
//!
//! P0-01 scope only: model status vocabulary, app health derivation, typed IPC
//! DTOs and the SQLite migration harness. No model runtime, indexing or
//! retrieval is implemented here yet — see `agent_tasks/` for later phases.

pub mod db;
pub mod dto;
pub mod error;
pub mod health;
pub mod ipc;
pub mod model;
pub mod settings;

/// Version of the UI <-> Rust IPC contract (docs/13_API_CONTRACTS.md).
/// Bump on any breaking change to a DTO in [`ipc`].
pub const IPC_API_VERSION: u32 = 1;
