# Soulseek downloads: design note

Status: **proposal, waiting for the owner's go-ahead** (2026-10-19). Nothing below is built yet.

Soulseek is the one download source Resonance allows (owner's decision, recorded in `AGENTS.md`).
This note covers what we build on, what the network needs, and where the password lives.

## Scope of the first version

- Sign in with a Soulseek account; sign out.
- Search. Results grouped by user, then by folder, showing format, bitrate (or sample rate and
  bit depth for lossless), length, size, and whether the user has a free upload slot.
- Download single files or a whole folder. Queue with progress, speed, cancel and retry.
- A download folder setting. Default: `<first library folder>\Soulseek`, so finished files are
  picked up by the folder watcher with no extra step.
- Finished files are marked read-only, like the rest of the library (the app never modifies music).

Not in v1: sharing/uploading, chat, rooms, browsing a user's whole share, wishlists, UPnP.

## Protocol implementation

Soulseek is a closed protocol. The working reference is the reverse-engineered
[Nicotine+ protocol doc](https://nicotine-plus.org/doc/SLSKPROTOCOL.html).

| Option | For | Against |
|---|---|---|
| **`soulseek-rs-lib`** ([crates.io](https://crates.io/crates/soulseek-rs-lib), [repo](https://github.com/michel/soulseek-rs)) | MIT. Zero dependencies. ~23k lines covering login, search, peer connections (direct and server-brokered "pierce firewall"), transfers with resume, cancel. Unit tests plus end-to-end tests against a local server (soulfind); CI runs on Windows. Blocking API with threads and `mpsc` channels, which fits our backend (no tokio in our code). | One maintainer, ~2k downloads. Very fast-moving: 29 releases, now at 19.1.0, so the API breaks often. Needs Rust 1.91 (we have 1.96). |
| `soulseek` 0.1.0 | MIT | Single release in 2023; a stub. |
| Write our own subset | No third-party protocol code; only what we need. | Login (MD5), server keep-alive, search with zlib-compressed peer replies, peer connections both ways (direct + indirect via server), the upload-initiated transfer flow, queue positions, resume. Realistically 2,500+ lines plus tests, and we'd re-discover the same edge cases. |

**Recommendation: use `soulseek-rs-lib`, pinned to an exact version (`=19.1.0`), behind our own
thin module** (`src-tauri/src/sources/soulseek/`). The app only talks to our wrapper (sign in,
search, download, cancel, retry, status events), so a breaking upstream release, or replacing the
crate with our own code later, touches one module. Upgrades are deliberate, never via `cargo update`.

The crate identifies itself to the server as major version "soulseek-rs", minor 1 ("unidentified
client"). That's fine for personal use; we can ask upstream for our own minor number later.

### Licence

`soulseek-rs-lib` is MIT, same family as the rest of our tree (MIT / Apache-2.0). No copyleft.
Nicotine+ is GPL-3.0, so we **read its protocol doc but copy none of its code**.

### What we test ourselves

The crate does the wire parsing and has its own tests. We add tests for the parsing we own:

- file attributes → format / bitrate / sample rate / bit depth / length (attribute codes 0–5),
- grouping results by user and folder (Soulseek paths use `\`, and the folder is everything
  before the last separator),
- turning a remote path into a safe local file name (no `..`, no reserved Windows names, no
  characters Windows rejects),
- download queue state transitions (queued → running → done / failed / cancelled → retry).

## Firewalls and NAT

How Soulseek connects people:

1. We keep one TCP connection to the server (`server.slsknet.org:2242`). Login and searches go
   over it. This is outgoing and works anywhere.
2. Search results and files come **from other users' computers**. A peer with results tries to
   connect *to us* on our listening port (default 2234).
3. If that fails (we're behind a router or firewall), the peer asks the server to tell *us* to
   connect *to them* instead ("pierce firewall"). That works if they are reachable.
4. Files are pushed by the uploader when it has a free slot, using the same direct-then-brokered
   dance. The download can sit in their queue for a while; we show "Queued".

What that means for the user:

- **Behind a normal home router, with no port forwarded:** most searches and downloads still work,
  because most sharers are reachable. Results and files from users who are *also* unreachable
  never arrive. Searches look a bit thinner; some downloads stay queued and then time out.
- **Port forwarded (TCP 2234 → this PC) or UPnP:** everything works. v1 shows the listening port
  in settings and a short help line; it does not do UPnP automatically (that would be another
  network action on the user's router, worth its own decision later).
- **Windows Defender Firewall** asks once, the first time the app listens on the port. Allowing it
  on private networks is enough at home. If refused, we behave like the "no port forwarded" case.
- Listening is on by default; a setting turns it off.

## Credentials

- **Password:** Windows Credential Manager, as a generic credential named
  `Resonance/Soulseek`, via `CredWriteW` / `CredReadW` / `CredDeleteW`. We already depend on the
  `windows` crate; this only adds the `Win32_Security_Credentials` feature, no new crate. The
  credential is per Windows user and encrypted with their login (DPAPI). It is never written to
  `settings.json`, logs or the frontend after sign-in.
- **Username, download folder, listen port, "sign in on start":** `settings.json`, as additive
  serde-default fields like the rest.
- Sign out deletes the stored credential.
- Under the test identifier (`app.resonance.player.visualtest`) the credential target gets the
  identifier as a suffix, so test runs never touch the real account entry.

Note: Soulseek's login itself sends the password in plain text over an unencrypted TCP
connection (the protocol has no TLS). That's a property of the network, not something we can fix;
the UI says so on the sign-in form: "Use a password you don't use anywhere else."

## Other decisions for the owner

- **No sharing in v1.** The app never serves your files to others. Some Soulseek users refuse to
  upload to people who share nothing, so some downloads will be denied ("File not shared" /
  "Banned"). Sharing could be added later as an opt-in folder list.
- **Network use:** `README.md` currently says the update check is the only network request. That
  line becomes: the update check, plus Soulseek when you've signed in.
- **Downloads are files from strangers.** We only save audio file types we can play
  (flac, mp3, m4a/aac, ogg/opus, wav, aiff, wv, ape, alac); other files in a folder download are
  skipped. Files are never opened until the library scanner reads their tags, the same as any
  file dropped into a library folder.

## Layout

- `src-tauri/src/sources/soulseek/`: wrapper (`client.rs`), credentials (`credentials.rs`),
  pure helpers with tests (`model.rs`: attributes, grouping, safe names, queue state).
- `commands.rs`: `slsk_status`, `slsk_sign_in`, `slsk_sign_out`, `slsk_search`, `slsk_download`,
  `slsk_cancel`, `slsk_retry`, `slsk_clear_finished`. Progress goes to the frontend as a
  `slsk://downloads` event.
- `src/views/SourcesView.tsx`: the Soulseek card becomes a sign-in form, then search + results +
  queue. The playlist-import cards stay "Coming soon".
