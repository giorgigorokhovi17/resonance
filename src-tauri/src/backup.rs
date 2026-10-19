//! Backup / restore: settings, EQ presets, library folders, playlists and artwork choices in one
//! file, for moving to a new PC. Music files themselves are not included.
//!
//! On restore, paths are remapped when the music lives somewhere else on the new PC (another drive
//! letter, user folder or library root): a missing folder is matched to an existing one under the
//! current library roots by its trailing path components ("…\Artist\Album").

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::commands::EqPreset;
use crate::library::db::{self, TrackArt};
use crate::library::Library;
use crate::settings::Settings;

const FORMAT: &str = "resonance-backup";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    format: String,
    version: u32,
    created_at: i64,
    app_version: String,
    settings: Settings,
    #[serde(default)]
    eq_presets: Vec<EqPreset>,
    #[serde(default)]
    library_folders: Vec<String>,
    #[serde(default)]
    playlists: Vec<BackupPlaylist>,
    #[serde(default)]
    track_art: Vec<TrackArt>,
    /// Artwork images (full-size JPEG, base64) by cover id.
    #[serde(default)]
    artwork: BTreeMap<String, String>,
    /// UI preferences stored by the frontend (sort orders, filters, …), passed through as-is.
    #[serde(default)]
    ui: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupPlaylist {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    cover_id: Option<String>,
    paths: Vec<String>,
    /// Import source, so re-importing the file after a restore still updates this playlist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub playlists: usize,
    pub presets: usize,
    pub artwork: usize,
    pub bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSummary {
    pub playlists: usize,
    pub presets: usize,
    pub artwork: usize,
    pub folders_added: Vec<String>,
    /// Library folders from the backup that don't exist on this PC.
    pub folders_missing: Vec<String>,
    /// Playlist songs found at a different location than on the old PC.
    pub relocated: usize,
    /// Playlist songs that couldn't be found anywhere.
    pub unresolved: usize,
    pub ui: serde_json::Value,
    pub settings: Settings,
}

pub fn export(lib: &Library, settings: Settings, eq_presets: Vec<EqPreset>, ui: serde_json::Value, file: &Path) -> Result<ExportSummary, String> {
    let (library_folders, playlists, track_art) = {
        let conn = lib.db.lock();
        let folders = db::folders(&conn).map_err(err)?.into_iter().map(|f| f.path).collect::<Vec<_>>();
        let mut playlists = Vec::new();
        for p in db::playlists(&conn).map_err(err)? {
            let paths = db::playlist_paths(&conn, p.id).map_err(err)?;
            playlists.push(BackupPlaylist {
                name: p.name,
                description: p.description,
                cover_id: p.cover_id,
                paths,
                source: p.source,
                source_id: p.source_id,
            });
        }
        (folders, playlists, db::track_art(&conn).map_err(err)?)
    };

    let mut artwork = BTreeMap::new();
    let ids = track_art.iter().filter_map(|a| a.cover_id.as_ref()).chain(playlists.iter().filter_map(|p| p.cover_id.as_ref()));
    for id in ids {
        if artwork.contains_key(id) {
            continue;
        }
        if let Some(bytes) = lib.covers.file(id, true).and_then(|f| std::fs::read(f).ok()) {
            artwork.insert(id.clone(), B64.encode(bytes));
        }
    }

    let mut settings = settings;
    settings.device_id = None; // device ids are specific to this PC
    let backup = Backup {
        format: FORMAT.into(),
        version: VERSION,
        created_at: db::now(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        settings,
        eq_presets,
        library_folders,
        playlists,
        track_art,
        artwork,
        ui,
    };
    let summary = (backup.playlists.len(), backup.eq_presets.len(), backup.artwork.len());
    let bytes = serde_json::to_vec(&backup).map_err(err)?;
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("Cannot write {}: {e}", file.display()))?;
    std::fs::rename(&tmp, file).map_err(|e| format!("Cannot write {}: {e}", file.display()))?;
    Ok(ExportSummary { playlists: summary.0, presets: summary.1, artwork: summary.2, bytes: bytes.len() as u64 })
}

pub fn read(file: &Path) -> Result<Backup, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("Cannot read {}: {e}", file.display()))?;
    let b: Backup = serde_json::from_slice(&bytes).map_err(|_| "That file isn't a Resonance backup.".to_string())?;
    if b.format != FORMAT {
        return Err("That file isn't a Resonance backup.".into());
    }
    if b.version > VERSION {
        return Err("This backup was made by a newer version of Resonance. Update the app first.".into());
    }
    Ok(b)
}

/// What `restore` changed; the caller applies settings/presets/folders to the live app.
pub struct Restored {
    pub summary: RestoreSummary,
    pub eq_presets: Vec<EqPreset>,
}

/// Restores library folders, artwork, artwork choices and playlists into the database.
/// `add_folder` adds a library folder (and starts scanning it).
pub fn restore(lib: &Library, b: Backup, mut add_folder: impl FnMut(&str) -> Result<(), String>) -> Result<Restored, String> {
    // 1. Library folders that exist here (and don't overlap a current one).
    let mut roots: Vec<String> = lib.folders().map_err(err)?.into_iter().map(|f| f.path).collect();
    let mut folders_added = Vec::new();
    let mut folders_missing = Vec::new();
    for f in &b.library_folders {
        let p = Path::new(f);
        if roots.iter().any(|r| p.starts_with(r) || Path::new(r).starts_with(p)) {
            continue;
        }
        if p.is_dir() {
            add_folder(f)?;
            roots.push(f.clone());
            folders_added.push(f.clone());
        } else {
            folders_missing.push(f.clone());
        }
    }

    // 2. Artwork images (re-stored, so ids are this PC's).
    let mut cover_map: HashMap<String, String> = HashMap::new();
    for (id, data) in &b.artwork {
        if let Some(new_id) = B64.decode(data).ok().and_then(|bytes| lib.covers.store(&bytes)) {
            cover_map.insert(id.clone(), new_id);
        }
    }

    let mut mapper = PathMapper::new(&b.library_folders, &roots);

    // 3. Per-song artwork choices.
    let art: Vec<TrackArt> = b
        .track_art
        .iter()
        .filter_map(|a| {
            let cover_id = match &a.cover_id {
                Some(c) => Some(cover_map.get(c)?.clone()),
                None => None,
            };
            let path = mapper.map(&a.path).unwrap_or_else(|| a.path.clone());
            Some(TrackArt { path, cover_id, hidden: a.hidden })
        })
        .collect();
    db::put_track_art(&mut lib.db.lock(), &art).map_err(err)?;

    // 4. Playlists: replace ones with the same name, create the rest.
    let (mut relocated, mut unresolved) = (0, 0);
    {
        let mut conn = lib.db.lock();
        let existing = db::playlists(&conn).map_err(err)?;
        for p in &b.playlists {
            let paths: Vec<String> = p
                .paths
                .iter()
                .map(|old| match mapper.map(old) {
                    Some(new) => {
                        if &new != old {
                            relocated += 1;
                        }
                        new
                    }
                    None => {
                        unresolved += 1;
                        old.clone()
                    }
                })
                .collect();
            let id = match existing.iter().find(|e| e.name.eq_ignore_ascii_case(&p.name)) {
                Some(e) => e.id,
                None => db::create_playlist(&conn, &p.name).map_err(err)?,
            };
            db::set_playlist_paths(&mut conn, id, &paths).map_err(err)?;
            db::update_playlist(&conn, id, &p.name, &p.description).map_err(err)?;
            let cover = p.cover_id.as_ref().and_then(|c| cover_map.get(c));
            db::set_playlist_cover(&conn, id, cover.map(|s| s.as_str())).map_err(err)?;
            if let (Some(source), Some(source_id)) = (&p.source, &p.source_id) {
                db::set_playlist_source(&conn, id, source, source_id).map_err(err)?;
            }
        }
    }

    Ok(Restored {
        summary: RestoreSummary {
            playlists: b.playlists.len(),
            presets: b.eq_presets.len(),
            artwork: cover_map.len(),
            folders_added,
            folders_missing,
            relocated,
            unresolved,
            ui: b.ui,
            settings: b.settings,
        },
        eq_presets: b.eq_presets,
    })
}

/// Maps file paths from the old PC to this one.
struct PathMapper {
    old_roots: Vec<PathBuf>,
    new_roots: Vec<PathBuf>,
    /// Old folder -> folder on this PC (None: not found).
    dirs: HashMap<PathBuf, Option<PathBuf>>,
}

impl PathMapper {
    fn new(old_roots: &[String], new_roots: &[String]) -> Self {
        Self {
            old_roots: old_roots.iter().map(PathBuf::from).collect(),
            new_roots: new_roots.iter().map(PathBuf::from).collect(),
            dirs: HashMap::new(),
        }
    }

    /// Lookups are cached per folder, so a backup full of paths on a drive that isn't here (or a
    /// disconnected network share) costs one check per folder, not per file.
    fn map(&mut self, old: &str) -> Option<String> {
        let p = Path::new(old);
        let (dir, name) = (p.parent()?, p.file_name()?);
        let new_dir = match self.dirs.get(dir) {
            Some(d) => d.clone(),
            None => {
                let d = self.find_dir(dir);
                self.dirs.insert(dir.to_path_buf(), d.clone());
                d
            }
        };
        let f = new_dir?.join(name);
        f.is_file().then(|| f.to_string_lossy().to_string())
    }

    fn find_dir(&self, old_dir: &Path) -> Option<PathBuf> {
        if old_dir.is_dir() {
            return Some(old_dir.to_path_buf());
        }
        // Folders below the old library root ("Artist\Album"); otherwise everything after the drive.
        let (rel, tail_len): (Vec<_>, usize) = match self.old_roots.iter().find(|r| old_dir.starts_with(r)) {
            Some(r) => {
                let tail: Vec<_> = old_dir.strip_prefix(r).ok()?.components().collect();
                let n = tail.len();
                // Include the old root's own name so "D:\Music" can match "E:\Stuff\Music".
                (r.file_name().map(std::path::Component::Normal).into_iter().chain(tail).collect(), n)
            }
            None => {
                let all: Vec<_> = old_dir.components().filter(|c| matches!(c, std::path::Component::Normal(_))).collect();
                let n = all.len();
                (all, n)
            }
        };
        // Longest trailing match first. Require two folder names when there are that many, so a
        // generic folder like "Disc 1" can't match the wrong album.
        let min = tail_len.min(2).max(1);
        for k in (min..=rel.len()).rev() {
            for root in &self.new_roots {
                let cand = rel[rel.len() - k..].iter().fold(root.clone(), |acc, c| acc.join(c));
                if cand.is_dir() {
                    return Some(cand);
                }
            }
        }
        // Files that sat directly in the old library root -> the (single) library root here.
        if self.old_roots.iter().any(|r| r == old_dir) && self.new_roots.len() == 1 {
            return Some(self.new_roots[0].clone());
        }
        None
    }
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaps_moved_library() {
        let base = std::env::temp_dir().join(format!("resonance-backup-test-{}", std::process::id()));
        let album = base.join("Stuff").join("Music").join("Artist").join("Album");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(album.join("01 Song.flac"), b"x").unwrap();
        std::fs::create_dir_all(base.join("Stuff").join("Music").join("Other").join("Disc 1")).unwrap();
        std::fs::write(base.join("Stuff").join("Music").join("loose.mp3"), b"x").unwrap();
        let new_music = base.join("Stuff").join("Music").to_string_lossy().to_string();
        let new_stuff = base.join("Stuff").to_string_lossy().to_string();
        let expected = album.join("01 Song.flac").to_string_lossy().to_string();
        // The "old PC" lives in a folder that doesn't exist (no real drive letters: they may be network shares).
        let old = |rest: &str| base.join("OldPC").join(rest).to_string_lossy().to_string();
        let old_root = old("Music");

        // Same root name, different location.
        let mut m = PathMapper::new(&[old_root.clone()], &[new_music.clone()]);
        assert_eq!(m.map(&old(r"Music\Artist\Album\01 Song.flac")), Some(expected.clone()));
        // An existing path stays as it is.
        assert_eq!(m.map(&expected), Some(expected.clone()));
        // Library root chosen one level higher on the new PC.
        let mut m = PathMapper::new(&[old_root.clone()], &[new_stuff]);
        assert_eq!(m.map(&old(r"Music\Artist\Album\01 Song.flac")), Some(expected.clone()));
        // File that wasn't under any backed-up root.
        let mut m = PathMapper::new(&[], &[new_music.clone()]);
        assert_eq!(m.map(&old(r"Users\me\Music\Artist\Album\01 Song.flac")), Some(expected));
        // Files directly in the old root.
        let mut m = PathMapper::new(&[old_root.clone()], &[new_music.clone()]);
        assert!(m.map(&old(r"Music\loose.mp3")).is_some());
        // A generic folder name alone must not match another album's "Disc 1".
        let mut m = PathMapper::new(&[old_root], &[new_music]);
        assert_eq!(m.map(&old(r"Music\Someone\Else\Disc 1\x.flac")), None);
        // Missing entirely.
        assert_eq!(m.map(&old(r"Music\Nope\Never\a.flac")), None);

        let _ = std::fs::remove_dir_all(&base);
    }
}
