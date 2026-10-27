# 10 — Windows-first, macOS-second Platform Contract

## Framework
Tauri v2 + Rust + React/TypeScript on same repo. Platform-specific shell operations behind `PlatformServices` trait; do not sprinkle Windows APIs through UI/core. Windows target x86_64; macOS target arm64/Apple Silicon first. CI builds binaries independently; model weights installed/imported per OS.

## Windows (first verification machine)
- Notification-area icon via Tauri tray APIs, including OS overflow (`^` hidden icons) behavior. Cannot programmatically force tray icon to always be visible in the overflow configuration.
- Close hides widget; show by tray/hotkey. Explicit Quit stops all worker process trees. Single-instance guard. Taskbar window hidden when widget hidden; Settings handles window separately.
- `Alt+Space` preference but Windows/system/PowerToys/other application conflicts possible. Detect registration failure, show warning and allow user reassignment (do not silently consume other app hotkeys).
- Local paths: per-user AppData app-specific dirs; model import path may point elsewhere; SQLite, logs, cache under app data. Use Windows file identity/reparse-point handling for path safety.
- GPU: RTX 4080 16 GB / CUDA runtime chosen by packaged binary validated on actual machine; system may have other consumers; no forced GPU overcommit.
- Installer `.msi`/NSIS choice spike; model weights NOT bundled by default unless licensing/size distribution plan deliberately changes.

## macOS (same code, validation on M1 Pro 16 GB)
- Menu-bar icon, explicit Quit, dock visibility/policy to be decided in macOS UX spike; avoid Windows-only tray assumptions.
- Default shortcut `Cmd+Shift+Space` (editable and conflict-aware); OS permission prompts for selected protected folders, Documents/Desktop/Downloads as applicable. Microphone permission explicit and revocable.
- `~/Library/Application Support/<app-id>` for SQLite/index/settings; `~/Library/Logs/<app-id>` for sanitized diagnostics; sandbox exceptions/notarization requirements assessed before public release.
- Metal/Apple Silicon inference runner must be built and tested; cannot copy Windows CUDA runtime; memory pressure must account for unified memory shared with system.
- `.app/.dmg` packaging; signing/notarization for outside developer test builds.

## Common invariants
Path normalization is OS-specific; store canonical path + user-facing raw path, source file ID when available, UTF-8 DB strings, case-sensitivity handling by filesystem, avoid assuming `C:\` outside Windows. Hotkey and tray APIs are platform adapters. UI responsive at HiDPI and multi-monitor.

## Capability probing
On first boot, detect OS/arch, runtime availability, GPU backend and inference build. Show what is supported, not what theoretically exists. If model unsupported on macOS but worked on Windows, register and mark `INCOMPATIBLE` with explanation; **do not auto-convert** or claim parity until tested.

## Minimum smoke tests
Tray show/hide + quit, shortcut conflict, folder picker, absolute/UNC or mac volume scope, permission revoke, screenshot indexing, model load/unload, voice permission denial, missing model path, machine sleep/resume, crash recovery, app update preservation, no model re-download on new OS.
