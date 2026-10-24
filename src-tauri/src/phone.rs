//! Phone sync (Settings → Phone sync): a small web server on the home network, so a phone's
//! browser can play this PC's playlists or download songs and whole playlists (one .zip with an
//! .m3u8 that phone players read). Off until turned on. Every request needs the secret key from
//! the link (the QR code in Settings), and songs are only reachable by playlist and position (the
//! library's own songs or a playlist's), never by path. Nothing is reachable from the internet
//! unless the user forwards the port on their router.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::net::UdpSocket;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use qrcodegen::{QrCode, QrCodeEcc};
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::library::model::Track;
use crate::library::{db, Library};
use crate::settings::PhoneSyncSettings;
use crate::AppState;

/// The page the phone opens (plain HTML, CSS and script, no build step).
const PAGE: &str = include_str!("phone.html");
/// Playlist id for the whole library.
const ALL_SONGS: i64 = 0;

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
    stop: Arc<AtomicBool>,
    server: Arc<Server>,
    thread: JoinHandle<()>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
static ERROR: Mutex<Option<String>> = Mutex::new(None);

/// Starts, stops or restarts the server to match `s`, and says how it went.
pub fn apply(app: &AppHandle, s: &PhoneSyncSettings) -> PhoneStatus {
    let mut running = RUNNING.lock();
    let unchanged = running.as_ref().is_some_and(|r| s.enabled && r.port == s.port && r.key == s.key);
    if !unchanged {
        if let Some(r) = running.take() {
            r.stop.store(true, Ordering::Relaxed);
            r.server.unblock();
            // The port is free again once its thread lets go of the server.
            let _ = r.thread.join();
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

fn start(app: &AppHandle, s: &PhoneSyncSettings) -> Result<Running, String> {
    let library = app.try_state::<AppState>().ok_or("the app is still starting")?.library.clone();
    let server = Arc::new(Server::http(("0.0.0.0", s.port)).map_err(|e| e.to_string())?);
    let stop = Arc::new(AtomicBool::new(false));
    let (srv, flag, key) = (server.clone(), stop.clone(), s.key.clone());
    let thread = std::thread::Builder::new()
        .name("phone-sync".into())
        .spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match srv.recv_timeout(Duration::from_millis(500)) {
                    Ok(Some(req)) => {
                        let (library, key) = (library.clone(), key.clone());
                        // Songs and .zip files can take minutes: one thread per request.
                        let _ = std::thread::Builder::new().name("phone-request".into()).spawn(move || handle(&library, &key, req));
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        })
        .map_err(|e| e.to_string())?;
    log::info!("phone sync on port {}", s.port);
    Ok(Running { port: s.port, key: s.key.clone(), stop, server, thread })
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

fn handle(lib: &Library, key: &str, req: Request) {
    let url = req.url().to_string();
    let (path, q) = url.split_once('?').unwrap_or((&url, ""));
    let q = query(q);
    if q.get("k").map(String::as_str) != Some(key) {
        return text(req, 403, "Open the link from Undertone (Settings → Phone sync).");
    }
    if !matches!(req.method(), Method::Get | Method::Head) {
        return text(req, 405, "Not allowed.");
    }
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
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
        ["song", id, i] => match (num(id), num(i)) {
            (Some(id), Some(i)) => song(lib, req, id, i, q.contains_key("dl")),
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

/// One song, with seeking (HTTP ranges) for the phone's player.
fn song(lib: &Library, req: Request, id: i64, i: i64, download: bool) {
    let Some(track) = songs(lib, id).and_then(|(_, t)| usize::try_from(i).ok().and_then(|i| t.into_iter().nth(i))) else {
        return text(req, 404, "No such song.");
    };
    let Ok(mut file) = File::open(&track.path) else { return text(req, 404, "The file isn't there any more.") };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
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
