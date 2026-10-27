# P5-01 Minimal Windows UI, Tray and Hotkey

**Phase:** P5 • **Owner:** UI • **Status:** TODO • **Dependencies:** P0-01; real acceptance needs P2-01 and P4-01 (develop against typed mocks first)

## Objective
A minimal, keyboard-first widget that lives in the Windows notification area (including the hidden-icons overflow), hides on close, quits for real from the tray, and exposes honest model/source status. Layout is editable; behavior contracts are not.

## Read before implementation
AGENTS.md; docs/09_UI_UX.md; docs/10_PLATFORM_WINDOWS_MACOS.md; docs/13_API_CONTRACTS.md; docs/21_USER_JOURNEYS.md (J01, J07, J08, J09, J10)

## Steps
1. Tray menu `Open Widget`, `Settings`, `Pause/Resume indexing`, `Unload Models`, `Quit`; close → hide; single instance; global hotkey with conflict detection and rebinding.
2. Widget: query field, push-to-talk button (disabled without STT), 2–4 result cards with path/snippet/Open/Reveal, one clarification bubble, history shortcut, status dot.
3. Settings `Models | Sources | Performance | Privacy | About`: EG2 card pinned first and marked required; per-model Add/Import/Validate/Load/Unload/Set Active/Remove, TTL dropdown, measured memory or `Not measured`.
4. Sources: add folder, `C:\` warning + dry-run preview + explicit confirm; distinct `Pause` vs `Remove & purge`; parent/child revoke dialog.
5. Onboarding from zero models; empty/loading/error states; i18n keys (TR + EN fallback); status never color-only; 200% scaling and keyboard focus.

## Acceptance criteria
- [ ] Close → tray, hotkey reopens with history intact, Quit terminates workers.
- [ ] Hotkey conflict shows warning and allows rebind.
- [ ] Missing EG2 red, optional missing neutral, AVAILABLE not green, READY/IDLE/BUSY green with text label.
- [ ] Source revoke and parent/child dialogs behave per docs/07.

## Deliverables and report
Code + automated tests + commands/results + platform/hardware/runtime versions + security/privacy notes. Mark real runtime tests as PENDING until observed. Keep weights and private data out of git.
