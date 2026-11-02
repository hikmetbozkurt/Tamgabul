//! Tamgabul model runtime (P0-02, ADR 019).
//!
//! - [`gguf`]: read-only GGUF header probe.
//! - [`eg2`]: EmbeddingGemma 2 profile — file validation, task prompts,
//!   normalization and Matryoshka truncation.
//! - [`locate`]: find `llama-server` (explicit, bundled, PATH, winget).
//! - [`process`] / [`worker`]: supervised workers on 127.0.0.1 with a
//!   per-process token; they die with the app (Job Object on Windows).
//! - [`client`]: HTTP calls to a worker.
//! - [`manager`]: on-demand load, idle unload, leases, shutdown.
//!
//! Models are never started at app launch and never stay resident without
//! a policy that says so (docs/05).

pub mod client;
pub mod eg2;
pub mod error;
pub mod gguf;
pub mod locate;
pub mod manager;
pub mod process;
pub mod worker;

pub use error::RuntimeError;
pub use manager::{Lease, ModelManager, ModelSnapshot, Policy, UnloadOutcome};
