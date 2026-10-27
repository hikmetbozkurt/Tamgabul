# EXP-NNN <Experiment title>

**Date:** YYYY-MM-DD • **Owner:** • **Related:** docs/16_LORA_LAB.md, V06

## Hypothesis
<What should improve, on which held-out slice, and why.>

## Do-not-train gate check
- [ ] Baseline measured on frozen eval corpus
- [ ] OCR / chunking / task prompts / fusion ruled out as root cause

## Setup
| Field | Value |
|---|---|
| Base model + revision + hash | |
| Framework versions (torch, sentence-transformers, peft) | |
| GPU / driver / CUDA | |
| Modalities enabled | |
| Dimension / normalization / task prompts | |
| Dataset fingerprint + split | |
| Seed | |
| Config file | ai_lab/<config>.yaml |

## Results
| Metric | Baseline | Candidate | Δ | CI |
|---|---|---|---|---|
| Recall@1 | | | | |
| Recall@3 | | | | |
| MRR@10 | | | | |
| nDCG@10 | | | | |
| TR-EN slice | | | | |
| Image slice | | | | |
| Peak VRAM / time | | | | |

## Decision
Ship / do not ship / iterate — with reason against release gates in docs/16.

## Artifacts
Checkpoint location (outside git), logs, license/provenance notes.
