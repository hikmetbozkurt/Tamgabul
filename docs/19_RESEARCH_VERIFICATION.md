# 19 — External Technical Verification (checked 2026-10-26)

## Verified from official project documentation (public sources)
1. **EmbeddingGemma 2** is a 740M parameter multimodal embedding model from Google with 270M text, 170M vision and 300M audio components, unified **768-dimensional** representation and **128/256/512/768 Matryoshka** options; Apache 2.0. Model card: https://ai.google.dev/gemma/docs/embeddinggemma/model_card_2 . Google launch: https://blog.google/innovation-and-ai/technology/developers-tools/embeddinggemma-2/ .
2. Official guide documents embedding generation with SentenceTransformers, and separate domain fine-tuning guide with contrastive triplets / MultipleNegativesRankingLoss: https://ai.google.dev/gemma/docs/embeddinggemma/multimodal-embeddinggemma-with-sentence-transformers and https://ai.google.dev/gemma/docs/embeddinggemma/fine-tuning-embeddinggemma-with-sentence-transformers . A multimodal guide exists: https://ai.google.dev/gemma/docs/embeddinggemma/fine-tuning-multimodal-embeddinggemma-with-sentence-transformers . **LoRA-specific PEFT compatibility for EG2 needs a spike.**
3. Official **Gemma 4 E4B instruction tuned** weights are labeled Apache 2.0 on Google Hugging Face page: https://huggingface.co/google/gemma-4-E4B-it . The user's existing `google/gemma-4-e4b` Q4_K_M GGUF may be a third-party quantization; verify concrete GGUF and its model provenance/license before packaging.
4. Tauri v2 documents system tray, global shortcut and bundling external binaries (sidecars): https://v2.tauri.app/learn/system-tray/ ; https://v2.tauri.app/plugin/global-shortcut/ ; https://v2.tauri.app/develop/sidecar/ . This does NOT prove particular Win/mac runtime builds work; test on real hardware.
5. llama.cpp server supports sleep-on-idle and model router API in its current README: https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md . Pin a version and probe actual command flags; do not equate sleep response with confirmed VRAM release.
6. SQLite FTS5 and trigram tokenizer are documented: https://www.sqlite.org/fts5.html . SQL vector extension integration and packaging remain engineering tasks.

## Product choices / proposals, NOT externally verified performance facts
- Tauri + Rust + SQLite architecture; UI size; state IDs; TTL defaults (5/5/1/2 min); manual indexing first; source purge semantics; all target latency/memory metrics. These are product requirements and hypotheses, not test results.
- `C:\` root opt-in is permitted by product policy, but OS ACLs, symlinks/junctions, protected file access and data volume cannot be bypassed by app promise.
- `LLM add any local model` means **registered files with adapter compatibility check**, not every file will run.
- `0 VRAM when unloaded`: measured outcome to verify, not an architectural guarantee under OS/runtime-specific allocation behavior.

## P0-02 findings (2026-11-02)
Checked from the published files and the llama.cpp source, then confirmed on Windows (V02).
- **GGUF pair:** `ggml-org/embeddinggemma-2-GGUF` revision `6ea3e3a309d12a4779cfda98b8c19800d2d5da71` has the text model (`embeddinggemma-2-{BF16,Q8_0}.gguf`, arch `gemma-embedding2`, `embedding_length_out = 768`, non-causal, pooling 1) and an **mmproj** (`mmproj-embeddinggemma-2-{BF16,Q8_0}.gguf`, arch `clip`) that contains **both** encoders: `clip.has_vision_encoder = true` (projector `gemma4v`) and `clip.has_audio_encoder = true` (projector `gemma4a`, 128 mel bins). Both project to 512 = the text model width. Read from the real file headers; SHA-256 values are pinned in `crates/runtime/src/eg2.rs`.
- **Converter:** the files were produced with llama.cpp commit `914f7f89142e33e77833254d9c9b90c3cef7303b` (`.src_sha` on the Hub). A llama.cpp vendored on 2026-10-01 (llama-cpp-python 0.3.36) does **not** know `gemma-embedding2`, so the runtime must be a build at least as new as that commit. `eg2_spike` prints the version.
- **llama-server API (source-checked):** `/health` is public and answers 503 while loading; other endpoints need the `--api-key` bearer token. `/v1/embeddings` returns OpenAI-shaped, L2-normalized (`--embd-normalize 2` default) vectors. `/embedding` accepts `{"content": {"prompt_string", "multimodal_data": [base64]}}` and returns `[{index, embedding: [[...]]}]`. The media marker is **random per process** unless `LLAMA_MEDIA_MARKER` is set; read it from `/props.media_marker`. `/props.modalities` reports `vision`/`audio`.
- **Model card prompts:** query `task: search result | query: …`, document `title: … | text: …`; media without prefix; truncated (Matryoshka) vectors must be re-normalized.
- **Lifecycle:** "unloaded" is implemented as *worker process exited*; the OS frees its RAM/VRAM. Windows ties workers to the app with a Job Object (`KILL_ON_JOB_CLOSE`), Linux with `PR_SET_PDEATHSIG`; macOS has no equivalent and relies on explicit shutdown (gap).

## Open verification tickets
- V01: Pin Windows llama.cpp binary and run local E4B Q4_K_M load, JSON output, unload and RSS/VRAM check.
- V02: Pin EG2 checkpoint/version, text+image encoding correct vectors, user-selected local checkpoint, CPU/GPU and runtime packaging with minimal stack. **DONE 2026-11-03 on Windows** with llama.cpp build 12024 (`ddf48f85c`): text, image and audio embeddings 768d and sane; unload and idle unload end the process. Details in agent_tasks/01.
- V03: Pinned sqlite-vec and FTS5 builds on Win/mac; fallback strategy.
- V04: Chosen STT/TTS implementations, exact model versions/licenses, Windows/mac binary packaging and voice metrics. Do not assume previously named TTS licenses/performance without checking each release artifact.
- V05: macOS window/menu-bar and shortcut behavior, permissions; packaging signing/notarization.
- V06: LoRA/PEFT adaptation of `google/embeddinggemma-2` exact modules/quantization/export to runtime under 16GB VRAM; only after MVP.

## Distinguish license layers
Model weights license, runtime binary license, dependency license and quantization derivative provenance are separate; record all. Original user weights referenced in-place are different from redistribution within installer.
