# 23 — Multi-Agent Handoff and Integration Rules

## Recommended role separation
- **Architecture/Tech lead agent**: ratified ADR coherence, scope control, schemas, integration gates.
- **Runtime agent**: model adapters, llama.cpp/embedding/ASR/TTS capabilities, load/unload, memory metrics.
- **Data agent**: SQLite migrations, secure roots, indexing, revocation, purge, source provenance.
- **Retrieval agent**: hybrid ranking, historical events, deterministic routing, LLM schema.
- **UI agent**: Windows tray, widget, settings, onboarding, accessibility, tests against the typed contracts.
- **QA agent**: synthetic eval, benchmarks, scripts, security cases, macOS matrix.

## Coordination
- Branch per ticket, one migration owner, interface contracts reviewed before parallel work.
- UI works against the `13_API_CONTRACTS.md` types; unimplemented commands return `NOT_IMPLEMENTED` and the UI shows that state. No mock/demo data (ADR 018); do not invent ad hoc IPC shapes.
- Runtime may stub EG2/LLM but integration label must be `MOCK ONLY` until real binary tests documented.
- Every task has narrowly defined touched paths, dependencies, acceptance criteria and commands; use `templates/TASK_TEMPLATE.md`.
- No model weights, private corpora, user-specific absolute file paths or raw voice recordings committed.

## Approval checkpoints
CP0 feasibility of E4B + EG2 packaging on Windows. CP1 scope revocation and schema invariants. CP2 retrieval metrics baseline. CP3 multi-turn history after worker unload/reboot. CP4 resource lifecycle. CP5 widget interactions. CP6 voice and installer. CP7 Mac parity. CP8 future LoRA.

## Standard completion report (each agent)
**Summary:** what changed. **Files touched:** paths. **Evidence:** test commands, pass/fail, screenshots if UI, benchmark versions if AI. **Known gaps:** real vs mocked functionality. **Security impact:** permissions and data handling. **Next dependency:** exact ticket. **Do not claim done without tests.**

## Handoff prompt to start repository
“Read AGENTS.md, README.md, docs/20_ADR.md and agent_tasks/00_BOOTSTRAP.md. Implement only P0 foundation using tests and typed contracts; do not integrate LM Studio, 3D UI or automatic downloads. Report results and blockers with commands.”
