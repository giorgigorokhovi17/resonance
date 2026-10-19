# Resonance: notes for AI coding agents

Shared by every agent on this repo (Claude Code, Codex, Antigravity). `CLAUDE.md` imports this
file; keep project rules here so they stay in one place.

## What this is

A personal, **local-only Windows** music player: Tauri 2 (Rust backend in `src-tauri/`) with
React + TypeScript + Tailwind (frontend in `src/`). No accounts, no telemetry; the update check is
the only network request. Music files are only ever read, never modified. See `README.md` for
features, architecture and data locations.

## Commands

Run from the repo root unless noted (PowerShell on Windows; pnpm comes through Corepack):

- Install: `corepack pnpm install`
- Run the app: `corepack pnpm tauri dev`
- Typecheck the frontend: `corepack pnpm typecheck`
- Rust tests (in `src-tauri/`): `cargo test`. More property-test cases: `$env:PROPTEST_CASES='3000'`
- Warnings check (in `src-tauri/`): `cargo check --all-targets` (keep it warning-free)
- Matcher speed (in `src-tauri/`): `cargo run --release --example match_bench`
- Release builds are signed: use `scripts/release.ps1`, not a bare `tauri build`.

Before saying a change works, run the tests and the typecheck, and report the actual output.

## Platform

- The app targets **Windows only** (WASAPI output, SMTC media keys, MSVC toolchain).
- `windows` and `wasapi` are currently unconditional dependencies, so **the crate does not build on
  Linux or macOS**. Cloud agents running Linux can edit code but can't build or test it until those
  dependencies are made Windows-only in `src-tauri/Cargo.toml`.

## Rules

- **Never commit** `fixtures/` (personal playlist exports) or the updater signing key
  (`%USERPROFILE%\.tauri\resonance-updater.key`).
- Never download or stream audio from any service.
- **Database:** SQLite at `%APPDATA%\app.resonance.player\library.db`, migrated in
  `src-tauri/src/library/db.rs` (`migrate`). Schema changes are additive (`add_column` /
  `CREATE ... IF NOT EXISTS`), never destructive. Add a test for each migration.
- **Playlist matcher** (`src-tauri/src/library/matcher/`): wrong matches are worse than misses.
  Any rule change must keep the hard-case tests in `matcher/tests.rs` passing. If you change a
  normalisation rule, bump `NORM_VERSION` in `normalize.rs`.
- Match the surrounding code's style and comment density. Keep user-facing text plain and short.

## Working alongside other agents

- One task per branch. Merge through pull requests.
- Never have two agents edit the same checkout at the same time. Use separate branches or
  `git worktree`.
- `git pull` before starting work.

## Docs

- `docs/playlist-interchange/phase0-findings.md`: playlist import/export and matcher plan,
  decisions and status. Update its status sections when that work changes.
