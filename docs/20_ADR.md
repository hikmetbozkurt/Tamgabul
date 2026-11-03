# 20 — Ratified Architecture Decisions (ADRs)

These decisions represent the user's stated priorities as of 2026-10-26. Changing one requires a new ADR with options, data and impact. Technical details flagged PROPOSED may change following feasibility testing.

| ADR | Decision | Status | Reason |
|---|---|---|---|
| 001 | Separate desktop project; provisional name LCW; **no Capsula/3D capsules** | ACCEPTED | Distinct product and MVP |
| 002 | Windows first, same Tauri/Rust/React codebase then macOS | ACCEPTED | Efficient portability; OS native tray/menu |
| 003 | Persistent SQLite memory not dependent on LLM KV cache | ACCEPTED | Queries accessible across model unload/reboots |
| 004 | EmbeddingGemma 2 mandatory for primary semantic retrieval | ACCEPTED | Core engine specifically selected |
| 005 | Local LLM optional for intent/context; deterministic search fallback | ACCEPTED | Minimize resource use and hallucination |
| 006 | Bring-your-own supported LLM/embedding/STT/TTS via Add Model; local only | ACCEPTED | User autonomy with compatibility validation |
| 007 | App manages model runtimes directly; LM Studio not required | ACCEPTED | Self-contained install, own lifecycle control |
| 008 | Model-specific configurable TTL and manual load/unload | ACCEPTED | CPU/GPU resource efficiency |
| 009 | Explicit source grants incl. guarded Windows drive root; revocable | ACCEPTED | User decides data surface; privacy |
| 010 | System tray/background resident; close hides, explicit Quit terminates | ACCEPTED | Widget desktop interaction |
| 011 | Minimal UI design mutable, clear colored and labeled health states | ACCEPTED | User-friendly and revision-ready UX |
| 012 | LoRA experiment for EG2 on RTX 4080 after working base system | ACCEPTED-FUTURE | Evaluation driven adaptation |
| 013 | Use Tauri sidecars/llama.cpp initially, alternative runtime if benchmarks justify | PROPOSED | Proven packaging approach; hardware spike required |
| 014 | SQLite, FTS5, versioned vector store, root-level ACL check on each result | PROPOSED | Transactionality and privacy |
| 015 | Source removal immediate search deny, purge derived data, never source delete | ACCEPTED | Revocable permission implies result revocation |
| 016 | Manual indexing by default, idle sync optional | PROPOSED | Avoid surprising background resource use |
| 017 | Default model TTLs and latency targets per docs | PROPOSED | Need test evidence |
| 018 | No mock, demo or sample data anywhere in the product/UI; screens show only real backend state, unimplemented parts say “Not available yet”. Fakes only inside automated tests | ACCEPTED | User decision 2026-11-02; enterprise-grade honesty |
| 019 | Runtime: llama.cpp `llama-server` workers managed by the app (`crates/runtime`) for the LLM (GGUF) and EmbeddingGemma 2 (GGUF BF16 + mmproj with vision/audio encoders). Started on demand, stopped on idle timeout; unload = process exit. An AVAILABLE (validated, unloaded) embedding model counts as searchable: the first query loads it | ACCEPTED (embedding, 2026-11-03) · LLM part PROPOSED | Windows `eg2_spike` run, docs/19 V02 |
| 020 | OCR, extra image codecs and PDF page rendering come from the OS (Windows: `Windows.Media.Ocr`, WIC, `Windows.Data.Pdf`); portable decoding (images: `image`, audio: `symphonia`, PDF text: `pdf-extract`, Office: zip+XML) in Rust. No OCR model is downloaded | PROPOSED | Confirm OCR quality on real Turkish/English screenshots and scans (docs/19 V07) |
| 021 | Small LLM has one job: turn a request (+ last 3 requests and titles already shown) into a search plan as grammar-constrained JSON (`tamgabul_core::understand`, schema `llm_schema()`). Rules always run first (TR/EN date and file-type phrases); the LLM adds base forms, TR↔EN synonyms, follow-up references, rejected words and at most one clarifying question. It never sees file contents and cannot name a file or widen scope. Tested model: Gemma 4 E2B-it Q4_0 (`ggml-org/gemma-4-E2B-it-GGUF`, Apache-2.0), 4096 context, idle TTL 5 min. Search never waits on a broken LLM (ERROR = skipped until Retry) | PROPOSED (2026-11-03) — accept after `llm_spike` on Windows | docs/08, docs/19 V01 |

**Versioning rule:** ADR change → update affected interface, tests, schema and roadmap. Never create contradictory duplicated 'truth' in multiple docs without recording priority of latest ADR.
