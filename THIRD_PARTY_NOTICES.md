# Third-Party Notices and License Layers

Tamgabul separates application code, optional bundled inference runtimes, user-supplied model weights, and transitive dependencies into distinct license layers.

## 1. Application

- **Tamgabul (Local Context Widget)** — Apache License, Version 2.0 ([LICENSE](LICENSE), [NOTICE](NOTICE)).

## 2. Inference Runtime (optional staged binary in packaged installers)

Model runner binaries are never committed to source control. When packaged installers bundle `llama-server`, the binary and its upstream license file are staged via `packaging/stage-runtime.ps1` (Windows) or `packaging/stage-runtime.sh` (macOS), which compute `SHA256SUMS.txt` and `runtime-manifest.json` directly from the staged files.

- **llama.cpp (`llama-server`)** — MIT License — <https://github.com/ggml-org/llama.cpp>
- **whisper.cpp (`whisper-server`, optional STT CLI/server)** — MIT License — <https://github.com/ggml-org/whisper.cpp>
- **Piper TTS (`piper`, optional external CLI process, never linked)** — `rhasspy/piper` (archived `2023.11.14-2`) is MIT Licensed (<https://github.com/rhasspy/piper>); the active successor `OHF-Voice/piper1-gpl` is GPL-3.0 Licensed (<https://github.com/OHF-Voice/piper1-gpl>). GPL-3.0 `piper1-gpl` binaries must **not** be bundled inside Apache-2.0 release installers without explicit license review; Tamgabul discovers and invokes user-installed external `piper` processes via CLI only.

## 3. Model Weights (user-supplied, never bundled or auto-downloaded)

Tamgabul does not bundle or auto-download model weights. Users either register local files in place from disk or explicitly consent to download pinned model presets from Hugging Face with SHA-256 verification before local validation, and remain responsible for reviewing each model card and derivative quantization provenance/license.

- **EmbeddingGemma 2 (`ggml-org/embeddinggemma-2-GGUF`)** — marked Apache-2.0 on Hugging Face — <https://huggingface.co/ggml-org/embeddinggemma-2-GGUF>
- **Gemma 4 E2B-it (`ggml-org/gemma-4-E2B-it-GGUF`)** — marked Apache-2.0 on Hugging Face — <https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF>
- **Whisper GGML weights (`ggerganov/whisper.cpp`)** — MIT License — <https://huggingface.co/ggerganov/whisper.cpp>
- **Piper Voices (`rhasspy/piper-voices`)** — repository card is MIT, while individual dataset licenses vary per voice `MODEL_CARD`:
  - `tr_TR-dfki-medium` dataset license: **CC-BY-NC-SA-4.0** (non-commercial) — <https://huggingface.co/rhasspy/piper-voices/blob/main/tr/tr_TR/dfki/medium/MODEL_CARD>
  - `en_US-ljspeech-medium` dataset license: **Public Domain (LJ Speech)** — <https://huggingface.co/rhasspy/piper-voices/blob/main/en/en_US/ljspeech/medium/MODEL_CARD>

## 4. Direct Libraries, Frameworks, and Fonts

The complete transitive dependency graph is recorded in `Cargo.lock` and `apps/desktop/package-lock.json`. Direct core dependencies listed in the in-app Licenses dialog (`apps/desktop/src/lib/licenses.ts`):

| Component | License | URL |
|---|---|---|
| SQLite (bundled via `rusqlite`) | Public domain | <https://www.sqlite.org/copyright.html> |
| Tauri + plugins (`dialog`, `autostart`, `global-shortcut`, `single-instance`, `window-state`) | Apache-2.0 OR MIT | <https://tauri.app/> |
| React, React DOM | MIT | <https://react.dev/> |
| `rusqlite` | MIT | <https://github.com/rusqlite/rusqlite> |
| `image` | MIT OR Apache-2.0 | <https://github.com/image-rs/image> |
| Symphonia (audio decoding) | MPL-2.0 | <https://github.com/pdeljanov/Symphonia> |
| `pdf-extract` | MIT | <https://github.com/jrmuizel/pdf-extract> |
| `zip` | MIT | <https://github.com/zip-rs/zip2> |
| `cfb` (legacy Office files) | MIT | <https://github.com/mdsteele/rust-cfb> |
| `calamine` (legacy Excel) | MIT | <https://github.com/tafia/calamine> |
| `notify` (folder watching) | CC0-1.0 | <https://github.com/notify-rs/notify> |
| `blake3` | CC0-1.0 OR Apache-2.0 | <https://github.com/BLAKE3-team/BLAKE3> |
| `unicode-normalization` | (MIT OR Apache-2.0) AND Unicode-3.0 | <https://github.com/unicode-rs/unicode-normalization> |
| `windows` (`windows-rs`) | MIT OR Apache-2.0 | <https://github.com/microsoft/windows-rs> |
| `ureq`, `serde`, `serde_json`, `sha2`, `base64`, `getrandom`, `win32job`, `thiserror` | MIT OR Apache-2.0 | <https://crates.io/> |
| `rustls`, `ring`, `rustls-webpki`, `webpki-roots`, `rustls-pki-types` (TLS for explicit model download only) | Apache-2.0 / MIT / ISC / CDLA-Permissive-2.0 | <https://crates.io/crates/rustls> |
| `ts-rs` | MIT | <https://crates.io/crates/ts-rs> |
| Inter (font, `@fontsource/inter`) | OFL-1.1 | <https://rsms.me/inter/> |
