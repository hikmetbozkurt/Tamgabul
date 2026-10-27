# 11 — Security, Privacy & Threat Model

## Trust boundaries
- Trusted: signed packaged Rust app controller, allowlisted/signed runtime binaries and SQLite under app data.
- Untrusted: source PDF/MD/OCR text, LLM responses, imported model files, user-supplied metadata paths, archives, symlink targets, third-party model repositories.
- User-authorized but sensitive: selected roots (including `C:\`), conversation history, embeddings, OCR snippets, transcripts and file metadata.

## Privacy defaults
- No automatic networking, downloads, analytics, crash reporting, cloud inference, or background recording.
- All inferences local. Active downloading is explicit with destination, size, origin, license and progress. Network disabled in local-only mode except deliberate user-approved downloader action.
- File-scoped consent before any scan; index-derived content inherits access policy. Recording begins only on mic button, input device indicator visible.
- Source-file access read-only. No irreversible source deletion/renaming; `Remove Scope` means derived data purge.
- Logs: only structured error codes, sizes and timings; redact file paths (except explicit local debug mode), prompts, raw snippets and speech transcripts.

## Defense-in-depth
- Tauri capabilities scoped to minimal commands; UI cannot call filesystem arbitrary paths or shell-exec; commands validate selected roots and per-action intent.
- Never accept LLM text as native command, query SQL, path or executable; JSON schema + allowlist + confirmation for high-impact actions.
- Local workers loopback bind or pipes only; authenticated local transport, random port, child-process handle, no public server.
- Imported model weights are untrusted; do not deserialize pickles or load unreviewed Python modules from user-selected directories. Prefer GGUF, safetensors/known formats and pinned runner toolchains, signed packaged runtimes. Hashes for integrity, license review for redistribution.
- Parsers set page count, decompression limits, text length/file size, memory/time quotas; spawn sandbox/isolation as appropriate. Filesystem traversal rejects symlink/junction escape and follows ACLs without admin.
- Local database has OS user-file permissions. Consider SQLCipher/file encryption opt-in later if threat model requires at-rest protection; SQLite alone is **not encrypted**. Never claim 'binary format' grants privacy.
- Source content instructions are inert data. RAG citations required; malicious PDF cannot change system behavior.

## Revocation privacy contract
Permission removal **immediately** blocks all search/result access at authorization layer, even before asynchronous index purge finishes. Delete vectors/chunks/OCR text and derived thumbnails & memory source references. Durable purge on crash. Explain backups, WAL and file system forensic limitations: SQLite deletion alone is not cryptographic erasure; consider `secure_delete`, `VACUUM`, WAL checkpoint and backup deletion strategy with OS limitations.

## Abuse/failure scenarios
Malicious model file → handler crash; malicious PDF → parser exploit; network request from plugin; accidental C:\ indexing → secrets collected; symlink outside scope; revoked content shown from UI cache; LLM prompt injection in document; loopback API exposed to another process; model weights licensed for restricted use; PII in logs; source delete by misinterpreted 'shred'. Include regression tests for every class.

## User-facing controls
Models paths/licensing, authorized sources, audit of scope changes, local data location, history retention/delete, auto-run with OS (opt-in), microphone settings, local diagnostics export with redaction, fully exit app, remove own indices/memory and optionally saved model copies (never user-original outside app data without explicit OS confirmation).
