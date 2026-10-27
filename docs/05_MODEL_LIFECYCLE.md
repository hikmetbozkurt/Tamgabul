# 05 — Model Lifecycle and Resource Policy

## Goal
Models do not monopolize VRAM/RAM when unnecessary; hot models answer quickly; users can independently choose automatic TTL/manual unload/pin mode for LLM, EmbeddingGemma 2, STT and TTS. A small tray host + DB may remain loaded.

## Required state machine
```text
MISSING/INCOMPATIBLE/ERROR
          ^
 REGISTERED -> VALIDATING -> AVAILABLE (= UNLOADED)
                                   |
                                 LOAD
                                   v
                                LOADING -> READY/IDLE <-> BUSY
                                   ^         |
                                   |         v TTL/manual/memory-pressure
                                  AVAILABLE <- UNLOADING
```
`AVAILABLE` means disk + adapter compatible, not memory resident. `READY/IDLE/BUSY` means model loaded. `STOPPED` applies to exited worker. Every transition tagged with epoch/request ID; late completions after unload/cancel must not set stale READY.

## Scheduling & concurrency
- Single lifecycle authority in Rust, async command queue serialized per model ID; locks/refcounts prevent unload while active inference. Loading same model twice returns shared in-flight future.
- Per-model last_used **monotonic** timestamp; TTL only while no active requests. TTS playback completion may differ from generation, policy based on generated requests plus playback grace.
- On manual Unload while busy: cancel compatible jobs or queue unload after current request, report which. Never kill without user-visible warning for data-loss risk.
- On OS suspend/resume: stop inference safely; reconstruct persisted sessions, re-check model process and timestamps; avoid duplicate workers.
- `Quit`: reject new jobs, cancel queues, stop watchers/indexers, flush SQLite, terminate managed child processes with bounded graceful timeout then force-kill process tree if necessary.
- Workers bind `127.0.0.1` only if HTTP required; random available port with per-process token and restricted access. Prefer direct stdin/IPC when practical.

## Initial defaults (hypotheses, configurable per-model)
| Model | Default TTL after last request | Common UI options |
|---|---:|---|
| LLM | 5 min | Immediately / 1 / 5 / 10 / 30 min / Keep Loaded |
| EmbeddingGemma 2 | 5 min after searches/index tasks | Same |
| STT | 1 min after recognition | Same |
| TTS | 2 min after generation | Same |
`Keep Loaded` is **opt-in** and warns about RAM/VRAM reservation. User-selectable `Always unload after job` tradeoff explains cold-start cost. Defaults may change after benchmark. Idle loaded GPU inference can use low compute while still reserving VRAM; show both utilization and allocated memory.

## Resource pressure and adaptive eviction
- On inference load request, estimate available device memory and active reservations; choose safe backend/device/offload or ask to evict warm models. Do not assume 16 GB always available on RTX 4080.
- Optional OS-reported memory-pressure watcher evicts idle models (least recently used first), never kills active inference without explicit fallback.
- Pre-warm only on user action or predictable event (opening widget) **after user setting**, not permanently in background by default.
- CPU fallback only if adapter supports it; surface predicted slowness and measured results, no fixed performance claims.

## Embedded vs sidecar inference
The user wants models **managed within the application** and no LM Studio. A bundled, supervised worker is acceptable as part of the installed app, not an external user-managed server. llama.cpp may be run as a child binary; embedding/STT/TTS can similarly use a private managed worker until native bindings are validated. Long-term FFI/native integration is an optimization not MVP prerequisite.

## Measurements & SLO protocol
Capture: validation_ms, cold_load_ms, warm_first_token_ms, embedding_batch_ms, STT_first_partial_ms, TTS_first_audio_ms, unload_ms, process RSS, GPU VRAM reserved/active, GPU utilization idle, peak memory, orphan process count. `unload successful` means the worker released resources verified through OS metrics (with tolerance), not only that the model returned an 'OK'. No universal latency guarantee until tested on Windows RTX 4080 and macOS M1 Pro.

## Tests
Double-load race, unload-on-busy, stale READY after crash, missing file mid-call, 100 rapid hotkey toggles, suspend/resume, RAM pressure, Settings TTL changes while loaded, explicit Quit with active embedding, worker killed unexpectedly, recoverable re-validation.
