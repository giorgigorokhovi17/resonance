//! The phone app's side of file sync (docs/phone-file-sync.md): brings the playlists the user picked
//! over from the PC, sends back songs only the phone has, and removes copies no picked playlist
//! needs any more. Runs on its own thread and can stop at any point: the next run carries on from
//! what's already here. Saving and deleting shared files are the platform's job (`publish` and
//! `remove` in [`Job`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::future::Future;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{CONTENT_LENGTH, RANGE};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::library::model::Track;
use crate::library::db::TrackArt;
use crate::library::{db, Library, ScanRequest};

/// The newest `/api/sync` this app can read.
const SYNC_VERSION: u32 = 1;
/// Marks the playlists this keeps in step with the PC.
const SOURCE: &str = "pc-sync";
/// The whole library on the PC. It has no playlist here.
const ALL_SONGS: i64 = 0;
/// Times the list is fetched again when the PC changes during a sync.
const ROUNDS: usize = 4;

const STOPPED: &str = "Stopped.";
const UNREACHABLE: &str = "Couldn't reach your PC. Is Undertone open there with Phone sync on, and is this phone on the same Wi-Fi?";
const REFUSED: &str = "Your PC refused this link. Paste a new one from Settings → Phone sync on the PC.";

/// Where to find the PC: the link from Settings → Phone sync.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    base: String,
    key: String,
}

impl Link {
    pub fn parse(text: &str) -> Result<Self, String> {
        const BAD: &str = "That doesn't look like the link from Settings → Phone sync on your PC.";
        let url = Url::parse(text.trim()).map_err(|_| BAD)?;
        let key = url.query_pairs().find(|(k, _)| k == "k").map(|(_, v)| v.to_string()).ok_or(BAD)?;
        let host = url.host_str().ok_or(BAD)?;
        if !matches!(url.scheme(), "http" | "https") || key.len() < 16 || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(BAD.into());
        }
        let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
        Ok(Self { base: format!("{}://{host}{port}", url.scheme()), key })
    }
}

#[derive(Deserialize)]
struct Manifest {
    version: u32,
    receive: bool,
    files: Vec<RemoteFile>,
    lists: Vec<RemoteList>,
}

#[derive(Deserialize)]
struct RemoteFile {
    id: String,
    list: i64,
    pos: usize,
    name: String,
    size: u64,
    #[serde(default)]
    dir: String,
    #[serde(default)]
    cover: Option<String>,
    #[serde(default)]
    art: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    lyrics: bool,
}

#[derive(Deserialize)]
struct RemoteList {
    id: i64,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    cover: Option<String>,
    songs: Vec<String>,
}

/// What the PC offers, for the screen that picks playlists.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub receive: bool,
    pub lists: Vec<OfferedList>,
    pub folders: Vec<OfferedFolder>,
}

#[derive(Serialize, Clone, Debug)]
pub struct OfferedFolder {
    pub path: String,
    /// Songs in it, subfolders included.
    pub count: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct OfferedList {
    pub id: i64,
    pub name: String,
    pub count: usize,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub fetched: usize,
    pub sent: usize,
    pub removed: usize,
    /// Songs that couldn't be moved this time.
    pub skipped: usize,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub running: bool,
    pub step: String,
    pub done: usize,
    pub total: usize,
    pub summary: Option<Summary>,
    pub error: Option<String>,
}

/// One run. `publish` moves a finished file into shared storage under the given name and returns
/// its path; `remove` deletes a shared file and says whether it's gone.
pub struct Job<'a> {
    pub lib: &'a Arc<Library>,
    pub link: Link,
    /// The PC's playlists to keep here.
    pub lists: Vec<i64>,
    /// The PC's music folders to keep here.
    pub folders: Vec<String>,
    /// Send songs only this phone has to the PC.
    pub send: bool,
    /// Private storage for downloads in progress.
    pub staging: PathBuf,
    /// The shared folder finished songs end up in.
    pub root: PathBuf,
    pub publish: &'a (dyn Fn(&Path, &str) -> Result<PathBuf, String> + Sync),
    pub remove: &'a (dyn Fn(&Path) -> bool + Sync),
    pub cancel: &'a AtomicBool,
    pub status: &'a (dyn Fn(&str, usize, usize) + Sync),
}

enum Fetched {
    Done,
    /// It's no longer the song the list named: get the list again.
    Changed,
    /// Gone from the PC. Skip it for now, and keep any copy here.
    Gone,
}

enum Sent {
    Stored(String),
    Skip,
    /// Sending won't work this run. Says why.
    Stop(String),
}

fn block<T>(task: impl Future<Output = T>) -> T {
    crate::rt::handle().block_on(task)
}

struct Client {
    http: reqwest::Client,
    link: Link,
}

impl Client {
    fn new(link: Link) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .read_timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { http, link })
    }

    fn url(&self, path: &str, extra: &[(&str, &str)]) -> Result<Url, String> {
        let params = std::iter::once(("k", self.link.key.as_str())).chain(extra.iter().copied());
        Url::parse_with_params(&format!("{}{path}", self.link.base), params).map_err(|e| e.to_string())
    }

    async fn manifest(&self) -> Result<Manifest, String> {
        let reply = self.http.get(self.url("/api/sync", &[])?).send().await.map_err(|_| UNREACHABLE)?;
        match reply.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(REFUSED.into()),
            // A PC too old to have this list answers 404.
            StatusCode::NOT_FOUND => return Err("Update Undertone on your PC to sync with this phone.".into()),
            code => return Err(format!("Your PC couldn't list its songs ({code}).")),
        }
        let m: Manifest = reply.json().await.map_err(|_| UNREACHABLE)?;
        if m.version > SYNC_VERSION {
            return Err("Update Undertone on this phone to sync with your PC.".into());
        }
        Ok(m)
    }

    /// Text or an image the sync can do without.
    async fn extra(&self, url: Url) -> Option<Vec<u8>> {
        let reply = self.http.get(url).send().await.ok()?;
        if !reply.status().is_success() {
            return None;
        }
        Some(reply.bytes().await.ok()?.to_vec())
    }

    /// Downloads `f` to `part`, carrying on from whatever is already there.
    async fn fetch(&self, f: &RemoteFile, part: &Path, cancel: &AtomicBool) -> Result<Fetched, String> {
        const NO_WRITE: &str = "Couldn't save a song on this phone. Is its storage full?";
        let have = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
        if have == f.size {
            return Ok(Fetched::Done);
        }
        let have = if have > f.size { 0 } else { have };
        let url = self.url(&format!("/song/{}/{}", f.list, f.pos), &[("dl", "1"), ("v", &f.id)])?;
        let mut reply = self.http.get(url).header(RANGE, format!("bytes={have}-")).send().await.map_err(|_| UNREACHABLE)?;
        let resume = match reply.status() {
            StatusCode::PARTIAL_CONTENT => true,
            StatusCode::OK => false,
            StatusCode::CONFLICT => return Ok(Fetched::Changed),
            StatusCode::NOT_FOUND => return Ok(Fetched::Gone),
            StatusCode::FORBIDDEN => return Err(REFUSED.into()),
            code => return Err(format!("Your PC couldn't send a song ({code}).")),
        };
        let mut out = OpenOptions::new().create(true).write(true).append(resume).truncate(!resume).open(part).map_err(|_| NO_WRITE)?;
        while let Some(chunk) = reply.chunk().await.map_err(|_| UNREACHABLE)? {
            if cancel.load(Ordering::Relaxed) {
                return Err(STOPPED.into());
            }
            out.write_all(&chunk).map_err(|_| NO_WRITE)?;
        }
        out.flush().map_err(|_| NO_WRITE)?;
        let got = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
        if got != f.size {
            let _ = std::fs::remove_file(part);
            return Err("A song arrived incomplete. Sync again to carry on.".into());
        }
        Ok(Fetched::Done)
    }

    async fn has(&self, size: u64, sha: &str) -> Result<Option<String>, String> {
        #[derive(Deserialize)]
        struct Stored {
            id: Option<String>,
        }
        let url = self.url("/api/sync/has", &[("size", &size.to_string()), ("sha256", sha)])?;
        let reply = self.http.get(url).send().await.map_err(|_| UNREACHABLE)?;
        if !reply.status().is_success() {
            return Ok(None);
        }
        Ok(reply.json::<Stored>().await.map_err(|_| UNREACHABLE)?.id)
    }

    async fn upload(&self, path: &Path, name: &str, sha: &str, size: u64) -> Result<Sent, String> {
        #[derive(Deserialize)]
        struct Stored {
            id: Option<String>,
        }
        let url = self.url("/upload", &[("name", name), ("sha256", sha)])?;
        // A damaged or broken-off send gets one more try.
        for _ in 0..2 {
            let Ok(file) = tokio::fs::File::open(path).await else { return Ok(Sent::Skip) };
            let request = self.http.put(url.clone()).header(CONTENT_LENGTH, size).body(reqwest::Body::from(file));
            let reply = request.send().await.map_err(|_| UNREACHABLE)?;
            match reply.status() {
                StatusCode::OK => {
                    return Ok(reply.json::<Stored>().await.map_err(|_| UNREACHABLE)?.id.map_or(Sent::Skip, Sent::Stored));
                }
                StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => {}
                StatusCode::FORBIDDEN | StatusCode::INSUFFICIENT_STORAGE => {
                    return Ok(Sent::Stop(reply.text().await.unwrap_or_default()));
                }
                _ => return Ok(Sent::Skip),
            }
        }
        Ok(Sent::Skip)
    }
}

/// What the PC has, for picking playlists.
pub fn offer(link: Link) -> Result<Offer, String> {
    let client = Client::new(link)?;
    let m = block(client.manifest())?;
    let mut folders: BTreeMap<String, usize> = BTreeMap::new();
    for f in m.files.iter().filter(|f| !f.dir.is_empty()) {
        let ends = f.dir.char_indices().filter(|(_, c)| *c == '/').map(|(i, _)| i).chain([f.dir.len()]);
        for end in ends {
            *folders.entry(f.dir[..end].to_string()).or_default() += 1;
        }
    }
    let folders = folders.into_iter().map(|(path, count)| OfferedFolder { path, count }).collect();
    let lists = m.lists.into_iter().map(|l| OfferedList { id: l.id, name: l.name, count: l.songs.len() }).collect();
    Ok(Offer { receive: m.receive, lists, folders })
}

fn in_folder(dir: &str, folder: &str) -> bool {
    dir.strip_prefix(folder).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The ids this phone should have: the picked playlists' songs and the picked folders' songs.
fn wanted(job: &Job, m: &Manifest) -> HashSet<String> {
    let lists = m.lists.iter().filter(|l| job.lists.contains(&l.id)).flat_map(|l| l.songs.iter().cloned());
    let folders = m.files.iter().filter(|f| job.folders.iter().any(|d| in_folder(&f.dir, d))).map(|f| f.id.clone());
    lists.chain(folders).collect()
}

/// A name that's safe in a folder, empty when nothing's left of it.
fn clean_part(part: &str) -> String {
    let clean: String = part.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    clean.trim().trim_matches('.').to_string()
}

/// Where a song goes under `root`, keeping the PC's folders, as `Song (2).flac` when it's taken.
fn free_name(root: &Path, dir: &str, name: &str, id: &str) -> String {
    let folders: Vec<String> = dir.split('/').map(clean_part).filter(|p| !p.is_empty()).collect();
    let folder = folders.join("/");
    let base = clean_part(name.rsplit(['/', '\\']).next().unwrap_or_default());
    let base = if base.is_empty() { id.to_string() } else { base };
    let (stem, ext) = base.rsplit_once('.').unwrap_or((&base, ""));
    let mut candidate = base.clone();
    let mut n = 2;
    while root.join(&folder).join(&candidate).exists() {
        candidate = format!("{stem} ({n}).{ext}");
        n += 1;
    }
    if folder.is_empty() {
        candidate
    } else {
        format!("{folder}/{candidate}")
    }
}

fn sha256_of(path: &Path) -> io::Result<String> {
    let mut hash = Sha256::new();
    io::copy(&mut File::open(path)?, &mut hash)?;
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Brings this phone up to date. On an error, what's done so far stays done.
pub fn run(job: &Job) -> Result<Summary, String> {
    let client = Client::new(job.link.clone())?;
    let mut summary = Summary::default();
    std::fs::create_dir_all(&job.staging).map_err(|e| e.to_string())?;
    (job.status)("Checking what's on your PC", 0, 0);

    // A file the user deleted here counts as not here.
    {
        let conn = job.lib.db.lock();
        for row in db::sync_files(&conn).map_err(|e| e.to_string())? {
            if !Path::new(&row.path).exists() {
                db::delete_sync_file(&conn, &row.id).map_err(|e| e.to_string())?;
            }
        }
    }

    // A song already on the phone is used as it is, not downloaded a second time.
    if !job.lists.is_empty() || !job.folders.is_empty() {
        for_each_own_song(job, "Checking the songs on this phone", |t, sha| {
            if let Some(id) = block(client.has(t.file_size, sha))? {
                record_own(job, &id, &t.path)?;
            }
            Ok(())
        })?;
    }

    let mut manifest = None;
    for _ in 0..ROUNDS {
        let m = block(client.manifest())?;
        if fetch_round(job, &client, &m, &mut summary)? {
            manifest = Some(m);
            break;
        }
    }
    let manifest = manifest.ok_or("Your PC's music keeps changing. Sync again in a moment.")?;

    let picked: Vec<&RemoteList> = manifest.lists.iter().filter(|l| job.lists.contains(&l.id)).collect();
    let needed = wanted(job, &manifest);
    (job.status)("Copying artwork and lyrics", 0, 0);
    let photos = copy_extras(job, &client, &manifest, &needed, &picked)?;
    update_playlists(job, &picked, &photos)?;
    // Nothing needed usually means the PC's library is empty for now, not that songs should go.
    if !needed.is_empty() {
        summary.removed = clean_up(job, &needed)?;
    }
    if summary.fetched > 0 || summary.removed > 0 {
        add_to_library(job)?;
    }
    if job.send && manifest.receive {
        send_songs(job, &client, &mut summary)?;
    }
    Ok(summary)
}

/// Fetches what's missing. False when the PC changed and the list should be read again.
fn fetch_round(job: &Job, client: &Client, m: &Manifest, summary: &mut Summary) -> Result<bool, String> {
    let needed = wanted(job, m);
    let here: HashSet<String> = db::sync_files(&job.lib.db.lock()).map_err(|e| e.to_string())?.into_iter().map(|r| r.id).collect();
    let missing: Vec<&RemoteFile> = m.files.iter().filter(|f| needed.contains(&f.id) && !here.contains(&f.id)).collect();
    for (done, f) in missing.iter().enumerate() {
        if job.cancel.load(Ordering::Relaxed) {
            return Err(STOPPED.into());
        }
        (job.status)("Getting songs from your PC", done, missing.len());
        if f.id.len() != 32 || !f.id.chars().all(|c| c.is_ascii_hexdigit()) {
            summary.skipped += 1;
            continue;
        }
        let part = job.staging.join(format!("{}.part", f.id));
        match block(client.fetch(f, &part, job.cancel))? {
            Fetched::Changed => return Ok(false),
            Fetched::Gone => summary.skipped += 1,
            Fetched::Done => {
                let path = (job.publish)(&part, &free_name(&job.root, &f.dir, &f.name, &f.id))?;
                db::put_sync_file(&job.lib.db.lock(), &f.id, &path.to_string_lossy(), db::SYNC_FETCHED).map_err(|e| e.to_string())?;
                summary.fetched += 1;
            }
        }
    }
    Ok(true)
}

/// A picture from the PC, in this phone's cover store.
fn fetch_cover(job: &Job, client: &Client, remote: &str) -> Option<String> {
    let url = client.url(&format!("/cover/{remote}"), &[]).ok()?;
    job.lib.covers.store(&block(client.extra(url))?)
}

/// Brings over what doesn't travel inside a song file: artwork, lyrics and playlist photos. Android
/// lets an app put only audio in `Music`, so they go into the app's own stores, where the player
/// finds them. A failure here costs a picture, not the sync. Returns the playlist photos by list id.
fn copy_extras(job: &Job, client: &Client, m: &Manifest, needed: &HashSet<String>, picked: &[&RemoteList]) -> Result<HashMap<i64, String>, String> {
    let rows = db::sync_files(&job.lib.db.lock()).map_err(|e| e.to_string())?;
    let fetched: HashMap<&str, &str> = rows.iter().filter(|r| r.origin == db::SYNC_FETCHED).map(|r| (r.id.as_str(), r.path.as_str())).collect();
    let has_art: HashSet<String> = db::track_art(&job.lib.db.lock()).map_err(|e| e.to_string())?.into_iter().filter(|a| a.cover_id.is_some()).map(|a| a.path).collect();
    let mut art = Vec::new();
    for f in m.files.iter().filter(|f| needed.contains(&f.id)) {
        let Some(path) = fetched.get(f.id.as_str()) else { continue };
        if job.cancel.load(Ordering::Relaxed) {
            return Err(STOPPED.into());
        }
        if f.hidden {
            art.push(TrackArt { path: path.to_string(), cover_id: None, motion_id: None, hidden: true });
        } else if let Some(cover) = f.cover.as_ref().filter(|_| f.art && !has_art.contains(*path)).and_then(|c| fetch_cover(job, client, c)) {
            art.push(TrackArt { path: path.to_string(), cover_id: Some(cover), motion_id: None, hidden: false });
        }
        let have = db::saved_lyrics(&job.lib.db.lock(), path).map_err(|e| e.to_string())?.is_some();
        if f.lyrics && !have {
            let url = client.url(&format!("/lyrics/{}/{}", f.list, f.pos), &[("v", &f.id)])?;
            if let Some(lrc) = block(client.extra(url)).and_then(|b| String::from_utf8(b).ok()) {
                db::save_lyrics(&job.lib.db.lock(), path, &lrc, "pc").map_err(|e| e.to_string())?;
            }
        }
    }
    db::put_track_art(&mut job.lib.db.lock(), &art).map_err(|e| e.to_string())?;
    let photo = |l: &&&RemoteList| Some((l.id, fetch_cover(job, client, l.cover.as_deref()?)?));
    Ok(picked.iter().filter_map(|l| photo(&l)).collect())
}

/// Each picked playlist here matches the PC. Changes made to them on the phone are overwritten.
fn update_playlists(job: &Job, picked: &[&RemoteList], photos: &HashMap<i64, String>) -> Result<(), String> {
    let mut conn = job.lib.db.lock();
    let err = |e: anyhow::Error| e.to_string();
    let paths: HashMap<String, String> = db::sync_files(&conn).map_err(err)?.into_iter().map(|r| (r.id, r.path)).collect();
    for list in picked.iter().filter(|l| l.id != ALL_SONGS) {
        let source_id = list.id.to_string();
        let id = match db::playlist_by_source(&conn, SOURCE, &source_id).map_err(err)? {
            Some((id, _)) => id,
            None => {
                let id = db::create_playlist(&conn, &list.name).map_err(err)?;
                db::set_playlist_source(&conn, id, SOURCE, &source_id).map_err(err)?;
                id
            }
        };
        db::update_playlist(&conn, id, &list.name, &list.description).map_err(err)?;
        db::set_playlist_cover(&conn, id, photos.get(&list.id).map(String::as_str), None).map_err(err)?;
        let items: Vec<(String, Option<String>)> = list.songs.iter().filter_map(|s| paths.get(s)).map(|p| (p.clone(), None)).collect();
        db::set_playlist_items(&mut conn, id, &items).map_err(err)?;
    }
    let keep: HashSet<String> = picked.iter().map(|l| l.id.to_string()).collect();
    for p in db::playlists(&conn).map_err(err)? {
        let unpicked = p.source.as_deref() == Some(SOURCE) && p.source_id.as_ref().is_some_and(|s| !keep.contains(s));
        if unpicked {
            db::delete_playlist(&conn, p.id).map_err(err)?;
        }
    }
    Ok(())
}

/// Deletes fetched copies nothing needs. Songs that started on this phone are never touched.
fn clean_up(job: &Job, needed: &HashSet<String>) -> Result<usize, String> {
    let rows = db::sync_files(&job.lib.db.lock()).map_err(|e| e.to_string())?;
    let mut removed = 0;
    for row in rows.iter().filter(|r| r.origin == db::SYNC_FETCHED && !needed.contains(&r.id)) {
        if (job.remove)(Path::new(&row.path)) {
            db::delete_sync_file(&job.lib.db.lock(), &row.id).map_err(|e| e.to_string())?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// The sync folder is scanned like any other, whether or not the user added it.
fn add_to_library(job: &Job) -> Result<(), String> {
    let folders = job.lib.folders().map_err(|e| e.to_string())?;
    if folders.iter().any(|f| job.root.starts_with(&f.path)) {
        job.lib.request_scan(ScanRequest::All { force: false });
    } else {
        job.lib.add_folder(&job.root.to_string_lossy()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// `sent`, so clean-up leaves it alone and it doesn't come back as a download.
fn record_own(job: &Job, id: &str, path: &str) -> Result<(), String> {
    db::put_sync_file(&job.lib.db.lock(), id, path, db::SYNC_SENT).map_err(|e| e.to_string())
}

/// Calls `visit` with each song that started on this phone and isn't matched to the PC yet, and its
/// checksum.
fn for_each_own_song(job: &Job, step: &str, mut visit: impl FnMut(&Track, &str) -> Result<(), String>) -> Result<(), String> {
    let known: HashSet<String> = db::sync_files(&job.lib.db.lock()).map_err(|e| e.to_string())?.into_iter().map(|r| r.path).collect();
    let tracks = job.lib.tracks().map_err(|e| e.to_string())?;
    let songs: Vec<_> = tracks.iter().filter(|t| !known.contains(&t.path) && !Path::new(&t.path).starts_with(&job.root)).collect();
    for (done, t) in songs.iter().enumerate() {
        if job.cancel.load(Ordering::Relaxed) {
            return Err(STOPPED.into());
        }
        (job.status)(step, done, songs.len());
        let cached = db::sync_hash(&job.lib.db.lock(), &t.path, t.file_size, t.mtime).map_err(|e| e.to_string())?;
        let sha = match cached {
            Some(sha) => sha,
            None => {
                // Unreadable right now: leave it for another run.
                let Ok(sha) = sha256_of(Path::new(&t.path)) else { continue };
                db::put_sync_hash(&job.lib.db.lock(), &t.path, t.file_size, t.mtime, &sha).map_err(|e| e.to_string())?;
                sha
            }
        };
        visit(t, &sha)?;
    }
    Ok(())
}

/// Copies songs that started on this phone to the PC, skipping any it already has.
fn send_songs(job: &Job, client: &Client, summary: &mut Summary) -> Result<(), String> {
    for_each_own_song(job, "Sending songs to your PC", |t, sha| {
        let id = match block(client.has(t.file_size, sha))? {
            Some(id) => id,
            None => {
                let name = Path::new(&t.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                match block(client.upload(Path::new(&t.path), &name, sha, t.file_size))? {
                    Sent::Stored(id) => {
                        summary.sent += 1;
                        id
                    }
                    Sent::Skip => {
                        summary.skipped += 1;
                        return Ok(());
                    }
                    Sent::Stop(why) => return Err(why),
                }
            }
        };
        record_own(job, &id, &t.path)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn links() {
        let l = Link::parse(&format!(" http://192.168.1.20:8787/?k={KEY}\n")).unwrap();
        assert_eq!((l.base.as_str(), l.key.as_str()), ("http://192.168.1.20:8787", KEY));
        assert!(Link::parse("http://192.168.1.20:8787/").is_err(), "no key");
        assert!(Link::parse("http://192.168.1.20/?k=short").is_err());
        assert!(Link::parse(&format!("file:///x?k={KEY}")).is_err());
        assert!(Link::parse("not a link").is_err());
    }

    #[test]
    fn folders_cover_what_is_inside_them() {
        assert!(in_folder("Music/Artist", "Music/Artist"));
        assert!(in_folder("Music/Artist/Album", "Music/Artist"));
        assert!(!in_folder("Music/Artist Two", "Music/Artist"), "another folder with the same start");
        assert!(!in_folder("Music", "Music/Artist"));
    }

    #[test]
    fn names_stay_in_the_folder_and_never_clash() {
        let root = std::env::temp_dir().join(format!("pc-sync-names-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(free_name(&root, "", "Song.flac", "id"), "Song.flac");
        assert_eq!(free_name(&root, "", r"..\..\Song?.flac", "id"), "Song_.flac");
        assert_eq!(free_name(&root, "", "../../Song.flac", "id"), "Song.flac");
        assert_eq!(free_name(&root, "", "...", "abc"), "abc");
        assert_eq!(free_name(&root, "Music/A: B/Album", "Song.flac", "id"), "Music/A_ B/Album/Song.flac");
        assert_eq!(free_name(&root, "../../etc", "Song.flac", "id"), "etc/Song.flac", "folders can't climb out");
        std::fs::write(root.join("Song.flac"), "x").unwrap();
        std::fs::write(root.join("Song (2).flac"), "x").unwrap();
        assert_eq!(free_name(&root, "", "Song.flac", "id"), "Song (3).flac");
        let _ = std::fs::remove_dir_all(&root);
    }
}
