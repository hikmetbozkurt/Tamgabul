# Search evaluation set

A small, synthetic, bilingual (TR/EN) set to measure search quality with the
real models. Every file in `corpus/` and `images/` is invented for this
set: no real person, account or document.

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

`--llm` is optional (without it the rules-only pipeline is measured). With
it, every LLM job is measured separately against rules-only:

| run | LLM understands the request | LLM picks / answers |
|---|---|---|
| `rules` | – | – |
| `plan` | ✓ | – |
| `pick` | – | ✓ |
| `all` | ✓ | ✓ |

`--modes rules,pick` runs a subset. The last table of each section
compares the runs; a job that does not raise hit@1 / MRR or "not found"
accuracy is turned off in Settings → Models → LLM jobs.

### Images

`images/` holds seven synthetic PNGs (a receipt, a bank letter scanned
sideways, a bar chart, a floor plan, a boarding pass, an error dialog and a
drawn cat) with camera/screenshot style names, so the file name never gives
the answer away. `images.jsonl` asks for them by what they show. They are
indexed twice: without and with LLM image descriptions (needs the vision
projector next to the LLM). OCR runs on Windows only. Regenerate them with
`python examples/eval/make_images.py`; `--images none` skips this part.
`--json report.json` writes per-query results. The runner indexes the corpus
into a temporary database, runs every request through the same engine as
the app (`tamgabul_core::engine`) and prints hit@1, hit@3, MRR, "not found"
accuracy and latency per kind. It changes no files.

## In CI

`crates/core/tests/eval_set.rs` checks the set is well formed and that every
`keyword` request is found in the top 3 without any model (words + names
only). Meaning-based kinds need the real models and are not run in CI.
