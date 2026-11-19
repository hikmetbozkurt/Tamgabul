"""Dataset validator for EmbeddingGemma 2 adaptation experiments (docs/16_LORA_LAB.md).

Pure Python standard library — runs completely offline with zero ML dependencies.
Validates schema, consent IDs, split isolation (`project_group` leakage and
normalized query overlap), and computes a deterministic SHA-256 fingerprint.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

ALLOWED_LANGUAGES = {"tr", "en", "mixed"}
ALLOWED_SPLITS = {"train", "dev", "val", "test"}
ALLOWED_MODALITIES = {"text", "text_to_image", "image_to_text", "text_to_audio"}
REQUIRED_FIELDS = (
    "query",
    "positive",
    "language",
    "modality",
    "project_group",
    "source_consent_id",
    "split",
)


def canonical_split(split: str) -> str:
    return "val" if split == "dev" else split


def normalize_text(value: str) -> str:
    return re.sub(r"\s+", " ", value.strip()).casefold()


def validate_record(row: Any, line_no: int, check_files: bool = False) -> list[str]:
    errors: list[str] = []
    if not isinstance(row, dict):
        return [f"line {line_no}: record must be a JSON object"]

    for field in REQUIRED_FIELDS:
        val = row.get(field)
        if not isinstance(val, str) or not val.strip():
            errors.append(f"line {line_no}: missing or empty '{field}'")

    has_hn_str = isinstance(row.get("hard_negative"), str) and bool(row["hard_negative"].strip())
    has_hn_list = (
        isinstance(row.get("hard_negatives"), list)
        and len(row["hard_negatives"]) > 0
        and all(isinstance(x, str) and bool(x.strip()) for x in row["hard_negatives"])
    )
    if not (has_hn_str or has_hn_list):
        errors.append(f"line {line_no}: provide non-empty 'hard_negative' or 'hard_negatives'")

    lang = row.get("language")
    if isinstance(lang, str) and lang not in ALLOWED_LANGUAGES:
        errors.append(f"line {line_no}: unsupported language '{lang}'")

    split = row.get("split")
    if isinstance(split, str) and split not in ALLOWED_SPLITS:
        errors.append(f"line {line_no}: unsupported split '{split}'")

    modality = row.get("modality")
    if isinstance(modality, str) and modality not in ALLOWED_MODALITIES:
        errors.append(f"line {line_no}: unsupported modality '{modality}'")

    if check_files and modality == "text_to_image":
        pos = row.get("positive")
        if isinstance(pos, str) and not (pos.startswith("<") and pos.endswith(">")):
            if not Path(pos).is_file():
                errors.append(f"line {line_no}: missing consented image file '{pos}'")

    return errors


def validate_file(path: Path, check_files: bool = False) -> dict[str, Any]:
    if not path.is_file():
        return {"ok": False, "errors": [f"dataset file not found: {path}"]}

    raw_bytes = path.read_bytes()
    fingerprint = hashlib.sha256(raw_bytes).hexdigest()

    errors: list[str] = []
    seen_ids: set[str] = set()
    group_splits: dict[str, set[str]] = defaultdict(set)
    split_queries: dict[str, set[str]] = defaultdict(set)
    counts_by_split: Counter[str] = Counter()
    counts_by_lang: Counter[str] = Counter()
    counts_by_modality: Counter[str] = Counter()
    total = 0

    for line_no, raw_line in enumerate(raw_bytes.decode("utf-8").splitlines(), start=1):
        stripped = raw_line.strip()
        if not stripped:
            continue
        total += 1
        try:
            row = json.loads(stripped)
        except json.JSONDecodeError as exc:
            errors.append(f"line {line_no}: invalid JSON ({exc.msg})")
            continue

        row_errors = validate_record(row, line_no, check_files=check_files)
        errors.extend(row_errors)
        if isinstance(row, dict):
            raw_split = row.get("split")
            raw_group = row.get("project_group")
            if isinstance(raw_split, str) and raw_split in ALLOWED_SPLITS:
                csplit = canonical_split(raw_split)
                if isinstance(raw_group, str) and raw_group.strip():
                    group_splits[raw_group.strip()].add(csplit)
                raw_query = row.get("query")
                if isinstance(raw_query, str) and raw_query.strip():
                    split_queries[csplit].add(normalize_text(raw_query))
        if row_errors:
            continue

        row_id = row.get("id")
        if row_id is not None:
            if not isinstance(row_id, str) or not row_id.strip():
                errors.append(f"line {line_no}: 'id' must be a non-empty string")
            elif row_id in seen_ids:
                errors.append(f"line {line_no}: duplicate id '{row_id}'")
            else:
                seen_ids.add(row_id)

        split = canonical_split(row["split"])
        counts_by_split[split] += 1
        counts_by_lang[row["language"]] += 1
        counts_by_modality[row["modality"]] += 1

    if total == 0:
        errors.append("dataset contains no records")

    # Cross-split leakage checks (docs/16_LORA_LAB.md).
    for group, splits in sorted(group_splits.items()):
        if len(splits) > 1:
            joined = ", ".join(sorted(splits))
            errors.append(f"leakage: project_group '{group}' appears in multiple splits ({joined})")

    train_queries = split_queries.get("train", set())
    for held_out in ("val", "test"):
        overlap = sorted(train_queries & split_queries.get(held_out, set()))
        if overlap:
            errors.append(
                f"leakage: {len(overlap)} normalized query overlap(s) between train and {held_out}"
            )

    return {
        "ok": len(errors) == 0,
        "total": total,
        "fingerprint": fingerprint,
        "splits": dict(counts_by_split),
        "languages": dict(counts_by_lang),
        "modalities": dict(counts_by_modality),
        "errors": errors,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Validate Tamgabul LoRA Lab JSONL dataset")
    parser.add_argument("dataset", type=Path, help="Path to triplets JSONL file")
    parser.add_argument(
        "--check-files",
        action="store_true",
        help="Verify that referenced non-placeholder image files exist on disk",
    )
    args = parser.parse_args(argv)
    report = validate_file(args.dataset, check_files=args.check_files)
    print(json.dumps(report, indent=2, ensure_ascii=False))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
