# 16 — Future EmbeddingGemma 2 Adaptation / LoRA Lab (Post-MVP)

> **Not in initial MVP.** Intended for local experiments on NVIDIA RTX 4080 16GB after a working search and evaluation baseline. Memory/time feasibility is **to be measured**, not a 15-minute promise. Training users' data requires consent and local-only handling.

## Hypothesis
Adapt `google/embeddinggemma-2` for Turkish-English mixed developer queries (e.g., “auction timer bug'ını bul”), unusual project names, code-related screenshot/doc associations. Success means **better held-out retrieval**, not simply lower training loss.

## Do-not-train gate
First benchmark base model across topical synthetic/private corpora. If retrieval issues stem from bad OCR, chunking, incorrect task prompts, missing images or incorrect ranking fusion, fix these first. Only adapt when error breakdown shows representation/domain gap.

## Research baseline
Google official guides demonstrate contrastive training of EmbeddingGemma 2 using `(anchor, positive, negative)` triplets with `SentenceTransformerTrainer` + `MultipleNegativesRankingLoss`, including multimodal triplets. This is a supported **fine-tuning workflow**, NOT proof that any particular PEFT LoRA target-module configuration works out of the box. LoRA integration must first be separately proven for exact revision/framework. Official guide: https://ai.google.dev/gemma/docs/embeddinggemma/fine-tuning-embeddinggemma-with-sentence-transformers ; multimodal: https://ai.google.dev/gemma/docs/embeddinggemma/fine-tuning-multimodal-embeddinggemma-with-sentence-transformers

## Dataset template
Local JSONL in `ai_lab/dataset_template.jsonl`. Fields: `query`, `positive`, `hard_negative`, `language`, `modality`, `project_group`, `source_consent_id`, `split`. For image inputs, provide local path with explicit consent; never embed raw private path/content into public git. `split` by *project and time*, avoid near-duplicate leakage, freeze held-out test before any experiments.

## Experiment checklist
1. Document baseline model hash, revision, modalities, encoder load config, normalization, task prompts and 768d vs 256d index; freeze eval corpus version.
2. Build ≥1,000 quality triplets as a *candidate target*, not requirement or guarantee; collect hard negatives (similar projects/screens) and multilingual positive paraphrases.
3. Run small reproducible text-only pilot with vision/audio encoders disabled to fit budget; CUDA bf16 on RTX 4080 if supported; measure peaks, batch, gradient accumulation, loss, out-of-memory.
4. Try baseline full/partial fine-tune using official supported pipeline. **If LoRA is pursued**, inspect module names and PEFT compatibility, choose target modules/rank/dropout empirically. Begin `r=8 or 16`, batch small; no unsupported magic constants. Do not export as production model until inference adapter supports chosen merged weights.
5. Multimodal trial (text→image, image→text) only after text baseline passes; verify cross-modal gains and regressions, adapter support and training memory.
6. Evaluate on frozen test corpus: Recall@1/3, MRR, nDCG, per modality, TR-EN split, false positives, latency and inference memory. Report bootstrap CI if feasible.
7. Save exact checkpoints, config, seed, dataset fingerprint, training logs, license/provenance; keep private assets outside repo.
8. New embedding space → **new index generation**; transactional reindex staged in background on user-approved resources, rollback to old base model and index if quality/performance regresses.

## Release decision gates
Ship only if statistically/operationally meaningful gain over base on held-out queries, no safety/privacy regression, performance budget met, clear user opt-in, clean fallback and migration/rollback. Do not fine-tune solely as resume/CV exercise without product gains.

## Record sheet
Copy `templates/EXPERIMENT_TEMPLATE.md` per experiment. Example knobs under `ai_lab/experiment_config.example.yaml`. LoRA export to ONNX/LiteRT or local native inference MUST be validated on a cloned model before marking deployment compatible.

## Known limitations
GPU 16GB does not guarantee full multimodal training fits. Mixed precision, freezing optional encoders, quantization and grad checkpointing may help with tradeoffs; benchmark actual drivers and library versions. Official multimodal example itself describes accelerator requirements; do not assume immediate GGUF output or seamless arbitrary model import.
