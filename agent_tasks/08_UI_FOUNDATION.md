# U0-U1 UI Foundation: design system, app shell, two windows

**Phase:** U0–U1 (merges P5-01 tray/hotkey) • **Owner:** UI • **Status:** DONE (Windows visual check by user OK) • **Dependencies:** P0-01 (done)

## Objective
The design system from the canvas exists as tokens + React components, and the desktop app has two windows (compact `widget`, expanded `app`) sharing one codebase with a custom title bar, side navigation and page routing. Pages are empty skeletons; real page content is U2+.

## Read before implementation
AGENTS.md; docs/20_ADR.md; docs/09_UI_UX.md; **docs/24_UI_UX_PLAN.md** (§1 decisions, §3 tokens, §4 components, §6 structure); docs/10_PLATFORM_WINDOWS_MACOS.md; agent_tasks/06_WINDOWS_UI.md (tray/hotkey parts); reference image `docs/design/reference-expanded.png`.

## Touched paths
`apps/desktop/src/**`, `apps/desktop/index.html`, `apps/desktop/package.json`, `apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/src-tauri/capabilities/**`, `apps/desktop/src-tauri/src/**`, `apps/desktop/src-tauri/Cargo.toml`, `crates/platform/**`.

## Steps
1. Add `styles/tokens.css` exactly as docs/24 §3 and `styles/base.css` with self-hosted Inter (`@fontsource/inter`). No remote fonts.
2. Build `ui/` components from docs/24 §4 with props for every variant; add a dev-only `/gallery` entry rendering all of them.
3. Configure two Tauri windows: `widget` (560×500, no decorations, always-on-top optional, bottom-right) and `app` (1366×956, min 1024×700, no decorations). Custom `TitleBar` with drag region and window buttons. X hides to tray.
4. `AppShell` + `SideNav` + hash router with five pages (Search, Memory, Models, Folders, Settings) as skeletons; widget expand button opens `app`, app minimize-to-widget returns.
5. Persist window size/position in `app_settings`; restore on start (clamp to visible monitor).
6. From P5-01: tray menu (`Open Widget`, `Settings`, `Pause/Resume indexing`, `Unload Models`, `Quit`), single instance, global hotkey with conflict detection behind `crates/platform` traits.
7. Playwright screenshot test for `/gallery` and the empty shell (Windows baseline).

## Acceptance criteria
- [ ] Gallery matches the canvas "Tasarım sistemi" board (screenshot attached to report).
- [ ] Both windows open; widget ↔ app switching works; X hides, tray reopens, Quit terminates.
- [ ] Hotkey conflict shows a warning and allows rebinding.
- [ ] Every component reachable and operable by keyboard; focus ring visible.
- [ ] No hard-coded colors/radii outside `tokens.css`.
- [ ] `npm run typecheck`, `npm test`, `cargo test --workspace` pass.
- [ ] No cloud calls, model auto-downloads, LM Studio or 3D UI.

## Deliverables and report
Code + automated tests + commands/results + platform/hardware/runtime versions + security/privacy notes. Mark real runtime tests as PENDING until observed. Keep weights and private data out of git.

### Completion report (docs/23) — 2026-11-02
- **Summary:** U0–U8 implemented (see docs/24 §11). Two windows, tray, hotkey, autostart, settings persistence are real; search/models/folders/history show real state; their commands return NOT_IMPLEMENTED until P1–P4. Mock/demo store removed 2026-11-02 (ADR 018).
- **Files touched:** `apps/desktop/src/**`, `apps/desktop/src-tauri/**`, `crates/core/src/{settings,dto,error,lib}.rs`.
- **Evidence:** `cargo test --workspace` (70 tests), `cargo clippy -D warnings` clean, `npm run typecheck`, `npm test` (34 tests incl. jsdom render tests), Playwright screenshots of every screen in browser mode, 15 s headless launch on Linux (no panic, DB created).
- **Known gaps:** Windows visual check, 200 % scaling, transparent widget corners and tray on real Windows are PENDING. No real search/model/scope backend yet. Mic never opened.
- **Security impact:** capabilities limited to window drag/min/max + dialog open; no fs/shell/http plugins; no network.
- **Next dependency:** P0-02 runtime spikes, then P1 scopes.
