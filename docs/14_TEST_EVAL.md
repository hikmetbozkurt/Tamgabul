# 14 — Test Strategy, AI Evals and Performance Budgets

## Quality philosophy
Test truthfulness, permission safety and repeatability before chasing speed. All figures below are **provisional release targets/hypotheses**; the baseline comes from actual RTX 4080 Windows and M1 Pro macOS measurements. Do not turn model vendor benchmarks into product claims.

## Test layers
| Layer | Examples | Run when |
|---|---|---|
| Unit | intent JSON schema; scope path canonicalization; TTL state transitions; memory result pointers | every PR |
| Component | model registry import with fake GGUF; SQL history lifecycle; FTS/vector adapter mocks | every PR |
| Integration | managed llama.cpp worker; EG2 encode; parsers; local STT/TTS; SQLite purge | milestone |
| E2E Windows | tray+hotkey+model config+source grant+query+revoke+reboot | release candidate |
| macOS parity | menu bar + Apple Silicon runtime + OS permissions + sleep | MVP2 |
| Security | symlink, malicious PDF, prompt injection, root drive scope, unauthorized IPC | every core change |
| Performance | cold/warm latency, RAM/VRAM, disk index size, model unload, battery/thermal when applicable | weekly |

## Dataset (no user secrets in repo)
Create consented/local-only datasets with 200+ labeled queries initially: 40 exact/filename, 40 paraphrase TR, 40 TR–EN mixed+typo, 30 screenshot descriptions, 30 multi-turn ambiguous, 20 historical references across restart; include distractors/negative matches. Split train/dev/test before tuning; never train on held-out test. Private corpora live outside git; public synthetic fixtures are checked in.

## Retrieval metrics
- `Recall@1, @3, @5`, `MRR@10`, `nDCG@10`, FTS-only vs vector-only vs hybrid, screenshot text-to-image retrieval, false-file citation rate.
- Track per-language and per-file-type slices; no score threshold reused across model revisions without calibration.
- **Starting target:** `Recall@3 ≥ 0.85` on representative held-out corpus, no unauthorized result (100% block). Change only after benchmark evidence & ADR.

## Dialogue/LLM metrics
- Intent JSON schema validity (target ≥99% after validator/fallback), correct slot extraction, reference resolution, context-carry rate, clarification utility, grounded answer rate, hallucinated path count (target zero for uncited claims), error recovery when LLM off.
- Test 3–7 turns, corrections, “geçen bana bulmuştun”, cross-language, ambiguous project, prior source revoked, after worker unload/reboot, response with no evidence.
- LLM vs deterministic baseline and time/cost comparison. Small LLM should earn its latency by improving task accuracy.

## Voice eval
- Turkish WER/CER, English WER/CER, mixed Turkish+English project names/code tokens word-error, STT streaming/first-final timings, push-to-talk privacy, TTS first-audio/RTF, number/identifier intelligibility. Compare at least two STT candidates if quality is insufficient; no blanket claim that embedding model is STT.

## Resource benchmark scenarios
1. Cold Windows launch; no model loaded: tray host RSS and GPU allocations measured.
2. Hotkey first query with embedding only; repeat warm query; after TTL observe actual GPU memory return.
3. LLM-assisted 5-turn dialogue with 6.3GB GGUF; unload LLM mid-session; recover conversation persisted.
4. Index 1k/10k docs and 100 screenshots; rate, peak RAM/VRAM, errors, disk index size.
5. User-selected `C:\` capped dry-run and cancellation; deliberate exclusions.
6. Active STT + embedding + LLM + TTS contention; memory-pressure eviction.
7. macOS M1 Pro unified-memory and sleep/resume (post-Windows).

## Instrumentation
Record P50/P95/P99 for `cold_load_ms`, `warm_search_ms`, `end_to_end_response_ms`, `first_token_ms`, `STT_final_ms`, `TTS_first_audio_ms`, `unload_ms`; process RSS, GPU VRAM idle/peak, CPU/GPU util, disk growth, orphan processes. Local logs scrub content; retain aggregate only.

## Suggested non-binding budgets
- Zero model GPU allocation at fully unloaded idle (target, OS GPU tools verification needed); tray RSS baseline **measured**, not presumed `35MB`.
- Median warm text retrieval target <300ms on reasonable 10k doc corpus (subject to hardware/implementation); first cold load measured without hard cap at kickoff.
- Model unload MUST stop worker and release resources; no orphan processes.
- Scope revoke search authorization immediate, purge durable, content unchanged.

## Gate evidence
CI test report, command logs, platform/driver/runtime versions, model exact hash, corpus version, task seed and evaluation script revision. Regression PR blocks if accuracy drops > agreed tolerance or P95 latency/storage expands > 20% without explicit approval. Time budget and target may be revised based on measured baselines.
