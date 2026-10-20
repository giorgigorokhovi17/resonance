//! Soulseek: sign in, search and download into a library folder.
//!
//! The protocol is `soulseek-rs-lib` (see docs/soulseek-design.md); this module is the only code
//! that touches it. Everything the UI sees goes through `Soulseek` and two events:
//! `slsk:status` (sign-in state) and `slsk:downloads` (the queue). Search results come as
//! `slsk:search`.

pub mod credentials;
pub mod model;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use soulseek_rs::{Client, ClientSettings, DownloadStatus, SearchResult};
use tauri::{AppHandle, Emitter};

use crate::settings::SoulseekSettings;
use credentials::Credentials;
use model::{DownloadState, RawFile, RawResponse, ResultUser};

/// How long a search collects answers. Most arrive in the first few seconds.
const SEARCH_WINDOW: Duration = Duration::from_secs(15);

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConnState {
    SignedOut,
    Connecting,
    SignedIn,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: ConnState,
    /// Signed-in user, or the saved one while signed out (to fill in the form).
    pub username: Option<String>,
    /// Whether a password is saved in Credential Manager.
    pub remembered: bool,
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchUpdate {
    pub id: u64,
    pub query: String,
    pub users: Vec<ResultUser>,
    pub file_count: usize,
    pub done: bool,
}

/// One entry in the download queue, as the UI sees it.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: u64,
    pub username: String,
    /// Remote path, as the user shares it.
    pub remote_path: String,
    pub name: String,
    pub size: u64,
    /// Folder the file goes into.
    pub folder: String,
    pub state: DownloadState,
    /// Imported playlist entry (matcher ref key) this download fills in, once it's done.
    pub link_key: Option<String>,
    pub bytes: u64,
    /// Bytes per second while downloading.
    pub speed: f64,
    pub error: Option<String>,
}

#[derive(serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub username: String,
    pub path: String,
    pub size: u64,
    /// Link the finished file to this imported playlist entry (matcher ref key).
    #[serde(default)]
    pub link_key: Option<String>,
}

/// Called with (ref key, file path) when a linked download finishes.
pub type OnLinkedDone = Arc<dyn Fn(String, PathBuf) + Send + Sync>;

struct Conn {
    client: Option<Arc<Client>>,
    state: ConnState,
    username: Option<String>,
    error: Option<String>,
    /// Bumped on every sign-in/out so stale watcher threads stop.
    generation: u64,
}

pub struct Soulseek {
    app: AppHandle,
    target: String,
    conn: Mutex<Conn>,
    jobs: Mutex<Vec<Job>>,
    next_job: AtomicU64,
    search_id: AtomicU64,
    search_cancel: Mutex<Option<Arc<AtomicBool>>>,
    on_linked_done: Mutex<Option<OnLinkedDone>>,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl Soulseek {
    pub fn new(app: AppHandle, identifier: &str) -> Arc<Self> {
        Arc::new(Self {
            app,
            target: credentials::target_name(identifier),
            conn: Mutex::new(Conn { client: None, state: ConnState::SignedOut, username: None, error: None, generation: 0 }),
            jobs: Mutex::new(Vec::new()),
            next_job: AtomicU64::new(1),
            search_id: AtomicU64::new(0),
            search_cancel: Mutex::new(None),
            on_linked_done: Mutex::new(None),
        })
    }

    pub fn status(&self) -> Status {
        let saved = credentials::load(&self.target);
        let c = self.conn.lock();
        Status {
            state: c.state,
            username: c.username.clone().or_else(|| saved.as_ref().map(|s| s.username.clone())),
            remembered: saved.is_some(),
            error: c.error.clone(),
        }
    }

    fn emit_status(&self) {
        let _ = self.app.emit("slsk:status", self.status());
    }

    fn client(&self) -> Result<Arc<Client>, String> {
        self.conn.lock().client.clone().ok_or_else(|| "Sign in to Soulseek first.".to_string())
    }

    /// Signs in with the saved account, if there is one. Used at startup.
    pub fn sign_in_saved(self: &Arc<Self>, settings: SoulseekSettings) {
        if let Some(c) = credentials::load(&self.target) {
            let me = self.clone();
            std::thread::spawn(move || {
                let _ = me.sign_in(c.username, c.password, true, settings);
            });
        }
    }

    /// Connects and logs in. Blocks for up to the server's login timeout.
    pub fn sign_in(self: &Arc<Self>, username: String, password: String, remember: bool, settings: SoulseekSettings) -> Result<Status, String> {
        let username = username.trim().to_string();
        if username.is_empty() || password.is_empty() {
            return Err("Enter your Soulseek username and password.".into());
        }
        let generation = {
            let mut c = self.conn.lock();
            if c.state == ConnState::Connecting {
                return Err("Already signing in.".into());
            }
            c.client = None; // drops (and disconnects) any previous session
            c.state = ConnState::Connecting;
            c.username = Some(username.clone());
            c.error = None;
            c.generation += 1;
            c.generation
        };
        self.emit_status();

        let result = (|| -> Result<Client, String> {
            let mut client = Client::with_settings(ClientSettings {
                username: username.clone(),
                password: password.clone(),
                enable_listen: settings.listen,
                listen_port: settings.listen_port,
                ..ClientSettings::default()
            });
            client.connect().map_err(|e| format!("Could not reach the Soulseek server: {e}"))?;
            match client.login() {
                Ok(true) => Ok(client),
                Ok(false) => Err("Soulseek rejected the sign-in. Check the username and password.".into()),
                Err(e) => Err(format!("Sign-in failed: {e}")),
            }
        })();

        let mut c = self.conn.lock();
        if c.generation != generation {
            return Err("Signed out.".into());
        }
        match result {
            Ok(client) => {
                c.client = Some(Arc::new(client));
                c.state = ConnState::SignedIn;
                drop(c);
                if remember {
                    if let Err(e) = credentials::save(&self.target, &Credentials { username, password }) {
                        log::warn!("{e}");
                    }
                } else {
                    credentials::delete(&self.target);
                }
                self.watch_session(generation);
                self.emit_status();
                Ok(self.status())
            }
            Err(e) => {
                c.state = ConnState::SignedOut;
                c.error = Some(e.clone());
                drop(c);
                self.emit_status();
                Err(e)
            }
        }
    }

    /// Notices when the server drops us (network gone, or the account signed in elsewhere).
    fn watch_session(self: &Arc<Self>, generation: u64) {
        let me = self.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(2));
            let mut c = me.conn.lock();
            if c.generation != generation {
                return;
            }
            let Some(loss) = c.client.as_ref().and_then(|cl| cl.session_loss()) else { continue };
            c.client = None;
            c.state = ConnState::SignedOut;
            c.error = Some(format!("Disconnected: {loss}."));
            drop(c);
            me.emit_status();
            return;
        });
    }

    pub fn sign_out(&self, forget: bool) {
        self.cancel_search();
        let client = {
            let mut c = self.conn.lock();
            c.generation += 1;
            c.state = ConnState::SignedOut;
            c.error = None;
            c.client.take()
        };
        if let Some(client) = client {
            let active: Vec<(String, String)> = self
                .jobs
                .lock()
                .iter()
                .filter(|j| j.state.can_cancel())
                .map(|j| (j.username.clone(), j.remote_path.clone()))
                .collect();
            for (user, path) in active {
                let _ = client.cancel_download(&user, &path);
            }
            // Disconnecting can take a moment (the listener is woken up); keep the UI responsive.
            std::thread::spawn(move || drop(client));
        }
        if forget {
            credentials::delete(&self.target);
            self.conn.lock().username = None;
        }
        self.emit_status();
    }

    // ---- search -------------------------------------------------------------------------------

    fn cancel_search(&self) {
        if let Some(flag) = self.search_cancel.lock().take() {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Starts a search and returns its id. Results stream in as `slsk:search` events, grouped
    /// by user and folder; the last one has `done: true`. A new search stops the previous one.
    pub fn search(&self, query: String) -> Result<u64, String> {
        let query = query.split_whitespace().collect::<Vec<_>>().join(" ");
        if query.chars().count() < 3 {
            return Err("Type at least 3 characters.".into());
        }
        let client = self.client()?;
        self.cancel_search();
        let id = self.search_id.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = Arc::new(AtomicBool::new(false));
        *self.search_cancel.lock() = Some(cancel.clone());

        let finished = Arc::new(AtomicBool::new(false));
        {
            let (client, query, cancel, finished) = (client.clone(), query.clone(), cancel.clone(), finished.clone());
            std::thread::spawn(move || {
                if let Err(e) = client.search_with_cancel(&query, SEARCH_WINDOW, Some(cancel)) {
                    log::warn!("soulseek search failed: {e}");
                }
                finished.store(true, Ordering::Relaxed);
            });
        }
        let app = self.app.clone();
        std::thread::spawn(move || {
            let mut last_count = usize::MAX;
            loop {
                std::thread::sleep(Duration::from_millis(800));
                let done = finished.load(Ordering::Relaxed);
                if cancel.load(Ordering::Relaxed) && !done {
                    continue; // the search thread stops shortly; report once it has
                }
                let results = client.get_search_results(&query);
                let count: usize = results.iter().map(|r| r.files.len()).sum();
                if count != last_count || done {
                    last_count = count;
                    let users = model::group_results(results.into_iter().map(raw_response).collect());
                    let file_count = users.iter().flat_map(|u| &u.folders).map(|f| f.files.len()).sum();
                    let _ = app.emit("slsk:search", SearchUpdate { id, query: query.clone(), users, file_count, done });
                }
                if done {
                    return;
                }
            }
        });
        Ok(id)
    }

    /// Runs one search to completion (`window`) and returns the answers, without touching the
    /// search shown on the Sources page. Used to find songs missing from imported playlists.
    pub fn search_collect(&self, query: &str, window: Duration, cancel: Option<Arc<AtomicBool>>) -> Result<Vec<RawResponse>, String> {
        let client = self.client()?;
        client.search_with_cancel(query, window, cancel).map_err(|e| format!("Soulseek search failed: {e}"))?;
        Ok(client.get_search_results(query).into_iter().map(raw_response).collect())
    }

    /// What to do when a download linked to a playlist entry finishes.
    pub fn set_on_linked_done(&self, f: OnLinkedDone) {
        *self.on_linked_done.lock() = Some(f);
    }

    pub fn stop_search(&self) {
        self.cancel_search();
    }

    // ---- downloads ----------------------------------------------------------------------------

    pub fn jobs(&self) -> Vec<Job> {
        self.jobs.lock().clone()
    }

    fn emit_jobs(&self) {
        let _ = self.app.emit("slsk:downloads", self.jobs());
    }

    fn update_job(&self, id: u64, f: impl FnOnce(&mut Job)) {
        if let Some(j) = self.jobs.lock().iter_mut().find(|j| j.id == id) {
            f(j);
        }
    }

    /// Queues files for download into `folder` (each into a subfolder named after the remote
    /// folder). Files already queued or downloading are skipped.
    pub fn download(self: &Arc<Self>, folder: &Path, items: Vec<DownloadRequest>) -> Result<usize, String> {
        let client = self.client()?;
        let mut started = Vec::new();
        {
            let mut jobs = self.jobs.lock();
            for it in items {
                let (_, name) = model::split_remote(&it.path);
                if !model::is_safe_component(name) || !crate::library::model::is_audio_file(Path::new(name)) {
                    continue;
                }
                let busy = jobs.iter().any(|j| j.username == it.username && j.remote_path == it.path && !j.state.is_finished());
                if busy {
                    continue;
                }
                let dir = match model::local_subfolder(&it.path) {
                    Some(sub) => folder.join(sub),
                    None => folder.to_path_buf(),
                };
                let id = self.next_job.fetch_add(1, Ordering::Relaxed);
                jobs.push(Job {
                    id,
                    username: it.username,
                    remote_path: it.path.clone(),
                    name: name.to_string(),
                    size: it.size,
                    folder: dir.to_string_lossy().into_owned(),
                    state: DownloadState::Queued,
                    link_key: it.link_key,
                    bytes: 0,
                    speed: 0.0,
                    error: None,
                });
                started.push(id);
            }
        }
        let n = started.len();
        for id in started {
            self.start(&client, id);
        }
        self.emit_jobs();
        Ok(n)
    }

    pub fn retry(self: &Arc<Self>, id: u64) -> Result<(), String> {
        let client = self.client()?;
        let job = self.jobs.lock().iter().find(|j| j.id == id).cloned().ok_or("Download not found.")?;
        if !job.state.can_retry() {
            return Ok(());
        }
        let _ = client.remove_download(&job.username, &job.remote_path);
        self.update_job(id, |j| {
            j.state = DownloadState::Queued;
            j.error = None;
            j.speed = 0.0;
        });
        self.start(&client, id);
        self.emit_jobs();
        Ok(())
    }

    pub fn cancel(&self, id: u64) {
        let Some(job) = self.jobs.lock().iter().find(|j| j.id == id).cloned() else { return };
        if !job.state.can_cancel() {
            return;
        }
        if let Some(client) = self.conn.lock().client.clone() {
            if !client.cancel_download(&job.username, &job.remote_path) {
                let _ = client.remove_queued_download(&job.username, &job.remote_path);
            }
        }
        self.update_job(id, |j| {
            j.state = DownloadState::Cancelled;
            j.speed = 0.0;
        });
        self.emit_jobs();
    }

    /// Removes finished entries. Leftover partial files of cancelled or failed ones are deleted.
    pub fn clear_finished(&self) {
        let mut jobs = self.jobs.lock();
        for j in jobs.iter().filter(|j| j.state.can_retry()) {
            let part = Path::new(&j.folder).join(format!("{}.part", j.name));
            let _ = std::fs::remove_file(part);
        }
        jobs.retain(|j| !j.state.is_finished());
        drop(jobs);
        self.emit_jobs();
    }

    fn fail(&self, id: u64, msg: &str) {
        self.update_job(id, |j| {
            j.state = j.state.next(DownloadState::Failed);
            if j.state == DownloadState::Failed {
                j.error = Some(msg.to_string());
            }
            j.speed = 0.0;
        });
    }

    /// Asks the user for the file and follows the transfer on its own thread.
    fn start(self: &Arc<Self>, client: &Arc<Client>, id: u64) {
        let Some(job) = self.jobs.lock().iter().find(|j| j.id == id).cloned() else { return };
        let dest = PathBuf::from(&job.folder).join(&job.name);
        if dest.exists() {
            self.fail(id, "A file with this name is already in the download folder.");
            return;
        }
        let rx = match client.download(job.remote_path.clone(), job.username.clone(), job.size, job.folder.clone()) {
            Ok((_, rx)) => rx,
            Err(e) => {
                self.fail(id, &err(e));
                return;
            }
        };
        let me = self.clone();
        // The thread only holds the receiver, so signing out (dropping the client) ends it.
        std::thread::spawn(move || {
            let mut last_emit = Instant::now();
            loop {
                let status = match rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(s) => s,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => {
                        me.fail(id, "The connection to Soulseek closed.");
                        me.emit_jobs();
                        return;
                    }
                };
                let terminal = status.is_terminal();
                me.update_job(id, |j| apply_status(j, status));
                if terminal {
                    let done = me.jobs.lock().iter().find(|j| j.id == id && j.state == DownloadState::Done).map(|j| j.link_key.clone());
                    if let Some(link) = done {
                        set_read_only(&dest);
                        let hook = me.on_linked_done.lock().clone();
                        if let (Some(key), Some(hook)) = (link, hook) {
                            hook(key, dest.clone());
                        }
                    }
                    me.emit_jobs();
                    return;
                }
                if last_emit.elapsed() >= Duration::from_millis(250) {
                    last_emit = Instant::now();
                    me.emit_jobs();
                }
            }
        });
    }
}

fn apply_status(j: &mut Job, status: DownloadStatus) {
    let reported = match status {
        DownloadStatus::Queued => DownloadState::Queued,
        DownloadStatus::InProgress { bytes_downloaded, total_bytes, speed_bytes_per_sec } => {
            if !j.state.is_finished() {
                j.bytes = bytes_downloaded;
                if total_bytes > 0 {
                    j.size = total_bytes;
                }
                j.speed = speed_bytes_per_sec;
            }
            DownloadState::Downloading
        }
        DownloadStatus::Paused { bytes_downloaded, .. } => {
            if !j.state.is_finished() {
                j.bytes = bytes_downloaded;
            }
            DownloadState::Queued
        }
        DownloadStatus::Completed => DownloadState::Done,
        DownloadStatus::Cancelled => DownloadState::Cancelled,
        DownloadStatus::Failed(reason) => {
            if !j.state.is_finished() {
                j.error = Some(reason.unwrap_or_else(|| "The download failed.".into()));
            }
            DownloadState::Failed
        }
        DownloadStatus::TimedOut => {
            if !j.state.is_finished() {
                j.error = Some("The user didn't start sending in time.".into());
            }
            DownloadState::Failed
        }
    };
    j.state = j.state.next(reported);
    if j.state == DownloadState::Done {
        j.bytes = j.size;
    }
    if j.state != DownloadState::Downloading {
        j.speed = 0.0;
    }
}

/// Downloaded music is treated like the rest of the library: the app never changes it.
fn set_read_only(path: &Path) {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let mut perms = meta.permissions();
            perms.set_readonly(true);
            if let Err(e) = std::fs::set_permissions(path, perms) {
                log::warn!("could not mark {} read-only: {e}", path.display());
            }
        }
        Err(e) => log::warn!("downloaded file missing at {}: {e}", path.display()),
    }
}

fn raw_response(r: SearchResult) -> RawResponse {
    RawResponse {
        username: r.username,
        free_slots: r.slots > 0,
        speed: r.speed,
        files: r.files.into_iter().map(|f| RawFile { path: f.name, size: f.size, attrs: f.attribs }).collect(),
    }
}

/// The download folder to use: the setting, or `Soulseek` inside the first library folder.
pub fn download_folder(setting: Option<&str>, library_folders: &[String]) -> Option<PathBuf> {
    match setting.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Some(PathBuf::from(s)),
        None => library_folders.first().map(|f| Path::new(f).join("Soulseek")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> Job {
        Job {
            id: 1,
            username: "u".into(),
            remote_path: r"u\A\1.flac".into(),
            name: "1.flac".into(),
            size: 100,
            folder: "x".into(),
            state: DownloadState::Queued,
            link_key: None,
            bytes: 0,
            speed: 0.0,
            error: None,
        }
    }

    #[test]
    fn progress_then_done() {
        let mut j = job();
        apply_status(&mut j, DownloadStatus::InProgress { bytes_downloaded: 40, total_bytes: 100, speed_bytes_per_sec: 10.0 });
        assert_eq!((j.state, j.bytes, j.speed), (DownloadState::Downloading, 40, 10.0));
        apply_status(&mut j, DownloadStatus::Completed);
        assert_eq!((j.state, j.bytes, j.speed), (DownloadState::Done, 100, 0.0));
    }

    #[test]
    fn late_reports_after_cancel_are_ignored() {
        let mut j = job();
        j.state = DownloadState::Cancelled;
        apply_status(&mut j, DownloadStatus::InProgress { bytes_downloaded: 90, total_bytes: 100, speed_bytes_per_sec: 5.0 });
        apply_status(&mut j, DownloadStatus::Failed(Some("late".into())));
        assert_eq!(j.state, DownloadState::Cancelled);
        assert_eq!(j.bytes, 0);
        assert_eq!(j.error, None);
    }

    #[test]
    fn failures_keep_a_reason() {
        let mut j = job();
        apply_status(&mut j, DownloadStatus::Failed(None));
        assert_eq!(j.state, DownloadState::Failed);
        assert!(j.error.is_some());
        let mut j = job();
        apply_status(&mut j, DownloadStatus::TimedOut);
        assert_eq!(j.state, DownloadState::Failed);
    }

    #[test]
    fn default_download_folder() {
        let libs = vec![r"D:\Music".to_string(), r"E:\More".to_string()];
        assert_eq!(download_folder(None, &libs), Some(PathBuf::from(r"D:\Music\Soulseek")));
        assert_eq!(download_folder(Some("  "), &libs), Some(PathBuf::from(r"D:\Music\Soulseek")));
        assert_eq!(download_folder(Some(r"F:\In"), &libs), Some(PathBuf::from(r"F:\In")));
        assert_eq!(download_folder(None, &[]), None);
    }
}
