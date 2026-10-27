# P0-02 LLM and Embedding Runtime Feasibility

**Phase:** P0 • **Owner:** Runtime • **Status:** TODO • **Dependencies:** 00 optional; can parallelize

## Objective
Prove the existing Gemma 4 E4B Q4_K_M runs without LM Studio and EmbeddingGemma 2 produces correct local vectors (text+image) through an app-managed worker prototype.

## Read before implementation
docs/04_MODEL_REGISTRY.md; docs/05_MODEL_LIFECYCLE.md; docs/19_RESEARCH_VERIFICATION.md

## Steps
1. Pin llama.cpp exact commit/build for Windows CUDA, verify E4B GGUF tokenizer/chat template; exercise strict JSON and unload/sleep.
2. Pin EG2 revision and SentenceTransformers versions; test text+image encoding 768d, normalized scoring and user-selected model path.
3. Probe bundled worker packaging and process supervision, measure memory release and cold/warm latency.
4. Record macOS portability unknowns and runtime license/provenance.

## Acceptance criteria
- [ ] Report actual command/version/model hash and stdout results (redact local paths).
- [ ] Show model reaches READY then truly unloads OS resources.
- [ ] Show EG2 768d vectors, separate query/doc normalization and image retrieval.
- [ ] Unknowns recorded, no fake time estimate.

## Deliverables and report
Code + automated tests + commands/results + platform/hardware/runtime versions + security/privacy notes. Mark real runtime tests as PENDING until observed. Keep weights and private data out of git.
