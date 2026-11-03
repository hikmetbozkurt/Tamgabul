# P1-01 SQLite, Source Scopes and Revocation

**Phase:** P1 • **Owner:** Data • **Status:** IN PROGRESS (core flow done; Windows run PENDING) • **Dependencies:** P0-01

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

### Progress report — 2026-11-03
- **Done:** `crates/core/src/index/` — `walk` (canonical roots, UNC refused, never follows symlinks/junctions/reparse dirs, skips OneDrive online-only files, default exclusions incl. drive-root OS folders, deny paths for app data + model folders, file/depth/time caps, preview `complete` flag), `text` (UTF-8/BOM, UTF-16 LE/BE, Windows-1254 fallback, binary refused; ~1000-char chunks with 1-based line provenance), `store` (scopes, overlap/parent detection, exclusions, `authorized()` for read/search-time checks, generations by embedding identity, per-document atomic commit that re-checks ACTIVE, delta via size+mtime then BLAKE3, unlink deleted files, revoke = deny first + durable purge with SQL verification + WAL checkpoint, resume on start), `job` (scan pipeline, cancel, progress, durable `index_jobs`). Embeddings are produced during the scan with EmbeddingGemma 2 (step pulled forward from P2-01); the model is held only while scanning.
- **App:** `list_scopes`, `index_stats`, `preview_scope`, `grant_scope` (starts the scan the user confirmed), `start_index`, `cancel_index`, `revoke_scope` (deny synchronously, purge in background), interrupted purges/scans resume at start; `scope_status` events drive the Folders page.
- **Tests:** 13 unit + 7 integration (`crates/core/tests/index.rs`): scan + delta rescan, Turkish FTS, line provenance, remove & purge leaves zero rows and source hashes unchanged, child-under-parent both choices, revoke during scan, purge resume + idempotence, stop-watching keeps index, cancel, generation switch.
- **Update 2026-11-03:** PDFs (text per page via `pdf-extract`, isolated thread + 30 s limit + panic catch; scanned PDFs skipped as `pdf_no_text`) and images (`png/jpg/jpeg/bmp/gif`, raw bytes to the EG2 vision encoder; skipped if no mmproj) are indexed. Permanent skips are not retried on rescan.
- **Not yet:** watching/idle sync (toggle disabled; ADR 016 manual default), editing exclusions, relink, OCR for scanned PDFs/screenshots, Office/code files, junction/OneDrive/long-path tests on real Windows, CPU/IO throttling.
