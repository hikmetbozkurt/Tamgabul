# 06 — Persistent Conversation Memory Engine

## Intent
Past user queries, clarifications and retrieved files remain recoverable **after LLM unload, widget exit and OS reboot**. The LLM KV cache is a disposable performance detail. Persistence is locally owned by the widget in SQLite, not a remote/chat-provider history.

## Three logical layers
1. **Working session memory (persisted)**: active turn chain, cleaned query, user corrections, active filters, last candidate result IDs and clarification slot. Recent turns loaded in bounded context; process memory is only a cache.
2. **Episodic event log (persisted)**: `asked`, `clarified`, `results_shown`, `file_opened`, `search_failed`, `scope_revoked`, with timestamps, filters, source IDs and confidence metadata. Deterministic re-find of previously shown files.
3. **Searchable semantic note memory (persisted, optional derived layer)**: short user-visible summaries, keyword/project/time tags and later a dedicated embedding index, versioned independently. Do not manufacture personal facts; save derivation source IDs and provide deletion/disable control.

## Example
User: “geçen bana auction timer ile ilgili bulduğun dosyayı aç” → regex + deterministic reference intent + query current episodic log by `auction`, `timer`, last shown/opened and date; confirm candidate still in granted scope and actual file exists; if ambiguous ask one discriminating question; if optional LLM available, have it map utterance into `lookup_prior_result` intent and selective filters; **LLM never invents a file reference ID**.

## Conversation compression
- Store raw user query only when user opted into conversation history (default proposed: ON with clearly disclosed local retention; user can turn OFF, see settings). Never store raw audio by default.
- For memory retrieval, prefer event summaries/IDs and selectively show last 4–8 turns; bounded byte/token budget configured for the chosen LLM. Clip large snippets and store reference IDs, not full documents in message history.
- Persistent `memory_notes` may contain compact structured summaries; `source_turn_ids` and timestamp mandatory. Provenance not a language-model confidence string.
- Supersession/corrections stored as revisions, original record kept until retention/purge; corrections must not become unsupported facts.

## Local search recipe
- If query includes temporal language (“geçen”, “dün”, “az önce”) inspect episodic memory before document search; combine FTS5, time, project, historical click/open emphasis.
- Resolve one concrete `file_id` only if authorized, path valid and identity re-verified; otherwise return 'Previously shown, but current file not accessible' without revealing revoked content.
- If nothing found, ask user for date/topic or search current scopes. If LLM absent, template-based follow-up.
- Time language uses local timezone for presentation, UTC for storage; ambiguity (“geçen”) stays broad instead of fabricated exact date.

## Consent, deletion and lifecycle
- History controls: keep history ON/OFF, delete selected session, clear entire conversation history, retention period (e.g. 30/90/forever); destructive confirmations and preview count.
- History OFF still needs ephemeral in-process session while active; must not persist raw query/turn text except transient operational records essential for current task, and those are cleared at session end.
- Revoked scope: scrub private snippets and derived vectors/event content tied to that scope; file IDs in past records become redacted tombstones and never open. Independent user utterances can be retained only under disclosed retention; safe privacy setting can purge whole related turns.
- DB backups inherit same retention/privacy policies, with secure cleanup documented; SQLite WAL and VACUUM behavior reviewed (`12_DATA_SCHEMA.md`).

## Evaluation
References to `last shown`, `last opened`, similar filenames across projects, correction two turns later, reboot then recall, no-LLM fallback, revoked scope, path moved, new file at same path (content/hash mismatch), history off. Verify historical lookup yields citeable record or explicit uncertainty.
