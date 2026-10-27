# P6-01 Local Voice and Offline Windows Packaging

**Phase:** P6 • **Owner:** Runtime + Release • **Status:** TODO • **Dependencies:** P4-01, P5-01

## Objective
Add approved local STT and TTS behind tested adapters with explicit mic consent, and ship a Windows installer that bundles signed/versioned workers but no model weights. Typed mode stays fully usable when voice is missing or denied.

## Read before implementation
AGENTS.md; docs/05_MODEL_LIFECYCLE.md; docs/09_UI_UX.md (Voice affordances); docs/14_TEST_EVAL.md (Voice eval); docs/17_DISTRIBUTION_OPS.md; docs/19_RESEARCH_VERIFICATION.md (V04)

## Steps
1. Choose and pin at least two STT candidates and one TTS per language; record exact versions and licenses (weights, runtime, dependencies separately).
2. STT/TTS adapters under the same lifecycle manager and TTL policy; push-to-talk default, visible recording indicator, transcript shown for correction, stoppable playback.
3. Voice eval: Turkish/English WER/CER, mixed project names and code tokens, first-partial and first-audio timings.
4. Installer (`.msi` or NSIS spike): bundled runners with license notices and SHA-256 inventory, data under AppData, no admin requirement, no mandatory update fetch.
5. Fresh-profile smoke test with zero models; packet/firewall observation for no network in local-only mode.

## Acceptance criteria
- [ ] Mic denial leaves typed search working; no ambient or stored recording by default.
- [ ] Local voice round trip in Turkish and English when models installed.
- [ ] No orphan processes after Quit; installer runs on a clean Windows profile.
- [ ] License manifest complete for every bundled binary.

## Deliverables and report
Code + automated tests + commands/results + platform/hardware/runtime versions + security/privacy notes. Mark real runtime tests as PENDING until observed. Keep weights and private data out of git.
