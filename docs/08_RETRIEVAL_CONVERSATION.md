# 08 — Retrieval, Intent Routing, and Natural Dialogue

## Roles and boundaries
- **EmbeddingGemma 2**: encode authorized content/query across text and screenshot when supported; never generative.
- **Deterministic router**: hotkey actions, time expressions, explicit project filters, pronoun/context carry-over and prior-result lookup.
- **Small local LLM (optional)**: interpret ambiguous language + normalize into strict intent JSON, choose follow-up slot, produce concise response grounded ONLY in retrieved records.
- **Search layer**: retrieves actual file records, source ID/path/line and relevance; file opener rechecks authorization/existence.

## Implementation (2026-11-03, ADR 021)
- `crates/core/src/understand.rs`: rules (`rules`), LLM prompt (`llm_messages`), schema (`llm_schema`), strict parse (`parse_llm`), merge (`plan`). Rules win on dates, the LLM wins on file type ("resim dersi notları" is not an image search).
- LLM output fields: `search_text`, `keywords` (≤8), `exclude` (≤4), `file_type`, `date`, `intent` (search/refine/clarify), `clarify_question`, `clarify_options` (≤4). This replaces the wider v1 intent schema for search planning; `open_result`/`model_control` stay deterministic UI actions.
- Filters run after fusion with the ACL check (`search::DocFilter`); requests with only filters ("dünkü ekran görüntüleri") list newest first (`search::browse`).
- One question at most: never two clarifications in a row; a picked option completes the earlier request.
- Context sent to the planning call: last 3 requests, up to 3 shown titles each, active filters. No file text. The pick call additionally sees short excerpts of the candidates (quoted data; output limited to candidate numbers).
- Images get a description at scan time (`describe.rs`, ADR 022); results are picked by the LLM from 20 candidates (`rerank.rs`), with an optional one-sentence answer tied to a shown file and hidden when that file is revoked.
- Whole request flow lives in `engine.rs` (ADR 023): prior-result lookup from history (`history::prior_results`), cited summaries (`rerank.rs`), in-memory int8 vector index (`vindex.rs`). The evaluation set and runner are in `examples/eval/` and `crates/runtime/examples/eval.rs`.
- Manual test 2026-11-04 (passport UK visa page, photo taken sideways): "not found" for every wording. Fixes: (1) filters come only from the user's words — the model may drop a type the rules saw or keep filters while refining, never add a date/type on its own ("en son aldığım vize" had become "last 14 days"); (2) if a filter still hides everything, results without it are shown with a notice; (3) a clarifying question only when the request has no content word; (4) OCR retries pictures turned a quarter both ways when the upright read finds few words (once per image).
- Answers show "Understood: …" (filters, rejected words, words the LLM added) so a wrong reading is visible.

## Intent vocabulary (initial)
`search_current`, `lookup_prior_result`, `open_result`, `refine_search`, `list_sources`, `model_control`, `cancel_action`, `clarify`, `unknown`. Model control commands are proposed only; Rust owns confirmation/permissions. Never grant file scopes from freeform LLM output.

## Entity slots
`project`, `date_range`, `file_type`, `filename_hint`, `topic_terms`, `result_ref`, `pronoun_ref`, `negative_terms`, `language`, `requested_action`; all optional, validate types/ranges. Explicit user filters outrank model guesses. Retain prior slots unless user negates/replaces them (“20coin değil, satranç projesi”).

## Retrieval plan
1. Normalize text minimally; protect names/code syntax/acronyms. Do not aggressively auto-rewrite mixed-language identifiers.
2. Attempt deterministic routing to current/episodic lookup. If ambiguity remains and LLM available, pass bounded session facts + compact schema and request JSON-only output; validate, repair once, then fallback deterministic.
3. Query authorized scope FTS5 and vector index for current embedding generation; filename/path/date/project boosts, reciprocal rank fusion baseline. The score is a **ranking statistic**, not calibrated probability.
4. When multiple close candidates, ask a clarification targeted at project/date/type rather than claim a match. Confidence policy calibrated on labeled eval set; **do not hardcode universal cosine 0.72 thresholds**.
5. Respond with result cards containing exact path (when user permits), source snippet/page/line, thumbnail for images, and open action. Any LLM prose cites `file_id + chunk_id`; missing citations means abstain.

## Memory-first historical retrieval
“Geçen bulmuştun” first searches persisted `search_events` + `result_interactions` by topic/time/last shown and checks current scope authorization; then optionally expands to new document search. A model unload/restart may discard KV cache without altering behavior.

## Dialogue behavior
- **Ask ONE useful question** when necessary: “Hangi proje: 20coin mi Empires of Chess mi?”
- Support correction and carry-over: query #1 screenshot UI bug; #2 “20coin'di”; #3 “şu timer olan” → merged structured slots, no naive linear vector blending as authoritative logic.
- If user says a model is not installed, show feature-specific missing status and guidance. No invented files.
- If context references revoked source, do not expose title/path/snippet.
- For TTS responses, speak concise context; avoid reading raw code unprompted. Optional voice confirmation for sensitive actions.

## Prompt-injection boundary
Document/PDF/OCR content is untrusted data. LLM prompt must distinguish retrieved text from system instructions. Model-produced tool arguments are parsed against strict schema and constrained to permitted operations; never shell execution, no arbitrary URL/network calls.

## Answer provenance contract
Each search result MUST include actual indexed `file_id`, `chunk_id`, `scope`, model generation, snippet and location. Generated narrative based on result without evidence MUST be labeled uncertain or omitted. UI links revalidate that the source remains authorized immediately before opening.

## Evaluation suite
TR-EN mixed queries, typos, multi-turn corrections, anaphora, ‘geçen hafta’ time expressions, requested file absent, 4 similar screens, delayed recall after restart, LLM unavailable, embedding absent, revoked source, injected instructions in document. See `examples/dialog-eval.example.jsonl` and `docs/14_TEST_EVAL.md`.
