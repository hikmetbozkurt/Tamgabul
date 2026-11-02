# P3-01 Persistent Conversation and Optional Local LLM

**Phase:** P3 • **Owner:** Retrieval/Dialog • **Status:** TODO • **Dependencies:** P2-01 (test-only fakes allowed in automated tests; no mock data in the product — ADR 018)

## Objective
Persisted sessions, turns and episodic events in SQLite so "geçen bulmuştun" works after model unload and reboot. A deterministic router handles common cases; the optional Gemma E4B adapter only emits strict intent JSON and grounded, cited replies.

## Read before implementation
AGENTS.md; docs/06_MEMORY_ENGINE.md; docs/08_RETRIEVAL_CONVERSATION.md; docs/13_API_CONTRACTS.md; examples/intent-output.schema.json; examples/dialog-eval.example.jsonl

## Steps
1. Session/turn/event persistence with History ON/OFF semantics and retention; corrections stored as revisions.
2. Deterministic router: time expressions (geçen, dün, az önce), explicit project filters, pronoun/prior-result references, slot carry-over and negation.
3. Memory-first lookup over `search_events` + `result_interactions` with scope re-authorization and file existence/identity check; tombstones never open.
4. LLM adapter (llama.cpp sidecar from P0-02): bounded session facts in, schema-validated JSON out, one repair attempt, then deterministic fallback. Server resolves `result_ref`; LLM never authors IDs or paths.
5. Grounded reply generation only from listed snippets with `file_id + chunk_id`; abstain when no evidence. One clarifying question when candidates are close.

## Acceptance criteria
- [ ] 5-turn correction scenario passes with LLM on and LLM off (template clarification).
- [ ] History recall passes after unloading all models and restarting the app.
- [ ] Zero invented file IDs/paths across the eval set; prompt-injection fixture causes no tool call outside the allowlist.
- [ ] Intent JSON validity reported (target ≥99% after validator/fallback).

## Deliverables and report
Code + automated tests + commands/results + platform/hardware/runtime versions + security/privacy notes. Mark real runtime tests as PENDING until observed. Keep weights and private data out of git.
