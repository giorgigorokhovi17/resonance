# Undertone on Android: design (Phase 0)

Status: **design only, no app code yet.** Written 2026-10-24 for a standalone build that runs on a
Samsung Galaxy S25 (arm64-v8a, One UI on Android 15/16) with no PC, while the Windows app keeps
building from the same repository.

Everything below comes from reading the current code (`src-tauri/src` at 0.7.11), the dependency
sources in the cargo registry (cpal 0.18.2, tao 0.37.1, ndk-context, soulseek-rs-lib 19.1.0), and
the Android/Tauri docs listed at the end. Claims I could not confirm from a source are marked
**(verify on device)**. They become the first checks of the Phase 1 step that needs them.

Open decisions for the owner are collected in [Decisions only you can make](#decisions-only-you-can-make).

---

## 0. Windows-only call sites and what replaces them

Found by searching `src-tauri/src` for `windows::`, `wasapi`, `souvlaki`, tray, global shortcuts,
`island`, `Command::new`, `cfg(windows)` and the desktop-only Tauri plugins.

| Where | What it does | Android |
|---|---|---|
| `audio/output.rs` `open_exclusive`, `hresult`, `explain`, `SampleKind::spec` (`wasapi` crate) | WASAPI exclusive mode, bit-perfect | **Drop.** Later option: Android 14+ bit-perfect USB output (`AudioMixerAttributes`, `MIXER_BEHAVIOR_BIT_PERFECT`) for USB DACs. Not in scope. |
| `audio/output.rs` `list_output_devices` (`wasapi::DeviceEnumerator`) | Output device list for the device picker | **Drop** the picker. Android routes to speaker/wired/Bluetooth/USB itself; the engine follows the default route (§3). |
| `audio/output.rs` `boost_thread_priority` (`AvSetMmThreadCharacteristicsW`) | MMCSS "Pro Audio" priority for the output thread | **Drop.** AAudio's callback thread already runs at audio priority. |
| `audio/output.rs` `open_shared` (cpal, WASAPI host) | Shared-mode output | **Replace** with a small AAudio backend on `ndk::audio` (§3 says why not cpal's). |
| `smtc.rs` (souvlaki, needs an `HWND`) | Media overlay, hardware media keys | **Replace** with Media3 `MediaSessionService` + a `SimpleBasePlayer` wrapping the Rust engine (§2). Lock screen, notification, Bluetooth/headset buttons, Assistant, Wear all come from that. |
| `lib.rs` `build_tray`, tray tooltip in `TrackChanged` | System tray icon and menu | **Drop.** The media notification is the equivalent. |
| `lib.rs` `on_window_event` (close-to-tray, `Destroyed` → `exit(0)`), `fit_to_work_area`, `window.hwnd()` | Desktop window handling | **Drop** (`cfg(desktop)`). Android: the Activity can be destroyed while playback continues (§2). |
| `island.rs` (whole file: `SetWinEventHook`, `SetWindowPos`, `GetAsyncKeyState`, `SHQueryUserNotificationState`), `commands::island_*`, `capabilities/island.json`, `views/Island.tsx` | Dynamic Island overlay window | **Drop.** Overlay windows over other apps need `SYSTEM_ALERT_WINDOW`; the media notification and lock screen cover the use. |
| `hotkeys.rs` (`tauri-plugin-global-shortcut`), `views/settings/Hotkeys.tsx` | Global keyboard shortcuts | **Drop.** Bluetooth/headset buttons come through the media session. |
| `lib.rs` `tauri_plugin_single_instance`, `path_args` / `open_external` | One instance; "Open with" and drag onto the .exe | **Drop** single-instance (Activity `launchMode="singleTask"`). "Open with" becomes an `ACTION_VIEW audio/*` intent filter later (not in the Phase 1 list). |
| `lib.rs` `tauri_plugin_updater`, `tauri_plugin_process`, `src/store/updater.ts` | Signed auto-update and relaunch | **Replace** with our own APK update check (§7). The Tauri updater has no Android support. |
| `commands.rs` `open_url` (`rundll32 url.dll,FileProtocolHandler`) | Opens an https link in the browser | **Replace** with an `Intent.ACTION_VIEW` from the Kotlin plugin (or `tauri-plugin-opener`, which supports Android). Same https-only check. |
| `sources/youtube.rs` `hidden()` (`CREATE_NO_WINDOW`), `Command::new("yt-dlp.exe")`, `qjs.exe`, `taskkill /PID … /T /F`, tool download of `.exe` assets | YouTube downloads through yt-dlp | **Cannot run as is** (§6). Apps can't execute files they downloaded on Android 10+ (W^X); there is no `.exe`, no `taskkill`. |
| `sources/soulseek/credentials.rs` (`CredWriteW`/`CredReadW`/`CredDeleteW`) | Soulseek sign-in in Windows Credential Manager | **Replace** with an Android Keystore AES-GCM key encrypting the sign-in in app-private storage (§5). |
| `library/mod.rs` file watcher (`notify` + `notify-debouncer-full`, `RecommendedWatcher`) | Rescan when files change | Compiles on Android (inotify), but inotify on shared storage (FUSE) misses changes made by other apps **(verify on device)**. **Replace** with a MediaStore `ContentObserver` plus a quick rescan when the app comes to the front (§4). |
| `phone.rs`, `phone.html`, `views/settings/PhoneSync.tsx` (tiny_http) | Serves the library to a phone on Wi-Fi | **Drop on Android** (the phone is now the player). Stays on desktop and becomes one way to hand the backup to the phone (§8). |
| `backup.rs` path remapping (`Path::components`) | Moves a library to a new PC | Portable code, but **a bug on Android**: Windows paths like `D:\Music\Artist` are one single component on Unix, so remapping would never match. Needs to split on `\` and `/` itself (§8). |
| `settings.rs` `close_to_tray`, `island`, `island_options`, `output_mode`, `device_id`, hotkeys | Desktop-only settings | Kept in the file (backups stay compatible), ignored on Android, hidden in the UI. |
| `tauri.conf.json` window size, `decorations: false`, `src/components/TitleBar.tsx` | Frameless desktop window with our own title bar | Android overrides in `tauri.android.conf.json`; the title bar is not rendered on mobile. |
| `.cargo/config.toml` CMake from VS Build Tools (libopus via `symphonia-adapter-libopus`) | Builds libopus | Must build with the NDK's CMake toolchain for `aarch64-linux-android`. **Risk:** first thing to check in step 2. |

Already portable (no change beyond moving): `audio/{decoder,dsp,effects,eq,resample,types,viz,engine}`,
all of `library/` except the watcher, `queue.rs`, `persist.rs`, `settings.rs`, `sources/links`,
`sources/lyrics_search.rs`, `sources/missing.rs`, `sources/soulseek` (soulseek-rs-lib depends only
on `std` and `libc`). Its `rust-version` is 1.91 while ours says 1.90, so bump ours.

The frontend uses `@tauri-apps/api/{core,event,webview,window,menu,app}` and the `dialog`,
`process` and `updater` plugins. `window`/`menu` are used only by the title bar, the island and
motion backgrounds; `process`/`updater` only by `store/updater.ts`.

---

## 1. Cargo workspace

### Layout

Keep the workspace root **inside `src-tauri/`** so `Cargo.lock`, `target/`, `scripts/release.ps1`
(`src-tauri\target\release\bundle`), the CI cache key and every command in `AGENTS.md`
(`cd src-tauri; cargo test`) keep working unchanged.

```
src-tauri/
  Cargo.toml              # [workspace] + the app package (crate "resonance", as today)
                          # default-members = [".", "core"] so plain `cargo test` runs both
  src/                    # the Tauri app: commands, setup, and the two shells
    lib.rs                # builder, protocols, shared setup
    commands.rs           # Tauri commands (shared; a few are cfg-gated)
    desktop/              # cfg(desktop): tray, island, hotkeys, smtc, phone sync, updater glue
    android/              # cfg(target_os = "android"): JNI bridge, credentials, storage, updater
  core/                   # crate "undertone-core": no Tauri dependency
    src/audio/            # decoder, dsp, effects, eq, resample, viz, engine, types
    src/audio/output/     # mod.rs (shared ring/handle code), wasapi.rs + cpal.rs (cfg windows),
                          # aaudio.rs (cfg android)
    src/library/          # db, scan, tags, covers, lyrics, interchange, matcher, m3u/pls/wpl/xspf/csv, media, stats
    src/sources/          # soulseek (minus the OS credential store), links, lyrics_search, missing, youtube (model + matching)
    src/backup.rs src/queue.rs src/persist.rs src/settings.rs src/sleep.rs
    proptest-regressions/ # moves with the tests that own it
  plugins/android/        # tauri-plugin-undertone-android: Rust shim + android/ Kotlin library
  gen/android/            # from `tauri android init`; committed (manifest and MainActivity get edited)
```

Why one Tauri app crate with two shell modules instead of two app crates: both shells expose the
same ~150 Tauri commands to the same React app. Two app crates would duplicate `commands.rs` or
need a third crate for it. Tauri's own `desktop`/`mobile` cfg aliases (set by `tauri-build`) are
made for this.

### Dependency gating

```toml
# core/Cargo.toml
[target.'cfg(windows)'.dependencies]
wasapi = "0.26"
windows = { version = "0.62", features = ["Win32_Foundation", "Win32_System_Threading"] }
cpal = "0.18"
[target.'cfg(target_os = "android")'.dependencies]
ndk = { version = "0.9", features = ["audio", "api-level-28"] }   # already in the tree through cpal

# src-tauri/Cargo.toml
[target.'cfg(not(any(target_os = "android", target_os = "ios")))'.dependencies]
tauri-plugin-single-instance = "2"
tauri-plugin-updater = "2"
tauri-plugin-process = "2"
tauri-plugin-global-shortcut = "2"
souvlaki = "0.8"
tiny_http = "0.12"
qrcodegen = "1"
[target.'cfg(windows)'.dependencies]
windows = { version = "0.62", features = ["Win32_Security_Credentials", "Win32_UI_Shell", "Win32_UI_WindowsAndMessaging", "Win32_UI_Accessibility", "Win32_UI_Input_KeyboardAndMouse"] }
[target.'cfg(target_os = "android")'.dependencies]
jni = "0.21"
tauri-plugin-undertone-android = { path = "plugins/android" }
```

`notify`/`notify-debouncer-full` stay in core but the watcher is only started on desktop.

### What step 1 changes, and only that

1. Move the portable modules into `core/` with **the same module paths** (`crate::library::…`
   still resolves inside core), so every test moves byte-for-byte. The app crate re-exports them
   (`pub use undertone_core::{audio, library, …};`) so `commands.rs` keeps its `crate::` paths.
2. Replace the four `AppHandle` uses in portable code with event callbacks, the pattern
   `Library::open` already uses (`Arc<dyn Fn(LibraryEvent)>`): `sleep.rs`, `sources/missing.rs`,
   `sources/youtube.rs`, `sources/soulseek/mod.rs` (`app.emit("slsk:…")`). The shell maps each
   event to `emit`.
3. Credentials behind a trait: `trait CredentialStore { save, load, delete }`. Windows impl stays
   in the app crate with its round-trip test unchanged.
4. Move `EqPreset` out of `commands.rs` (backup.rs needs it) into core.
5. `cfg` gates as above. Nothing Android-specific is added yet.

Proof for step 1: `cargo test` (both members) with the same test count as before the split,
`cargo check --all-targets` warning-free, `corepack pnpm typecheck`, the desktop app starts and
plays. CI also gets `cargo check -p undertone-core --target aarch64-linux-android` so core stays
portable from day one.

---

## 2. Where the audio engine lives on Android

### The constraint

Tauri runs inside the Activity. Android destroys Activities freely (swipe from Recents, rotation,
memory pressure), while music must keep playing. So the engine can't belong to the UI.

What the code says (tao 0.37.1 `platform_impl/android/ndk_glue.rs`): Tauri's `main()`, and so our
`setup` closure, runs **once per process**, on the first Activity create. Later Activity re-creates
attach to the already-running app. This is good: if the process survives a swipe-away, the engine
survives, and a new Activity gets the same state.

The process survives only while something keeps it in the foreground: the playback service while
playing, a download job while downloading (§5). Without either, Android caches and then kills it.

### Design

- **One process, one engine.** `undertone_core::host::Host` holds Library, Engine, Soulseek,
  SleepTimer and settings in a process-wide `OnceLock`. Whoever comes first creates it:
  the Tauri `setup` (normal launch) or the playback service (Bluetooth "play" after the app was
  killed: media-button playback resumption). The other one attaches.
- **Playback service (Kotlin, in the plugin):** `PlaybackService : MediaSessionService`, declared
  with `foregroundServiceType="mediaPlayback"` and `FOREGROUND_SERVICE_MEDIA_PLAYBACK`. Its
  `Player` is a `SimpleBasePlayer` subclass that does no audio itself: `getState()` reads a
  snapshot pushed from Rust; `handleSetPlayWhenReady/handleSeek/handleSetMediaItems…` call Rust
  through JNI (`Java_app_resonance_player_NativeEngine_*` functions in `src/android/bridge.rs`).
  Media3 then gives us the media notification, the lock-screen controls, Bluetooth/headset/Wear
  buttons and Assistant without more code. ExoPlayer is not used: the Rust engine (EQ, ReplayGain,
  crossfade, gapless, effects) stays the one engine on both platforms.
- **UI ↔ engine:** unchanged. The WebView calls Tauri commands, which call the `Host` in the same
  process; engine events go out as Tauri events when an app handle is registered, and always to
  the Kotlin player through a JNI callback (`invalidateState()`). No sockets, no second process.
- **Swipe-away:** Media3's default `onTaskRemoved` keeps the service running if playing and stops
  it if paused; that's the behaviour we want. Activity destroyed → Tauri events simply have no
  listener; the next Activity re-reads state via the existing `player_status` command.
- **Screen off:** an AAudio stream that is playing keeps the CPU awake through the audio HAL; the
  foreground service keeps the process out of the cached-app freezer. No wake lock of our own.
  **(verify on device: 30 min screen-off, no xruns.)**
- **Playback resumption:** enable `MediaButtonReceiver` and implement `onPlaybackResumption` from
  the saved `queue.json` (already written every 5 s by `persist.rs`).
- **Panics:** every `extern "system"` JNI function wraps its body in `catch_unwind`; a panic across
  JNI aborts the process.

### The ndk-context trap

cpal's AAudio backend reads the Android context through `ndk-context`, and tao initializes that
context on the first Activity create. `ndk_context::initialize_android_context` **asserts** it was
never set before. If the service starts the engine first and sets the context so cpal can work,
the later Activity launch panics inside tao. This is one reason for §3's own AAudio backend: plain
AAudio (`ndk::audio`) needs no Java context at all.

---

## 3. Audio output

### Why not cpal's AAudio backend

From cpal 0.18.2 `src/host/aaudio/mod.rs`:

- It never sets `AAUDIO_PERFORMANCE_MODE_POWER_SAVING`. Without the `realtime` feature it sets no
  mode (`NONE`); with it, `LOW_LATENCY`. Power-saving mode ("larger internal buffers and a data
  path that trades off latency for lower power", AAudio guide) is exactly what a music player
  wants, and it is the main battery lever for screen-off playback.
- It sets no usage/content type, and its device listing and burst sizing go through Java
  (`AudioManager`) via `ndk-context`, which hits the trap above.

So `core/src/audio/output/aaudio.rs` uses `ndk::audio::AudioStreamBuilder` directly (≈250 lines,
the same shape as the WASAPI exclusive backend): `PowerSaving`, `Shared`, `Usage::Media`,
`ContentType::Music`, format `F32`, stereo, data callback reading the existing `rtrb` ring.

### Sample rate, resampling, gapless

- Open the stream **without** a sample rate, read back what AAudio chose (normally 48 kHz on the
  S25 **(verify on device)**), and resample in our engine with the existing rubato path, as shared
  mode already does on Windows. One stream for the whole session: no reopen between tracks, so
  gapless stays the engine's job, as today.
- Don't ask AAudio for 44.1 kHz: requested rates are honoured by resampling inside the framework
  with a resampler we don't control.
- 24/32-bit and hi-res files decode to f64 internally and leave as f32, as on desktop.

### Audio focus, calls, unplug, Bluetooth

With a custom player Media3 does not manage focus (it only does for ExoPlayer), so the Kotlin
player does it:

- `AudioFocusRequest(AUDIOFOCUS_GAIN)` with `USAGE_MEDIA`/`CONTENT_TYPE_MUSIC`, requested on play.
  On Android 15+ this only succeeds from the top app or a foreground service, which is why play
  first promotes the service.
- `LOSS` → pause, no auto-resume. `LOSS_TRANSIENT` (incoming call) → pause and remember; resume on
  `GAIN`. `CAN_DUCK` → Android 12+ ducks automatically; we do nothing. Abandon focus on stop.
- Headphones unplugged / Bluetooth disconnect: `ACTION_AUDIO_BECOMING_NOISY` receiver → pause.
- Route changes (Bluetooth connects, USB DAC plugged in): AAudio disconnects the stream. The error
  callback hands off to another thread (the guide warns closing from the callback can deadlock),
  which closes and reopens on the new default device at whatever rate it reports, and the engine
  resumes from the same sample position. The ring holds 0.5 s, so a reroute can drop at most that.
- Bluetooth codec choice (LDAC/AAC/SBC) is the system's; we can show it read-only later.

---

## 4. Music access, scanning, watcher, database

The core keys everything by file path: `tracks.path`, `playlist_items.path`, `saved_lyrics.path`,
match decisions, the backup remapper, `Decoder::open(Path)`, `walkdir`. Any design that keeps real
paths keeps the matcher, backups and tests identical across platforms.

### Options

| | A. Media permission + real paths (recommended) | B. SAF folder only | C. All files access |
|---|---|---|---|
| How | User picks the folder with the SAF picker (`ACTION_OPEN_DOCUMENT_TREE`); we keep the persistable URI and also resolve it to a path (`primary:Music` → `/storage/emulated/0/Music`). Audio is read by path with `READ_MEDIA_AUDIO`. | Everything through `content://` URIs and file descriptors from `ContentResolver`. | `MANAGE_EXTERNAL_STORAGE`, then plain paths everywhere. |
| What the app can read | All audio on the phone (incl. voice notes, WhatsApp audio), by path | Only the picked folder | Every file on shared storage |
| Core changes | None for audio. Non-audio files (`.lrc`, `folder.jpg`) fall back to an fd from the SAF tree. | A filesystem trait under scan, tags, covers, decoder, lyrics, backup; DB keys become synthetic paths. | None |
| Speed | Sequential reads by path ≈ MediaStore (Android docs) | One Binder call per directory listing; fine for 47 files, slow for 50k | Same as A |
| Play Store | Allowed | Allowed | Not allowed for a media player ("Media Files access" is listed as an invalid use) |

Recommendation: **A**, because it keeps the core untouched and desktop/phone paths comparable;
SAF still decides *which* folders are libraries. Fallback to B if the step-4 spike shows path reads
failing **(verify on device: FLAC/MP3/M4A/Opus in `Music/` open by path with only
`READ_MEDIA_AUDIO`; does `folder.jpg` in `Music/` need SAF?)**. This is a privacy trade-off, so it's
a question below.

### Scanning and watching

- Scanning: unchanged `scan.rs` (rayon + walkdir over the resolved path). Embedded tags and art by
  lofty, folder images and `.lrc` through the SAF fallback.
- Watching: register a `ContentObserver` on `MediaStore.Audio.Media.EXTERNAL_CONTENT_URI`
  (debounced 2 s like today) plus a quick incremental scan on every `onResume`. Soulseek downloads
  trigger a scan of their folder directly, as now.
- New files we create (downloads) are announced with `MediaScannerConnection.scanFile` so other
  apps see them.

### Where things live

`app_data_dir`/`app_config_dir` on Android resolve to app-private internal storage
(`/data/user/0/app.resonance.player/…`): `library.db`, `settings.json`, `queue.json`, the cover
cache in `app_cache_dir`. Auto Backup is turned off (`android:allowBackup="false"`) so the
database isn't copied to Google Drive and the Keystore-encrypted sign-in isn't restored to a
device that can't decrypt it. Migrations are the same `db.rs` code: additive only, same tests.

---

## 5. Soulseek on a phone

### Reachability

- **Mobile data:** carriers use CGNAT, so nobody can connect to the phone. **Listening is off by
  default on Android**, and the listen port setting is hidden. We still connect *out* to every
  peer that is reachable, and the server brokers the rest ("pierce firewall"). As
  `docs/soulseek-design.md` already says for a router without a forwarded port: searches look
  thinner; downloads from users who are also unreachable stay queued and time out.
- **Home Wi-Fi:** the same, unless the owner forwards a port to the phone's fixed local address.
  Not worth a UI in v1.
- **Wi-Fi-only switch** (default **on**): downloads need an unmetered network
  (`NET_CAPABILITY_NOT_METERED`). Searching on mobile data is allowed (small traffic).
- **Online only when useful:** with no visible UI, no playback and no download job, the cached-app
  freezer stops our threads and the server connection drops. So Soulseek signs in when the app is
  opened or a download job starts, and signs out ~5 minutes after both end. Ongoing searches only
  run while the app is visible.

### Downloads that survive the app closing

- **User-initiated data transfer job** (`JobInfo.Builder.setUserInitiated(true)`, Android 14+,
  `RUN_USER_INITIATED_JOBS`): built for "user tapped download". Not subject to standby-bucket
  quotas, no fixed time limit, starts immediately, shows a progress notification
  (`setNotification`). Must be scheduled while the app is visible, which is always the case when
  the user taps Download.
- Not a `dataSync` foreground service: on Android 15 those are capped at 6 h per 24 h and get
  `onTimeout`.
- The job's only work is to keep the process alive while the Rust queue (unchanged) runs: Kotlin
  `onStartJob` → JNI `soulseek_job_started`; Rust calls `jobFinished` back when the queue is empty.
  The queue state is persisted so a job killed without `onStopJob` (low memory, Task Manager)
  resumes on next launch (soulseek-rs-lib supports resume).
- Swiping the app from Recents doesn't stop the job **(verify on device)**. Force-stop kills
  everything, as with any app.
- Samsung's "Sleeping apps" can still be aggressive; the Sources screen links to the battery
  setting and suggests "Unrestricted" if a download stalls.

### Where files go

Download into app-specific external storage (`Android/data/app.resonance.player/files/soulseek/`,
no permission), then on completion publish into `Music/Undertone/Soulseek/<user>/…` through
MediaStore (`RELATIVE_PATH`, `IS_PENDING`), so other apps see it and the file survives uninstall.
**Rule difference:** desktop marks downloads read-only; on Android shared storage there is no
read-only attribute to set. The app still never writes to a finished file. That needs your OK.

### Credentials

A non-exportable AES-256-GCM key in the Android Keystore (`KeyGenParameterSpec`, purpose
encrypt/decrypt), username + password encrypted with it into an app-private file. Not
EncryptedSharedPreferences: the security-crypto library is deprecated with no further releases.
Rust reaches it through the same `CredentialStore` trait as Windows, implemented with JNI calls
into the plugin. The password never goes to `settings.json`, logs or the WebView after sign-in,
same as desktop.

---

## 6. YouTube

yt-dlp is a Python program; desktop runs `yt-dlp.exe` (Python bundled) plus `qjs.exe`, fetched
from GitHub with a SHA-256 check. Since late 2025 yt-dlp needs a JavaScript runtime (Deno, Node,
QuickJS-ng, Bun) for full YouTube support. Android forbids executing files an app downloaded
(W^X since Android 10): executables must ship inside the APK as `lib*.so` in `nativeLibraryDir`.

| Option | How | Cost | License & rule fit |
|---|---|---|---|
| **youtubedl-android** library (used by Seal) | Ships Python (and optionally ffmpeg) as `.so`; yt-dlp itself is a Python zip updated at runtime | APK grows by tens of MB. **No JS runtime bundled** today; Seal has an open proposal to bundle Deno as a `.so`. We download `.m4a` directly, so ffmpeg is not needed. | **GPL-3.0** (Undertone is MIT), and Python would be **bundled**, needing an exception to the `AGENTS.md` "never bundled" rule. |
| **Chaquopy + yt-dlp + QuickJS-ng `.so`** | Chaquopy (MIT) embeds CPython at build time with `yt-dlp`, paired with a compiled QuickJS-ng `.so` in `jniLibs` | Similar size; a second Python toolchain in the build | **MIT-compatible**, but Python, yt-dlp and QuickJS-ng are **bundled** in the APK, needing an exception to the `AGENTS.md` "never bundled" rule. |
| **Remote helper** | The desktop app runs yt-dlp and the phone asks it | Small | Breaks "no PC needed". |
| **Drop on Android** | Soulseek only; YouTube results greyed out with "Desktop only" | None | Fits all rules and keeps the MIT license clean. |

**Status: Open decision for Step 7** (see §Decisions below).

---

## 7. Distribution

### Sideload vs Play Store

- **Play Store:** YouTube downloaders are refused (cited under Device and Network Abuse), P2P
  download apps face IP-policy review, and Play requires a recent target API and 16 KB page-size
  alignment. Soulseek alone is a real rejection risk. Not recommended.
- **Signed APK, sideloaded** from GitHub Releases: recommended.
- **Android developer verification:** from 2026-09-30 certified devices in Brazil, Indonesia,
  Singapore and Thailand only install apps from verified developers, globally in 2027. Installs
  over ADB and through the "advanced flow" stay possible, and there are free limited-distribution
  accounts (email only, up to 20 devices). Whether to register is a question below.

### Signing

- Release keystore outside the repo, e.g. `%USERPROFILE%\.android\undertone-release.jks`, like the
  updater key. Passwords from environment variables read by `gen/android/app/build.gradle.kts`,
  never in `gradle.properties` in the repo. `.gitignore` gets `*.jks`, `*.keystore`,
  `keystore.properties`.
- Lose the key and existing installs can never be updated (uninstall + reinstall loses data). Keep
  an offline copy.
- CI: the keystore as a base64 GitHub secret, decoded to a temp file in the job; v2+v3 signature
  schemes; `apksigner verify --print-certs` as a step.

### Update check

The Tauri updater doesn't support Android, so:

1. On launch (and from Settings), fetch `android-latest.json` from the latest GitHub release:
   `{ version, versionCode, url, sha256, notes }`. Same network allowance as the desktop check.
2. If newer, show it; on "Update", download the APK into the cache, check the SHA-256, and install
   with a `PackageInstaller` session. Android itself refuses an APK signed with a different key.
3. Needs `REQUEST_INSTALL_PACKAGES`; the first time, the user allows "Install unknown apps" for
   Undertone in Settings.

`scripts/release.ps1` learns to build the APK and write `android-latest.json` next to
`latest.json`.

---

## 8. Importing the desktop library and playlists

The existing `resonance-backup` v1 JSON already holds settings, EQ presets, library folders,
playlists (by path, plus matcher ref keys for imported ones), artwork choices and small media.
On the phone:

1. Desktop: Settings → Backup → Save (as today). Get the file to the phone any way (USB, cloud,
   or a "Backup for your phone" link on the existing phone-sync page — read-only, behind the key,
   so it fits the phone-sync rules).
2. Phone: pick the file with the SAF document picker; Kotlin opens it and passes the bytes to
   `backup::restore`.
3. Restore remaps each Windows folder to a phone folder by trailing path components, which already
   handles "D:\Music\Artist\Album" → "/storage/emulated/0/Music/Artist/Album". **Fix needed:** split
   stored paths on both `\` and `/` rather than `Path::components`, which on Android sees a Windows
   path as one component. New tests, existing ones untouched.
4. Playlist entries whose file isn't found after remapping fall back to the matcher, with the same
   rules as desktop: unsure stays a miss, never a guess.
5. Desktop-only settings (output mode, device, island, hotkeys, close-to-tray) are kept but ignored.

Identical matches: the matcher runs from the same core code with the same `NORM_VERSION`. A
golden test in core runs a fixed export against a fixed fake library and checks the result; CI
runs it on Windows and, through the Android build, on the device in step 5 (§9).

---

## 9. Measurable targets and how they're measured

| Target | Goal | Measured with |
|---|---|---|
| Cold start to first frame | ≤ 700 ms | `adb shell am start -W -n app.resonance.player/.MainActivity` (TotalTime), 10 runs after `am force-stop`, median |
| Cold start to library shown | ≤ 1.5 s | Perfetto trace (`adb shell perfetto -o /data/misc/perfetto-traces/start.pftrace -t 10s sched freq am wm webview`) with a `reportFullyDrawn()` marker |
| Battery, screen-off local playback | ≤ 2 % per hour (FLAC, wired or BT) | `adb shell dumpsys batterystats --reset`, 60 min unplugged, then `dumpsys batterystats --charged app.resonance.player`; Battery Historian for the picture |
| Dropouts | 0 in 30 min screen-off | `adb shell dumpsys media.audio_flinger` underrun counts, our own AAudio `getXRunCount()` logged every minute to logcat |
| Gaps between tracks | 0 samples | Core test: two files cut from one sine sweep decode back-to-back without a discontinuity; on device, the same files by ear with a gapless album |
| Memory | ≤ 250 MB PSS with UI; ≤ 80 MB with the Activity destroyed and music playing | `adb shell dumpsys meminfo app.resonance.player` |
| APK size | ≤ 30 MB (arm64-v8a only, no YouTube) | `apkanalyzer apk file-size` on the release APK |
| Lock screen / Bluetooth | Play, pause, next, previous, seek work | `adb shell dumpsys media_session`; `adb shell cmd media_session dispatch play-pause`; a real BT headset |
| Phone call | Pauses, resumes after | Call the phone; logcat for the focus change |

The rest of your proof list (Maestro flow, `apksigner verify`, identical matches, CI on both
platforms) is attached to the Phase 1 steps that produce them.

---

## Phase 1 plan, adjusted

Same order you gave, with what this design adds:

1. **Workspace split** (§1). Also: CI `cargo check` of core for `aarch64-linux-android`.
2. **Android skeleton:** `tauri android init`, NDK r28+ (16 KB pages by default), JDK 17,
   arm64-v8a only, `minSdk 34` (UIDT needs 34; the S25 runs 15/16), `targetSdk 36`. Get libopus
   building with the NDK first. **Not in your list but needed: a phone layout for the React
   UI.** Today's UI is a 1280×820 desktop window with its own title bar. Minimum: hide desktop-only
   views, bottom navigation, a now-playing sheet, touch-sized controls (see questions).
3. **Playback service** (§2, §3): own AAudio backend, `Host` singleton, JNI bridge, Media3 service,
   focus/noisy handling.
4. **Library** (§4): folder picker, permission flow, path-access spike, MediaStore observer.
5. **Playlist import + matcher**, plus the backup restore and path-splitting fix (§8).
6. **Soulseek** (§5): Keystore credentials, UIDT job, Wi-Fi-only, MediaStore publishing.
7. **YouTube:** per the decision below.
8. **Release:** signed APK, update check, CI job (Ubuntu runner with NDK, or Windows; either works
   for Android) that builds, signs and runs `apksigner verify`.

---

## Owner's decisions

1. **Storage access — Decided: Option A.** `READ_MEDIA_AUDIO` with real file paths; the system
   folder picker (`ACTION_OPEN_DOCUMENT_TREE`) only chooses folders. Implemented in Step 4.
2. **YouTube on Android — Open (Step 7).** Not decided yet:
   - Option 1: Seal's library (`youtubedl-android`), which is **GPL-3.0** (Undertone is MIT) and
     bundles no JavaScript engine.
   - Option 2: **Chaquopy** (MIT) + `yt-dlp` + a compiled **QuickJS-ng `.so`**.
   - Both options require an exception to the `AGENTS.md` rule "never bundled" (or YouTube can be
     dropped on Android).
3. **Downloads not read-only on Android — Decided.** Android shared storage has no read-only flag;
   a finished download is never written again (noted in `AGENTS.md`).
4. **Distribution — Decided.** A signed APK on GitHub Releases with an in-app update check.
5. **Developer verification — Decided.** Sideloaded APK / ADB as needed.
6. **Minimum and target Android versions — Decided.** `minSdk 34`, `targetSdk 36`, `arm64-v8a` only.
7. **Phone UI — Decided.** Responsive version of the desktop UI.
8. **Soulseek on mobile data — Decided.** Wi-Fi only by default, with a setting that allows mobile
   data.
9. **Release keystore — Decided.** At `%USERPROFILE%\.android\undertone-release.jks`, never
   committed; passwords in environment variables, and a GitHub secret for CI.
10. **App identifier — Decided.** `app.resonance.player`.

---

## Sources

- Media3 background playback: <https://developer.android.com/media/media3/session/background-playback>
- Foreground service types: <https://developer.android.com/develop/background-work/services/fgs/service-types>
- User-initiated data transfer jobs: <https://developer.android.com/develop/background-work/background-tasks/uidt>
- Data transfer options: <https://developer.android.com/develop/background-work/background-tasks/data-transfer-options>
- Shared storage, media files: <https://developer.android.com/training/data-storage/shared/media>
- AAudio guide: <https://developer.android.com/ndk/guides/audio/aaudio/aaudio>
- Audio focus: <https://developer.android.com/media/optimize/audio-focus>
- Doze and App Standby: <https://developer.android.com/training/monitoring-device-state/doze-standby>
- Tauri 2 mobile plugins: <https://v2.tauri.app/develop/plugins/develop-mobile/>
- All files access policy: <https://support.google.com/googleplay/android-developer/answer/10467955>
- Developer verification: <https://android-developers.googleblog.com/2026/03/android-developer-verification-rolling-out-to-all-developers.html>
- yt-dlp JS runtime requirement: <https://github.com/yt-dlp/yt-dlp/wiki/ejs>, <https://github.com/yt-dlp/yt-dlp/issues/14404>
- Seal proposal to bundle Deno: <https://github.com/JunkFood02/Seal/issues/2640>
- EncryptedSharedPreferences deprecation: <https://blog.includesecurity.com/2026/08/encryptedsharedpreferences-is-dead-heres-what-you-should-use-instead/>
- Code read in the cargo registry: cpal 0.18.2 `src/host/aaudio/mod.rs`, tao 0.37.1
  `src/platform_impl/android/ndk_glue.rs`, ndk-context `src/lib.rs`, soulseek-rs-lib 19.1.0 `Cargo.toml`.
