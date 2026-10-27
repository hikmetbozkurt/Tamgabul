# 02 — MVP Scope, Acceptance Tests and Exit Criteria

## MVP0 (headless technical spine)
- Rust core (or isolated Python-only spike explicitly marked temporary) with SQLite, index scopes, file parser, history.
- EmbeddingGemma 2 text & image modality feasibility measured locally; 768d indexed vectors, exact model revision pinned.
- GGUF LLM (existing Gemma 4 E4B) by app-managed local runtime, no LM Studio; structured intent extraction validated.
- CLI/test harness exercising plain text/mixed TR-EN/past-reference/correction.

## MVP1 (Windows usable product)
- System tray (notification area) with Show Widget / Settings / Pause Indexing / Quit. Close main window **hides**, explicit Quit exits.
- Hotkey default `Alt+Space` with collision handling and UI rebinding; tray click always usable.
- Compact widget with keyboard query, context follow-ups, citations and open-file action; Settings tabs Models, Sources, Performance, Privacy.
- Model import/validation for supported GGUF LLM; EmbeddingGemma 2 checkpoint import/managed download **only on explicit consent**; STT/TTS runtime selection from supported manifests. Configuring arbitrary model through manifest is allowed but incompatible remains disabled.
- Folders explicit opt-in; manual indexing, pause/cancel, progress, delta reindex, remove scope purge. Drive root confirmation/preview/exclusions.
- SQLite persistent memory independent of KV cache; transient LLM unavailable path does not lose event history.
- Per-model manual unload and configurable auto-unload TTL. Show actual status, resource measurements and errors.
- Audio typed-first milestone; local STT/TTS integrated as MVP1 completion gate, behind tested runtime adapters.
- Windows installer packaging; no external LM Studio, internet required only if user explicitly downloads model.

## MVP2 (macOS validation, same repo)
- macOS menu bar, shortcut fallback, permission prompts, `~/Library/Application Support` paths.
- Apple Silicon runtime backend validated; privacy control behavior matches Windows.
- macOS `.dmg`/`.app`, codesigning/notarization plan for release (developer test builds may be unsigned).

## Definition of 'working'
1. Import a supported embedding checkpoint and GGUF model, validate, visibly transition to `AVAILABLE` then `READY` after loading.
2. Select permitted folder with docs and screenshot; search different-language paraphrase and see correct file, path, snippet, clickable reveal.
3. Follow-up “hayır X projesindeydi” updates filters without destroying prior session.
4. Reboot/reopen after all model unload; ask “geçen bana bulduğun dosya” and retrieve from persisted event record, or ask for a missing discriminator.
5. Remove indexing scope; file no longer appears immediately (even if purge job still draining); counts and derived data removed per completion report.
6. Source file hashes before/after match; app owns no write permission to source paths beyond OS normal capability, and never calls write on them.
7. Full Quit leaves zero managed children; auto-unload across idle TTL observed with OS metrics.
8. Missing mandatory embedding shows red; no false green due solely to registered model.
9. No upload/network call during local-only operation (including logs and crash collection).
10. Local STT/TTS voice roundtrip when installed; safe typed mode otherwise.

## Quality gates
- Correctness > speed. Percent targets and resource budgets come from `14_TEST_EVAL.md` and may be changed only after baseline evidence.
- No critical data leakage, path traversal, unauthorized directory reading, source-file mutation or child-process orphan bugs.
- Fresh Windows profile onboarding works with **zero pre-installed models**; app remains in explicit not-ready state, not broken.
- `C:\` root and revocation tested against nested junction/symlinks, locked directories, large directory trees, cancel/restart.
- Fixed non-destructive upgrades and downgrade policy for schema changes.

## Deferred despite being tempting
Automatic blanket import of all user models, auto ML model download, continuous full drive crawling, disk-wide watch services, multimodal video/audio ingestion, semantic clustering, autonomous content summarization without citations, LoRA training in-app, cross-device sync.
