# Undertone (formerly Resonance): notes for AI coding agents

Shared by every agent on this repo (Claude Code, Codex, Antigravity). `CLAUDE.md` imports this
file; keep project rules here so they stay in one place.

## What this is

A personal, **local-only Windows** music player: Tauri 2 (Rust backend in `src-tauri/`) with
React + TypeScript + Tailwind (frontend in `src/`). No telemetry; the network is used only for the
update check, pasted playlist links, lyrics searches the user clicks (LRCLIB,
`src-tauri/src/sources/lyrics_search.rs`), Soulseek after the user signs in, and YouTube downloads
after the user turns them on. Music files are only ever read, never modified. See `README.md` for
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
  Linux or macOS**. On Linux, cross-check and test against the Windows GNU target instead:
  install `gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 cmake ninja-build wine64`, run
  `rustup target add x86_64-pc-windows-gnu`, then in `src-tauri/`
  `CMAKE=/usr/bin/cmake cargo check --target x86_64-pc-windows-gnu --all-targets`. For tests, build
  with `cargo test --target x86_64-pc-windows-gnu --no-run`, copy `WebView2Loader.dll` (under
  `target/x86_64-pc-windows-gnu/debug/build/webview2-com-sys-*/out/x64/`) next to the test exe in
  `target/x86_64-pc-windows-gnu/debug/deps/`, and run it with `wine`. Audio output can't be tried
  this way.

## Rules

- **Never commit** `fixtures/` (personal playlist exports) or the updater signing key
  (`%USERPROFILE%\.tauri\resonance-updater.key`).
- Never download or stream audio from any service. **Exception (owner's decision, 2026-10-19):**
  Soulseek downloading (`src-tauri/src/sources/soulseek/`, design in `docs/soulseek-design.md`).
  Nothing is shared or uploaded, and downloaded files are marked read-only. **Second exception
  (owner's decision, 2026-10-23):** YouTube downloads through yt-dlp (`src-tauri/src/sources/youtube.rs`),
  off by default, for songs Soulseek doesn't have or when the user picks YouTube's quality on import.
  yt-dlp and QuickJS are fetched from their GitHub releases (SHA-256 checked) into
  `%LOCALAPPDATA%\app.resonance.player\tools`, never bundled. Spotify and SoundCloud stay playlist
  imports that match against local files, never downloads.
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
- `docs/soulseek-design.md`: Soulseek protocol choice, NAT/firewall behaviour, credentials.
