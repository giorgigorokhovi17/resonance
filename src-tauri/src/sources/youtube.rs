//! YouTube downloads through yt-dlp (owner's decision, 2026-10-23): for songs Soulseek doesn't
//! have, and for YouTube playlists the user chooses to take at YouTube's quality.
//!
//! Off until turned on in Settings → Sources. Turning it on downloads two programs from their
//! official GitHub releases into the app's tools folder, each checked against the SHA-256 that
//! GitHub publishes for it: yt-dlp.exe, and QuickJS (qjs.exe), the JavaScript engine yt-dlp needs
//! to get past YouTube's checks. yt-dlp updates itself weekly, since YouTube breaks old versions.
//!
//! Audio is YouTube's own AAC stream (about 128 kbps) saved as .m4a without re-encoding. The new
//! file gets title/artist tags, then is marked read-only like the rest of the library. Nothing is
//! played or streamed from YouTube, and nothing is uploaded.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use super::missing::{queries, title_matches, Found};
use super::soulseek::model::FileAttrs;
use crate::library::interchange::TrackRef;
use crate::library::matcher::normalize::{key, readings};

/// The `username` of YouTube results and downloads, so they share the Soulseek queue and screens.
pub const SOURCE: &str = "YouTube";

/// (GitHub repository, release asset, file name in the tools folder).
const TOOLS: [(&str, &str, &str); 2] = [
    ("yt-dlp/yt-dlp", "yt-dlp.exe", "yt-dlp.exe"),
    ("quickjs-ng/quickjs", "qjs-windows-x86_64.exe", "qjs.exe"),
];
const UPDATE_EVERY: Duration = Duration::from_secs(7 * 24 * 3600);
const LENGTH_TOLERANCE_S: f64 = 3.0;
/// YouTube Music candidates whose details are looked up (each lookup takes a few seconds).
const DETAIL_LOOKUPS: usize = 2;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolState {
    /// Not downloaded yet.
    Missing,
    Installing,
    Ready,
    Failed,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct YtStatus {
    pub enabled: bool,
    pub state: ToolState,
    /// yt-dlp's version, once installed.
    pub version: Option<String>,
    pub error: Option<String>,
}

struct Inner {
    state: ToolState,
    version: Option<String>,
    error: Option<String>,
}

pub struct YouTube {
    app: AppHandle,
    dir: PathBuf,
    enabled: AtomicBool,
    inner: Mutex<Inner>,
}

/// A YouTube video id: 11 characters of [A-Za-z0-9_-].
pub fn valid_video_id(id: &str) -> bool {
    id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The video id in a watch URL we made (`…watch?v=ID`).
pub fn video_id_of(url: &str) -> Option<&str> {
    let id = url.split_once("watch?v=")?.1.split(['&', '#']).next()?;
    valid_video_id(id).then_some(id)
}

/// "Artist - Title.m4a", safe as a Windows file name.
pub fn file_name(artist: &str, title: &str) -> String {
    let base = if artist.trim().is_empty() { title.trim().to_string() } else { format!("{} - {}", artist.trim(), title.trim()) };
    let mut s: String = base
        .chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { '_' } else { c })
        .collect();
    s = s.chars().take(150).collect::<String>().trim_end_matches(['.', ' ']).to_string();
    if s.is_empty() {
        s = "YouTube audio".into();
    }
    format!("{s}.m4a")
}

/// Details of one video, as yt-dlp prints them.
#[derive(Debug, Clone, PartialEq)]
pub struct Details {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Option<u32>,
    pub channel: String,
}

/// Parses the tab-separated line `details` asks for. yt-dlp prints "NA" for missing fields.
pub fn parse_details(line: &str) -> Option<Details> {
    let f: Vec<&str> = line.trim_end_matches(['\r', '\n']).split('\t').collect();
    if f.len() < 7 || !valid_video_id(f[0]) {
        return None;
    }
    let s = |v: &str| if v == "NA" { String::new() } else { v.to_string() };
    let track = s(f[1]);
    Some(Details {
        id: f[0].to_string(),
        title: if track.is_empty() { s(f[6]) } else { track },
        // "Artist, Other Artist" from YouTube Music; an "… - Topic" channel names the artist too.
        artist: s(f[2]),
        album: s(f[3]),
        duration: f[4].parse::<f64>().ok().map(|d| d.round() as u32),
        channel: s(f[5]).trim_end_matches(" - Topic").to_string(),
    })
}

/// A YouTube result in the same shape as a Soulseek one, ranked below every FLAC and MP3.
pub fn found(d: &Details, wanted_ms: Option<u64>) -> Found {
    let artist = if d.artist.is_empty() { d.channel.clone() } else { d.artist.split(',').next().unwrap_or("").trim().to_string() };
    let delta = match (wanted_ms, d.duration) {
        (Some(ms), Some(len)) => Some((ms as f64 / 1000.0 - len as f64).abs()),
        _ => None,
    };
    let mut why = vec!["AAC ~128 kbps".to_string()];
    why.push(match delta {
        Some(d) => format!("length matches (Δ {d:.0} s)"),
        None => "length unknown".into(),
    });
    Found {
        username: SOURCE.into(),
        path: format!("https://music.youtube.com/watch?v={}", d.id),
        name: file_name(&artist, &d.title),
        folder: SOURCE.into(),
        // About 130 kbps.
        size: d.duration.map(|s| s as u64 * 16_250).unwrap_or(0),
        format: "AAC".into(),
        attrs: FileAttrs { bitrate: Some(128), length: d.duration, vbr: false, sample_rate: Some(44_100), bit_depth: None },
        free_slots: true,
        speed: 0,
        score: 50.0 + if delta.is_some() { 20.0 } else { 0.0 },
        why: why.join(" · "),
    }
}

/// Whether a looked-up video is the song: same title and version, the main artist in its artist
/// or channel, and (when both know it) a length within 3 s.
pub fn accepts(r: &TrackRef, d: &Details) -> bool {
    let wanted = readings(&r.title, &r.artists);
    let mut file = readings(&d.title, &[]);
    if !d.artist.is_empty() {
        file.extend(readings(&d.title, &[d.artist.clone()]));
    }
    if !title_matches(&file, &wanted) {
        return false;
    }
    if let Some(a) = wanted.iter().find_map(|w| w.artists.first().cloned()) {
        let names = key(&format!("{} {} {}", d.artist, d.channel, d.title));
        if !names.contains(a.as_str()) {
            return false;
        }
    }
    !matches!((r.duration_ms, d.duration), (Some(ms), Some(len)) if (ms as f64 / 1000.0 - len as f64).abs() > LENGTH_TOLERANCE_S)
}

fn hidden(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

impl YouTube {
    pub fn new(app: AppHandle, dir: PathBuf, enabled: bool) -> Arc<Self> {
        let _ = std::fs::create_dir_all(&dir);
        let me = Arc::new(Self {
            app,
            dir,
            enabled: AtomicBool::new(enabled),
            inner: Mutex::new(Inner { state: ToolState::Missing, version: None, error: None }),
        });
        if me.installed() {
            me.inner.lock().state = ToolState::Ready;
        }
        me
    }

    fn tool(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn installed(&self) -> bool {
        TOOLS.iter().all(|(_, _, f)| self.tool(f).is_file())
    }

    /// Turned on and installed.
    pub fn ready(&self) -> bool {
        self.enabled.load(Ordering::Relaxed) && self.inner.lock().state == ToolState::Ready
    }

    pub fn status(&self) -> YtStatus {
        let i = self.inner.lock();
        YtStatus { enabled: self.enabled.load(Ordering::Relaxed), state: i.state, version: i.version.clone(), error: i.error.clone() }
    }

    fn emit(&self) {
        let _ = self.app.emit("yt:status", self.status());
    }

    /// Turns YouTube downloads on or off. Turning on installs the tools if they're missing, and
    /// otherwise updates yt-dlp when it's more than a week old.
    pub fn set_enabled(self: &Arc<Self>, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
        if on {
            if self.installed() {
                self.refresh_version();
                self.update_if_old();
            } else {
                self.install();
            }
        }
        self.emit();
    }

    /// Downloads both tools in the background (again, if they failed before).
    pub fn install(self: &Arc<Self>) {
        {
            let mut i = self.inner.lock();
            if i.state == ToolState::Installing {
                return;
            }
            i.state = ToolState::Installing;
            i.error = None;
        }
        self.emit();
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let result = me.fetch_tools().await;
            if result.is_ok() {
                let me2 = me.clone();
                let _ = tauri::async_runtime::spawn_blocking(move || me2.refresh_version()).await;
            }
            {
                let mut i = me.inner.lock();
                match &result {
                    Ok(()) => i.state = ToolState::Ready,
                    Err(e) => {
                        i.state = ToolState::Failed;
                        i.error = Some(e.clone());
                    }
                }
            }
            me.emit();
        });
    }

    async fn fetch_tools(&self) -> Result<(), String> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("Undertone/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| e.to_string())?;
        for (repo, asset, file) in TOOLS {
            let release: serde_json::Value = client
                .get(format!("https://api.github.com/repos/{repo}/releases/latest"))
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| format!("Couldn't reach GitHub for {file}: {e}"))?
                .json()
                .await
                .map_err(|e| format!("GitHub's answer for {file} couldn't be read: {e}"))?;
            let a = release["assets"]
                .as_array()
                .and_then(|list| list.iter().find(|a| a["name"] == asset))
                .ok_or_else(|| format!("The latest {repo} release has no {asset}."))?;
            let url = a["browser_download_url"].as_str().ok_or("Missing download link.")?;
            let digest = a["digest"]
                .as_str()
                .and_then(|d| d.strip_prefix("sha256:"))
                .ok_or_else(|| format!("GitHub gives no checksum for {asset}, so it isn't installed."))?
                .to_ascii_lowercase();
            let bytes = client
                .get(url)
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| format!("Downloading {file} failed: {e}"))?
                .bytes()
                .await
                .map_err(|e| format!("Downloading {file} failed: {e}"))?;
            let got = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect::<String>();
            if got != digest {
                return Err(format!("{file} didn't match its published checksum, so it wasn't installed."));
            }
            let dest = self.tool(file);
            let tmp = dest.with_extension("download");
            std::fs::write(&tmp, &bytes).map_err(|e| format!("Couldn't save {file}: {e}"))?;
            std::fs::rename(&tmp, &dest).map_err(|e| format!("Couldn't save {file}: {e}"))?;
        }
        Ok(())
    }

    fn refresh_version(&self) {
        let out = hidden(Command::new(self.tool("yt-dlp.exe")).arg("--version")).output();
        let v = out.ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        self.inner.lock().version = v;
    }

    /// yt-dlp updates itself when it's more than a week old (YouTube changes often).
    pub fn update_if_old(self: &Arc<Self>) {
        let exe = self.tool("yt-dlp.exe");
        let old = std::fs::metadata(&exe)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age > UPDATE_EVERY);
        if !old {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            let ok = hidden(Command::new(&exe).args(["-U", "--no-warnings"])).output().is_ok_and(|o| o.status.success());
            if ok {
                // Mark it checked even when it was already the newest.
                let _ = std::fs::File::options().append(true).open(&exe).and_then(|f| f.set_modified(SystemTime::now()));
            } else {
                log::warn!("yt-dlp couldn't update itself");
            }
            me.refresh_version();
            me.emit();
        });
    }

    /// yt-dlp with the options every call shares.
    fn command(&self) -> Command {
        let mut c = Command::new(self.tool("yt-dlp.exe"));
        hidden(&mut c)
            .args(["--ignore-config", "--no-warnings", "--no-playlist", "--js-runtimes"])
            .arg(format!("quickjs:{}", self.tool("qjs.exe").display()))
            .stdin(Stdio::null());
        c
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let out = self.command().args(args).output().map_err(|e| format!("yt-dlp couldn't start: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let line = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("yt-dlp failed.").trim();
            return Err(line.trim_start_matches("ERROR: ").to_string());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn details(&self, id: &str) -> Result<Details, String> {
        let url = format!("https://music.youtube.com/watch?v={id}");
        let line = self.run(&[
            "--skip-download",
            "--print",
            "%(id)s\t%(track)s\t%(artist)s\t%(album)s\t%(duration)s\t%(channel)s\t%(title)s",
            &url,
        ])?;
        parse_details(line.lines().next().unwrap_or("")).ok_or_else(|| "YouTube gave no details for that video.".into())
    }

    /// (video id, title) of YouTube Music's song results for a search.
    fn search_songs(&self, query: &str) -> Result<Vec<(String, String)>, String> {
        let q = percent_encoding::utf8_percent_encode(query, percent_encoding::NON_ALPHANUMERIC).to_string();
        let json = self.run(&["--flat-playlist", "-J", "--playlist-end", "8", &format!("https://music.youtube.com/search?q={q}#songs")])?;
        let v: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        Ok(v["entries"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|e| Some((e["id"].as_str()?.to_string(), e["title"].as_str().unwrap_or("").to_string())))
                    .filter(|(id, _)| valid_video_id(id))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Finds a song on YouTube. A reference imported from YouTube is taken as it is; otherwise
    /// YouTube Music's song results with the right title are checked for artist and length.
    pub fn find(&self, r: &TrackRef, cancel: &AtomicBool) -> Result<Vec<Found>, String> {
        if let Some(id) = r.service_ids.get("youtube").filter(|id| valid_video_id(id)) {
            let d = self.details(id)?;
            return Ok(vec![found(&d, r.duration_ms)]);
        }
        let Some(q) = queries(r).into_iter().next() else { return Ok(Vec::new()) };
        let wanted = readings(&r.title, &r.artists);
        let candidates: Vec<String> = self
            .search_songs(&q)?
            .into_iter()
            .filter(|(_, title)| title_matches(&readings(title, &[]), &wanted))
            .map(|(id, _)| id)
            .take(DETAIL_LOOKUPS)
            .collect();
        for id in candidates {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if let Ok(d) = self.details(&id) {
                if accepts(r, &d) {
                    return Ok(vec![found(&d, r.duration_ms)]);
                }
            }
        }
        Ok(Vec::new())
    }

    /// Downloads a video's audio into `dir` as `name` (an .m4a), reporting (bytes, total, speed).
    /// Tags the file with `title`/`artist`, the album and the cover YouTube shows.
    pub fn download(
        &self,
        url: &str,
        dir: &Path,
        name: &str,
        tags: (&str, &str),
        cancel: Arc<AtomicBool>,
        mut progress: impl FnMut(u64, u64, f64),
    ) -> Result<PathBuf, String> {
        let id = video_id_of(url).ok_or("Not a YouTube video link.")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
        let stem = name.strip_suffix(".m4a").unwrap_or(name);
        let dest = dir.join(format!("{stem}.m4a"));
        if dest.exists() {
            return Err("A file with this name is already in the download folder.".into());
        }
        let template = dir.join(format!("{stem}.%(ext)s"));
        let mut child = self
            .command()
            .args(["-f", "bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]", "--no-mtime", "--newline", "-o"])
            .arg(&template)
            .args([
                "--progress-template",
                "download:UTP %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.speed)s",
                "--print",
                "after_move:filepath",
                "--print",
                "after_move:META album %(album|)s",
                "--print",
                "after_move:META thumb %(thumbnail|)s",
            ])
            .arg(format!("https://music.youtube.com/watch?v={id}"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("yt-dlp couldn't start: {e}"))?;
        let stdout = child.stdout.take().ok_or("No output from yt-dlp.")?;
        let mut stderr = child.stderr.take().ok_or("No output from yt-dlp.")?;
        let err_reader = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = stderr.read_to_string(&mut s);
            s
        });
        // Stop yt-dlp when the download is cancelled, even while it's quiet.
        let finished = Arc::new(AtomicBool::new(false));
        let killer = {
            let finished = finished.clone();
            let cancel = cancel.clone();
            let pid = child.id();
            std::thread::spawn(move || {
                while !finished.load(Ordering::Relaxed) {
                    if cancel.load(Ordering::Relaxed) {
                        let _ = hidden(Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"])).output();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            })
        };
        let mut saved: Option<PathBuf> = None;
        let (mut album, mut thumb) = (String::new(), String::new());
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(v) = line.strip_prefix("META album ") {
                album = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("META thumb ") {
                thumb = v.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("UTP ") {
                let n: Vec<Option<f64>> = rest.split_whitespace().map(|x| x.parse::<f64>().ok()).collect();
                let get = |i: usize| n.get(i).copied().flatten();
                let total = get(1).or(get(2)).unwrap_or(0.0);
                progress(get(0).unwrap_or(0.0) as u64, total as u64, get(3).unwrap_or(0.0));
            } else if !line.trim().is_empty() && Path::new(line.trim()).is_absolute() {
                saved = Some(PathBuf::from(line.trim()));
            }
        }
        let status = child.wait();
        finished.store(true, Ordering::Relaxed);
        let _ = killer.join();
        let err = err_reader.join().unwrap_or_default();
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled.".into());
        }
        let ok = status.is_ok_and(|s| s.success());
        let file = saved.filter(|p| p.is_file()).filter(|_| ok).ok_or_else(|| {
            let line = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("The download failed.");
            line.trim().trim_start_matches("ERROR: ").to_string()
        })?;
        let cover = thumb.starts_with("https://").then(|| fetch_cover(&thumb)).flatten();
        if let Err(e) = write_tags(&file, tags.0, tags.1, &album, cover) {
            log::warn!("couldn't tag {}: {e}", file.display());
        }
        Ok(file)
    }
}

/// The video's thumbnail as a square JPEG cover (video thumbnails are 16:9 with the art in the
/// middle). None when it can't be fetched; the song is fine without one.
fn fetch_cover(url: &str) -> Option<Vec<u8>> {
    let bytes = tauri::async_runtime::block_on(async {
        let r = reqwest::Client::builder().timeout(Duration::from_secs(20)).build().ok()?.get(url).send().await.ok()?;
        r.error_for_status().ok()?.bytes().await.ok()
    })?;
    let img = image::load_from_memory(&bytes).ok()?;
    let side = img.width().min(img.height());
    let square = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side);
    let square = if side > 600 { square.resize(600, 600, image::imageops::FilterType::Lanczos3) } else { square };
    let mut out = std::io::Cursor::new(Vec::new());
    square.to_rgb8().write_to(&mut out, image::ImageFormat::Jpeg).ok()?;
    Some(out.into_inner())
}

/// Tags on a freshly downloaded file (before it's marked read-only).
fn write_tags(path: &Path, title: &str, artist: &str, album: &str, cover: Option<Vec<u8>>) -> Result<(), String> {
    use lofty::config::WriteOptions;
    use lofty::picture::{MimeType, Picture, PictureType};
    use lofty::prelude::*;
    use lofty::tag::Tag;
    let mut tagged = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    if tagged.primary_tag().is_none() {
        let kind = tagged.primary_tag_type();
        tagged.insert_tag(Tag::new(kind));
    }
    let tag = tagged.primary_tag_mut().ok_or("no tag")?;
    if !title.is_empty() {
        tag.set_title(title.to_string());
    }
    if !artist.is_empty() {
        tag.set_artist(artist.to_string());
    }
    if !album.is_empty() && album != "NA" {
        tag.set_album(album.to_string());
    }
    if let Some(data) = cover {
        tag.push_picture(Picture::unchecked(data).pic_type(PictureType::CoverFront).mime_type(MimeType::Jpeg).build());
    }
    tagged.save_to_path(path, WriteOptions::default()).map_err(|e| e.to_string())
}

/// ("Artist", "Title") from a name made by [`file_name`].
pub fn tags_from_name(name: &str) -> (String, String) {
    let stem = name.strip_suffix(".m4a").unwrap_or(name);
    match stem.split_once(" - ") {
        Some((a, t)) => (t.to_string(), a.to_string()),
        None => (stem.to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(title: &str, artists: &[&str], ms: Option<u64>) -> TrackRef {
        TrackRef { title: title.into(), artists: artists.iter().map(|a| a.to_string()).collect(), duration_ms: ms, ..Default::default() }
    }

    fn d(title: &str, artist: &str, channel: &str, secs: Option<u32>) -> Details {
        Details { id: "Q6FarZpy67M".into(), title: title.into(), artist: artist.into(), album: String::new(), duration: secs, channel: channel.into() }
    }

    #[test]
    fn video_ids_and_names() {
        assert!(valid_video_id("Q6FarZpy67M"));
        assert!(!valid_video_id("Q6FarZpy67"));
        assert!(!valid_video_id("../../x/abc"));
        assert_eq!(video_id_of("https://music.youtube.com/watch?v=Q6FarZpy67M&list=x"), Some("Q6FarZpy67M"));
        assert_eq!(video_id_of("https://example.com/watch?v=bad"), None);
        assert_eq!(file_name("Steve Lacy", "Dark Red"), "Steve Lacy - Dark Red.m4a");
        assert_eq!(file_name("AC/DC", "What? *Now*."), "AC_DC - What_ _Now_.m4a");
        assert_eq!(file_name("", " "), "YouTube audio.m4a");
        assert_eq!(tags_from_name("Steve Lacy - Dark Red.m4a"), ("Dark Red".into(), "Steve Lacy".into()));
        assert_eq!(tags_from_name("Untitled.m4a"), ("Untitled".into(), String::new()));
    }

    #[test]
    fn details_line() {
        let x = parse_details("Q6FarZpy67M\tDark Red\tSteve Lacy\tDark Red\t173.0\tSteve Lacy - Topic\tDark Red\n").unwrap();
        assert_eq!(x, Details { id: "Q6FarZpy67M".into(), title: "Dark Red".into(), artist: "Steve Lacy".into(), album: "Dark Red".into(), duration: Some(173), channel: "Steve Lacy".into() });
        let y = parse_details("Q6FarZpy67M\tNA\tNA\tNA\tNA\tSome Uploader\tSteve Lacy - Dark Red (Audio)").unwrap();
        assert_eq!((y.title.as_str(), y.artist.as_str(), y.duration), ("Steve Lacy - Dark Red (Audio)", "", None));
        assert!(parse_details("not\tenough").is_none());
    }

    #[test]
    fn accepts_only_the_same_song() {
        let want = r("Dark Red", &["Steve Lacy"], Some(173_000));
        assert!(accepts(&want, &d("Dark Red", "Steve Lacy", "Steve Lacy", Some(173))));
        assert!(accepts(&want, &d("Dark Red", "", "Steve Lacy", Some(174))), "the channel names the artist");
        assert!(!accepts(&want, &d("Dark Red (Sped Up)", "Steve Lacy", "", Some(150))), "another version");
        assert!(!accepts(&want, &d("Dark Red", "Someone Else", "Covers", Some(173))), "another artist");
        assert!(!accepts(&want, &d("Dark Red", "Steve Lacy", "", Some(200))), "length too far off");
        assert!(accepts(&r("Dark Red", &["Steve Lacy"], None), &d("Dark Red", "Steve Lacy", "", Some(200))), "no length to compare");
    }

    #[test]
    fn results_rank_below_soulseek_files() {
        let f = found(&d("Dark Red", "Steve Lacy, Other", "", Some(173)), Some(173_000));
        assert_eq!((f.username.as_str(), f.name.as_str(), f.format.as_str()), (SOURCE, "Steve Lacy - Dark Red.m4a", "AAC"));
        assert_eq!(f.path, "https://music.youtube.com/watch?v=Q6FarZpy67M");
        assert!(f.score < 100.0, "any accepted MP3 or FLAC scores 100 or more");
        assert!(f.why.starts_with("AAC ~128 kbps · length matches"));
    }
}
