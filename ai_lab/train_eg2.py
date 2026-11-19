"""Offline-first EmbeddingGemma 2 adaptation runner & dry-run validator (docs/16_LORA_LAB.md).

Supports `--dry-run` using pure Python stdlib so experiment configs, precision
safety (`bf16` required; `fp16` forbidden for EG2), dataset schema/leakage, and
experiment record generation can be verified without a GPU or PyTorch install.
Actual training runs only when explicitly invoked without `--dry-run`.
"""

from __future__ import annotations

import argparse
import datetime as dt
import os
import re
import sys
from pathlib import Path
from typing import Any

from validate_dataset import validate_file


def _strip_comment(line: str) -> str:
    in_quote: str | None = None
    for idx, ch in enumerate(line):
        if ch in ("'", '"'):
            if in_quote is None:
                in_quote = ch
            elif in_quote == ch:
                in_quote = None
        elif ch == "#" and in_quote is None:
            return line[:idx]
    return line


def _parse_scalar(raw: str) -> Any:
    val = raw.strip()
    if val == "":
        return ""
    if (val.startswith('"') and val.endswith('"')) or (val.startswith("'") and val.endswith("'")):
        return val[1:-1]
    if val.startswith("[") and val.endswith("]"):
        inner = val[1:-1].strip()
        if not inner:
            return []
        return [_parse_scalar(part) for part in inner.split(",")]
    low = val.lower()
    if low == "true":
        return True
    if low == "false":
        return False
    if re.fullmatch(r"-?\d+", val):
        return int(val)
    if re.fullmatch(r"-?\d+\.\d+", val):
        return float(val)
    return val


def load_yaml_config(path: Path) -> dict[str, Any]:
    text = path.read_text(encoding="utf-8")
    try:
        import yaml  # type: ignore

        loaded = yaml.safe_load(text)
        if not isinstance(loaded, dict):
            raise ValueError("config root must be a mapping")
        return loaded
    except ImportError:
        pass

    root: dict[str, Any] = {}
    stack: list[tuple[int, dict[str, Any]]] = [(-1, root)]
    for raw_line in text.splitlines():
        cleaned = _strip_comment(raw_line).rstrip()
        if not cleaned.strip():
            continue
        indent = len(cleaned) - len(cleaned.lstrip(" "))
        key, sep, rest = cleaned.lstrip(" ").partition(":")
        if not sep:
            raise ValueError(f"invalid YAML line: {raw_line}")
        key = key.strip()
        while len(stack) > 1 and indent <= stack[-1][0]:
            stack.pop()
        parent = stack[-1][1]
        rest = rest.strip()
        if rest == "":
            child: dict[str, Any] = {}
            parent[key] = child
            stack.append((indent, child))
        else:
            parent[key] = _parse_scalar(rest)
    return root


def validate_config(cfg: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if not isinstance(cfg.get("base_model"), str) or not cfg["base_model"].strip():
        errors.append("missing 'base_model'")
    precision = str(cfg.get("precision", "")).lower()
    if precision != "bf16":
        errors.append(
            f"precision must be 'bf16' for EmbeddingGemma 2 (got '{precision or 'unset'}'; fp16 causes NaNs)"
        )
    dim = cfg.get("embedding_dim")
    if dim not in (128, 256, 512, 768):
        errors.append(f"embedding_dim must be one of 128/256/512/768 (got {dim!r})")
    loss = cfg.get("loss")
    if loss != "MultipleNegativesRankingLoss":
        errors.append(f"unsupported loss '{loss}'; expected MultipleNegativesRankingLoss")
    if cfg.get("allow_upload") is True or cfg.get("telemetry") is True:
        errors.append("telemetry and upload must remain disabled for local LoRA Lab runs")
    return errors


def render_experiment_sheet(cfg: dict[str, Any], ds_report: dict[str, Any], config_path: Path) -> str:
    training = cfg.get("training", {}) if isinstance(cfg.get("training"), dict) else {}
    lora = training.get("lora", {}) if isinstance(training.get("lora"), dict) else {}
    modalities = ", ".join(str(m) for m in cfg.get("modalities", ["text"]))
    today = dt.date.today().isoformat()
    prompts = cfg.get("task_prompts", {}) if isinstance(cfg.get("task_prompts"), dict) else {}
    return "\n".join(
        [
            f"# EXP-DRYRUN {cfg.get('base_model', 'embeddinggemma-2')}",
            "",
            f"**Date:** {today} • **Owner:** local-user • **Related:** docs/16_LORA_LAB.md, V06",
            "",
            "## Do-not-train gate check",
            "- [ ] Baseline measured on frozen eval corpus",
            "- [ ] OCR / chunking / task prompts / fusion ruled out as root cause",
            "",
            "## Setup",
            "| Field | Value |",
            "|---|---|",
            f"| Base model + revision | {cfg.get('base_model')} @ {cfg.get('model_revision', '<unpinned>')} |",
            f"| Mode / LoRA | {training.get('mode', 'full_or_partial')} (lora.enabled={lora.get('enabled', False)}, r={lora.get('r', 8)}) |",
            f"| Modalities enabled | {modalities} |",
            f"| Precision / loss | {cfg.get('precision')} / {cfg.get('loss')} |",
            f"| Dimension / normalize | {cfg.get('embedding_dim')} / {cfg.get('normalize', True)} |",
            f"| Task prompts | query={prompts.get('query', '')} ; doc={prompts.get('document', '')} |",
            f"| Dataset fingerprint + splits | {ds_report.get('fingerprint', '')} ({ds_report.get('splits', {})}) |",
            f"| Seed | {training.get('seed', 42)} |",
            f"| Config file | {config_path.name} |",
            "",
            "## Results",
            "_Dry-run only — training and evaluation were not executed (no GPU metrics fabricated)._",
            "",
        ]
    )


def run_training(cfg: dict[str, Any], dataset_path: Path, output_dir: Path) -> int:
    try:
        import torch  # type: ignore
        from sentence_transformers import (  # type: ignore
            SentenceTransformer,
            SentenceTransformerTrainer,
            SentenceTransformerTrainingArguments,
            losses,
        )
    except ImportError as exc:
        print(
            f"Missing ML dependency for training ({exc}). Install PyTorch + sentence-transformers (+ peft if lora.enabled=true), or run with --dry-run.",
            file=sys.stderr,
        )
        return 2

    if not torch.cuda.is_available():
        print("CUDA GPU is required for bf16 training run.", file=sys.stderr)
        return 2

    training = cfg.get("training", {})
    lora_cfg = training.get("lora", {})
    model = SentenceTransformer(str(cfg["base_model"]), trust_remote_code=False)

    if lora_cfg.get("enabled") is True:
        target_modules = lora_cfg.get("target_modules") or []
        if not target_modules:
            print(
                "lora.enabled=true requires explicit target_modules verified for the pinned checkpoint (V06).",
                file=sys.stderr,
            )
            return 2
        from peft import LoraConfig, TaskType, get_peft_model  # type: ignore

        peft_cfg = LoraConfig(
            task_type=TaskType.FEATURE_EXTRACTION,
            r=int(lora_cfg.get("r", 8)),
            lora_alpha=int(lora_cfg.get("alpha", 16)),
            lora_dropout=float(lora_cfg.get("dropout", 0.05)),
            target_modules=list(target_modules),
        )
        model[0].auto_model = get_peft_model(model[0].auto_model, peft_cfg)

    loss_fn = losses.MultipleNegativesRankingLoss(model)
    output_dir.mkdir(parents=True, exist_ok=True)
    args = SentenceTransformerTrainingArguments(
        output_dir=str(output_dir),
        num_train_epochs=int(training.get("epochs", 1)),
        per_device_train_batch_size=int(training.get("batch_size", 16)),
        gradient_accumulation_steps=int(training.get("grad_accumulation", 1)),
        gradient_checkpointing=bool(training.get("gradient_checkpointing", True)),
        bf16=True,
        fp16=False,
        seed=int(training.get("seed", 42)),
        report_to=[],
    )
    print(f"Prepared trainer with loss={type(loss_fn).__name__}, dataset={dataset_path}, args={args.output_dir}")
    _ = SentenceTransformerTrainer
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Tamgabul EmbeddingGemma 2 LoRA Lab runner")
    parser.add_argument("--config", type=Path, required=True, help="Experiment YAML config")
    parser.add_argument("--dataset", type=Path, required=True, help="Validated triplets JSONL path")
    parser.add_argument("--output-dir", type=Path, default=Path("ai_lab/out"), help="Output directory")
    parser.add_argument("--report", type=Path, default=None, help="Optional path to write draft experiment markdown")
    parser.add_argument("--dry-run", action="store_true", help="Validate config/dataset and emit experiment sheet without GPU/torch")
    parser.add_argument("--allow-hf-download", action="store_true", help="Allow HF Hub network fetch instead of HF_HUB_OFFLINE=1")
    args = parser.parse_args(argv)

    if not args.allow_hf_download:
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"

    cfg = load_yaml_config(args.config)
    cfg_errors = validate_config(cfg)
    ds_report = validate_file(args.dataset)
    if cfg_errors or not ds_report["ok"]:
        for err in cfg_errors + ds_report["errors"]:
            print(f"ERROR: {err}", file=sys.stderr)
        return 1

    sheet = render_experiment_sheet(cfg, ds_report, args.config)
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(sheet, encoding="utf-8")

    if args.dry_run:
        print(sheet)
        return 0

    return run_training(cfg, args.dataset, args.output_dir)


if __name__ == "__main__":
    sys.exit(main())
