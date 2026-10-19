//! Incremental, parallel folder scanning.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, UNIX_EPOCH};

use anyhow::Result;
use parking_lot::Mutex;
use rayon::prelude::*;
use serde::Serialize;
use walkdir::WalkDir;

use super::covers::{find_folder_image, CoverCache};
use super::db::{self, Folder};
use super::model::{is_audio_file, Track};
use super::tags::read_track;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub phase: &'static str,
    pub done: usize,
    pub total: usize,
    pub folder: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub total: usize,
    pub errors: usize,
    pub elapsed_ms: u64,
}

impl ScanSummary {
    pub fn merge(&mut self, o: &ScanSummary) {
        self.added += o.added;
        self.updated += o.updated;
        self.removed += o.removed;
        self.total += o.total;
        self.errors += o.errors;
        self.elapsed_ms += o.elapsed_ms;
    }
    pub fn changed(&self) -> bool {
        self.added + self.updated + self.removed > 0
    }
}

struct FileStat {
    path: String,
    size: u64,
    mtime: i64,
}

fn dir_of(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Scan one library root. Only new or modified files (size/mtime) are re-read unless `force`.
pub fn scan_folder(
    conn: &Mutex<rusqlite::Connection>,
    covers: &CoverCache,
    folder: &Folder,
    force: bool,
    progress: &(dyn Fn(ScanProgress) + Sync),
) -> Result<ScanSummary> {
    let t0 = Instant::now();
    let root = Path::new(&folder.path);
    let mut summary = ScanSummary::default();
    if !root.is_dir() {
        // Folder unavailable (e.g. unplugged drive): keep its tracks rather than wiping them.
        log::warn!("library folder not reachable: {}", folder.path);
        return Ok(summary);
    }
    progress(ScanProgress { phase: "walking", done: 0, total: 0, folder: folder.path.clone() });

    // 1. Walk the tree (stat only).
    let files: Vec<FileStat> = WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && is_audio_file(e.path()))
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            let mtime = md
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            Some(FileStat { path: e.path().to_string_lossy().to_string(), size: md.len(), mtime })
        })
        .collect();
    summary.total = files.len();

    let existing = db::existing_for_folder(&conn.lock(), folder.id)?;
    let seen: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let removed: Vec<i64> = existing
        .iter()
        .filter(|(p, _)| !seen.contains(p.as_str()))
        .map(|(_, e)| e.id)
        .collect();
    let to_read: Vec<&FileStat> = files
        .iter()
        .filter(|f| match existing.get(&f.path) {
            // Also retry files that previously yielded no duration (unreadable tags).
            Some(e) => force || e.size != f.size || e.mtime != f.mtime || e.duration <= 0.0,
            None => true,
        })
        .collect();

    // 2. Read tags in parallel.
    let total = to_read.len();
    let done = AtomicUsize::new(0);
    let last_emit = Mutex::new(Instant::now());
    let errors = AtomicUsize::new(0);
    let mut read: Vec<(Track, String)> = to_read
        .par_iter()
        .filter_map(|f| {
            let res = read_track(Path::new(&f.path), false);
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(mut le) = last_emit.try_lock() {
                if le.elapsed().as_millis() > 120 {
                    *le = Instant::now();
                    progress(ScanProgress { phase: "reading", done: n, total, folder: folder.path.clone() });
                }
            }
            match res {
                Ok((t, _)) => {
                    let dir = dir_of(&t.path);
                    Some((t, dir))
                }
                Err(e) => {
                    log::warn!("cannot read {}: {e}", f.path);
                    errors.fetch_add(1, Ordering::Relaxed);
                    None
                }
            }
        })
        .collect();
    summary.errors = errors.load(Ordering::Relaxed);

    // 3. Resolve cover art once per (directory, album) group.
    let mut known_covers: HashMap<(String, String), String> = HashMap::new();
    for e in existing.values() {
        if let Some(c) = &e.cover_id {
            known_covers.entry((e.dir.clone(), e.album.to_lowercase())).or_insert_with(|| c.clone());
        }
    }
    let mut groups: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (i, (t, dir)) in read.iter().enumerate() {
        groups.entry((dir.clone(), t.album.to_lowercase())).or_default().push(i);
    }
    let group_list: Vec<((String, String), Vec<usize>)> = groups.into_iter().collect();
    let gtotal = group_list.len();
    let gdone = AtomicUsize::new(0);
    let resolved: Vec<(Vec<usize>, Option<String>)> = group_list
        .par_iter()
        .map(|(key, idxs)| {
            let n = gdone.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(mut le) = last_emit.try_lock() {
                if le.elapsed().as_millis() > 120 {
                    *le = Instant::now();
                    progress(ScanProgress { phase: "artwork", done: n, total: gtotal, folder: folder.path.clone() });
                }
            }
            // Re-use art already resolved for this album when its files didn't all change.
            if !force {
                if let Some(c) = known_covers.get(key) {
                    if covers.file(c, false).is_some() {
                        return (idxs.clone(), Some(c.clone()));
                    }
                }
            }
            let mut cover = None;
            for &i in idxs.iter().take(3) {
                if let Ok((_, Some(pic))) = read_track(Path::new(&read[i].0.path), true) {
                    cover = covers.store(&pic.data);
                    if cover.is_some() {
                        break;
                    }
                }
            }
            if cover.is_none() {
                if let Some(img) = find_folder_image(Path::new(&key.0)) {
                    if let Ok(bytes) = std::fs::read(&img) {
                        cover = covers.store(&bytes);
                    }
                }
            }
            (idxs.clone(), cover)
        })
        .collect();
    for (idxs, cover) in resolved {
        for i in idxs {
            read[i].0.cover_id = cover.clone();
        }
    }

    // 4. Persist.
    progress(ScanProgress { phase: "saving", done: 0, total: read.len(), folder: folder.path.clone() });
    for (t, _) in &read {
        if existing.contains_key(&t.path) {
            summary.updated += 1;
        } else {
            summary.added += 1;
        }
    }
    {
        let mut c = conn.lock();
        for chunk in read.chunks(2000) {
            db::upsert_tracks(&mut c, folder.id, chunk)?;
        }
        if !removed.is_empty() {
            db::delete_tracks(&mut c, &removed)?;
        }
    }
    summary.removed = removed.len();
    summary.elapsed_ms = t0.elapsed().as_millis() as u64;
    log::info!(
        "scanned {}: {} files, +{} ~{} -{} ({} ms)",
        folder.path,
        summary.total,
        summary.added,
        summary.updated,
        summary.removed,
        summary.elapsed_ms
    );
    Ok(summary)
}
