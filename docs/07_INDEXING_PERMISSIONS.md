# 07 — Folder Permissions, Indexing and Revocation

## Core principle
Only selected user-authorized source roots are scanned. **User can choose `C:\`** (Windows drive root), but app MUST guard against surprising scope, access errors, protected paths, massive sizes and accidental sensitive collection. App never assumes admin rights or bypasses ACLs.

## Scope state
`PENDING_PREVIEW → AWAIT_CONFIRM → ACTIVE → PAUSED → REVOKING → REVOKED` or `ERROR`. Each root has immutable ID, canonical path, user-facing display path, platform volume identity (when available), include and exclude patterns, created/revoked times, enable status, scan mode, last generation, counts and error summary.

## Grant root flow
1. OS-native picker; canonicalize path, resolve/identify reparse points and volumes, reject app data/model cache as target; validate read access without elevation.
2. If root is drive/volume root or sensitive path: prominent warning with estimated files, disk-space impact, indexing time unknown and excluded-system-folder preview. No silent auto-start.
3. Dry-run preview with capped traversal (e.g. first 10k entries/time cap); show approximate counts and **unknown** if incomplete. User confirms exactly what is indexed, path filters, max file size, mode.
4. Start scheduled bounded traversal; show progress/errors; can pause/cancel, resume after restart; CPU throttling and low-priority IO.
5. Apply canonical-path and ancestry inclusion/exclusion checks **at every read and search-time**. Scoped access is not equivalent to unrestricted machine access.

## Filters, ignore & recursion
Default exclude: OS directories (`Windows`, `$Recycle.Bin`, `System Volume Information`), hidden/protected locations requiring elevated rights, app runtime/data/models/temp, `.git`, `node_modules`, `.venv`, `dist`, `build`, caches, symlinks/junctions/reparse-point targets and network shares unless separately granted. User can customize exclusions but protected system paths still skipped unless explicit future audited support. No following links outside a confirmed scope, no junction cycles, limit depth, total files, max size, archive decompression size, per-file parse timeout and memory.

## Root overlap semantics
- Explicitly granted roots are independent. Root A + nested root B can coexist; each source record links to one or more grants via join table.
- Deny/exclusion rule for revoked root MUST be evaluated independently of broader active parents: **if user revokes child B while parent A still covers B, show a dialog:** (a) keep child indexed under A, or (b) create explicit persistent exclusion for B. Default propose (b) for privacy. This prevents silent re-collection.
- Every scope's ACL determines whether content may appear; do not leak through historical or vector caches.

## Index pipeline
`discover → classify → hash/stat → parse → normalize → chunk → embed → atomic commit → search-ready`.
- Parsers initial: UTF-8/UTF-16 `.md`, `.txt`, supported `.pdf`, `.png/.jpg/.webp` images. Code `.py/.ts/.js/.rs` MAY enter MVP with explicit extension list, no arbitrary files. OCR from OS adapter or approved local engine; screenshot visual embedding used only if validated.
- PDF large pages/image OCR fallback bounded; encrypted PDFs skipped with reason. Each chunk has page/line/bounding-box provenance when parser supports it.
- Delta: normalized path + platform file ID if available, modification size/time quick pass and BLAKE3 content hash on changed items; hash alone does not authorize access.
- Dedup by source content hash and generation; preserve multiple path identities where relevant. Real file modifications mark old derived content stale immediately while updating.
- FTS/text chunk and vector write use staged generation commit; never expose partially indexed/stale generation as current.
- Manual indexing first; idle sync opt-in. User can choose manual/idle and never background in initial default.

## Revoke / remove folder — safety critical
1. UI confirmation: **Remove from search + erase derived index**; original files untouched. Distinguish `Pause Indexing` (keeps searchable current data) vs `Remove Source` (immediate deauthorization).
2. Atomically mark scope REVOKING and deny read/search/results first; cancel workers, invalidate active result cards and active session cache.
3. Purge chunks, vectors, OCR text, thumbnails, derived memories and references exclusive to that scope. Shared sources remain only if independently granted and user explicitly opted to retain under another grant. Tombstone identifiers may remain without original path/snippet under local audit retention.
4. Handle interrupted purge with durable job retry at restart. Complete only after SQL checks prove no unauthorized derived records remain; follow with WAL checkpoint / secure delete policy.
5. Removing `C:\` does not delete source files or grant elevated privileges; show estimated purge size and status.

## Threat tests
Drive root scan under symlink loops; OneDrive junctions/redirects; UNC path error; OS permission denied; archive bomb; malicious PDF; long paths; deletion during read; disk full; app power loss; hide window while indexer running; switch scopes during live query; new scope overlaps revoked subtree; near-identical file replaced at same path.
