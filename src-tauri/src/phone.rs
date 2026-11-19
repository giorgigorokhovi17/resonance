//! Phone sync (Settings → Phone sync): a small web server on the home network, so a phone's
//! browser can play this PC's playlists or download songs and whole playlists (one .zip with an
//! .m3u8 that phone players read). Off until turned on. Every request needs the secret key from
//! the link (the QR code in Settings), and songs are only reachable by playlist and position (the
//! library's own songs or a playlist's), never by path. Nothing is reachable from the internet
//! unless the user forwards the port on their router.
//!
//! The phone app syncs through here too (docs/phone-file-sync.md): `/api/sync` lists what's on this
//! PC, and `/song` checks the song is still the one listed. With "Receive songs from my phone" on,
//! `/upload` adds new songs to a `Phone` folder in the library. That's the only write: it never
//! replaces, changes or deletes a file.

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use qrcodegen::{QrCode, QrCodeEcc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::library::model::{is_audio_file, Track};
use crate::library::{covers, db, lyrics, tags, Library, ScanRequest};
use crate::settings::PhoneSyncSettings;
use crate::AppState;

/// The page the phone opens (plain HTML, CSS and script, no build step).
const PAGE: &str = include_str!("phone.html");
/// Playlist id for the whole library.
const ALL_SONGS: i64 = 0;
/// Goes up when `/api/sync` changes in a way an older phone app can't read.
const SYNC_VERSION: u32 = 1;
/// Where songs from the phone land, inside the first library folder.
const PHONE_FOLDER: &str = "Phone";
/// Far past any song or album-long file.
const MAX_UPLOAD: u64 = 2 << 30;
/// An upload that would leave less than this on the drive is refused.
const KEEP_FREE: u64 = 1 << 30;

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PhoneStatus {
    pub running: bool,
    /// The link to open on the phone (with the key).
    pub url: Option<String>,
    /// The same link as a QR code (SVG).
    pub qr: Option<String>,
    /// Why it isn't running, in plain words.
    pub error: Option<String>,
}

struct Running {
    port: u16,
    key: String,
    /// Read for each request, so turning it on or off needs no restart.
    receive: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    server: Arc<Server>,
    thread: JoinHandle<()>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
static ERROR: Mutex<Option<String>> = Mutex::new(None);
/// Checksums of the uploads in progress: the same song sent twice at once would share a `.part`.
static UPLOADING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Starts, stops or restarts the server to match `s`, and says how it went.
pub fn apply(app: &AppHandle, s: &PhoneSyncSettings) -> PhoneStatus {
    let mut running = RUNNING.lock();
    let unchanged = running.as_ref().filter(|r| s.enabled && r.port == s.port && r.key == s.key);
    if let Some(r) = unchanged {
        r.receive.store(s.receive, Ordering::Relaxed);
    } else {
        if let Some(Running { port, stop, server, thread, .. }) = running.take() {
            stop.store(true, Ordering::Relaxed);
            server.unblock();
            let _ = thread.join();
            drop(server);
            free_port(port);
        }
        *ERROR.lock() = None;
        if s.enabled {
            match start(app, s) {
                Ok(r) => *running = Some(r),
                Err(e) => {
                    log::warn!("phone sync: {e}");
                    *ERROR.lock() = Some(format!("Couldn't start on port {}: another app may be using it. ({e})", s.port));
                }
            }
        }
    }
    drop(running);
    status()
}

/// Lets go of the port after the server is dropped. tiny_http wakes its listener by connecting to
/// 0.0.0.0, which Windows refuses, so without this the port stays taken until the app closes.
fn free_port(port: u16) {
    let _ = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_secs(1));
}

/// Binds the port, waiting briefly in case it's still being let go of.
fn bind(port: u16) -> Result<Server, String> {
    for _ in 0..20 {
        if let Ok(server) = Server::http(("0.0.0.0", port)) {
            return Ok(server);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Server::http(("0.0.0.0", port)).map_err(|e| e.to_string())
}

fn start(app: &AppHandle, s: &PhoneSyncSettings) -> Result<Running, String> {
    let library = app.try_state::<AppState>().ok_or("the app is still starting")?.library.clone();
    let server = Arc::new(bind(s.port)?);
    let stop = Arc::new(AtomicBool::new(false));
    let receive = Arc::new(AtomicBool::new(s.receive));
    let (srv, flag, key, accept) = (server.clone(), stop.clone(), s.key.clone(), receive.clone());
    let thread = std::thread::Builder::new()
        .name("phone-sync".into())
        .spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match srv.recv_timeout(Duration::from_millis(500)) {
                    Ok(Some(req)) => {
                        let (library, key, receive) = (library.clone(), key.clone(), accept.load(Ordering::Relaxed));
                        // Songs and .zip files can take minutes: one thread per request.
                        let _ = std::thread::Builder::new().name("phone-request".into()).spawn(move || handle(&library, &key, receive, req));
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        })
        .map_err(|e| e.to_string())?;
    log::info!("phone sync on port {}", s.port);
    Ok(Running { port: s.port, key: s.key.clone(), receive, stop, server, thread })
}

pub fn status() -> PhoneStatus {
    match RUNNING.lock().as_ref() {
        Some(r) => {
            let host = lan_ip().unwrap_or_else(|| "localhost".into());
            let url = format!("http://{host}:{}/?k={}", r.port, r.key);
            PhoneStatus { running: true, qr: Some(qr_svg(&url)), url: Some(url), error: None }
        }
        None => PhoneStatus { error: ERROR.lock().clone(), ..PhoneStatus::default() },
    }
}

/// This PC's address on the home network. Connecting a UDP socket only picks the route; nothing
/// is sent (192.0.2.1 is a documentation address).
fn lan_ip() -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then(|| ip.to_string())
}

/// The link as a black-on-white QR code.
fn qr_svg(text: &str) -> String {
    let Ok(qr) = QrCode::encode_text(text, QrCodeEcc::Medium) else { return String::new() };
    let border = 2;
    let size = qr.size() + 2 * border;
    let mut path = String::new();
    for y in 0..qr.size() {
        for x in 0..qr.size() {
            if qr.get_module(x, y) {
                path.push_str(&format!("M{},{}h1v1h-1z", x + border, y + border));
            }
        }
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" shape-rendering="crispEdges"><rect width="100%" height="100%" fill="#fff"/><path d="{path}" fill="#000"/></svg>"##
    )
}

// ---- requests -----------------------------------------------------------------------------------

type Body = Box<dyn Read + Send>;

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap_or_else(|_| Header::from_bytes(&b"X-Ignored"[..], &b"1"[..]).expect("valid"))
}

fn respond(req: Request, code: u16, kind: &str, body: Body, len: Option<usize>, extra: Vec<Header>) {
    let mut headers = vec![header("Content-Type", kind), header("Cache-Control", "no-store")];
    headers.extend(extra);
    let _ = req.respond(Response::new(StatusCode(code), headers, body, len, None));
}

fn text(req: Request, code: u16, msg: &str) {
    let bytes = msg.as_bytes().to_vec();
    let len = bytes.len();
    respond(req, code, "text/plain; charset=utf-8", Box::new(io::Cursor::new(bytes)), Some(len), vec![]);
}

fn json<T: Serialize>(req: Request, value: &T) {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let len = bytes.len();
    respond(req, 200, "application/json", Box::new(io::Cursor::new(bytes)), Some(len), vec![]);
}

fn query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter_map(|p| p.split_once('=').or(Some((p, ""))))
        .filter(|(k, _)| !k.is_empty())
        .map(|(k, v)| (k.to_string(), percent_decode_str(v).decode_utf8_lossy().to_string()))
        .collect()
}

fn handle(lib: &Library, key: &str, receive: bool, req: Request) {
    let url = req.url().to_string();
    let (path, q) = url.split_once('?').unwrap_or((&url, ""));
    let q = query(q);
    if q.get("k").map(String::as_str) != Some(key) {
        return text(req, 403, "Open the link from Undertone (Settings → Phone sync).");
    }
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    if *req.method() == Method::Put && parts == ["upload"] {
        return upload(lib, receive, req, &q);
    }
    if !matches!(req.method(), Method::Get | Method::Head) {
        return text(req, 405, "Not allowed.");
    }
    let num = |s: &str| s.parse::<i64>().ok();
    match parts.as_slice() {
        [""] => {
            let len = PAGE.len();
            respond(req, 200, "text/html; charset=utf-8", Box::new(io::Cursor::new(PAGE.as_bytes())), Some(len), vec![])
        }
        ["api", "lists"] => match lists(lib) {
            Some(l) => json(req, &l),
            None => text(req, 500, "Couldn't read the playlists."),
        },
        ["api", "list", id] => match num(id).and_then(|id| songs(lib, id)) {
            Some((name, tracks)) => json(req, &ListJson { name, songs: tracks.iter().map(SongJson::from).collect() }),
            None => text(req, 404, "No such playlist."),
        },
        ["api", "sync"] => match sync_lists(lib, receive) {
            Some(m) => json(req, &m),
            None => text(req, 500, "Couldn't read the playlists."),
        },
        ["api", "sync", "has"] => {
            let size = q.get("size").and_then(|s| s.parse().ok()).unwrap_or(0);
            let sha = q.get("sha256").map(|s| s.to_ascii_lowercase()).unwrap_or_default();
            json(req, &Stored { id: find_copy(lib, size, &sha) })
        }
        ["song", id, i] => match (num(id), num(i)) {
            (Some(id), Some(i)) => song(lib, req, id, i, q.contains_key("dl"), q.get("v").map(String::as_str)),
            _ => text(req, 404, "No such song."),
        },
        ["lyrics", id, i] => match (num(id), num(i)) {
            (Some(id), Some(i)) => song_lyrics(lib, req, id, i, q.get("v").map(String::as_str)),
            _ => text(req, 404, "No such song."),
        },
        ["cover", id] => match lib.covers.file(id, true).and_then(|p| std::fs::read(p).ok()) {
            Some(bytes) => {
                let len = bytes.len();
                let cache = header("Cache-Control", "max-age=31536000, immutable");
                let _ = req.respond(Response::new(StatusCode(200), vec![header("Content-Type", "image/jpeg"), cache], io::Cursor::new(bytes), Some(len), None));
            }
            None => text(req, 404, "No cover."),
        },
        ["zip", id] => match num(id).and_then(|id| songs(lib, id)) {
            Some((name, tracks)) => zip(req, &name, tracks),
            None => text(req, 404, "No such playlist."),
        },
        _ => text(req, 404, "Not found."),
    }
}

// ---- playlists and songs ------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ListInfo {
    id: i64,
    name: String,
    count: i64,
    cover: Option<String>,
}

#[derive(Serialize)]
struct ListJson {
    name: String,
    songs: Vec<SongJson>,
}

#[derive(Serialize)]
struct SongJson {
    title: String,
    artist: String,
    album: String,
    duration: f64,
    cover: Option<String>,
    ext: String,
}

impl From<&Track> for SongJson {
    fn from(t: &Track) -> Self {
        Self {
            title: t.title.clone(),
            artist: if t.artist.is_empty() { t.album_artist.clone() } else { t.artist.clone() },
            album: t.album.clone(),
            duration: t.duration,
            cover: t.cover_id.clone(),
            ext: extension(&t.path),
        }
    }
}

fn extension(path: &str) -> String {
    Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// "All songs" first, then the playlists.
fn lists(lib: &Library) -> Option<Vec<ListInfo>> {
    let all = lib.tracks().ok()?;
    let mut out = vec![ListInfo {
        id: ALL_SONGS,
        name: "All songs".into(),
        count: all.len() as i64,
        cover: all.iter().find_map(|t| t.cover_id.clone()),
    }];
    let playlists = db::playlists(&lib.db.lock()).ok()?;
    out.extend(playlists.into_iter().map(|p| ListInfo {
        id: p.id,
        name: p.name,
        count: p.track_count,
        cover: p.cover_id.or_else(|| p.preview_covers.first().cloned()),
    }));
    Some(out)
}

/// A playlist's name and its songs that are on this PC, in order. The library for `ALL_SONGS`,
/// by artist, album and track.
fn songs(lib: &Library, id: i64) -> Option<(String, Vec<Track>)> {
    if id == ALL_SONGS {
        let mut all = lib.tracks().ok()?;
        all.sort_by(|a, b| {
            let artist = |t: &Track| (if t.album_artist.is_empty() { &t.artist } else { &t.album_artist }).to_lowercase();
            (artist(a), a.album.to_lowercase(), a.disc_no, a.track_no).cmp(&(artist(b), b.album.to_lowercase(), b.disc_no, b.track_no))
        });
        return Some(("All songs".into(), all));
    }
    let (name, paths) = {
        let conn = lib.db.lock();
        let name = db::playlists(&conn).ok()?.into_iter().find(|p| p.id == id)?.name;
        let paths: Vec<String> = db::playlist_items(&conn, id).ok()?.into_iter().map(|(p, _)| p).collect();
        (name, paths)
    };
    let tracks = lib.resolve_paths(&paths).into_iter().filter(|(_, missing)| !missing).map(|(t, _)| t).collect();
    Some((name, tracks))
}

fn mime(ext: &str) -> &'static str {
    match ext {
        "flac" => "audio/flac",
        "mp3" => "audio/mpeg",
        "m4a" | "mp4" | "aac" | "alac" => "audio/mp4",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" | "wave" => "audio/wav",
        "aif" | "aiff" | "aifc" => "audio/aiff",
        "mka" => "audio/x-matroska",
        _ => "application/octet-stream",
    }
}

/// `Content-Disposition` for a download, with the real name for browsers that read it.
fn attachment(name: &str) -> Header {
    let plain: String = name.chars().map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' { c } else { '_' }).collect();
    header("Content-Disposition", &format!("attachment; filename=\"{plain}\"; filename*=UTF-8''{}", utf8_percent_encode(name, NON_ALPHANUMERIC)))
}

/// `bytes=a-b`, `bytes=a-` or `bytes=-n` within a file of `len` bytes.
fn parse_range(value: &str, len: u64) -> Option<(u64, u64)> {
    let spec = value.trim().strip_prefix("bytes=")?;
    if spec.contains(',') || len == 0 {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", n) => {
            let n: u64 = n.parse().ok()?;
            (len.saturating_sub(n), len - 1)
        }
        (a, "") => (a.parse().ok()?, len - 1),
        (a, b) => (a.parse().ok()?, b.parse::<u64>().ok()?.min(len - 1)),
    };
    (start <= end && start < len).then_some((start, end))
}

fn track_at(lib: &Library, id: i64, i: i64) -> Option<Track> {
    let (_, tracks) = songs(lib, id)?;
    tracks.into_iter().nth(usize::try_from(i).ok()?)
}

/// A song's lyrics from outside its file: the `.lrc` beside it, or what was found online. Lyrics
/// in the tags travel with the song.
fn song_lyrics(lib: &Library, req: Request, id: i64, i: i64, want: Option<&str>) {
    let Some(track) = track_at(lib, id, i) else { return text(req, 404, "No such song.") };
    if want.is_some_and(|v| v != sync_id(&track)) {
        return text(req, 409, "This song changed. Get the list again.");
    }
    let saved = || db::saved_lyrics(&lib.db.lock(), &track.path).ok().flatten().map(|(lrc, _)| lrc);
    match lyrics::sidecar(Path::new(&track.path)).or_else(saved) {
        Some(lrc) => text(req, 200, &lrc),
        None => text(req, 404, "No lyrics."),
    }
}

/// One song, with seeking (HTTP ranges) for the phone's player. With `want` (a sync id from
/// `/api/sync`), answers 409 unless that's still the song here, so a playlist reordered since the
/// list was made can't put the wrong song on the phone.
fn song(lib: &Library, req: Request, id: i64, i: i64, download: bool, want: Option<&str>) {
    let Some(track) = track_at(lib, id, i) else { return text(req, 404, "No such song.") };
    if want.is_some_and(|v| v != sync_id(&track)) {
        return text(req, 409, "This song changed. Get the list again.");
    }
    let Ok(mut file) = File::open(&track.path) else { return text(req, 404, "The file isn't there any more.") };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    // Changed on disk and not scanned yet: its listed size would no longer be right.
    if want.is_some() && len != track.file_size {
        return text(req, 409, "This song changed. Get the list again.");
    }
    let ext = extension(&track.path);
    let mut extra = vec![header("Accept-Ranges", "bytes")];
    if download {
        let name = Path::new(&track.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| format!("song.{ext}"));
        extra.push(attachment(&name));
    }
    let range = req.headers().iter().find(|h| h.field.equiv("Range")).and_then(|h| parse_range(h.value.as_str(), len));
    match range {
        Some((start, end)) if file.seek(SeekFrom::Start(start)).is_ok() => {
            let n = end - start + 1;
            extra.push(header("Content-Range", &format!("bytes {start}-{end}/{len}")));
            respond(req, 206, mime(&ext), Box::new(file.take(n)), usize::try_from(n).ok(), extra);
        }
        _ => respond(req, 200, mime(&ext), Box::new(file), usize::try_from(len).ok(), extra),
    }
}

// ---- the list the phone app syncs from ----------------------------------------------------------

/// What's on this PC: each song once, and the playlists as lists of those songs.
#[derive(Serialize)]
struct SyncManifest {
    version: u32,
    /// Whether `/upload` is turned on.
    receive: bool,
    files: Vec<SyncFile>,
    lists: Vec<SyncList>,
}

#[derive(Serialize)]
struct SyncFile {
    id: String,
    /// Where to get it: `/song/<list>/<pos>?v=<id>`.
    list: i64,
    pos: usize,
    /// The file's own name, without its folders.
    name: String,
    size: u64,
    /// Its folder from the library folder's own name down, like `Music/Artist/Album`. Nothing above
    /// the library folder leaves the PC.
    dir: String,
    /// The artwork came from outside the file: a picture beside it, or one the user picked.
    art: bool,
    /// The user hid this song's artwork.
    hidden: bool,
    /// `/lyrics/<list>/<pos>` has lyrics for it.
    lyrics: bool,
    #[serde(flatten)]
    song: SongJson,
}

#[derive(Serialize)]
struct SyncList {
    id: i64,
    name: String,
    description: String,
    /// The playlist's own photo.
    cover: Option<String>,
    /// File ids in playlist order.
    songs: Vec<String>,
}

/// A list going into the manifest.
struct SyncSource {
    id: i64,
    name: String,
    description: String,
    cover: Option<String>,
    tracks: Vec<Track>,
}

/// What's beside the songs in one folder.
struct Beside {
    art: bool,
    /// Names of `.lrc` files, lower case and without the extension.
    lrc: HashSet<String>,
}

fn beside(dir: &Path) -> Beside {
    let entries = std::fs::read_dir(dir).into_iter().flatten().flatten();
    let is_lrc = |p: &Path| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lrc"));
    let lrc = entries.map(|e| e.path()).filter(|p| is_lrc(p)).filter_map(|p| Some(p.file_stem()?.to_string_lossy().to_lowercase())).collect();
    Beside { art: covers::find_folder_image(dir).is_some(), lrc }
}

/// `path`'s folder from its library folder's own name down, split by `/`. Empty outside the library.
fn library_dir(path: &str, roots: &[String]) -> String {
    let Some(parent) = Path::new(path).parent() else { return String::new() };
    let inside = roots.iter().find_map(|r| Some((Path::new(r), parent.strip_prefix(r).ok()?)));
    let Some((root, rest)) = inside else { return String::new() };
    let name = root.file_name().map_or_else(|| "Music".to_string(), |n| n.to_string_lossy().to_string());
    std::iter::once(name).chain(rest.iter().map(|c| c.to_string_lossy().to_string())).collect::<Vec<_>>().join("/")
}

/// Names one version of one file without saying where it is. A new id means the phone's copy is
/// out of date.
fn sync_id(t: &Track) -> String {
    let hash = Sha256::digest(format!("{}\0{}\0{}", t.path, t.file_size, t.mtime));
    hash[..16].iter().map(|b| format!("{b:02x}")).collect()
}

fn manifest(lists: Vec<SyncSource>, roots: &[String], saved_lyrics: &HashSet<String>, receive: bool) -> SyncManifest {
    let (mut files, mut seen, mut out) = (Vec::new(), HashSet::new(), Vec::new());
    let mut folders: HashMap<PathBuf, Beside> = HashMap::new();
    for list in lists {
        let mut songs = Vec::with_capacity(list.tracks.len());
        for (pos, t) in list.tracks.iter().enumerate() {
            let file = sync_id(t);
            if seen.insert(file.clone()) {
                let path = Path::new(&t.path);
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let dir = path.parent().unwrap_or(Path::new(""));
                let near = folders.entry(dir.to_path_buf()).or_insert_with(|| beside(dir));
                let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
                files.push(SyncFile {
                    id: file.clone(),
                    list: list.id,
                    pos,
                    name,
                    size: t.file_size,
                    dir: library_dir(&t.path, roots),
                    art: t.custom_cover || (near.art && t.cover_id.is_some()),
                    hidden: t.cover_hidden,
                    lyrics: near.lrc.contains(&stem) || saved_lyrics.contains(&t.path),
                    song: SongJson::from(t),
                });
            }
            songs.push(file);
        }
        out.push(SyncList { id: list.id, name: list.name, description: list.description, cover: list.cover, songs });
    }
    SyncManifest { version: SYNC_VERSION, receive, files, lists: out }
}

fn sync_lists(lib: &Library, receive: bool) -> Option<SyncManifest> {
    let roots: Vec<String> = lib.folders().ok()?.into_iter().map(|f| f.path).collect();
    let (playlists, saved) = {
        let conn = lib.db.lock();
        (db::playlists(&conn).ok()?, db::saved_lyric_paths(&conn).ok()?)
    };
    let sources = lists(lib)?.into_iter().filter_map(|l| {
        let own = playlists.iter().find(|p| p.id == l.id);
        songs(lib, l.id).map(|(name, tracks)| SyncSource {
            id: l.id,
            name,
            description: own.map(|p| p.description.clone()).unwrap_or_default(),
            cover: own.and_then(|p| p.cover_id.clone()),
            tracks,
        })
    });
    Some(manifest(sources.collect(), &roots, &saved, receive))
}

// ---- songs from the phone -----------------------------------------------------------------------

/// A song's sync id on this PC, once it's here.
#[derive(Serialize)]
struct Stored {
    id: Option<String>,
}

fn sha256_hex(mut from: impl Read) -> io::Result<String> {
    let mut hash = Sha256::new();
    io::copy(&mut from, &mut hash)?;
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// A library song with exactly these bytes. Only songs of the same size get read, so it's quick.
fn find_copy(lib: &Library, size: u64, sha: &str) -> Option<String> {
    let tracks = lib.tracks().ok()?;
    let mut same = tracks.iter().filter(|t| t.file_size == size);
    same.find(|t| File::open(&t.path).and_then(sha256_hex).is_ok_and(|h| h == sha)).map(sync_id)
}

/// The name to save a song under: its last part only, safe on Windows, and an audio file.
fn upload_name(name: &str) -> Option<String> {
    let clean = safe_name(name.rsplit(['/', '\\']).next()?);
    let first = clean.split('.').next()?.to_ascii_uppercase();
    let numbered = first.len() == 4 && (first.starts_with("COM") || first.starts_with("LPT")) && first.ends_with(|c: char| c.is_ascii_digit());
    let reserved = numbered || matches!(first.as_str(), "CON" | "PRN" | "AUX" | "NUL");
    (!reserved && clean.chars().count() <= 200 && is_audio_file(Path::new(&clean))).then_some(clean)
}

fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is a null-terminated path that outlives the call, and `free` is a local.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&raw mut free), None, None) }.ok()?;
    Some(free)
}

/// Writes exactly `len` bytes to `path`, returning their checksum.
fn write_part(body: &mut dyn Read, path: &Path, len: u64) -> io::Result<String> {
    let mut out = BufWriter::with_capacity(1 << 16, File::create(path)?);
    let (mut hash, mut buf, mut got) = (Sha256::new(), vec![0u8; 1 << 16], 0u64);
    let mut body = body.take(len);
    loop {
        let n = body.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        got += n as u64;
    }
    out.flush()?;
    if got != len {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the phone stopped sending"));
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Moves a finished upload to `name` in `dir`, or `name (2)` and so on. The empty file reserves the
/// name first, because a plain rename would replace a song that's already there.
fn place(part: &Path, dir: &Path, name: &str) -> io::Result<PathBuf> {
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    for n in 1..1000 {
        let dest = dir.join(if n == 1 { name.to_string() } else { format!("{stem} ({n}).{ext}") });
        match OpenOptions::new().write(true).create_new(true).open(&dest) {
            Ok(_) => {
                return std::fs::rename(part, &dest).map(|()| dest.clone()).inspect_err(|_| {
                    let _ = std::fs::remove_file(&dest);
                })
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("too many songs with this name"))
}

/// Saves a song from the phone into the library's `Phone` folder, read-only like a download, and
/// returns its sync id. Anything that fails leaves nothing behind.
fn store(lib: &Library, body: &mut dyn Read, name: &str, sha: &str, len: u64) -> Result<String, (u16, &'static str)> {
    const NO_SAVE: (u16, &str) = (500, "Couldn't save the song on this PC.");
    let folders = lib.folders().map_err(|_| NO_SAVE)?;
    let root = folders.iter().find(|f| Path::new(&f.path).is_dir()).ok_or((409, "Add a music folder in Undertone first."))?;
    let dir = Path::new(&root.path).join(PHONE_FOLDER);
    std::fs::create_dir_all(&dir).map_err(|_| NO_SAVE)?;
    if free_bytes(&dir).is_some_and(|free| free < len.saturating_add(KEEP_FREE)) {
        return Err((507, "This PC's music drive is nearly full."));
    }

    let part = dir.join(format!("{sha}.part"));
    let saved = (|| {
        match write_part(body, &part, len) {
            Ok(got) if got == sha => {}
            Ok(_) => return Err((422, "The song arrived damaged. Try again.")),
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Err((400, "The song didn't arrive whole. Try again.")),
            Err(_) => return Err(NO_SAVE),
        }
        match tags::read_track(&part, false) {
            Ok((t, _)) if t.duration > 0.0 => {}
            _ => return Err((415, "That isn't a song Undertone can play.")),
        }
        place(&part, &dir, name).map_err(|_| NO_SAVE)
    })();
    let dest = saved.inspect_err(|_| {
        let _ = std::fs::remove_file(&part);
    })?;

    let id = tags::read_track(&dest, false).map(|(t, _)| sync_id(&t)).map_err(|_| NO_SAVE)?;
    match std::fs::metadata(&dest) {
        Ok(meta) => {
            let mut perms = meta.permissions();
            perms.set_readonly(true);
            if let Err(e) = std::fs::set_permissions(&dest, perms) {
                log::warn!("could not mark {} read-only: {e}", dest.display());
            }
        }
        Err(e) => log::warn!("song from the phone missing at {}: {e}", dest.display()),
    }
    lib.request_scan(ScanRequest::Folder(root.id));
    Ok(id)
}

/// `PUT /upload?name=...&sha256=...` with the song as the body.
fn upload(lib: &Library, receive: bool, mut req: Request, q: &HashMap<String, String>) {
    if !receive {
        return text(req, 403, "Receiving songs is off. Turn it on in Undertone (Settings → Phone sync).");
    }
    let Some(name) = q.get("name").and_then(|n| upload_name(n)) else {
        return text(req, 400, "That isn't an audio file name.");
    };
    let sha = q.get("sha256").map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return text(req, 400, "The song's checksum is missing.");
    }
    let Some(len) = req.body_length().map(|n| n as u64) else {
        return text(req, 411, "The song's size is missing.");
    };
    if len > MAX_UPLOAD {
        return text(req, 413, "That file is too big.");
    }
    if let Some(id) = find_copy(lib, len, &sha) {
        return json(req, &Stored { id: Some(id) });
    }
    {
        let mut busy = UPLOADING.lock();
        if busy.contains(&sha) {
            drop(busy);
            return text(req, 409, "This song is already on its way.");
        }
        busy.push(sha.clone());
    }
    let saved = store(lib, req.as_reader(), &name, &sha, len);
    UPLOADING.lock().retain(|s| *s != sha);
    match saved {
        Ok(id) => json(req, &Stored { id: Some(id) }),
        Err((code, msg)) => text(req, code, msg),
    }
}

// ---- a playlist as one .zip ---------------------------------------------------------------------

/// Hands the .zip from its writer thread to the response as it's made.
struct ChanWriter(SyncSender<Vec<u8>>);

impl Write for ChanWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.send(buf.to_vec()).map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "the phone stopped the download"))?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct ChanReader {
    rx: Receiver<Vec<u8>>,
    chunk: Vec<u8>,
    at: usize,
}

impl Read for ChanReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.at >= self.chunk.len() {
            match self.rx.recv() {
                Ok(c) => {
                    self.chunk = c;
                    self.at = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.chunk.len() - self.at);
        buf[..n].copy_from_slice(&self.chunk[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// Characters Windows, Android and zip tools all accept in a name.
fn safe_name(s: &str) -> String {
    let s: String = s.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() {
        "Playlist".into()
    } else {
        s
    }
}

/// The playlist's songs, numbered in order, in a folder named after it, with an .m3u8 next to
/// them that phone players open as the playlist. Stored, not compressed: music doesn't shrink.
fn zip(req: Request, name: &str, tracks: Vec<Track>) {
    let folder = safe_name(name);
    let (tx, rx) = sync_channel::<Vec<u8>>(8);
    let dir = folder.clone();
    let _ = std::thread::Builder::new().name("phone-zip".into()).spawn(move || {
        let mut zip = zip::ZipWriter::new_stream(BufWriter::with_capacity(1 << 16, ChanWriter(tx)));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(true);
        let mut m3u = String::from("#EXTM3U\n");
        for (n, t) in tracks.iter().enumerate() {
            let base = Path::new(&t.path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            let file = safe_name(&format!("{:03} - {base}", n + 1));
            let Ok(mut src) = File::open(&t.path) else { continue };
            if zip.start_file(format!("{dir}/{file}"), opts).is_err() || io::copy(&mut src, &mut zip).is_err() {
                return; // the phone stopped the download
            }
            let artist = if t.artist.is_empty() { &t.album_artist } else { &t.artist };
            m3u.push_str(&format!("#EXTINF:{},{artist} - {}\n{file}\n", t.duration.round() as i64, t.title));
        }
        if zip.start_file(format!("{dir}/{dir}.m3u8"), opts).is_ok() && zip.write_all(m3u.as_bytes()).is_ok() {
            if let Ok(mut out) = zip.finish() {
                let _ = out.flush();
            }
        }
    });
    let body = ChanReader { rx, chunk: Vec::new(), at: 0 };
    respond(req, 200, "application/zip", Box::new(body), None, vec![attachment(&format!("{folder}.zip"))]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=500-", 1000), Some((500, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=900-5000", 1000), Some((900, 999)), "an end past the file is the end");
        assert_eq!(parse_range("bytes=1000-", 1000), None, "starts past the file");
        assert_eq!(parse_range("bytes=0-1,5-6", 1000), None, "several ranges aren't supported");
        assert_eq!(parse_range("items=0-1", 1000), None);
    }

    #[test]
    fn names_and_queries() {
        assert_eq!(safe_name(" Gym: Mix / 2026? "), "Gym_ Mix _ 2026_");
        assert_eq!(safe_name("..."), "Playlist");
        let q = query("k=abc123&dl=1&x=a%20b");
        assert_eq!((q["k"].as_str(), q["dl"].as_str(), q["x"].as_str()), ("abc123", "1", "a b"));
        assert_eq!(mime("flac"), "audio/flac");
    }

    #[test]
    fn qr_code_is_an_svg() {
        let svg = qr_svg("http://192.168.1.20:8787/?k=0123456789abcdef0123456789abcdef");
        assert!(svg.starts_with("<svg") && svg.contains("<path d=\"M"));
    }

    #[test]
    fn sync_lists_each_song_once_without_paths() {
        let track = |path: &str, mtime| Track { path: path.into(), title: "T".into(), file_size: 10, mtime, ..Default::default() };
        let (a, b) = (track(r"C:\Secret\Music\a.flac", 1), track(r"C:\Secret\Music\b.mp3", 1));
        let list = |id, name: &str, tracks: Vec<Track>| SyncSource { id, name: name.into(), description: String::new(), cover: None, tracks };
        let lists = vec![list(ALL_SONGS, "All songs", vec![a.clone(), b.clone()]), list(7, "Gym", vec![b.clone(), a.clone(), b.clone()])];
        let m = manifest(lists, &[r"C:\Secret\Music".to_string()], &HashSet::new(), false);

        assert_eq!(m.files.len(), 2, "a song in several lists is one file");
        assert_eq!((m.files[1].list, m.files[1].pos, m.files[1].name.as_str()), (ALL_SONGS, 1, "b.mp3"));
        assert_eq!(m.lists[1].songs, [sync_id(&b), sync_id(&a), sync_id(&b)], "a playlist keeps its order and its repeats");
        assert_ne!(sync_id(&a), sync_id(&track(&a.path, 2)), "an edited file is a new version");
        assert_eq!(sync_id(&a).len(), 32);
        assert_eq!(m.files[0].dir, "Music");
        assert_eq!(library_dir(r"C:\Secret\Music\Artist\Album\a.flac", &[r"C:\Secret\Music".to_string()]), "Music/Artist/Album");
        assert_eq!(library_dir(r"E:\Elsewhere\a.flac", &[r"C:\Secret\Music".to_string()]), "", "outside the library");
        assert!(!serde_json::to_string(&m).unwrap().contains("Secret"), "what's above the library folder stays on the PC");
    }

    /// A real server, a real library, plain HTTP: the status code and body of one request.
    fn call(server: &Server, lib: &Library, receive: bool, method: &str, path: &str, body: &[u8]) -> (u16, String) {
        use std::io::Read as _;
        let port = server.server_addr().to_ip().unwrap().port();
        let mut conn = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(conn, "{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: {}\r\n\r\n", body.len()).unwrap();
        conn.write_all(body).unwrap();
        handle(lib, "key", receive, server.recv().unwrap());
        let mut reply = Vec::new();
        conn.read_to_end(&mut reply).unwrap();
        let reply = String::from_utf8_lossy(&reply).to_string();
        let code = reply.split(' ').nth(1).unwrap().parse().unwrap();
        (code, reply.split_once("\r\n\r\n").unwrap().1.to_string())
    }

    fn get(server: &Server, lib: &Library, path: &str) -> (u16, String) {
        call(server, lib, false, "GET", path, b"")
    }

    #[test]
    fn a_stale_sync_id_never_gets_the_wrong_song() {
        let dir = std::env::temp_dir().join(format!("phone-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let lib = Library::open(&dir.join("library.db"), dir.join("covers"), dir.join("media"), Arc::new(|_| {})).unwrap();
        let add = |name: &str, body: &str, conn: &mut rusqlite::Connection, folder| {
            let path = dir.join(name).to_string_lossy().to_string();
            std::fs::write(&path, body).unwrap();
            let t = Track { path: path.clone(), title: name.into(), file_size: body.len() as u64, mtime: 1, ..Default::default() };
            db::upsert_tracks(conn, folder, &[(t, dir.to_string_lossy().to_string())]).unwrap();
            path
        };
        let (first, second, list) = {
            let mut conn = lib.db.lock();
            let folder = db::add_folder(&conn, &dir.to_string_lossy()).unwrap();
            let first = add("first.flac", "first song", &mut conn, folder);
            let second = add("second.flac", "second song", &mut conn, folder);
            let list = db::create_playlist(&conn, "Mix").unwrap();
            db::set_playlist_items(&mut conn, list, &[(first.clone(), None), (second.clone(), None)]).unwrap();
            (first, second, list)
        };
        let server = Server::http("127.0.0.1:0").unwrap();

        assert_eq!(get(&server, &lib, "/api/sync").0, 403, "no key, no list");
        let (code, body) = get(&server, &lib, "/api/sync?k=key");
        assert_eq!(code, 200);
        let m: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(m["version"], SYNC_VERSION);
        assert_eq!(m["lists"][1]["name"], "Mix");
        let first_id = m["lists"][1]["songs"][0].as_str().unwrap().to_string();

        let at = format!("/song/{list}/0?k=key&v={first_id}");
        assert_eq!(get(&server, &lib, &at), (200, "first song".to_string()));

        // The playlist is reordered after the phone read the list.
        db::set_playlist_items(&mut lib.db.lock(), list, &[(second, None), (first.clone(), None)]).unwrap();
        assert_eq!(get(&server, &lib, &at).0, 409, "position 0 is another song now");
        assert_eq!(get(&server, &lib, &format!("/song/{list}/0?k=key")), (200, "second song".to_string()), "the browser page doesn't check");

        // The file is edited and not scanned yet.
        std::fs::write(&first, "first song, longer").unwrap();
        assert_eq!(get(&server, &lib, &format!("/song/{list}/1?k=key&v={first_id}")).0, 409);

        drop(lib);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stopped_server_lets_go_of_its_port() {
        let first = Server::http(("0.0.0.0", 0)).unwrap();
        let port = first.server_addr().to_ip().unwrap().port();
        drop(first);
        free_port(port);
        assert!(bind(port).is_ok(), "the port can be used again without closing the app");
    }

    #[test]
    fn upload_names_stay_in_their_folder() {
        assert_eq!(upload_name("Song.flac").as_deref(), Some("Song.flac"));
        assert_eq!(upload_name(r"..\..\Windows\evil.flac").as_deref(), Some("evil.flac"));
        assert_eq!(upload_name("../../evil.MP3").as_deref(), Some("evil.MP3"));
        assert_eq!(upload_name("what?.flac").as_deref(), Some("what_.flac"));
        for bad in ["evil.exe", "song.flac.part", "..", "", "noextension", ".flac", "CON.flac", "nul.mp3", "com1.flac", "lpt9.x.flac"] {
            assert_eq!(upload_name(bad), None, "{bad}");
        }
        assert!(upload_name(&format!("{}.flac", "a".repeat(300))).is_none());
    }

    /// A short silent mono WAV, which the tag reader takes for real audio.
    fn wav(frames: u32) -> Vec<u8> {
        let data = frames * 2;
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&8000u32.to_le_bytes());
        w.extend_from_slice(&16000u32.to_le_bytes());
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&data.to_le_bytes());
        w.resize(w.len() + data as usize, 0);
        w
    }

    #[test]
    fn songs_from_the_phone_only_ever_add_files() {
        let dir = std::env::temp_dir().join(format!("phone-upload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let music = dir.join("music");
        std::fs::create_dir_all(&music).unwrap();
        let lib = Library::open(&dir.join("library.db"), dir.join("covers"), dir.join("media"), Arc::new(|_| {})).unwrap();
        let server = Server::http("127.0.0.1:0").unwrap();
        let phone = music.join(PHONE_FOLDER);
        let send = |receive, name: &str, sha: &str, body: &[u8]| call(&server, &lib, receive, "PUT", &format!("/upload?k=key&name={name}&sha256={sha}"), body);
        let (song, other) = (wav(800), wav(1600));
        let (sha, other_sha) = (sha256_hex(&song[..]).unwrap(), sha256_hex(&other[..]).unwrap());

        assert_eq!(send(false, "a.wav", &sha, &song).0, 403, "off until turned on");
        assert_eq!(send(true, "a.wav", &sha, &song).0, 409, "no library folder to put it in");
        assert!(!phone.exists());
        db::add_folder(&lib.db.lock(), &music.to_string_lossy()).unwrap();

        assert_eq!(send(true, "a.exe", &sha, &song).0, 400);
        assert_eq!(send(true, "a.wav", "abc", &song).0, 400);
        assert_eq!(send(true, "a.wav", &other_sha, &song).0, 422, "the checksum has to match");
        let junk = b"not a song at all";
        assert_eq!(send(true, "a.wav", &sha256_hex(&junk[..]).unwrap(), junk).0, 415);
        assert_eq!(std::fs::read_dir(&phone).unwrap().count(), 0, "refused uploads leave nothing behind");

        let (code, body) = send(true, "a.wav", &sha, &song);
        assert_eq!(code, 200, "{body}");
        let first = phone.join("a.wav");
        assert_eq!(std::fs::read(&first).unwrap(), song);
        assert!(std::fs::metadata(&first).unwrap().permissions().readonly(), "read-only, like a download");
        let id: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(id["id"].as_str().unwrap().len(), 32);

        // Same name, different song: both stay.
        assert_eq!(send(true, "a.wav", &other_sha, &other).0, 200);
        assert_eq!(std::fs::read(&first).unwrap(), song, "the first file wasn't replaced");
        assert_eq!(std::fs::read(phone.join("a (2).wav")).unwrap(), other);

        for f in std::fs::read_dir(&phone).unwrap() {
            let f = f.unwrap().path();
            let mut perms = std::fs::metadata(&f).unwrap().permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            std::fs::set_permissions(&f, perms).unwrap();
        }
        drop(lib);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_phone_can_ask_before_sending() {
        let dir = std::env::temp_dir().join(format!("phone-has-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let lib = Library::open(&dir.join("library.db"), dir.join("covers"), dir.join("media"), Arc::new(|_| {})).unwrap();
        let path = dir.join("a.flac");
        std::fs::write(&path, "same bytes").unwrap();
        let t = Track { path: path.to_string_lossy().to_string(), file_size: 10, mtime: 1, ..Default::default() };
        {
            let mut conn = lib.db.lock();
            let folder = db::add_folder(&conn, &dir.to_string_lossy()).unwrap();
            db::upsert_tracks(&mut conn, folder, &[(t.clone(), dir.to_string_lossy().to_string())]).unwrap();
        }
        let sha = sha256_hex(&b"same bytes"[..]).unwrap();
        assert_eq!(find_copy(&lib, 10, &sha), Some(sync_id(&t)));
        assert_eq!(find_copy(&lib, 11, &sha), None, "another size");
        assert_eq!(find_copy(&lib, 10, &sha256_hex(&b"diff bytes"[..]).unwrap()), None, "same size, other bytes");
        drop(lib);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_playlist_zips_with_its_m3u() {
        let dir = std::env::temp_dir().join(format!("phone-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let song = dir.join("Song.flac");
        std::fs::write(&song, b"fake audio").unwrap();
        let t = Track { path: song.to_string_lossy().to_string(), title: "Song".into(), artist: "Artist".into(), duration: 61.4, ..Default::default() };

        let (tx, rx) = sync_channel::<Vec<u8>>(8);
        {
            let mut zip = zip::ZipWriter::new_stream(ChanWriter(tx));
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            zip.start_file("x/001 - Song.flac", opts).unwrap();
            io::copy(&mut File::open(&t.path).unwrap(), &mut zip).unwrap();
            zip.finish().unwrap();
        }
        let mut bytes = Vec::new();
        ChanReader { rx, chunk: Vec::new(), at: 0 }.read_to_end(&mut bytes).unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04", "a zip file");
        assert!(bytes.windows(10).any(|w| w == b"fake audio"), "the song is stored as it is");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
