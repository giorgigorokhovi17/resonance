# Playlist interchange: Phase 0 findings

Checked 2026-10-18. No feature code has been written. The only code added is a read-only audit tool, `src-tauri/examples/id_audit.rs`.

## 0. Blockers to resolve first

1. **`/fixtures` does not exist.** None of the sample exports were provided, so none of the format notes in §2 have been checked against your real files. §2 has export steps for each one.
2. **The library is 38 tracks, and that's the whole collection** (confirmed by you; `C:\Users\Giorgi\Desktop\MusicG14`, 877 MB). The golden-set evaluation has been re-planned around this; see §5.
3. **Fingerprinting doesn't fit the matcher as specified.** Spotify, YouTube and Apple exports carry no audio fingerprints, so Chromaprint can't match an *external* TrackRef against a local file. It only helps (a) local-to-local matching, such as phone files against PC files, which path matching already covers better, or (b) through the online AcoustID lookup (fingerprint → AcoustID → MBID), which needs an API key and the network. `fpcalc` is also not installed on this machine. **Decided 2026-10-18: fingerprinting is dropped from the matcher tiers.** There is no fpcalc dependency and no setting for it.
4. **A tier is missing: path/location.** M3U, PLS, XSPF, WPL and Library.xml all carry file locations. **Decided 2026-10-18: a path tier is added ahead of ISRC.** The tier order is now **path → ISRC → MBID → fuzzy**.
   - **Exact path:** the location is normalised (`file://` URI decoded, `/` → `\`, NFC, case-insensitive) and equals a library path. This auto-accepts.
   - **Suffix path:** the trailing components match a library path, as in `/storage/emulated/0/Music/Artist/01 Song.flac` → `…\MusicG14\Artist\01 Song.flac`. This auto-accepts only when at least the filename plus its parent folder match, exactly one library file matches, and any `#EXTINF`/`Length`/`duration` from the playlist doesn't contradict the file (within ±3 s). Otherwise the item goes to review and records the reason.
   - **No resolvable path:** the item falls through to ISRC/fuzzy, using whatever title, artist and duration the format carries.
5. **Phase 3 changes the app's promise.** The README says "No accounts, no network access", and the only network call today is the update check. The online connectors need the README, the CSP and the capabilities updated, even with them off by default.

## 1. Online services (official docs)

### Spotify Web API

| Topic | Current rule | Source |
|---|---|---|
| Development Mode | The app owner **must have Premium** (the app stops working if it lapses). **5 allowlisted users** per app; anyone else gets 403. Applied to new apps from 2026-02-11 and to existing apps from 2026-03-09. | [Migration guide](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide), [Quota modes](https://developer.spotify.com/documentation/web-api/concepts/quota-modes) |
| Extended Quota | Since 2025-05-15, organizations only: a registered business with a launched service and ≥250k MAU. **Not available to Resonance.** | [Quota modes](https://developer.spotify.com/documentation/web-api/concepts/quota-modes) |
| Quota | Counted **per developer account** across its Client IDs (up to 25). When exceeded: `429` with `{"error":{"status":429,"message":"Too many requests","reason":"QUOTA_EXCEEDED"}}`. | [Blog 2026-07-23](https://developer.spotify.com/blog/2026-07-23-web-api-quota-updates), [Changelog Jul 2026](https://developer.spotify.com/documentation/web-api/references/changes/july-2026) |
| Rate limit | Rolling 30-second window. A `429` normally carries `Retry-After` in seconds. A rate-limit 429 and a quota 429 need different handling: quota means stop, rate limit means back off. | [Rate limits](https://developer.spotify.com/documentation/web-api/concepts/rate-limits) |
| Read playlists | `GET /me/playlists`, then `GET /playlists/{id}/items`. **Items are returned only for playlists the user owns or collaborates on**, so followed playlists come back with no `items`. Response field `tracks` was renamed to `items` (`items.items[].item`). | [Migration guide](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide) |
| Write playlists | `POST /me/playlists` (replaces `/users/{id}/playlists`), `POST`/`PUT`/`DELETE /playlists/{id}/items` (the `tracks` param is now `items`). | same |
| ISRC | `external_ids` was removed in February, then **reverted in March 2026**: Track and Album keep it. | [Changelog Mar 2026](https://developer.spotify.com/documentation/web-api/references/changes/march-2026) |
| Lookups | Batch `GET /tracks` was removed, so tracks are fetched one at a time. Search `limit` is now max 10, default 5. This makes resolving local tracks to Spotify URIs expensive on export. | [Migration guide](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide) |
| Redirect URI | `http://127.0.0.1:PORT/...` or `http://[::1]:PORT/...`. **`localhost` is rejected.** Loopback may use a dynamic port. HTTPS is required for anything else. | [Redirect URI](https://developer.spotify.com/documentation/web-api/concepts/redirect_uri) |
| Policy | §III.9: "Do not build an SDA that enables the transfer of data to another service, except … the metadata of the user's playlists to another service." This explicitly allows playlist metadata transfer. §III.5 forbids integrating with *streams or content* from another service (we don't). §III.13 forbids analysing Spotify Content (keep to matching; no stats or derived metrics). §I.2: delete the user's data when they disconnect. Effective 2025-05-15. *(Not legal advice.)* | [Developer Policy](https://developer.spotify.com/policy) |

**What it means:** read/write works for a personal desktop app, but each user brings their **own Client ID** and needs Premium. There is no shared Resonance Client ID beyond 5 people. Exporting to Spotify is limited by search (≤10 results per call, one call per unresolved track).

### YouTube Data API v3

| Topic | Current rule | Source |
|---|---|---|
| Daily quota | 10,000 units per project per day. `search.list` and `videos.insert` each have a separate limit of **100 calls/day**. | [Quota costs](https://developers.google.com/youtube/v3/determine_quota_cost) (page updated 2026-10-15) |
| Costs | `playlists.list` 1 · `playlists.insert/update/delete` 50 · `playlistItems.list` 1 · `playlistItems.insert/update/delete` **50** · `videos.list` 1 · `search.list` 1 (capped at 100/day) · `channels.list` 1 | same |
| Practical limits | Creating a playlist plus adding items: **~199 inserts/day**. Reordering costs 50 per moved item. Resolving local tracks to video IDs: **≤100 searches/day**. A 300-track export takes ≥3 days, so it must be resumable. Reading is cheap: 50 items per unit, and `videos.list` hydrates 50 IDs per unit. | derived |
| OAuth | Desktop clients use the loopback IP `http://127.0.0.1:<random port>` (recommended). Custom schemes and copy/paste OOB are **no longer supported**. **PKCE required** (S256). Desktop apps are public clients, so the secret is not confidential. | [OAuth for native apps](https://developers.google.com/identity/protocols/oauth2/native-app) |
| Refresh tokens | A project in **"Testing"** status gets refresh tokens that **expire after 7 days**, so you would re-auth weekly unless the app is published (which likely means Google verification for the `youtube` scope; not checked). Limit: 100 refresh tokens per account per client. | [OAuth 2.0 overview](https://developers.google.com/identity/protocols/oauth2) |
| Policy | No downloading, caching or offline playback of audiovisual content (III.E.1), which matches your "never download" rule. Stored API data must be **refreshed or deleted after 30 days** (III.E.4). Data must be deleted **within 7 days** of the user revoking consent (III.D.2.3). A quota increase requires an API Compliance Audit. | [Developer Policies](https://developers.google.com/youtube/terms/developer-policies) (updated 2026-10-15) |

**What it means:** persist only video IDs and our own match decisions. Re-fetch or drop cached YouTube titles after 30 days, and purge everything on disconnect. Data from **Takeout is your own export, not API data**, so these storage rules don't apply to it. YouTube Music has no separate API; YTM playlists are expected to be ordinary YouTube playlists of video IDs. That's unverified, and the online round-trip test in Phase 3 will confirm it.

### Apple Music API

- Every request needs a developer token: a JWT signed with a **MusicKit private key**. Creating that key requires the "Account Holder or Admin" role ([Apple help](https://developer.apple.com/help/account/capabilities/create-a-media-identifier-and-private-key)). Those roles exist only in **paid Apple Developer Program** teams; free personal teams can't create keys. Apple's doc pages render client-side and I couldn't load their bodies, so treat the fee ($99/yr) and the exact eligibility as stated-by-Apple-elsewhere, not re-verified here.
- Library access also needs a **Music User Token**, available only from MusicKit on Apple platforms or MusicKit JS. There is no Windows SDK, so we'd have to host MusicKit JS in a WebView. The user also needs an active **Apple Music subscription**.
- **Conclusion:** not practical without a paid account. Skip the connector and rely on `Library.xml`. Note that the **Apple Music app for Windows appears to have no XML export** ([forum thread, Sep 2025, unanswered](https://developer.apple.com/forums/thread/802317)). An XML export needs iTunes for Windows or a Mac.

## 2. File formats: what to expect, and how to export each sample

Nothing here has been checked against your real files yet. Copy every file onto the PC **byte-for-byte** (don't open and re-save it in Excel or Notepad), put it in `fixtures/<source>/`, and keep `fixtures/` out of any public repo, since these files contain personal data.

| Format | How to export | Expected fields | Known quirks to check in your sample |
|---|---|---|---|
| **Spotify account data** (`Playlist1.json`, `YourLibrary.json`) | spotify.com → Account → Privacy settings → *Download your data* → tick **Account data** → confirm the email → download the zip when it arrives (it takes days). | Per playlist: name, last-modified date, description, follower count, items: track name, artist name, album name, track URI, local-track name, added date ([Spotify](https://support.spotify.com/us/article/understanding-my-data/); JSON key names unconfirmed). | **No duration and no ISRC**, so fuzzy matching only, with no ±3 s check. Only one artist per item. Local tracks and episodes are mixed in. Large libraries split into `Playlist1.json`, `Playlist2.json`, and so on. |
| **Exportify CSV** | exportify.app → log in with Spotify → *Export* (or *Export All* for a zip). | Track URI, Track Name, Artist URI(s), Artist Name(s), Album…, Disc/Track Number, **Track Duration (ms)**, Explicit, Popularity, **ISRC**, Added By, Added At ([README](https://github.com/watsonbox/exportify)) | The best offline Spotify source. UTF-8. The multi-artist separator isn't documented (expect a comma inside a quoted field). Exportify uses the Web API, so after the 2026 changes it may fail or drop fields (Popularity was removed for dev-mode apps). Check this first. |
| **Google Takeout (YouTube / YT Music)** | takeout.google.com → *Deselect all* → tick **YouTube and YouTube Music** → *All YouTube data included* → keep **playlists** and **music-library-songs** (and *music-uploads* if you uploaded) → format CSV → export. | `playlists.csv`: Playlist ID, title, description, visibility, timestamps. **Per-playlist CSV: only `Video ID`, `Playlist Video Creation Timestamp`.** `music library songs.csv`: Video ID, Song Title, Album Title, Artist Name ([schema](https://developers.google.com/data-portability/schema-reference/youtube)) | **Playlists carry no titles.** Offline we can only join them on Video ID against the library-songs CSV. Anything not in your YTM library needs `videos.list` (online, 1 unit per 50 IDs). File and folder names are localised to the account language. |
| **iTunes `Library.xml`** | iTunes for Windows: *Edit → Preferences → Advanced → Share iTunes Library XML with other applications*, or *File → Library → Export Library*. Mac Music app: *File → Library → Export Library*. | Apple plist: `Tracks` dict (Track ID → Name, Artist, Album Artist, Album, Total Time ms, Track/Disc Number, Persistent ID, **Location**), `Playlists` array (Name, Playlist Persistent ID, Parent Persistent ID, Playlist Items → Track ID). | `Location` is a percent-encoded `file://localhost/C:/…` URL, which maps to the path tier. System playlists (Master, Music, Podcasts, Distinguished Kind), smart playlists and folders need filtering. No ISRC. Track IDs change between exports, but Persistent IDs don't, so use the playlist's Persistent ID as `source_id`. |
| **M3U / M3U8** (phone) | Copy the files from the phone over USB. Tell me which app made them and the phone's music root (e.g. `/storage/emulated/0/Music`). | Paths (absolute, relative or `file://`), optional `#EXTINF:<sec>,<Artist - Title>` | Phone paths won't exist on the PC, so they need suffix matching and an EXTINF fallback. Encoding may be UTF-8 without a BOM even as `.m3u`. |
| **PLS** | as above | INI: `[playlist]`, `File<n>`, `Title<n>`, `Length<n>` (seconds, `-1` = unknown), `NumberOfEntries`, `Version=2` | Entries can be numbered out of order or have gaps. There's no encoding declaration. |
| **XSPF** | as above (VLC: *Media → Save Playlist to File*) | `<trackList><track>` with `location` (URI), `title`, `creator`, `album`, `duration` (**ms**), `trackNum`, `identifier` (URI), `meta`, `extension` ([spec](https://www.xspf.org/spec)) | VLC adds `<extension application="http://www.videolan.org/vlc/playlist/0">`. XSPF has **no standard ISRC field**: for our exporter I propose `<identifier>https://musicbrainz.org/recording/<mbid></identifier>`, with ISRC in `<meta rel="…/isrc">`. |
| **WPL** | Which phone app produces WPL? It's Windows Media Player's format, so please tell me the source. | SMIL XML: `<smil><head><title>`, `<body><seq><media src="…" tid="…" cid="…"/>` | `src` is usually relative with `..\`. XML entity escaping (`&amp;`, `&apos;`). |

### Gaps in the current `m3u.rs` (for Phase 1, not fixed yet)

- `#EXTINF` duration and title are thrown away. They're the only fallback when a phone path doesn't resolve.
- `percent_decode` also runs on plain paths, so a real file named `100%AB.mp3` would be mangled. It should only decode `file://` URIs.
- `file://localhost/…`, `file://server/share/…` (UNC) and UTF-16 files aren't handled. The non-UTF-8 fallback is Latin-1, not Windows-1252, so `€`, curly quotes and dashes come out wrong.
- Skipped URLs aren't counted, so the import report can't mention them.

## 3. What identifiers the local files carry

`cargo run --example id_audit -- "C:\Users\Giorgi\Desktop\MusicG14"`. It reads tags with lofty 0.25.4, the same reader the scanner uses, then cross-checks with a raw byte scan of each file's first 2 MiB.

| ext | files | tags unreadable | ISRC (valid) | MBID recording (valid) | duration > 1 s | lofty vs decoder Δ > 1 s | title | artist | multi-artist string |
|---|---|---|---|---|---|---|---|---|---|
| flac | 29 | 0 | 16 (15) | 8 (8) | 29 | 0 | 29 | 29 | 7 |
| m4a | 1 | **1** | 0 | 0 | 0* | 0 | 0 | 0 | 0 |
| ogg | 1 | 0 | 0 | 0 | 1 | 0 | 1 | **0** | 0 |
| opus | 7 | 0 | 0 | 0 | 7 | 0 | 7 | 7 | 0 |
| **total** | **38** | 1 | **15 valid (39%)** | **8 (21%)** | 37 (38 via decoder) | 0 | 37 | 36 | 7 |

\* The m4a is really a `3gp4` container (likely a phone recording). lofty can't read it, but the scanner's decoder fallback still gets its duration.

Quirks found in the actual files:
- `ISRC=50999 9 09 790 2 6`: a **barcode stored in the ISRC field**. ISRCs must be validated (`CC-XXX-YY-NNNNN`, 12 characters), or a bad tag will produce a confident wrong match.
- One empty `ISRC=` field, and one lowercase `isrc=` key (lofty handles both correctly).
- Every MBID is the recording ID (`MUSICBRAINZ_TRACKID` in Picard's naming), and each of those 8 files also has a release-track ID.
- 7 artist fields hold several artists in one string (`FloyyMenor & Cris MJ`, `Usher feat. Pitbull` in a filename), so primary-artist splitting matters.
- One filename is NFD-decomposed (`pasión` with a combining accent), and 4 are non-ASCII (e.g. Cyrillic `Мало тебя`). This confirms the need for NFKC on both the matching and the path tiers.
- **The DB doesn't store ISRC or MBID today** (`tracks` has neither column, and `tags.rs` doesn't read them). Phase 1 needs a schema migration plus a forced re-read, because the incremental scan skips unchanged files.
- SQLite FTS5 is available: the bundled `libsqlite3-sys 0.38.2` builds with `-DSQLITE_ENABLE_FTS5`.

## 4. What I need from you before Phase 1

1. The real fixture files in `fixtures/`, plus a list of the sources you actually use (§5.2 builds the golden set from those).
2. ~~Fingerprinting and the path tier~~ Decided: fingerprinting is dropped, and a path tier is added ahead of ISRC (§0.3, §0.4).
3. ~~Spotify JSON auto-accept~~ Decided 2026-10-18: matches from Spotify account-data JSON **always go to review and never auto-accept**, because there's no duration to confirm the version. The matcher still scores them and ranks candidates, with no exceptions. In the golden-set report, Spotify JSON refs count toward review quality (top-1 / top-3) and recall, and never toward auto-accept precision.
4. ~~The definition of a correct match and the cost weight~~ Decided: same recording; DRIFT goes to review only; W = 10 (§5.3, §5.5).

## 5. Golden-set evaluation, re-planned for a 38-track library

### 5.1 What changes

With 38 local files, the external playlists will be mostly tracks you **don't** have. That shifts the question. It's no longer mainly "does the matcher find the file?", because at most 36 songs can be found. The main question becomes "**does it ever attach an absent track to the wrong file?**" Your rule that wrong matches count worse than misses makes the negatives the most important part of the set.

So the set is built on purpose around your 36 real songs, instead of 500 random playlist entries. `Cha-Ching` (1.5 s) and `Dodoxvrinva` (20.8 s, a 3gp voice-style file) are not songs. Files under 30 s are excluded from auto-accept entirely.

The library itself shows where matching will fail, and that decides what goes into the set:

| Seen in your library | Risk | Consequence for the matcher |
|---|---|---|
| `sins (let me in)` and `sins (let me in) [DarillisBeats Edit]`, **both 128.3 s** | Duration can't tell them apart | "Edit", "Remix", "Slowed", "Sped Up", "Reverb", "Instrumental", "Extended", "Hardstyle", "TikTok version", "Best Part", "V3" are **version qualifiers, not noise**. They must be extracted and must match exactly before an auto-accept. Only packaging noise is stripped (Official Video/Audio, Lyrics, Visualizer, HD/4K, `- Topic`, VEVO). |
| `Swimming Pools (Drank) (instrumental)` and `[Extended]`, but **not the original** | The original is the most likely external ref, and stripping qualifiers would match it to one of these | This is the canonical hard negative: it must end up as MISSING. |
| Artist = uploader: `RiFix` / `Ke$ha - TiK ToK [Hardstyle]`, `X_.M150` / `Lady Gaga - Paparazzi (…) {Sped Up}`, `𝘼𝙨𝙝𝙬𝙞𝙣𝙜⠀𖣂` | "Primary artist" is wrong for about a fifth of the library | Parse `Artist - Title` out of the title as an alternative reading. NFKC turns `𝘼𝙨𝙝𝙬𝙞𝙣𝙜` into `Ashwing` but leaves `⠀` (U+2800) and `𖣂`, so invisible and symbol characters must also be stripped. |
| `feat.` in the artist field (`Katy Perry feat. Kanye West`), `(Feat. Marc Anthony)` in the title, `vivi baby; ovg!` vs `vivi baby & ovg!` | Artist sets differ by source | Split on `feat.` / `ft.` / `&` / `;` / `,` / ` x `, and compare artist sets. |
| Generic titles: `Falling`, `Heartbeat`, `Overdrive`, `Dance...`, `Keep You`, `E.T.`, `Point of No Return` | Same title, different artist | A title-only match never auto-accepts. |
| Compilations (`Now That's What I Call Music! 94`, `Top 100 Van 2011`) | The album differs from the service's album | Album gets little weight, and the ISRC still matches (`Into You` carries the original `USUM71601827`). |
| `Мало тебя` (Cyrillic) | A service may list it transliterated (`Malo tebya`) | Transliterated matches go to review and are never auto-accepted. |
| 15 distinct valid ISRCs, no duplicates; 1 barcode in the ISRC field | — | ISRC auto-accepts only when it's valid and maps to exactly one local file. |

### 5.2 Composition (~450–500 refs)

| Bucket | Target | How to build it |
|---|---|---|
| **A. Positives** | ≥120 refs | Each of the 36 songs as it appears in **every source you use**: Exportify, Spotify JSON, Takeout plus library-songs CSV, phone M3U, and so on. Duplicates inside a playlist count. The positive count grows with the number of sources, not with library size. |
| **B. Hard negatives** | ≥120 refs | For each song, the neighbouring versions that exist on the services: the original vs the remix, slowed or sped up, instrumental vs vocal, extended vs radio edit, clean vs explicit, the same title by another artist. You make one "confusers" playlist per service and export it the same way as the others. |
| **C. Background negatives** | ~200 refs | Your real playlists as they are. They measure the false-accept rate under normal conditions. |

The tier coverage this set can actually measure, stated upfront:
- **Path:** phone M3U/PLS/WPL, Library.xml.
- **ISRC:** only Exportify (and the Spotify API later) carries ISRCs, and only 15/36 local songs have one, so this tier has small numbers.
- **MBID:** no in-scope external source carries MBIDs. The tier is reported as **n/a** in the golden set and tested only through our own XSPF round-trip.
- **Fuzzy:** most of the set, especially YouTube.

### 5.3 Labelling protocol

1. After the Phase 1 parsers exist, a command writes `fixtures/golden/sheet.csv` with one row per ref: source, file, playlist, position, title, artists, album, duration, IDs, and an empty `label`. It is generated **blind**: no matcher suggestions, so the labels can't inherit the matcher's mistakes.
2. A label is one of: the local file's path relative to `MusicG14`; `MISSING`; or `OTHER_VERSION:<path>`, which scores as MISSING but tags the row as a hard negative so errors can be grouped by type.
3. **Definition of correct (decided 2026-10-18):** *same recording at the same speed and edit*. A remaster or a clean/explicit pair of the same recording is labelled `DRIFT:<path>`. Drift is never auto-accepted (an auto-accepted DRIFT counts as a wrong match), but choosing it in review isn't an error. A different uploader's sped-up version of the same remix counts as a different recording.
4. A sanity pass, not done by the matcher, flags any label whose file duration differs from the ref by more than 10 s, so you can re-check it. Then the sheet is **frozen**, with a hash recorded in the run report.
5. **Split by song, not by ref:** 18 songs (with all their refs) go to `dev` for tuning, and 18 go to `test`. Background playlists are split by playlist. Thresholds are tuned only on `dev`, and `test` is scored once per matcher version, so leaked refs can't inflate the numbers.

### 5.4 Metrics table (the format every run reports)

Per tier (path / ISRC / MBID / fuzzy): refs decided at that tier, auto-accepted, correct, **wrong**, precision with a one-sided 95% Clopper–Pearson lower bound, sent to review, review cases with the right candidate ranked first or in the top 3, and recall.

Overall: positives, negatives (hard / background), auto TP / FP / FN, review queue size (the human cost of safety), **false-accept rate over all refs with its 95% upper bound**, recall, and weighted cost. Breakdowns are by source format and by hard-negative type (remix, slowed, sped up, instrumental, extended, same title / other artist, drift).

### 5.5 Pass gates and what the numbers can prove

- **Gate 1: zero wrong auto-accepts on `test`.** Any FP fails the run. An FP on a hard negative is a design bug, not tuning noise.
- **Gate 2, honestly bounded:** with ~120–150 positives, "0 wrong" gives a precision lower bound of **97.1–98.0%** (95%, one-sided). Proving ≥99% would need ≥299 auto-accepts with zero errors, which a 36-song library can't supply even across several sources. What it *can* prove strongly is the thing that matters here: **0 false accepts across ~500 refs bounds the false-accept rate at ≤0.6%**. The report states both, so neither figure is overstated.
- **Gate 3, review quality:** for positives that land in review, the right file is the top candidate in ≥90% of cases. Recall gets no hard gate; it's reported so you can see the cost of the safety margin.
- **Weighted cost:** `cost = W × FP + FN`, with **W = 10** (decided 2026-10-18). It's used to compare matcher versions, never to excuse an FP.
- **Reasons:** every unmatched ref has a reason. On MISSING refs, a hard negative must show "ambiguous" or "version mismatch" rather than "no candidate"; this is spot-checked.

### 5.6 The 50k-track requirement

Your real library can't exercise it, so it's split in two:
- **Speed:** a generated 50k-track library (as for the README's 20k benchmark), with timings for FTS5 candidate narrowing and full matching.
- **Discrimination at scale:** a stress variant of the test DB that adds every bucket-B confuser as a metadata-only phantom track next to your 38 real files. Positives must still pick the real file, not its phantom sibling. This is the closest offline stand-in for a library full of near-duplicates.

## 6. Phase 1 status (2026-10-18, partial)

**Done:**
- Canonical model: `src-tauri/src/library/interchange.rs` (`Playlist`, `PlaylistItem`, `TrackRef`). Order and duplicates are kept. `TrackRef` also has a `location` field, which the path tier needs.
- Parsers: M3U/M3U8 (extended in `m3u.rs`, not duplicated), PLS, XSPF and WPL. Exporters: M3U8 and XSPF.
- Rule: parsers don't guess. A display string like "Artist - Title" stays in `title`, and an artist tag like `vivi baby; ovg!` stays one string. Splitting is the matcher's job.
- Fixed the `m3u.rs` gaps from §2: `#EXTINF` metadata is kept; `%XX` is no longer decoded in plain paths that exist on disk; `file://localhost`, UNC paths and UTF-16 files are handled; legacy files decode as Windows-1252; streams are counted (`skipped`).
- Idempotent import: `playlists.source` + `source_id` with a unique index. A file is keyed by its absolute path, case-insensitive; an XSPF `<identifier>` takes precedence. Re-importing replaces the items and keeps the name, description and photo. Backups carry the key.
- Tests: proptest `parse(write(x)) == project(x)` for all four formats, where `project` drops what a format can't carry. The round-trip was confirmed to catch a planted escaping bug. `examples/playlist_export_check` exports the real library to M3U8, M3U and XSPF and reads each back: 38/38 entries, same order, all files exist.

- **CSV export with ISRC** (`csv.rs`). Columns: Position, Title, Artist, Album, Duration (ms), ISRC, MusicBrainz Recording ID, Location, Added At (RFC 3339 UTC). RFC 4180 quoting, UTF-8 with a BOM for Excel. Cells starting with `= + - @` get a leading `'` (formula-injection guard), and the reader reverses this exactly. Our own CSVs can be imported again. Exportify CSVs are recognised and refused until a real sample exists.
- **Identifiers in the library:** `tracks.isrc` and `tracks.mbid`, read from tags. Only valid ISRCs and UUID-shaped MBIDs are kept. The v5 migration invalidates old rows' `mtime` once, so the next startup scan re-reads their tags; artwork is reused. On the real library: **15 ISRCs and 8 MBIDs**, matching the Phase 0 audit, with the barcode in the Usher track's ISRC field rejected. XSPF and CSV exports carry them, and `playlist_export_check` confirms 15/8 survive a round trip in both.

**Not done yet:**
- Spotify JSON, Exportify, Takeout and Library.xml importers (waiting for the real files in `fixtures/`).
- `real_fixture_files_parse` runs, but checks **0 files** until `fixtures/` exists.
- ~~VLC and foobar2000 are not installed~~ Checked 2026-10-18 with VLC 3.0.24 and foobar2000 2.26.1 (installed via winget), using the 38-track export from `playlist_export_check`:

  | Export | VLC 3.0.24 | foobar2000 2.26.1 |
  |---|---|---|
  | `.m3u8` | 38/38 listed; 38/38 files opened and played, in playlist order; no errors | 38/38 entries, same files, same order |
  | `.m3u` (UTF-8 BOM) | 38/38 listed; 38/38 opened, in order; no errors | 38/38 entries, same files, same order |
  | `.xspf` | 38/38 listed; 38/38 opened, in order; no errors (`&apos;` in a location decoded correctly) | **N/A (decided 2026-10-18).** foobar2000 has no built-in XSPF support: it adds the `.xspf` file itself as one entry, and no foobar2000 binary mentions XSPF. **M3U8 is the export format for foobar2000**, so the XSPF-in-foobar2000 criterion doesn't apply. |

  How: VLC was driven through its localhost remote-control interface, and every opened file was confirmed against VLC's debug log. foobar2000 was loaded with `/add`, then its saved playlist (`playlists-v2.0\*.fplite`) was compared line by line. The first foobar2000 run opened and played the M3U8 and saved file info for all 38 tracks. All four non-ASCII filenames opened in both players.
- Unresolved references (phone paths, later Spotify and YouTube tracks) are stored only as paths for now. Storing the full `TrackRef` for each item comes with the Phase 2 matcher.
- WPL `duration` is assumed to be in milliseconds, which hasn't been checked against a real WMP file.

## 7. Phase 2 status: matcher (2026-10-18)

**Built** (`src-tauri/src/library/matcher/`):
- **Tiers, in order:** earlier decision → exact path → folder + file name (suffix) → ISRC → MusicBrainz ID → fuzzy. Fingerprinting is dropped, as decided.
- **Fuzzy candidates:** SQLite FTS5 (`track_match_fts`) narrows the library. It first asks for tracks containing every title word plus an artist word, and falls back to any shared word, ranked by bm25, when that finds fewer than 8. The index syncs incrementally through `track_match_state` (track mtime + rules version) and caches each track's parsed readings.
- **Normalisation** (`normalize.rs`): NFKC/NFKD, accents removed, case folded, `$`→`s`, invisible characters and symbols removed. It separates four kinds of brackets and suffixes:
  - **Packaging noise**, removed: Official Video, Lyrics, Audio, HD, prod. credits, YouTube IDs, `- Topic`, VEVO.
  - **Featured artists**, moved into the artist set: feat., ft., with.
  - **Version qualifiers**, compared exactly: remix, edit, mix, slowed, sped up, reverb, instrumental, extended, live, acoustic, hardstyle, best part, Vn, …
  - **Drift**: remaster, explicit/clean, album/single version, original mix.
- **Two readings per title:** as tagged, and "Artist - Title" taken from the title itself. This covers uploader-tagged files and M3U display strings.
- **Auto-accept rule (fuzzy):**
  - title identical after normalisation;
  - artists ≥ 0.9;
  - identical version and drift sets;
  - both durations known and within ±3 s;
  - local file ≥ 30 s;
  - the source allows it (Spotify account data never does);
  - **exactly one** candidate passes all of the above.

  Two passing candidates come out "ambiguous". Both sides naming artists that don't overlap means "no candidate", which keeps same-title songs by other artists out of review. Different ISRCs or MBIDs on the two sides block an auto-accept.
- **Every result** carries a score, a one-line explanation and up to 3 ranked candidates. Reasons: no candidate / ambiguous / duration mismatch / version mismatch / remaster-explicit difference / low confidence / marked missing / skipped.
- **Decisions** (`match_decisions`: accept a file, missing, skip) are applied before any tier, so nothing is asked twice. If an accepted file disappears, the reference is asked about again, with a note.
- **Imports run through the matcher.** Each item stores its reference (`match_refs` + `playlist_items.ref_key`), which survives reordering in the app. **Export unmatched report…** on an imported playlist re-matches against the current library and writes a CSV with the reasons.

**Tests:** 76 Rust tests in total. The 27 matcher tests use a library that mirrors MusicG14's metadata and cover every hard case from §5.1: original vs instrumental vs extended *Swimming Pools*, the two 128.3 s *sins*, `RiFix`/`Ke$ha`, `X_.M150`/Paparazzi, `vivi baby; ovg!`, generic titles by other artists, the 1.5 s file, ISRC duration conflicts, duplicates, phone paths, NFD paths, and decisions.

**50k synthetic benchmark** (`cargo run --release --example match_bench`): generated library with ~8% sibling versions, ~2% duplicate copies, multi-artist credits and a 120-word vocabulary.

| | Result |
|---|---|
| Open matcher, first time (FTS5 build for 50,000 tracks + load) | 0.70 s |
| Open matcher afterwards (nothing to re-index) | 0.25 s (FTS sync check 12 ms, load tracks 52 ms) |
| Per reference | p50 2.5 ms · p95 5.9 ms · p99 7.5 ms · max 11 ms (373 refs/s) |

| Synthetic refs (**not the golden set**) | n | auto, correct | **auto, wrong** | review | unmatched |
|---|---|---|---|---|---|
| positives (with case, noise, Topic, "Artist - Title", ISRC variations) | 1,930 | 1,930 | **0** | 0 | 0 |
| song stored twice (either copy is right) | 70 | 1 (via ISRC) | **0** | 69 (ambiguous) | 0 |
| version not in the library | 1,500 | 0 | **0** | 1,500 (version mismatch) | 0 |
| same title, other artist | 500 | 0 | **0** | 294 | 206 |
| absent | 1,000 | 0 | **0** | 321 | 679 |

The synthetic numbers check that the rules behave as designed. They say nothing about real-world precision, which needs the golden set from real playlists (§5).

**Not done yet:**
- **Review UI** (pick a candidate / skip / mark missing). The decision storage and the matcher side are ready, but there are no screens yet.
- Golden-set evaluation, which waits for `fixtures/`.
- Unresolved references with no location (future Spotify/Takeout imports) are stored as `unmatched:<ref key>` paths, which show as missing until the review UI exists.

## 8. Paste-a-link import + Soulseek for missing songs (2026-10-20, branch `feat/link-import`)

Modelled on Riff (github.com/rootscripts/riff, MIT): **paste a public link, no sign-in**. This replaces the Phase 3 OAuth plan for reading playlists. Only track metadata is read. Riff streams each song from YouTube or SoundCloud; Undertone instead matches against local files and gets missing songs from Soulseek.

| Service | How it's read | Live check (2026-10-20) |
|---|---|---|
| Spotify playlist | Embed page (`__NEXT_DATA__`: name, first tracks, anonymous token), then the web player's `pathfinder` `fetchPlaylist` persisted query, 100 per page. Uses the track's artists, not the album's (Riff uses the album's). | "All Out 2010s": 150/150 tracks with artists, duration and ID |
| Spotify album | Embed page | "DAMN.": 14/14 |
| YouTube / YouTube Music playlist | `youtubei/v1/browse` (`VL<id>`, then continuation tokens). Reads both `lockupViewModel` and `playlistVideoRenderer` entries. Riff uses yt-dlp instead. Mixes (`RD…`) are refused. | YouTube Charts "Top 100 Songs Global": 99 of 100 |
| Apple Music playlist | Page `serialized-server-data` JSON (JSON-LD as fallback). Apple's page only carries the first 100 songs. | "Today's Hits": 50/50 |
| Apple Music album | `itunes.apple.com/lookup?id=…&entity=song` | "good kid, m.A.A.d city": 12/12 |

**Live matching against the real 38-track library** (a copy of the database), using `examples/link_import_check.rs`:
- **Auto-matched, all correct:** Apple's "Swimming Pools (Drank) [Extended Version]" → the local `[Extended]` file; Spotify's "DJ Got Us Fallin' In Love (feat. Pitbull)" and "Cool for the Summer".
- **Not matched, also correct:** Spotify's plain "TiK ToK" went to review as a version mismatch against the local Hardstyle edit.
- **Wrong auto-accepts:** 0.

That run exposed review clutter: songs sharing only an artist (for example "7 rings" vs "Into You") were offered as candidates. Fix: a candidate now needs ≥ 50% of its title words in common, with a test added. After the fix, the review queue held only the TiK ToK decision.

**Soulseek for missing songs** (`sources/missing.rs`):
- **Search:** "artist + title + version words", then "title + version words".
- **Acceptable file:** same title and version as the reference (file name read with and without a track number, alone or under an "Artist - Album" folder), main artist in the path, length within 3 s when both sides know it, FLAC or MP3 ≥ 320 kbps.
- **Ranking:** FLAC first, then a confirmed length, a free slot, and upload speed.
- **Downloads:** never automatic. The user ticks files; a finished download is recorded as the user's choice for that reference, which replaces the missing entry in every playlist.
- **Testing:** unit-tested on synthetic answers (including extended, instrumental, 128 kbps and wrong-artist decoys). **Not yet run against the live Soulseek network.**
