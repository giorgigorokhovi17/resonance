# Undertone (formerly Resonance): notes for AI coding agents

Shared by every agent on this repo (Claude Code, Codex, Antigravity). `CLAUDE.md` imports this
file; keep project rules here so they stay in one place.

## What this is

A personal, **local-only Windows** music player: Tauri 2 (Rust backend in `src-tauri/`) with
React + TypeScript + Tailwind (frontend in `src/`). No telemetry; the network is used only for the
update check, pasted playlist links, lyrics searches the user clicks (LRCLIB,
`core/src/sources/lyrics_search.rs`), online song suggestions the user clicks for (Deezer,
`core/src/sources/similar.rs`), Soulseek after the user signs in, YouTube downloads
after the user turns them on, and phone sync on the home network after the user turns it on. Music
files are only ever read, never modified, except that the user may delete a song (see Rules). See `README.md` for features, architecture and data
locations.

Code layout: `core/` is the platform-neutral crate (audio engine, library, matcher, backups,
sources; no Tauri, events go out through `core/src/events.rs`). `src-tauri/` is the app and the
Cargo workspace root (`Cargo.lock`, `target/`), so `cargo test` there runs both crates. Android
plans: `docs/android-design.md`.

## Commands

Run from the repo root unless noted (PowerShell on Windows; pnpm comes through Corepack):

- Install: `corepack pnpm install`
- Run the app: `corepack pnpm tauri dev`
- Typecheck the frontend: `corepack pnpm typecheck`
- Rust tests (in `src-tauri/`): `cargo test`. More property-test cases: `$env:PROPTEST_CASES='3000'`
- Warnings check (in `src-tauri/`): `cargo check --all-targets` (keep it warning-free)
- Matcher speed (in `src-tauri/`): `cargo run --release --example match_bench`
- Release builds are signed: use `scripts/release.ps1`, not a bare `tauri build`.

Before saying a change works, run the typecheck, and report the actual output. Run the Rust
warnings check and tests only when something in `src-tauri/` changed (besides a version bump), or
when the owner asks: they take over 10 minutes and add nothing for frontend-only changes.

GitHub Actions (`.github/workflows/checks.yml`) runs the typecheck on a Windows runner for every
pull request and every push to `main`, and the warnings check (warnings fail it) and the Rust tests
when `src-tauri/` changed. Put `[full checks]` in a pull request's title, or start the workflow by
hand, to run everything. Don't merge a pull request whose checks fail.

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
  Soulseek downloading (`core/src/sources/soulseek/`, design in `docs/soulseek-design.md`).
  Nothing is shared or uploaded, and downloaded files are marked read-only. **Second exception
  (owner's decision, 2026-10-23):** YouTube downloads through yt-dlp (`core/src/sources/youtube.rs`),
  off by default, for songs Soulseek doesn't have or when the user picks YouTube's quality on import.
  yt-dlp and QuickJS are fetched from their GitHub releases (SHA-256 checked) into
  `%LOCALAPPDATA%\app.resonance.player\tools`, never bundled. Spotify and SoundCloud stay playlist
  imports that match against local files, never downloads.
- **Online song suggestions (owner's decision, 2026-10-25):** a playlist's **Find more online**
  button (`core/src/sources/similar.rs`) asks Deezer's public API (no key) for top songs by
  the playlist's artists and similar artists. Only when the user clicks; sends only the playlist's
  main artist names. Only names come back: never fetch or play Deezer's audio previews. Adding a
  suggestion stores it as a missing song, found through the usual Soulseek / YouTube search.
- **Phone sync (owner's decision, 2026-10-24):** `src-tauri/src/phone.rs` serves the library and
  playlists to a phone's browser on the home network (`tiny_http`, page in `phone.html`). Off by
  default. Every request needs the secret key from the link; songs are reached by playlist and
  position only, never by file path; nothing is uploaded anywhere. Keep it that way: no file paths
  in URLs, no writes, no access without the key.
- **Deleting songs (owner's decision, 2026-10-25):** the user may delete a song's file with
  **Delete from disk…** (`core/src/library/recycle.rs`). It always goes to the Windows Recycle
  Bin, never a hard delete: drives without a Recycle Bin, a bin that's turned off, and files bigger
  than the bin are refused. Each deletion is recorded in `deleted_tracks` so Settings → Recently
  deleted can restore it. Apart from that, music files are never written, moved or deleted.
- **Database:** SQLite at `%APPDATA%\app.resonance.player\library.db`, migrated in
  `core/src/library/db.rs` (`migrate`). Schema changes are additive (`add_column` /
  `CREATE ... IF NOT EXISTS`), never destructive. Add a test for each migration.
- **Playlist matcher** (`core/src/library/matcher/`): wrong matches are worse than misses.
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
