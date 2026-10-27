# P4-01 Model Registry and Lifecycle Manager

**Phase:** P4 • **Owner:** Runtime • **Status:** TODO • **Dependencies:** P0-02, P2-01

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
