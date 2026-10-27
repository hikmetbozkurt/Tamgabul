# 18 — Risk Register

| ID | Risk | Impact | Mitigation | Owner / gate |
|---|---|---|---|---|
| R01 | EG2 adapter cannot be bundled efficiently in Tauri Windows/macOS | High | packaging spike; managed pinned sidecar first; never promise Rust-native | Model/platform owner P0 |
| R02 | Gemma E4B cold load degrades interactive flow | High | benchmark keep-warm vs TTL, deterministic router bypass, user settings | Runtime P0/P4 |
| R03 | User imports incompatible/untrusted weights | High | allowlisted formats and signed runners, static probe, smoke test, red incompatibility | Model P4 |
| R04 | `C:\` scan collects secrets/causes huge IO | Critical | opt-in dry-run, protected exclusions, quotas, manual default, ACL-aware | Index/Security P1 |
| R05 | Revoked source remains in vector/history caches | Critical | immediate authorization deny, durable purge, memory/tombstone redaction, tests | DB P1 |
| R06 | Parent scope re-indexes revoked child | Critical | persistent deny under parent, explicit overlap dialog | Index P1 |
| R07 | Model status falsely green after crash/unload/reboot | High | backend health + restart-to-AVAILABLE; status tests | Runtime/UI P4 |
| R08 | Memory persists sensitive transcripts indefinitely | High | configurable retention, raw audio off, per-session delete, content-free telemetry | Memory/UX P3 |
| R09 | LLM invents source path or follows malicious document instructions | Critical | schema validation, factual source IDs, allowlisted tools, no exec | Dialog P3 |
| R10 | Audio TR-EN code-switch failure | Medium | benchmark WER on real mixed language, compare adapters | Audio P6 |
| R11 | Windows shortcut collisions or tray hidden by OS | Medium | configurable shortcuts, visible onboarding instruction and menu fallback | UI P5 |
| R12 | Mac M1 Pro memory/backend mismatch | High | Apple Silicon smoke tests, adapted runtime binary and memory settings | Platform P8 |
| R13 | sqlite-vec/FTS extension binary mismatch | High | version/pin package spike; SQL FTS fallback | DB P0/P1 |
| R14 | LoRA improves train but degrades other domains | Medium | frozen eval, hard negatives, rollback/index versioning | AI lab P9 |
| R15 | License obligations for bundled TTS/STT/weights unclear | High | per-file license manifest, legal review, explicit origin/terms, no unsupported redistribution | Release P6/P7 |
| R16 | Promised resource figures unattainable | Medium | measured CPU/GPU/RSS across machines; avoid fixed marketing claims | QA P7 |
| R17 | Disk deletion not securely erased or backups keep content | High | privacy warning, secure-delete/vacuum policy, storage/backups architecture | Security P1 |

Risks are living records: a ticket changes `Open → Mitigating → Verified → Accepted`, with evidence; do not silently mark Closed.
