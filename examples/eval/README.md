# Search evaluation set

A small, synthetic, bilingual (TR/EN) set to measure search quality with the
real models. Every file in `corpus/` is invented for this set: no real
person, account or document. No images or audio (OCR and the vision models
need Windows and are tested separately).

- `corpus/` — 24 text files (Markdown, plain text, HTML).
- `queries.jsonl` — one request per line:
  `{"id", "query", "kind", "expect": [file names]}`. `expect: []` means
  nothing relevant exists and the best answer is "not found".
  Kinds: `keyword` (exact words of the file), `paraphrase`, `cross_lingual`
  (asked in the other language), `question`, `typo`, `none`.

## Run with your models

```powershell
cargo run -p tamgabul-runtime --release --example eval -- `
  --embedding C:\AI\models\embeddinggemma-2 `
  --llm C:\AI\models\gemma-4-e2b-it
```

`--llm` is optional (without it the rules-only pipeline is measured).
`--json report.json` writes per-query results. The runner indexes the corpus
into a temporary database, runs every request through the same engine as
the app (`tamgabul_core::engine`) and prints hit@1, hit@3, MRR, "not found"
accuracy and latency per kind. It changes no files.

## In CI

`crates/core/tests/eval_set.rs` checks the set is well formed and that every
`keyword` request is found in the top 3 without any model (words + names
only). Meaning-based kinds need the real models and are not run in CI.
