# 04 — User-Managed Model Registry & Capability Validation

## Requirement
Users may **register** their own local LLM, embedding checkpoint, STT and TTS model. Product only **activates compatible models**. UI provides Add Model per category plus import existing path and optional explicit download from known trusted sources. Never silently download anything.

## Categories
| Category | Required | MVP runtime | Fallback |
|---|---|---|---|
| Embedding | **YES: EmbeddingGemma 2 profile** | pinned EG2 adapter (text+image initial) | semantic search disabled; previously built index must not be scored with other model |
| LLM | No | llama.cpp + validated GGUF text instruction model | deterministic typed/search flow |
| STT | No | approved local Whisper adapter | text input |
| TTS | No | approved local model/runner per language | text output |

## Add Model workflow
1. User chooses model type in Settings → Models; `Import local folder/file`, or `Download` after license/size/destination confirmation.
2. App computes metadata (path, canonical source, model files, format, hash if feasible, model card/license acknowledgment if distributing).
3. Adapter performs cheap static probe (GGUF header/config/tokenizer/files); unsupported format → `INCOMPATIBLE`, with reason and link to compatible format help.
4. Explicit sandboxed smoke inference/embedding (on user action or first use) → capability detection, latency, memory, output schema/dimension validation. All errors recoverable and logged locally without model input content.
5. Model saved as `AVAILABLE`, not green `READY` until runtime loaded + health check succeeds. Set as Active only on successful compatible verification.
6. If active EmbeddingGemma 2 path missing/corrupt → show **red mandatory status**; user can repair and/or reimport. **No unrelated embedding model substituted silently**.

## Status protocol
- `MISSING`: required profile absent / referenced weights deleted (RED, action required).
- `REGISTERED`: metadata known, not validated (neutral).
- `VALIDATING`: test in progress (spinner).
- `INCOMPATIBLE`: unsupported/runtime mismatch (RED).
- `AVAILABLE`: validated and on disk, runtime not loaded (NEUTRAL or amber; NOT green).
- `LOADING`, `UNLOADING`: transitional, spinner.
- `READY`: model actually loaded and healthy (GREEN).
- `BUSY`: executing inference (GREEN + activity label).
- `IDLE`: loaded, warm and not busy (GREEN + TTL countdown).
- `ERROR`: failure to load or infer (RED, reason and retry).

Optional category absent should be `Not configured` neutral, NOT global red. Required embedding missing red. Accessible text state, not color only. `Available` must not be mislabeled active. Status transitions dispatched from backend telemetry, never inferred from a button click.

## Supported vs merely importable
MVP LLM is tested **GGUF supported architectures** via pinned `llama.cpp` binary. A `safetensors` path cannot be passed to GGUF runtime without an explicit supported conversion workflow. Arbitrary custom compiled executables **not** accepted as add-ons (security). Support for ONNX/LiteRT/TTS/PyTorch depends on separately signed/validated app-managed adapters, not file extension guesses.

## Compatibility facts and pinned manifest
Store: `model_id`, `category`, `provider`, `path`, `format`, `architecture`, `weights_sha256`, `model_revision`, `license_name`, `license_url`, `runtime_adapter`, `adapter_version`, `capabilities`, `language_tags`, `embedding_dim` (if relevant), `preprocessing_fingerprint`, `validated_at`, `status`, `last_error`. Example under `examples/model-manifest.example.json`.

## EmbeddingGemma 2 advanced details
Official model: 740M total / 270M text / 170M vision / 300M audio; 768d base with 128/256/512 alternatives. Initial text+image to meet screenshot use case; audio/video embeddings deferred. Selected dimension, normalization, task instructions and model revision are indexed together. Truncated embeddings require re-normalization. Test query/document task prompts as described in official model card; do not assume cosine scores are calibrated confidence probabilities.

## Custom model UX
Show display name, format, model size, source path, last validation, adapter, memory cost (measured or `Not measured`), primary role, load/unload controls, editable TTL and health/error log. Swapping embedding model requires **new index generation / migration**, never direct reuse of incompatible vectors.
