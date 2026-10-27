# P0-01 Monorepo Bootstrap

**Phase:** P0 • **Owner:** Lead/Platform • **Status:** TODO • **Dependencies:** None

## Objective
Initialize a genuinely buildable Rust workspace + Tauri v2 React TypeScript frontend with placeholder widget and typed IPC, correct repository architecture, i18n and CI. No AI runtime feature claims.

## Read before implementation
AGENTS.md; docs/02_MVP_SCOPE.md; docs/03_ARCHITECTURE.md; docs/20_ADR.md

## Steps
1. Create Rust workspace crates/core + crates/platform and Tauri v2 app.
2. Add lint/test/typecheck CI and typed UI↔Rust ping/status API.
3. Add app single-instance and placeholder tray host behind platform trait, with real Windows integration in P5.
4. Create migration harness and seed tests (do not bring real user models).

## Acceptance criteria
- [ ] `cargo test` and TS typechecks pass; one typed IPC request works on Windows.
- [ ] Documentation references and scripts point to real existing paths.
- [ ] No cloud calls, model auto downloads or 3D UI.

## Deliverables and report
Code + automated tests + commands/results + platform/hardware/runtime versions + security/privacy notes. Mark real runtime tests as PENDING until observed. Keep weights and private data out of git.
