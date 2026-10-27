# 21 — User Journeys / End-to-End Scenarios

## J01 Zero-model installation
User starts clean Windows install → tray icon appears → opens widget → mandatory EG2 red ‘Not installed’; other model cards neutral ‘Optional’ → can import EG2 checkpoint and run validation; until ready no pretend semantic answers. Settings and source-management remain operable.

## J02 Add current GGUF LLM
Settings > Models > LLM > Add > Existing GGUF → metadata check/compatibility probe → registered → validated AVAILABLE → user clicks Load, health READY green; set Active → 5-turn dialogue test → idle TTL → AVAILABLE (no green) and memory persists.

## J03 Add EmbeddingGemma 2
Settings > Models > Embedding (Required) > Import `google/embeddinggemma-2` local folder → check tokenizer/config/all weight files, selected modalities & normalization → encoding smoke test dims → AVAILABLE → Load → READY green → choose source → Index Now → search. If model missing or swapped incompatible, previously built vectors cannot be silently reused.

## J04 Import STT and TTS
STT Add > local compatible runner/weight validation; TTS Add > language voice profile; voice UI shows test recording with OS mic permission and explicit transcript; fallback typing if missing/denied; TTS voice playback always stoppable.

## J05 Search vague bug in two turns
Utterance 1: “geçen oyun ekranında buton çalışmıyordu” → 3 candidate records across projects; app clarifies project. Utterance 2: “20coin, auction timer” → structured filter resolves; show file with path/snippet and preview. Persist memory event and shown result ID. If LLM not loaded, template-based clarification still works.

## J06 Recall after days / after unloading everything
LLM idle-unloaded. User: “Geçen bana timer dosyasını bulmuştun” → episodic FTS/last-shown lookup; reauthorize document, check it exists; show exact previous result. May load LLM only if ambiguous. Clear when result missing/revoked.

## J07 Allow drive root then revoke
Sources > Add Folder > `C:\` → risk warning, limited preview, exclusions and explicit confirmation → capped index job with errors visible. Two months later Sources > Remove → immediate exclusion from search; asynchronous purge including chunks, vectors, OCR, related memory, progress and restart resilience; original files remain untouched.

## J08 Parent root + child revoke
A user authorizes `C:\Projects` and independently `C:\Projects\Private`. Removing child shows choice between retention via parent or block under parent; privacy-preserving default creates persistent exclusion; previously indexed content cannot be silently re-collected.

## J09 Missing/incompatible model
User points to unsupported files → card INCOMPATIBLE red with reason + supported formats link, not misleading green. UI remains functional and gives explicit remediation.

## J10 App close and full Quit
Click window X → widget disappears and tray remains; hotkey reopens, history intact. Tray > Quit → cancel active jobs, unload all models, flush DB, terminate sidecars and disappear. On next startup, no persisted green runtime status.

## J11 macOS parity
On Mac M1 Pro same project compiles; menu bar opens widget; shortcut conflict handled; import compatible model and selected Documents folder after permission; Metal/unified-memory version measured; history local to Mac without automatic sync.
