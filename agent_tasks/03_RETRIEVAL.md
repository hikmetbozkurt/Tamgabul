# P2-01 EmbeddingGemma 2 Retrieval

**Phase:** P2 • **Owner:** Retrieval + Runtime • **Status:** IN PROGRESS (hybrid search + history live; Windows run PENDING) • **Dependencies:** P1-01, P0-02 (feasibility recorded)

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

### Progress report — 2026-11-03
- **Done:** `crates/core/src/search.rs` — query embedded with the model-card query prompt; brute-force cosine over the CURRENT generation (vectors L2-normalized), FTS5 trigram word match, file-name match; reciprocal rank fusion (k=60) grouped per document; meaning-only hits kept within 0.12 of the best cosine (heuristic, not calibrated); every result re-authorized against live grants + exclusions. Cards: snippet around the first matched word with highlights, folder, file kind, modified time, match reason (meaning / word / file name / image). `crates/core/src/history.rs` — sessions, turns, search events and result references in SQLite; results rebuilt and re-authorized on every read (revoked folders vanish from old conversations); opened marks; history-off sessions dropped at start.
- **App:** `search`, `preview_result`, `open_result` (re-checks grant + file, then Explorer open / select), `list_history`, `session_turns`, `delete_session`, `clear_history`. Falls back to word + file-name search if the model can't start.
- **Tests:** search unit tests + integration test (meaning, words in PDF, file name of an image, card/snippet/highlights, history round trip, revoke hides results now and in history).
- **Not yet:** LLM clarification, vector index (brute force is linear in chunks), query-time image search by example, evaluation set.

### Update — 2026-11-03
- **Fix:** meaning matches are now ranked per modality (text/OCR, image, audio) before fusion. Before, the band was relative to the best text↔text cosine, so text↔image hits (lower scale) were almost always filtered out.
- Word search is accent/case-insensitive and Turkish-aware (folded FTS + folded query); OCR words are reported as "Text in image"; image/audio results show the image's OCR text as snippet when there is one.
- Tests: image vector ranks images first; OCR words find screenshots and scanned PDFs; folded highlights keep original spelling.
