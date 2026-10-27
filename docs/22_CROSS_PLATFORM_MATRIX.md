# 22 — Platform Compatibility and Smoke Matrix

| Scenario | Win 11 RTX 4080 (first) | macOS M1 Pro (second) | Required evidence |
|---|---|---|---|
| Tauri tray/menu icon | REQUIRED | menu-bar REQUIRED | OS screenshot + auto-test/manual steps |
| Close-to-tray; explicit Quit | REQUIRED | REQUIRED | children terminated |
| Global shortcut conflict/fallback | REQUIRED | REQUIRED | collision and rebound |
| Native folder picker, grant | REQUIRED | REQUIRED | scoped access test |
| Drive root `C:\` preview and cancel | REQUIRED | N/A; volume root equivalence separately | 10k-entry cap + no protected files |
| Symlink/junction escapes | junction REQUIRED | symlink REQUIRED | unauthorized read zero |
| SQLite migrations / FTS5 | REQUIRED | REQUIRED | clean + upgrade + rollback |
| Vector adapter packaging | REQUIRED | REQUIRED | model-specific dimension validation |
| EG2 text/image model path | REQUIRED | REQUIRED if supported on device | measured docs/screenshots retrieval |
| GGUF LLM local runtime | REQUIRED | REQUIRED | no LM Studio and actual unload |
| STT/TTS local | REQUIRED by MVP1 | REQUIRED by MVP2 | mic opt-in, voice output |
| CPU/GPU backend | CUDA test | Metal test | measured RSS/VRAM/unified mem |
| Suspend/resume | REQUIRED | REQUIRED | no dead worker stuck READY |
| Build installer | .msi/.exe | .app/.dmg | fresh machine smoke |
| Privacy/no network | REQUIRED | REQUIRED | packet/firewall observation |

Columns `REQUIRED` describe intended acceptance, **not results**. Fill build SHA, OS version, GPU/driver, model hash, latency, tester and date in actual test report.
