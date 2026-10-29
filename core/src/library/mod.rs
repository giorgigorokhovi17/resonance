pub mod covers;
pub mod csv;
pub mod db;
pub mod interchange;
pub mod lyrics;
pub mod m3u;
pub mod matcher;
pub mod media;
pub mod model;
pub mod pls;
pub mod recommend;
pub mod recycle;
pub mod scan;
pub mod stats;
pub mod tags;
pub mod wpl;
pub mod xspf;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use parking_lot::Mutex;
use rusqlite::Connection;

use covers::CoverCache;
use db::Folder;
use media::MediaStore;
use model::Track;
use scan::{ScanProgress, ScanSummary};

pub enum LibraryEvent {
    Progress(ScanProgress),
    ScanStarted,
    Finished(ScanSummary),
}

pub type EventSink = Arc<dyn Fn(LibraryEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum ScanRequest {
    All { force: bool },
    Folder(i64),
}

pub struct Library {
    pub db: Mutex<Connection>,
    pub covers: CoverCache,
    /// Custom GIFs, videos and background photos (by content hash).
    pub media: MediaStore,
    scan_tx: Sender<ScanRequest>,
    scanning: AtomicBool,
    watcher: Mutex<Option<Debouncer<notify::RecommendedWatcher, RecommendedCache>>>,
}

impl Library {
    pub fn open(db_path: &Path, cover_dir: PathBuf, media_dir: PathBuf, events: EventSink) -> Result<Arc<Self>> {
        let conn = db::open(db_path)?;
        let (scan_tx, scan_rx) = unbounded();
        let lib = Arc::new(Library {
            db: Mutex::new(conn),
            covers: CoverCache::new(cover_dir),
            media: MediaStore::new(media_dir),
            scan_tx,
            scanning: AtomicBool::new(false),
            watcher: Mutex::new(None),
        });
        let worker_lib = lib.clone();
        std::thread::Builder::new()
            .name("library-scanner".into())
            .spawn(move || worker_lib.scan_worker(scan_rx, events))?;
        Ok(lib)
    }

    pub fn is_scanning(&self) -> bool {
        self.scanning.load(Ordering::Relaxed)
    }

    pub fn request_scan(&self, req: ScanRequest) {
        let _ = self.scan_tx.send(req);
    }

    pub fn folders(&self) -> Result<Vec<Folder>> {
        db::folders(&self.db.lock())
    }

    pub fn tracks(&self) -> Result<Vec<Track>> {
        db::all_tracks(&self.db.lock())
    }

    pub fn tracks_by_ids(&self, ids: &[i64]) -> Result<Vec<Track>> {
        db::tracks_by_ids(&self.db.lock(), ids)
    }

    /// Resolve file paths to tracks: library rows when known, otherwise tags read from disk.
    /// Missing files are returned as placeholders with `missing = true`.
    pub fn resolve_paths(&self, paths: &[String]) -> Vec<(Track, bool)> {
        let known: Vec<Option<Track>> = {
            let conn = self.db.lock();
            paths.iter().map(|p| db::track_by_path(&conn, p).ok().flatten()).collect()
        };
        paths
            .iter()
            .zip(known)
            .map(|(p, k)| match k {
                Some(t) => (t, false),
                None => {
                    let path = Path::new(p);
                    match tags::read_track(path, false) {
                        Ok((t, _)) if path.is_file() => (t, false),
                        _ => (
                            Track {
                                path: p.clone(),
                                title: path
                                    .file_stem()
                                    .map(|s| s.to_string_lossy().to_string())
                                    .unwrap_or_else(|| p.clone()),
                                ..Default::default()
                            },
                            true,
                        ),
                    }
                }
            })
            .collect()
    }

    /// Deletes custom media that nothing uses any more: database rows, and files without a row
    /// (left by an import that was refused or interrupted). `also_keep` holds ids used outside the
    /// database (the app background). Returns how many files were deleted.
    pub fn sweep_media(&self, also_keep: &HashSet<String>) -> Result<usize> {
        let conn = self.db.lock();
        let mut keep = db::prune_media(&conn, also_keep)?;
        // A row whose file is gone can't be shown; things that used it fall back to the poster.
        keep.retain(|id| {
            let present = self.media.file(id).is_some();
            if !present {
                let _ = db::delete_media(&conn, id);
            }
            present
        });
        Ok(self.media.sweep(&keep))
    }

    pub fn add_folder(self: &Arc<Self>, path: &str) -> Result<i64> {
        let id = db::add_folder(&self.db.lock(), path)?;
        self.request_scan(ScanRequest::Folder(id));
        Ok(id)
    }

    pub fn remove_folder(&self, id: i64) -> Result<()> {
        db::remove_folder(&self.db.lock(), id)
    }

    fn scan_worker(self: Arc<Self>, rx: Receiver<ScanRequest>, events: EventSink) {
        while let Ok(first) = rx.recv() {
            // Coalesce bursts (e.g. many watcher events) into one pass.
            std::thread::sleep(Duration::from_millis(150));
            let mut reqs = vec![first];
            reqs.extend(rx.try_iter());
            let force = reqs.iter().any(|r| matches!(r, ScanRequest::All { force: true }));
            let all = reqs.iter().any(|r| matches!(r, ScanRequest::All { .. }));
            let ids: Vec<i64> = reqs
                .iter()
                .filter_map(|r| if let ScanRequest::Folder(id) = r { Some(*id) } else { None })
                .collect();
            let folders = match self.folders() {
                Ok(f) => f,
                Err(e) => {
                    log::error!("cannot list folders: {e}");
                    continue;
                }
            };
            let targets: Vec<Folder> = folders.into_iter().filter(|f| all || ids.contains(&f.id)).collect();
            if targets.is_empty() {
                continue;
            }
            self.scanning.store(true, Ordering::Relaxed);
            events(LibraryEvent::ScanStarted);
            let mut total = ScanSummary::default();
            let progress = |p: ScanProgress| events(LibraryEvent::Progress(p));
            for folder in &targets {
                match scan::scan_folder(&self.db, &self.covers, folder, force, &progress) {
                    Ok(s) => total.merge(&s),
                    Err(e) => {
                        log::error!("scan of {} failed: {e}", folder.path);
                        total.errors += 1;
                    }
                }
            }
            self.scanning.store(false, Ordering::Relaxed);
            events(LibraryEvent::Finished(total));
        }
    }

    /// (Re)start watching all library folders; changes trigger an incremental rescan.
    pub fn set_watching(self: &Arc<Self>, enabled: bool) {
        let mut guard = self.watcher.lock();
        *guard = None;
        if !enabled {
            return;
        }
        let folders = match self.folders() {
            Ok(f) => f,
            Err(_) => return,
        };
        if folders.is_empty() {
            return;
        }
        let roots: Vec<(i64, PathBuf)> = folders.iter().map(|f| (f.id, PathBuf::from(&f.path))).collect();
        let tx = self.scan_tx.clone();
        let handler = move |res: DebounceEventResult| {
            let Ok(events) = res else { return };
            let mut hit: Vec<i64> = Vec::new();
            for ev in events {
                // inotify (Linux, Android) also reports reads; a scan reads every file, so reacting
                // to them would rescan forever. Windows never reports reads.
                if ev.kind.is_access() {
                    continue;
                }
                for p in &ev.paths {
                    let relevant = model::is_audio_file(p)
                        || p.extension().is_none()
                        || p
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png"))
                            .unwrap_or(false);
                    if !relevant {
                        continue;
                    }
                    if let Some((id, _)) = roots.iter().find(|(_, r)| p.starts_with(r)) {
                        if !hit.contains(id) {
                            hit.push(*id);
                        }
                    }
                }
            }
            for id in hit {
                let _ = tx.send(ScanRequest::Folder(id));
            }
        };
        match new_debouncer(Duration::from_secs(2), None, handler) {
            Ok(mut d) => {
                for f in &folders {
                    if let Err(e) = d.watch(Path::new(&f.path), RecursiveMode::Recursive) {
                        log::warn!("cannot watch {}: {e}", f.path);
                    }
                }
                *guard = Some(d);
            }
            Err(e) => log::warn!("file watcher unavailable: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_sweep_removes_orphaned_media() {
        let dir = std::env::temp_dir().join(format!("resonance-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let lib = Library::open(&dir.join("library.db"), dir.join("covers"), dir.join("media"), Arc::new(|_| {})).unwrap();
        let row = |id: &str| db::Media {
            id: id.into(),
            kind: media::MediaKind::Video,
            mime: "video/mp4".into(),
            width: 640,
            height: 360,
            duration_ms: Some(4000),
            bytes: 10,
            poster_id: None,
        };
        // Used by a playlist, used by the app background, unused, and a file never committed.
        let used = lib.media.put(b"playlist background", "mp4").unwrap();
        let skin = lib.media.put(b"app background", "mp4").unwrap();
        let unused = lib.media.put(b"removed background", "mp4").unwrap();
        let staged = lib.media.put(b"refused import", "mp4").unwrap();
        {
            let conn = lib.db.lock();
            for id in [&used, &skin, &unused] {
                db::put_media(&conn, &row(id)).unwrap();
            }
            // A row whose file was deleted by hand.
            db::put_media(&conn, &row("abcdef")).unwrap();
            let p = db::create_playlist(&conn, "P").unwrap();
            db::set_playlist_background(&conn, p, Some(&media::Backdrop { media_id: used.clone(), ..Default::default() })).unwrap();
        }
        let keep: HashSet<String> = [skin.clone()].into_iter().collect();
        assert_eq!(lib.sweep_media(&keep).unwrap(), 2, "the unused file and the refused import");
        assert!(lib.media.file(&used).is_some() && lib.media.file(&skin).is_some());
        assert!(lib.media.file(&unused).is_none() && lib.media.file(&staged).is_none());
        let mut left: Vec<String> = db::media(&lib.db.lock()).unwrap().into_iter().map(|m| m.id).collect();
        left.sort();
        let mut want = vec![used, skin];
        want.sort();
        assert_eq!(left, want, "rows for deleted or missing files are gone too");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
