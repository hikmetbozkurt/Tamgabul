# Tamgabul LoRA Lab (`P9` Post-MVP Prep)

Offline-first tooling for domain adaptation experiments on **EmbeddingGemma 2**, aligned with [`docs/16_LORA_LAB.md`](../docs/16_LORA_LAB.md), [`templates/EXPERIMENT_TEMPLATE.md`](../templates/EXPERIMENT_TEMPLATE.md), and verification ticket **V06**.

> **Important:** Nothing here runs in MVP production builds or uploads user data. Any adapted model creates a **new `index_generation`** in SQLite without mixing old vectors, and rolling back to the base model reactivates the preserved generation cleanly.

## 1. Validate a triplet dataset (offline, stdlib only)

```bash
python ai_lab/validate_dataset.py ai_lab/dataset_template.jsonl
```

Checks performed:
- Required fields (`query`, `positive`, `hard_negative` / `hard_negatives`, `language`, `modality`, `project_group`, `source_consent_id`, `split`).
- Cross-split leakage detection: fails if any `project_group` spans multiple splits or any normalized `query` overlaps between `train` and held-out `val`/`test` splits.
- Deterministic SHA-256 dataset fingerprint.

## 2. Dry-run an experiment config (offline, no GPU required)

```bash
python ai_lab/train_eg2.py \
  --config ai_lab/experiment_config.example.yaml \
  --dataset ai_lab/dataset_template.jsonl \
  --report ai_lab/out/EXP-DRYRUN.md \
  --dry-run
```

Enforces:
- `precision: bf16` (`fp16` is rejected for EmbeddingGemma 2).
- `MultipleNegativesRankingLoss` configuration and disabled telemetry/uploads.
- `HF_HUB_OFFLINE=1` unless `--allow-hf-download` is explicitly provided.

## 3. Run unit tests

```bash
python -m unittest discover -s ai_lab -v
```
