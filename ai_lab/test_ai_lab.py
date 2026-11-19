"""Offline unit tests for Tamgabul LoRA Lab dataset validation & dry-run runner."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

AI_LAB_DIR = Path(__file__).resolve().parent
if str(AI_LAB_DIR) not in sys.path:
    sys.path.insert(0, str(AI_LAB_DIR))

import train_eg2  # noqa: E402
import validate_dataset  # noqa: E402


class TestValidateDataset(unittest.TestCase):
    def test_template_dataset_is_valid(self) -> None:
        report = validate_dataset.validate_file(AI_LAB_DIR / "dataset_template.jsonl")
        self.assertTrue(report["ok"], report["errors"])
        self.assertEqual(report["total"], 2)
        self.assertEqual(report["splits"], {"train": 1, "val": 1})
        self.assertEqual(len(report["fingerprint"]), 64)

    def test_rejects_missing_consent_and_project_group_leakage(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "leak.jsonl"
            rows = [
                {
                    "query": "q1",
                    "positive": "pos1",
                    "hard_negative": "neg1",
                    "language": "en",
                    "modality": "text",
                    "project_group": "shared_proj",
                    "source_consent_id": "c1",
                    "split": "train",
                },
                {
                    "query": "q2",
                    "positive": "pos2",
                    "hard_negative": "neg2",
                    "language": "tr",
                    "modality": "text",
                    "project_group": "shared_proj",
                    "source_consent_id": "",
                    "split": "test",
                },
            ]
            p.write_text("\n".join(json.dumps(r) for r in rows), encoding="utf-8")
            report = validate_dataset.validate_file(p)
            self.assertFalse(report["ok"])
            joined = "\n".join(report["errors"])
            self.assertIn("source_consent_id", joined)
            self.assertIn("project_group 'shared_proj'", joined)

    def test_rejects_query_overlap_between_train_and_test(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "q_overlap.jsonl"
            rows = [
                {
                    "query": "Find   Invoice",
                    "positive": "doc A",
                    "hard_negative": "doc B",
                    "language": "en",
                    "modality": "text",
                    "project_group": "proj_a",
                    "source_consent_id": "c1",
                    "split": "train",
                },
                {
                    "query": "find invoice",
                    "positive": "doc C",
                    "hard_negative": "doc D",
                    "language": "en",
                    "modality": "text",
                    "project_group": "proj_b",
                    "source_consent_id": "c1",
                    "split": "test",
                },
            ]
            p.write_text("\n".join(json.dumps(r) for r in rows), encoding="utf-8")
            report = validate_dataset.validate_file(p)
            self.assertFalse(report["ok"])
            self.assertTrue(any("query overlap" in e for e in report["errors"]))


class TestTrainEg2(unittest.TestCase):
    def test_rejects_fp16_config(self) -> None:
        cfg = train_eg2.load_yaml_config(AI_LAB_DIR / "experiment_config.example.yaml")
        self.assertEqual(train_eg2.validate_config(cfg), [])
        cfg["precision"] = "fp16"
        errs = train_eg2.validate_config(cfg)
        self.assertTrue(any("bf16" in e for e in errs), errs)

    def test_dry_run_generates_experiment_report_offline(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_md = Path(tmp) / "EXP-DRYRUN.md"
            code = train_eg2.main(
                [
                    "--config",
                    str(AI_LAB_DIR / "experiment_config.example.yaml"),
                    "--dataset",
                    str(AI_LAB_DIR / "dataset_template.jsonl"),
                    "--report",
                    str(out_md),
                    "--dry-run",
                ]
            )
            self.assertEqual(code, 0)
            content = out_md.read_text(encoding="utf-8")
            self.assertIn("MultipleNegativesRankingLoss", content)
            self.assertIn("bf16", content)
            self.assertIn("Do-not-train gate check", content)


if __name__ == "__main__":
    unittest.main()
