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

**Versioning rule:** ADR change → update affected interface, tests, schema and roadmap. Never create contradictory duplicated 'truth' in multiple docs without recording priority of latest ADR.
