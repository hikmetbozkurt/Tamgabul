# 03 — System Architecture

## Logical graph
```text
                        global shortcut / tray
                                |
                      Tauri UI (React/TS)
                                | typed IPC
                   Rust App Controller / IPC guard
             +------------------+-------------------+
             |                  |                   |
    Conversation Router    Scope & Indexer    Model Manager
       |           |           |                   |
  SQLite Memory  Search    parsers / OCR       approved runners
       |        |      |            +----------+------+------+------+
       |       FTS5  vector        GGUF LLM  EG2 emb   STT   TTS
       |          \    /             workers (localhost/IPC only)
       +------ Retrieval ----+---- provenance / score / response
                         |
                   Result card + TTS
```

## Deployment and process decisions
- `apps/desktop`: **Tauri v2** host and React + TypeScript UI, i18n keys; UI performs no privileged file traversal.
- `crates/core`: Rust service libraries for DB, scope validation, indexing scheduler, context and model manager, exposed as Tauri commands. No cloud dependency.
- `crates/platform`: adapters for tray/menu-bar, paths, autostart (opt-in), opening source file in file explorer, CPU/GPU metrics.
- `runtimes/llm`: bundled **llama.cpp** binary or embedding integration proven in spike (LLM must not depend on LM Studio).
- `runtimes/embedding`: EmbeddingGemma 2 model adapter. PoC may use pinned Python/SentenceTransformers managed sidecar; production may replace with LiteRT/ONNX/native adapter **only after feature parity & licensing checks**. A sidecar can be part of the app; users must not install Python separately for a release.
- `runtimes/audio`: STT/TTS workers under the same process lifecycle manager; voice model/runtime compatibility is explicitly tested.
- SQLite is the single local authority for scopes, session records, events, source records, model registry/config and index manifests. Vectors can be in `sqlite-vec` extension or separate local file if adapter benchmark says so, but transactional logical relationships MUST remain.

## Cross-cutting invariants
- UI does not invoke shell commands or traverse unscoped paths; Rust validates each command.
- Query intent has machine-validated schema; no LLM output is accepted as file path authority or permission grant.
- Embedding model revision + dimension + preprocessing/task prompt + adapter revision = **index generation identity**. Versions never mixed.
- Model runner execution isolated behind standardized adapters; child process death transitions to ERROR and returns a typed error.
- Running models can consume memory; dormant workers and readiness checks must not accidentally wake them.
- App stores source paths and derived content locally; data is confidential, with opt-in history retention and guarded diagnostics.

## Typical query
1. UI sends session-aware utterance (text or locally-transcribed audio) to Rust.
2. Memory retrieves last active session plus relevant past events using FTS/time/file relationships; deterministic rules recognize simple direct commands.
3. Optional LLM parses ambiguous requests into strict JSON action proposal; Rust validates intent/filters against allowed enum and scope.
4. Search uses FTS5 + embedding vectors + filename/metadata boosting (RRF optional); only authorized, current index-generation sources compete.
5. Model may create a concise answer **only from listed result snippets**, including file citations. Weak results trigger clarification instead of fabricated success.
6. Memory persists user query, recognized intent, source IDs returned/shown/opened, correction and errors; app offers an inspectable history UI.

## Background indexing
Folder watcher optional; MVP default Manual. Scheduler lists only authorized roots, observes exclusions/symlink policy, creates parse tasks and embeddings with bounded concurrency, supports cancellation and durable checkpointing. No unbounded `C:\` walk.

## Failure isolation
No embedding: semantic features unavailable; DB and settings still work; optional FTS fallback labeled degraded. LLM crash: context FSM + retrieval work. STT/TTS crash: text input/output remain usable. DB migration fails: back up, fail safely and guide recovery; never silently reset user history.

## Architecture verification spikes
A1: E4B Q4_K_M in pinned llama.cpp, structured JSON validity, load and unload metrics. A2: EG2 native vs packaged managed worker parity for 768d text/image; tokenizer prompts, CPU/GPU resource footprint. A3: SQLite/fts5/vector extension packaging on Windows/macOS. A4: tray hotkey collisions & close-to-tray under both OSes.
