# File sync between the PC app and the phone app

Status: the PC side is built. The phone side isn't started; it goes on top of the Android branches
(`feat/android-phone-ui`).

## What it does

Songs move both ways over the home network, through the Phone sync server
(`src-tauri/src/phone.rs`). Every request needs the key, songs are reached by playlist and position,
and no path leaves the PC.

- **PC to phone:** the phone keeps a copy of the playlists the user picks.
- **Phone to PC:** songs that are only on the phone, such as its Soulseek downloads, are copied to
  the PC. This is the server's only write, and it's off until the user turns on **Receive songs
  from my phone**.

Files are what sync both ways. Three things deliberately don't:

- **Playlists go PC to phone only.** A playlist made on the phone stays on the phone, because two
  sides editing one list needs merging. Its songs still reach the PC.
- **Deletions never cross to the PC.** Nothing on the phone can remove or change a PC file. Removing
  a PC song stops it being synced, and the phone's synced copy goes in the clean-up step.
- **A sent song stays on the phone,** until the user removes it there.

## What the PC offers

Every request carries the key as `?k=`.

`GET /api/sync` lists what's on the PC:

```json
{
  "version": 1,
  "receive": false,
  "files": [
    {
      "id": "32 hex characters",
      "list": 0, "pos": 12,
      "name": "Song.flac", "size": 31234567, "ext": "flac",
      "title": "Song", "artist": "Artist", "album": "Album", "duration": 214.5, "cover": null
    }
  ],
  "lists": [
    { "id": 0, "name": "All songs", "songs": ["id", "..."] },
    { "id": 7, "name": "Gym", "songs": ["id", "..."] }
  ]
}
```

- `id` is a hash of the song's path, size and modified time. The same file keeps its id, and an
  edited or moved file gets a new one. It says nothing about where the file is.
- `files` has each song once, however many lists it's in. `lists` keeps order and repeats.
- `version` goes up only for changes an older phone app can't read. The phone refuses a newer one
  and says to update.

`GET /song/<list>/<pos>?k=…&dl=1&v=<id>` fetches a song, from `list` and `pos` in its `files` row.
HTTP ranges work, so a stopped download can carry on.

- `200` or `206`: this is that song.
- `409`: it isn't any more (a playlist was reordered, or the file changed). Get the list again.
- `404`: the file is gone from the PC. Skip it for now. Don't delete the phone's copy because of it.

`v` is what makes position safe to use for syncing. The browser page doesn't send it and behaves as
before.

`GET /api/sync/has?k=...&size=<bytes>&sha256=<hex>` asks whether the PC already has a song with
exactly these bytes, in any folder under any name. It answers `{"id": "..."}`, or `{"id": null}`.
Only songs of the same size get read, so it's quick.

`PUT /upload?k=...&name=<file name>&sha256=<hex>`, with the song as the body and a `Content-Length`,
adds a song to `Phone` inside the first library folder. It answers `{"id": "..."}`, the song's id in
`/api/sync`, also when the PC turns out to have it already. Otherwise:

| Code | Meaning | What the phone does |
| --- | --- | --- |
| `400` | Bad name or checksum, or the song stopped arriving | Try once more, then report it |
| `403` | Receiving is off (`receive` in `/api/sync` says so up front) | Don't send |
| `409` | No library folder on the PC, or this song is already being sent | Report it, or try later |
| `411`, `413` | No size given, or over 2 GiB | Skip the song |
| `415` | Not audio Undertone can read | Skip the song |
| `422` | Checksum doesn't match | Try once more |
| `507` | Under 1 GiB would be left on the drive | Stop sending and say so |

What the PC guarantees:

- Only the last part of `name` is used, it must be an audio file name, and names Windows reserves
  are refused, so a song can't land outside `Phone`.
- A file is never replaced: a name that's taken becomes `Song (2).flac`.
- The song is written to `<sha256>.part`, then checked for size, checksum and readable audio before
  it gets its name. A refused upload leaves nothing.
- Saved songs are read-only, like downloads, and the library rescans.

## What the phone does

1. **Connect.** The user scans the QR code from Settings → Phone sync, or pastes the link. The app
   keeps the address and key. The PC's address can change, so a failed connection asks to scan again.
2. **Pick.** The user chooses playlists from `lists`. "All songs" is one of them.
3. **Compare.** Fetch `/api/sync`, work out which ids the picked lists need, and compare them with
   the ids already on the phone.
4. **Fetch.** Download into app storage as `<id>.part`, resume with a range, and check the size
   against `size` before publishing. A `409` means fetch the list again and carry on from step 3.
5. **Publish.** Move finished files into `Music/Undertone/Synced` through MediaStore (`IS_PENDING`),
   the same way Soulseek downloads are published, under the name from `name`. Run it from a
   user-initiated data transfer job, like Soulseek, so it survives the app closing.
6. **Playlists.** Create or update each picked playlist on the phone with the same name and order,
   after its songs are in place. Mark these playlists as synced, so a change on the phone is
   overwritten rather than merged.
7. **Clean up.** Delete a synced file once no picked list needs it, but only rows in `sync_files`
   marked `fetched`. Songs the user put on the phone, or got from Soulseek, are never touched. Do
   this last, and only after a sync that finished.
8. **Send.** If `receive` is on, and the user turned on **Send songs to my PC** on the phone, go
   through the phone's library, skipping anything in `sync_files`. For each song: work out its
   SHA-256 (keep it by path, size and modified time, so it's only worked out once), ask
   `/api/sync/has`, and upload if the PC doesn't have it. Record the returned id in `sync_files`,
   marked `sent`. That stops the song coming straight back as a download, and because it isn't
   `fetched`, clean-up never deletes it: it's the phone's own copy.

Only Wi-Fi is used in practice, since the PC is on the home network. A sync can stop at any point:
the next one carries on from what's already on the phone. An upload that stops starts that song
again, since `/upload` doesn't resume.

## Still to settle

- **Sync on its own.** The first version syncs when the user presses the button. Starting by itself
  on home Wi-Fi can come later, once the manual path is solid.
