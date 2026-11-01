# 09 — UI/UX Design Direction (Editable, Not a Locked Visual System)

> **Görsel tasarım ve uygulama planı:** `docs/24_UI_UX_PLAN.md`. Ayrı Settings penceresi yerine genişletilmiş uygulamanın Models / Folders / Settings sayfaları kullanılır; aşağıdaki işlev sözleşmeleri geçerliliğini korur.


## Visual principles
A **minimal desktop utility**, not a chatbot web app, dashboard or 3D visualization. Neutral native-feeling light theme first; reserve option for dark mode later if desired. Sparse typography (system UI font), large target sizes, keyboard-first, restrained accent, gentle 180–220 ms motion respecting reduced-motion preferences. Layout is adaptable during implementation; preserve behavior/information architecture.

## Screens
1. **Widget** (rough starting size ~560×420, responsive): query field, microphone push-to-talk, 2–4 result cards, optional follow-up bubble, history shortcut and status dot. Expand/collapse rather than permanent chat wall. Resize/reposition; remember last location/monitor.
2. **Settings** (single plain window): `Models | Sources | Performance | Privacy | About`. This is not a project map.
3. **Onboarding**: setup steps for required EmbeddingGemma 2, optional LLM/STT/TTS, first folder grant, index preview, global hotkey. Keyboard search possible even without audio.
4. **History**: small list of recent search sessions, source/result references, local delete controls; no user-visible binary blobs.

## Low-fidelity layout
```text
┌──────────────────── Local Context Widget ─────────────────────┐
│  Search computer...                                     [🎙]  │
│  “geçen timer hatasıyla ilgili bulduğun dosya”                 │
├───────────────────────────────────────────────────────────────┤
│  I found two previous results. Which project?                 │
│  [20coin] [Other project]                                      │
│                                                               │
│  📄 auction_timer_notes.md                       [Open]        │
│     docs/20coin/ ... • matched previous search                 │
├───────────────────────────────────────────────────────────────┤
│  [History]                    ● Models ready  [Settings]       │
└───────────────────────────────────────────────────────────────┘
```

## Settings — Model cards
Each category gets independent `Add Model`, `Import existing file/folder`, `Validate`, `Load`, `Unload`, `Set Active`, `Remove registration`, runtime/format details, health error, measured memory, last verified, configurable TTL. Required `EmbeddingGemma 2` card pinned first. LLM/STT/TTS optional. Download button only if verified official source is supported, clear file size/license, user confirmation. State semantics:
- **Green** filled dot: loaded and healthy `READY | IDLE | BUSY` (plus explicit text).
- **Amber/neutral** dot: available on disk but unloaded or validating, not “running”.
- **Red** dot: mandatory model missing, invalid/incompatible model or actual runtime error; optional not configured is neutral gray.
- Status must never be color-only for accessibility. Global app health is derived from required embedding + DB + scope readiness, not simply all models present.

## Sources settings
A table of user-selected roots with path, included types, exclusions, last indexed time, file count, status/progress, estimated storage, action menu (`Index now`, `Pause`, `Edit exclusions`, `Remove & purge`). `Add folder` picker; selecting `C:\` displays large warning + dry-run preview before explicit confirmation. Scope removal explicitly distinguishes **stop scanning** from **erase indexed artifacts**.

## Performance settings
Model-specific TTL dropdown, live loaded/unloaded indicators, RAM/VRAM usage (source & timestamp), indexing mode Manual / Idle Sync, worker logs without user content, disk index size, `Unload all models` button. No misleading universal “0 VRAM” claim unless measured.

## Tray/menu bar behavior
Windows: icon always present in notification-area app (OS may hide it under overflow arrow); user can pin OS tray icon manually, app cannot force OS visibility. Menu: `Open Widget`, `Settings`, `Pause/Resume indexing`, `Unload Models`, `Quit`. Clicking window X hides to tray; explicit Quit terminates. macOS: menu-bar status item equivalent, default Cmd+Shift+Space shortcut fallback if conflict.

## Voice affordances
Mic permission request on first explicit tap. Recording visibly active with level/timer, clear cancel, push-to-talk as safe default. TTS playback stop/pause/replay. Never listen passively or store recordings unless future opt-in. Show STT transcript for correction before optional action.

## Empty, loading and error states
No models installed: welcoming setup list + mandatory red EmbeddingGemma card; search doesn't pretend ready. Loading model: progress if available, otherwise “Yükleniyor” with cancel. No results: suggested next filter. Error: actionable reason and retry. Missing source: “Path inaccessible” + re-link. Screen-reader labels, keyboard focus, 200% scaling, minimum clickable targets.

## UX change control
Layout, palette, spacing, typography are provisional and may be revised in design iterations. Command names, visibility of privacy, accurate status semantics, model add/remove and source revoke flows are functional contracts and require ADR to change.
