//! Typed IPC DTOs shared with the React UI.
//!
//! Running `cargo test -p tamgabul-core` regenerates the TypeScript types in
//! `apps/desktop/src/bindings/` (ts-rs). Commit the generated files so the UI
//! typecheck fails when Rust and TS drift apart.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub struct PingRequest {
    /// Echoed back; lets the UI ignore stale replies (docs/13).
    pub request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/desktop/src/bindings/")]
pub struct PingResponse {
    pub api_version: u32,
    pub request_id: String,
    pub app_version: String,
    pub os: String,
    pub arch: String,
}

pub fn ping(request: PingRequest, app_version: &str, os: &str, arch: &str) -> PingResponse {
    PingResponse {
        api_version: crate::IPC_API_VERSION,
        request_id: request.request_id,
        app_version: app_version.to_owned(),
        os: os.to_owned(),
        arch: arch.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_echoes_request_id_and_version() {
        let r = ping(
            PingRequest {
                request_id: "r-1".into(),
            },
            "0.1.0",
            "windows",
            "x86_64",
        );
        assert_eq!(r.request_id, "r-1");
        assert_eq!(r.api_version, crate::IPC_API_VERSION);
        let json = serde_json::to_value(&r).unwrap();
        assert!(
            json.get("request_id").is_some(),
            "snake_case field names are the wire format"
        );
    }
}
