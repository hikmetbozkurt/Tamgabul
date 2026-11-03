# 13 — IPC and Runtime Adapter Contracts

## Principles
All frontend↔Rust interactions are typed, versioned, permission-checked. Frontend never sees raw OS process handles or internal worker ports. Commands return discriminated results with machine codes; streams expose monotonic event sequence IDs. Command names below are **proposed API**, not implemented.

## UI → Rust commands
| Method | Input | Key behavior |
|---|---|---|
| `get_app_health()` | none | mandatory EG2 readiness, DB and index states |
| `list_models(category?)` | category filter | full registry including compatible, status, error |
| `register_model(path, category, adapter)` | user-selected path | metadata registration, no implied load |
| `validate_model(model_id)` | ID | static probe + optional smoke run |
| `set_active_model(category, model_id)` | refs | requires VERIFIED compatibility; embedding index generation change |
| `load_model(model_id)` / `unload_model(model_id)` | ID | idempotent, race-safe transitions |
| `set_model_policy(model_id, ttl_policy)` | policy | persist per-model TTL, `keep_loaded` caution |
| `list_scopes()` | none | all selected grants, state and counts |
| `preview_scope(path, filters)` | picker result | dry-run with capped estimate, risk flags |
| `grant_scope(preview_token, consent)` | preview + explicit confirm | durable authorization; invalid tokens rejected |
| `revoke_scope(scope_id, policy)` | ID | immediate ACL deny; purge job |
| `start_index_job(scope_id)` / `cancel_job(job_id)` | refs | bounded work, persisted checkpoint |
| `submit_utterance(session_id, text)` | typed text | structured transcript/intent/result cards |
| `submit_audio(session_id, device_ref)` | user microphone | only with explicit recording state |
| `open_result(result_ref)` | opaque result ID | revalidate scope, inode/hash/path before opener |
| `list_history(...)`, `delete_history(...)` | filter | retention/erase; scoped ACL |
| `quit()` | none | graceful app+worker shutdown |

## Backend → UI events
`model_status_changed`, `model_metrics`, `index_progress`, `index_error`, `scope_revoked`, `search_results`, `dialog_clarification`, `audio_transcription`, `tts_playback`, `worker_crashed`, `app_warning`. Every event contains `event_version`, `timestamp`, `request_id`, `sequence` and structured redacted payload. UI ignores stale request versions.

## Runtime adapter trait (illustrative Rust)
```rust
#[async_trait]
pub trait ModelAdapter: Send + Sync {
    async fn probe(&self, path: &Path) -> Result<ModelCapabilities, ModelError>;
    async fn load(&self, model_id: Uuid, opts: LoadOptions) -> Result<(), ModelError>;
    async fn health(&self, model_id: Uuid) -> Result<RuntimeHealth, ModelError>;
    async fn infer(&self, model_id: Uuid, request: InferenceRequest) -> Result<InferenceOutput, ModelError>;
    async fn unload(&self, model_id: Uuid) -> Result<(), ModelError>;
}
```
Separate interfaces for embedding batch vs generative LLM vs ASR vs speech synthesis preferred; trait above represents lifecycle only, inference payloads are category-specific.

## Structured LLM output
`search(req)` takes `utc_offset_min` (user's clock, for date filters) and answers with `question` (clarify) and `understood` (filters + LLM-added words); history turns carry the same fields.

Search planning uses `understand::llm_schema()` (ADR 021, docs/08 "Implementation"); `examples/intent-output.schema.json` remains the wider dialogue design. `file_id` from model **cannot authorize file open**; LLM can only propose known `result_ref` resolved server-side. Unknown action/enum/extra field rejected. Response generation uses source evidence IDs to avoid false claims.

## Error taxonomy
`MODEL_MISSING`, `MODEL_INCOMPATIBLE`, `RUNTIME_LOAD_FAILED`, `INFERENCE_TIMEOUT`, `EMBEDDING_GENERATION_STALE`, `SCOPE_NOT_AUTHORIZED`, `SCOPE_REVOKED`, `SOURCE_CHANGED`, `INDEX_UNAVAILABLE`, `MIC_PERMISSION_DENIED`, `DB_MIGRATION_FAILED`, `USER_CANCELLED`. Messages localized; avoid raw file paths in default logs.
