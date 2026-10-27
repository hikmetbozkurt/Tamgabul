# 15 — Agent-Executable Development Roadmap

## Phase dependencies
`P0 Repo + ADR → P1 Data + Scope → P2 Embedding retrieval → P3 LLM conversation → P4 Model lifecycle → P5 UI/tray → P6 Voice → P7 Windows release → P8 macOS parity → P9 LoRA experiment`.
A one-off headless spike for E4B/EG2 may start alongside P0; integration work is blocked until adapter feasibility recorded.

## P0 — Engineering foundation [BLOCKER]
- Repo structure, Rust workspace, Tauri v2 React TS shell, lint/test CI, README/AGENTS, i18n, app ID.
- Run runtime probes on actual hardware: EmbeddingGemma 2 checkpoint text+image, E4B GGUF via bundled llama.cpp, Windows package load/unload, sqlite-vec extension packaging.
- Acceptance: `cargo test`, `npm test`, mock runtime health; report observed compatibility, no invented performance.
- Ticket: `agent_tasks/00_BOOTSTRAP.md` and `agent_tasks/01_RUNTIME_SPIKES.md`.

## P1 — SQLite and sources [BLOCKER]
- Migrations, scope grants, durable jobs, `.git/node_modules/OS` default exclusions, source metadata; file parse adapters, read-only invariant; grant/revoke + crash-safe purge.
- Acceptance: integration tests with scoped temp dirs, overlapping roots, symlink escapes, revocation and restart.
- Ticket: `agent_tasks/02_DATA_AND_SCOPES.md`.

## P2 — Embedding retrieval [BLOCKER]
- EmbeddingGemma 2 import/verify, local embeddings for `.md/.txt/.pdf/.png/.jpg`, generation fingerprint, hybrid FTS/vector search, evidenced result cards. Embedding unavailable red/degraded mode.
- Acceptance: correct outputs across synthetic multilingual dataset, no vectors from different model generations mixed, quoted source snippet.
- Ticket: `agent_tasks/03_RETRIEVAL.md`.

## P3 — Conversation & local LLM
- Persisted sessions/events, lookup prior result, deterministic router, optional Gemma E4B adapter and JSON schema; grounded summarizer, validation/fallback, after-restart recovery.
- Acceptance: 5-turn correction and history from DB after model unload and reboot; no invented file ID.
- Ticket: `agent_tasks/04_DIALOGUE.md`.

## P4 — Model registry & lifecycle
- Import/validate LLM/embedding/STT/TTS manifests; compatibility statuses; TTL, memory manager, manual unload, crash/sleep behavior. Each model own setting.
- Acceptance: no LM Studio dependency; user-imported compatible model starts; erroneous model never falsely GREEN; Quit cleans children.
- Ticket: `agent_tasks/05_MODEL_MANAGER.md`.

## P5 — Minimal Windows UI
- Window/tray/hotkey, search/result widget, settings Models/Sources/Performance/Privacy, onboarding; keyboard only and accessible status indicators; dry-run C:\ scan.
- Acceptance: close→tray, shortcut conflict resolution, model missing red, source revoke proper dialog.
- Ticket: `agent_tasks/06_WINDOWS_UI.md`.

## P6 — Voice and offline packaging
- Approved STT and TTS runtimes, mic opt-in, transcript correction, multilingual evaluation; installer with bundled signed workers but weights user chooses; explicit downloader optional.
- Acceptance: typed remains functional on mic denial, no ambient recording, local voice roundtrip in Turkish/English, no orphan processes.
- Ticket: `agent_tasks/07_VOICE_RELEASE.md`.

## P7 — Windows MVP release gate
- Security/benchmark matrix; onboarding from zero models; installer update/migration; private and synthetic test datasets; local help docs.

## P8 — macOS parity
- Build on M1 Pro, Metal backend, menu-bar, shortcut/permissions, notarization checklist, performance parity as observed not promised.

## P9 — Post-MVP EmbeddingGemma 2 LoRA lab
- Baseline benchmark → curated triplets → training pilot on 4080 16GB → held-out eval + regression → generation migration plan → rollback. See `16_LORA_LAB.md`.

## Parallel-agent boundaries
P1 SQLite and P0 runtime feasibility can proceed on separate branches. P2 depends on P1 schema and embedding spike. P3 dialogues can be mocked pending P2, but may not claim integrated completion. UI can develop against typed mocks; real tray acceptance needs runtime. One agent owns schema migration at a time.
