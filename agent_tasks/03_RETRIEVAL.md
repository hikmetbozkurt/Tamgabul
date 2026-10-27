# P2-01 EmbeddingGemma 2 Retrieval

**Phase:** P2 • **Owner:** Retrieval + Runtime • **Status:** TODO • **Dependencies:** P1-01, P0-02 (feasibility recorded)

## Objective
Index authorized content with EmbeddingGemma 2 under a pinned index generation and return evidenced hybrid (FTS5 + vector) results. Missing/invalid EG2 shows red and disables semantic search; nothing substitutes another embedding model.

## Read before implementation
AGENTS.md; docs/04_MODEL_REGISTRY.md; docs/08_RETRIEVAL_CONVERSATION.md; docs/12_DATA_SCHEMA.md; docs/14_TEST_EVAL.md; docs/19_RESEARCH_VERIFICATION.md (V02, V03)

## Steps
1. EG2 adapter behind the embedding interface using the runtime proven in P0-02; separate query/document task prompts, normalization, chosen dimension.
2. Generation fingerprint (model hash/revision, adapter, preprocessing, instructions, dimension, normalization, modality); staged build and atomic switch to `CURRENT`.
3. Embed `.md/.txt/.pdf` text chunks and `.png/.jpg` images (image only if validated in P0-02); bounded concurrency, cancel, checkpoint.
4. Hybrid search: FTS5 + vector over current generation only, filename/path/date boosts, RRF baseline; ACL re-check on every result.
5. Result card payload per provenance contract: `file_id`, `chunk_id`, `scope`, generation, snippet, location.

## Acceptance criteria
- [ ] Synthetic multilingual fixture (TR, EN, mixed, typos, screenshot descriptions) passes; metrics reported as Recall@1/3/5, MRR@10, nDCG@10 for FTS-only, vector-only and hybrid.
- [ ] Test proves vectors from two generations are never scored together.
- [ ] EG2 `MISSING`/`INCOMPATIBLE` → red health, semantic search off, optional FTS fallback labeled degraded.
- [ ] Revoked scope content never appears, including from vector cache.

## Deliverables and report
Code + automated tests + commands/results + platform/hardware/runtime versions + security/privacy notes. Mark real runtime tests as PENDING until observed. Keep weights and private data out of git.
