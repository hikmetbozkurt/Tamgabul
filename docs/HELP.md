# Tamgabul Help

Tamgabul is an offline desktop widget that finds files inside folders you explicitly allow, using everyday Turkish or English descriptions. Nothing leaves your computer.

## 1. First-Time Setup

1. **Install `llama.cpp` (`llama-server`)** if your package did not bundle it:
   - Windows: `winget install ggml.llamacpp`
   - macOS: `brew install llama.cpp`
2. **Add the required embedding model (EmbeddingGemma 2):**
   - Open **Models → Add model → Embedding** and either choose your local `embeddinggemma-2` folder (`embeddinggemma-2-*.gguf` plus `mmproj-embeddinggemma-2-*.gguf` for images and audio) or switch to **Download official model** and confirm explicit consent (origin, total size, Apache-2.0 license, destination folder, SHA-256 verified).
   - Tamgabul validates file readability, GGUF headers, SHA-256 checksum, runtime compatibility, and runs a short test before saving.
3. **Add an optional local LLM (recommended: Gemma 4 E2B-it):**
   - Open **Models → Add model → LLM** and choose your local GGUF instruction model (`gemma-4-E2B-it-*.gguf`; `mmproj-*` in the same folder enables image descriptions during scanning), or use the consent-gated downloader for the pinned Gemma 4 E2B-it + vision preset.
   - Typed search still works without an LLM.
4. **Allow folders:**
   - Open **Folders → Add folder**, pick a directory, inspect the dry-run preview (file count, estimated index size, automatic exclusions), and click **Allow and scan**.

## 2. Searching

- Open the widget anytime with the global shortcut (`Ctrl + Shift + Space` on Windows, `Cmd + Shift + Space` on macOS) or click the tray / menu bar icon.
- Describe what you remember in Turkish or English (for example, `geçen haftaki fatura pdf` or `the passport page with my UK visa`).
- **Built-in filters:** Words like *bugün / today, dün / yesterday, geçen hafta / last week, bu ay / this month, PDF, ekran görüntüsü / screenshot, ses kaydı / audio, video* filter by date and file type. A request with only filters lists the newest matching files.
- **Prior results:** Phrases like *geçen bana bulmuştun* or *the file you showed me* check persisted search history first and re-verify folder access. If a folder was removed, Tamgabul tells you without revealing the revoked file.
- **With an optional LLM:** Depending on the job toggles in **Settings → Performance → LLM jobs**, the LLM can expand synonyms/exclusions, pick the best matches among the top 20 candidates (while keeping other matches visible under **Other results**), give a grounded one-sentence answer, write a cited `[1]` summary, and describe images once during scanning.

## 3. Models and Memory

Model **installation/validation** is separate from **running state**:

| Status | Color | Meaning |
|---|---|---|
| `Not installed` (Embedding) / `Error` / `Incompatible` | Red | Required embedding is missing, or a model failed validation / runtime startup. Semantic search is disabled until EmbeddingGemma 2 is valid. |
| `On disk` / `Validating` / `Loading` / `Unloading` | Amber | Validated on disk (`AVAILABLE`) or transitioning. `On disk` enables search—the worker starts automatically when needed. |
| `Ready` / `Idle` / `Busy` | Green | Worker process is currently loaded in memory. |
| `Optional` (LLM / STT / TTS) | Neutral | Optional capability is not installed; typed search works normally. |

- **Idle unload (TTL):** Configure per-model idle unload timers on **Models** (`Right after use`, `5 min`, `10 min`, `30 min`, `1 hour`, or `Never`).
- **Manual unload:** Click **Unload** on a model card, **Unload all**, or **Unload Models** in the tray / menu bar menu. Unloading exits the `llama-server` process so RAM/VRAM returns to the OS.

## 4. Folder Privacy and Safety

- **Read-only scanning:** Tamgabul never modifies or deletes your source files.
- **Automatic and per-folder exclusions:** Hidden folders, `.git`, `node_modules`, build/cache directories, symlinks/junctions (never followed), and online-only cloud placeholders are skipped automatically. Use **Exclusions** in a folder's `…` menu to exclude additional subfolders immediately without touching source files.
- **Drive roots (`C:\` or `/`) and network folders (`\\server\share`):** Allowed with an explicit risk confirmation checkbox. Drive roots exclude OS system directories automatically; network shares default to manual scans and hide their results if unreachable.
- **Folder watching & relink:** Toggle watching per folder to automatically run delta scans ~10 seconds after file changes settle. If you move or rename a folder, use **Relink** to point it to the new location without re-embedding unchanged files.
- **Removing a folder:**
  - **Stop watching only:** Stops future scans while keeping the existing index searchable.
  - **Remove and purge:** Immediately blocks the folder from search and history, then erases its derived index rows.
  - **Child inside an allowed parent:** Choose **Block and purge** (never scanned again even via the parent) or **Remove the separate grant only** (stays searchable through the parent).

## 5. System Tray / Menu Bar and Shortcut

- Closing a window (`X`) hides it to the tray (Windows) or menu bar (macOS) so background state stays ready.
- Use **Quit** in the tray / menu bar menu to terminate all model worker processes cleanly.
- If another app already owns your global shortcut, **Settings → General** highlights the conflict so you can bind a different modifier + key combination.

## 6. Troubleshooting

- **Search bar disabled / red status:** Check **Models**. EmbeddingGemma 2 must pass validation (`On disk` or loaded).
- **`llama-server` not found:** Install `llama.cpp` or ensure `llama-server` is bundled next to the executable, on `PATH`, or set in `TAMGABUL_LLAMA_SERVER`.
- **No text found in Turkish screenshots/scans (Windows):** Install the Turkish OCR feature in *Windows Settings → Time & language → Language*. (On macOS, Vision OCR is built in; live verification pending.)
- **Voice input / read aloud (optional STT & TTS):** Install `whisper-server` (for STT `.bin` Whisper GGML models, which also transcribe spoken words inside indexed audio files) and/or `piper` (for `.onnx` + `.onnx.json` Piper voices; note `tr_TR-dfki-medium` is CC-BY-NC-SA-4.0 non-commercial). Typing always works without them.
- **Exporting bug reports safely:** Use **Settings → Privacy → Export diagnostics**. The exported JSON contains versions, counts, statuses, and error codes only—no file paths, file names, queries, or conversation text.
