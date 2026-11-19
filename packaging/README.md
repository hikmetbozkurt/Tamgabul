# Packaging & Offline Release Guide

Tamgabul ships without model weights and without an auto-updater (`createUpdaterArtifacts: false`). Users add local GGUF models in place from disk.

## 1. Windows Installer (NSIS & WiX MSI)

`apps/desktop/src-tauri/tauri.conf.json` configures:
- **NSIS (`.exe`):** `installMode: "currentUser"` (no administrator elevation required), English + Turkish language selector.
- **WiX (`.msi`):** `en-US` and `tr-TR` localization, `allowDowngrades: false`.
- **WebView2:** `webviewInstallMode: { "type": "skip" }` by default (preinstalled on Windows 11 and most Windows 10 systems; avoids unexpected network bootstrapper fetches). For a fully offline Windows 10 installer that includes the Microsoft Edge WebView2 standalone installer, override `bundle.windows.webviewInstallMode` to `offlineInstaller` in a build-specific config overlay.
- **Bundled notices & offline help:** `LICENSE`, `NOTICE`, `THIRD_PARTY_NOTICES.md`, `docs/HELP.md`, and `docs/HELP.tr.md` are packaged into the app resource directory.

### Build without bundled `llama-server` (app uses `TAMGABUL_LLAMA_SERVER`, `PATH`, or `winget`)

```powershell
cd apps\desktop
npm ci
npm run tauri build
```

### Build with locally staged `llama-server`

1. Stage your verified local `llama.cpp` directory (containing `llama-server.exe`, required DLLs, and `llama.cpp`'s `LICENSE`):

   ```powershell
   .\packaging\stage-runtime.ps1 -SourceDir "C:\path\to\llama-cpp"
   ```

   This generates `packaging\staged\llama\SHA256SUMS.txt` and `runtime-manifest.json` from the actual staged binaries without touching the network.

2. Build the installer using the runtime overlay config:

   ```powershell
   cd apps\desktop
   npm run tauri build -- --config ..\..\packaging\tauri.bundle-runtime.conf.json
   ```

## 2. macOS `.app` & `.dmg`

`apps/desktop/src-tauri/tauri.conf.json` configures:
- `minimumSystemVersion`: `"13.0"` (macOS Ventura+)
- `hardenedRuntime`: `true`
- `entitlements`: `apps/desktop/src-tauri/entitlements.plist` (App Sandbox omitted for arbitrary user-granted folders and local child processes; microphone entitlement intentionally omitted until P6 voice lands)
- `infoPlist`: `apps/desktop/src-tauri/Info.plist` (`NSDesktopFolderUsageDescription`, `NSDocumentsFolderUsageDescription`, `NSDownloadsFolderUsageDescription`)

### Build unsigned / developer test `.app` & `.dmg` on Apple Silicon

```bash
# Optional: stage local macOS llama-server + Metal/dylib files + LICENSE
./packaging/stage-runtime.sh /path/to/llama-cpp-bin

cd apps/desktop
npm ci
npm run tauri build
# Or with staged runtime:
# npm run tauri build -- --config ../../packaging/tauri.bundle-runtime.conf.json
```

`locate::find_llama_server` checks `Contents/MacOS/`, `Contents/Resources/llama/`, `PATH`, and Homebrew (`/opt/homebrew/bin/llama-server`, `/usr/local/bin/llama-server`).

### macOS Signing & Notarization Checklist (for public release)

- [ ] Set `APPLE_SIGNING_IDENTITY` ("Developer ID Application: ...") and sign any staged `llama-server` / `.dylib` binaries with `--options runtime` before bundling.
- [ ] Verify bundle signature: `codesign --verify --deep --strict --verbose=2 Tamgabul.app`
- [ ] Submit `.dmg` to Apple notarization: `xcrun notarytool submit Tamgabul_*.dmg --keychain-profile ... --wait`
- [ ] Staple ticket: `xcrun stapler staple Tamgabul_*.dmg`
- [ ] Verify Gatekeeper acceptance: `spctl -a -t open --context context:primary-signature -v Tamgabul_*.dmg`

## 3. Clean-Profile & Offline Verification Checklist (Release Gate, P7)

Real hardware and clean-VM verification status is **Unverified/Blocked** until executed on target OS profiles:

- [ ] **Clean Windows user profile (zero models):** Install per-user NSIS `.exe` without admin; confirm onboarding opens, EmbeddingGemma 2 shows red `Not installed`, typed UI/Folders/Settings work without crash, and Quit leaves zero child processes.
- [ ] **No outbound network traffic:** Observe under firewall/packet capture during install, first run, indexing, and search that zero outbound connections occur.
- [ ] **Upgrade & schema backup:** Upgrade over an older DB version and verify `<app_data_dir>/tamgabul.db.bak-v<N>` is written before migrations run.
- [ ] **macOS parity (M1 Pro / Apple Silicon):** Verify menu-bar icon, `Cmd+Shift+Space` shortcut, Metal `llama-server` execution, folder permission prompts, sleep/resume behavior, and Quit cleanup.
