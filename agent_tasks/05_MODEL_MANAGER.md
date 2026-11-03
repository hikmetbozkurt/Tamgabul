# P4-01 Model Registry and Lifecycle Manager

**Phase:** P4 • **Owner:** Runtime • **Status:** IN PROGRESS (embedding done; LLM/STT/TTS adapters later) • **Dependencies:** P0-02, P2-01

## Objective
Users add their own LLM / EmbeddingGemma 2 / STT / TTS models; only compatible ones activate. A single Rust lifecycle authority loads on demand, unloads on per-model TTL or manually, and proves resource release. No LM Studio.

## Read before implementation
AGENTS.md; docs/04_MODEL_REGISTRY.md; docs/05_MODEL_LIFECYCLE.md; docs/13_API_CONTRACTS.md; examples/model-manifest.example.json

## Steps
1. `register_model` / `validate_model`: static probe (GGUF header, config/tokenizer files), then explicit smoke inference; statuses `MISSING | REGISTERED | VALIDATING | AVAILABLE | LOADING | READY | BUSY | IDLE | UNLOADING | ERROR | INCOMPATIBLE`.
2. Per-model serialized command queue, refcounts, shared in-flight load, epoch/request IDs so late completions never set stale READY.
3. TTL on monotonic clock while no active requests; modes `ttl`, `keep_loaded` (opt-in with warning), `unload_after_job`; manual Unload and Unload All.
4. Worker supervision: loopback/pipe only with per-process token; crash → ERROR with typed error; suspend/resume reconciliation; `quit()` graceful then force-kill process tree.
5. Resource metrics: RSS, VRAM reserved/active, cold/warm load, unload verified by OS metrics.

## Acceptance criteria
- [ ] Tests: double-load race, unload-on-busy, crash → ERROR, missing file mid-call, TTL change while loaded, 100 rapid toggles, Quit with active embedding.
- [ ] Incompatible or merely registered models never show green; after restart persisted READY shows AVAILABLE until health check.
- [ ] Quit leaves zero managed child processes (observed).
- [ ] No LM Studio process or API used anywhere.

## Deliverables and report
Code + automated tests + commands/results + platform/hardware/runtime versions + security/privacy notes. Mark real runtime tests as PENDING until observed. Keep weights and private data out of git.

### Progress report — 2026-11-03
- **Done (embedding):** registry in SQLite (`crates/core/src/registry.rs`: activate/replace, TTL policy, snapshot status, remove — files never touched); Tauri commands `list_models`, `validate_model` (steps streamed over a channel: files → GGUF format/encoders → SHA-256 vs pinned release → llama-server found + version → smoke test with a private worker: text, generated image, generated tone; cancellable), `register_model` (only accepts a validation from this session), `load_model`, `unload_model`, `unload_all_models`, `set_model_ttl` (new option: right after use), `remove_model`. Active models are attached to the lifecycle manager at start-up without loading. List/health show live status; missing files → MISSING (red for embedding); llama-server missing → ERROR with reason. IPC errors carry a code and fixed hint only (no paths/logs).
- **Rule change (ADR 019):** a validated, unloaded EmbeddingGemma 2 (AVAILABLE) counts as searchable; the first query starts it. Rust and TS health rules + tests updated.
- **Tests:** core registry (4), runtime smoke fixtures (2), desktop mapping/redaction (2); full suite green. Windows end-to-end (Add model → Load → idle unload in the UI) PENDING.
- **Not yet:** LLM/STT/TTS validation ("supported in a later version"), RSS/VRAM in the UI (`Not measured`), suspend/resume reconciliation, memory-pressure eviction, 100-toggle stress test.
