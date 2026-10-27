# P1-01 SQLite, Source Scopes and Revocation

**Phase:** P1 • **Owner:** Data • **Status:** TODO • **Dependencies:** P0-01

## Objective
Make the local SQLite authority real: versioned migrations, explicit folder grants (including guarded drive root), bounded read-only traversal with default exclusions, and crash-safe revoke + purge. No embeddings yet; parsers may produce text chunks only.

## Read before implementation
AGENTS.md; docs/07_INDEXING_PERMISSIONS.md; docs/11_SECURITY_PRIVACY.md; docs/12_DATA_SCHEMA.md; database/001_initial.sql; docs/20_ADR.md (009, 014, 015, 016)

## Steps
1. Migration runner in `crates/core` applying `migrations/` (seeded from `database/001_initial.sql`), backup-before-migrate, never silent reset.
2. Scope state machine `PENDING_PREVIEW → AWAIT_CONFIRM → ACTIVE → PAUSED → REVOKING → REVOKED | ERROR`; `preview_scope` with capped dry-run (entry/time cap, `unknown` when incomplete) and `grant_scope(preview_token, consent)`.
3. Path safety: canonicalization, reparse point/junction/symlink handling, default exclusions (OS dirs, `.git`, `node_modules`, `.venv`, `dist`, `build`, caches, app data/models), ancestry check at read AND search time, persistent `scope_exclusions` for child-revoke-under-parent.
4. Parsers for UTF-8/UTF-16 `.md`/`.txt` into `documents` + `chunks` with line provenance; durable `index_jobs` with checkpoint, pause/cancel/resume.
5. `revoke_scope`: deny first (atomic REVOKING), cancel workers, purge exclusive chunks/FTS/derived memory, tombstone `result_interactions`, durable `purge_jobs` with SQL zero-leftover verification, WAL checkpoint per policy.

## Acceptance criteria
- [ ] Integration tests with scoped temp dirs: overlapping roots, child revoke under active parent (both dialog choices), symlink/junction escape, cycle, permission denied, long path.
- [ ] Revoke blocks results immediately even while purge is draining; purge resumes after simulated crash/restart and verifies zero leftovers.
- [ ] Source file hashes identical before/after all operations; no write call on source paths.
- [ ] Drive-root preview respects caps and exclusions; nothing indexes without explicit confirm.

## Deliverables and report
Code + automated tests + commands/results + platform/hardware/runtime versions + security/privacy notes. Mark real runtime tests as PENDING until observed. Keep weights and private data out of git.
