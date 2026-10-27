# 01 — Product Requirements Document (PRD)

## Identity
**Working name:** Local Context Widget (LCW). **First platform:** Windows desktop. **Second:** macOS (Apple Silicon). **Audience:** developers, researchers, busy knowledge workers who recall meaning rather than filenames. **Languages:** Turkish + English and mixed-language utterances. Product remains independent of Capsula.

## Problem statement
Users have scattered `.md`, `.txt`, PDFs, screenshots and later other media. They remember vague episodes (“geçen hafta timer hatası bulmuştun”) instead of exact paths. Existing filename search does not reconstruct dialog context. A large always-on LLM is unnecessary and resource-expensive for this narrow purpose.

## Product promise
A tray resident keyboard/voice widget that searches *only authorized local content* with **EmbeddingGemma 2**, tracks persistent conversational search events in SQLite, asks clarifying follow-ups when ambiguity remains, and can open the original document with evidence. The local LLM is an optional intent/context collaborator, never the source of truth.

## Users / primary jobs to be done
- Find a file or screenshot using natural Turkish/English descriptions, not exact filename.
- Recover previously shown/search-result documents by time, topic and source reference.
- Correct earlier utterances: “Hayır, öteki projeydi” without resetting the query.
- Speak hands-free using local STT/TTS or type without them.
- Inspect/enable/replace model files and idle policies without technical consoles.
- Explicitly grant/revoke folder indexing, including a guarded drive root.

## Principles
Privacy without hidden network calls; minimal persistent resident footprint; traceable citations; user autonomy for models & folders; no hallucinated file claims; deterministic fallbacks; accessibility and OS conventions; adaptive resource management.

## Success criteria (initial hypotheses to validate)
- Usable **typed** search and history when LLM/STT/TTS missing, provided embedding engine healthy.
- A full local voice→context→retrieval→spoken reply round trip when all models healthy.
- Historical retrieval survives widget close, model unload, reboot, rename/move (where detectable).
- No source-file mutation. No leaked content in diagnostics.
- Explicit scope removal immediately excludes content from results and initiates derived data purge.
- Runtime shutdown verified: `Quit` kills children, no stale model processes.

## Must-have UX
One main widget surface, a small Settings window; model cards for LLM / required EmbeddingGemma 2 / STT / TTS; real state and errors with status text alongside accessible colored indicators; file/folder permission manager; search result provenance and “Open in folder”; tray/menu bar; editable shortcuts, mic mode, language and per-model TTL.

## Failure behavior
No embedding → banner “EmbeddingGemma 2 gerekli” and disabled semantic functions with model-add CTA. No LLM → deterministic search + limited context references. No STT → typed search. No TTS → text replies. Invalid file → incompatible card with exact reason, retry/replace; NEVER green on mere registration.

## Explicit non-goals for MVP
Cloud sync, accounts, browser scraping, autonomous actions, system-wide full text capture, real-time screen watching, mouse automation, 3D capsules, large dashboard, mobile port, third-party arbitrary executable plugins, universal support for all model formats, guaranteed sub-second cold response.

## Product analytics: only local opt-in-style diagnostics
Show user-visible counters for retrieval precision, model errors, memory usage and index progress. No content or IDs exported. All diagnostics deleteable and default to local only.
