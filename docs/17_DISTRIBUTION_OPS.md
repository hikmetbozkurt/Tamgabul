# 17 — Packaging, Local Ops & Release

## Distribution goals
One installable Windows desktop app that operates with **no LM Studio and no Python manually preinstalled**. Bundled runner executables/required native libraries are supervised and versioned. Downloading large model weights is optional/user initiated; copying from an existing local path remains primary.

## Installer checklist
- Windows `.msi` or NSIS selection; unsigned dev builds first, signed installer later. Tauri updater only after signing/security policy, opt-in check when offline-first; no mandatory update fetch.
- Bundle correct Windows target `llama.cpp` and selected embedding/STT/TTS worker binaries with their complete license notices and SHA-256 inventory.
- Store generated SQLite, indices and logs under OS app data; never inside app install directory; user imported model files remain where selected unless user chooses `Copy into app models`.
- No admin installer requirement unless strictly unavoidable; do not scan privileged directories by auto elevation.
- Keep versioning `app_version / schema_version / runtime_adapter_version / index_generation`. Schema migrations backup before application; do not downgrade DB without explicit compatible migration.

## Update/migration
- Persist user settings/scope IDs, model paths, TTL, history, index metadata; backup and test migration. If index fingerprint changes, show reindex size/time estimate and preserve previous generation until validated.
- If model file moved, mark MISSING and allow re-link, not auto-delete user data.
- If sidecar binary is missing, show model ERROR, keep app UI and local records accessible.

## Start/exit
Single instance with foreground existing window; autostart only if user enables. Tray continues running while widget hidden, but explicit Quit terminates all models/jobs. Safe restart recovery of job queue and purge queue, corrupted DB backup/recovery flow.

## macOS packaging
Build native Apple Silicon runner, `.app/.dmg`; assess Gatekeeper, codesigning/notarization, mic permission metadata and user selected folder read grants. Same logical data migrations and model manifests.

## Operational scripts (planned)
`check-runtime` lists runner versions and supported capabilities; `diagnostics` displays safe anonymized metrics; `index-diagnostics` reports jobs and orphan records without exposing content; `export-settings` exports non-secret setup; `delete-local-data` requires confirmation and clearly distinguishes user model files.

## Release notes template
Supported model formats/architectures and tested versions, exact macOS/Windows support, data privacy statement, installation known issues, benchmark machine and version, migrations, security changes, rollback instructions, SHA-256 of bundled runners.
