# P0-02 LLM and Embedding Runtime Feasibility

**Phase:** P0 • **Owner:** Runtime • **Status:** IN PROGRESS (code done; Windows run PENDING) • **Dependencies:** 00 optional; can parallelize

## Objective
Prove the existing Gemma 4 E4B Q4_K_M runs without LM Studio and EmbeddingGemma 2 produces correct local vectors (text+image) through an app-managed worker prototype.

## Read before implementation
docs/04_MODEL_REGISTRY.md; docs/05_MODEL_LIFECYCLE.md; docs/19_RESEARCH_VERIFICATION.md

## Steps
1. Pin llama.cpp exact commit/build for Windows CUDA, verify E4B GGUF tokenizer/chat template; exercise strict JSON and unload/sleep.
2. Pin EG2 revision and SentenceTransformers versions; test text+image encoding 768d, normalized scoring and user-selected model path.
3. Probe bundled worker packaging and process supervision, measure memory release and cold/warm latency.
4. Record macOS portability unknowns and runtime license/provenance.

## Acceptance criteria
- [ ] Report actual command/version/model hash and stdout results (redact local paths).
- [ ] Show model reaches READY then truly unloads OS resources.
- [ ] Show EG2 768d vectors, separate query/doc normalization and image retrieval.
- [ ] Unknowns recorded, no fake time estimate.

## Deliverables and report
Code + automated tests + commands/results + platform/hardware/runtime versions + security/privacy notes. Mark real runtime tests as PENDING until observed. Keep weights and private data out of git.

### Progress report — 2026-11-02
- **Done:** `crates/runtime` — GGUF header probe, EG2 profile (file pair by header, 768d check, encoder check, pinned SHA-256, model-card prompts, Matryoshka), `llama-server` locator (explicit / env / bundled / PATH / winget), supervised worker (127.0.0.1, random port, per-process token, health wait, startup timeout, cancel, Job Object / PDEATHSIG, log tail), HTTP client (text + image/audio embeddings), lifecycle manager (on-demand load, shared in-flight load, leases, idle TTL default 5 min, unload-after-job, keep-loaded opt-in, deferred unload when busy, epochs against stale READY, crash detection, shutdown). App: manager in `AppState`, tray "Unload Models" and Quit stop workers, `model_status` event.
- **Evidence:** `cargo test -p tamgabul-runtime` (24 unit + 16 lifecycle + 5 real-process tests, test-only stand-in server), clippy clean. Header probe run against the real BF16 files: 768d, BF16, vision + audio encoders present.
- **PENDING (Windows):** `cargo run -p tamgabul-runtime --release --example eg2_spike -- --models C:\AI\models\embeddinggemma-2` → text/image/audio vectors, unload and idle unload with OS process + VRAM check. LLM (Gemma 4 E4B) part not started.
- **Unknowns:** whether the user's winget llama.cpp build is new enough for `gemma-embedding2` (needs ≥ commit 914f7f8); GPU backend of that build; macOS has no kernel tie between app and worker.
