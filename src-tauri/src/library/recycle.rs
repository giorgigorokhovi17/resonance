//! "Delete from disk": songs go to the Windows Recycle Bin, never straight to a hard delete, and
//! can be put back from there (Settings → Recently deleted).
//!
//! Windows deletes a file for good, without asking, when its drive has no Recycle Bin (USB sticks,
//! memory cards, network shares), when the bin is turned off for the drive, or when the file is
//! bigger than the bin. Those cases are refused up front instead.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use trash::TrashItem;

/// Checks the file can go to the Recycle Bin (rather than being deleted for good).
pub fn check_can_recycle(path: &Path, size: u64) -> Result<(), String> {
    let drive = win::drive(path).ok_or("Can't tell which drive this song is on.")?;
    if !drive.fixed {
        return Err("This drive has no Recycle Bin, so Undertone won't delete from it.".into());
    }
    if drive.nuke_on_delete {
        return Err("The Recycle Bin is turned off for this drive, so Undertone won't delete from it.".into());
    }
    if drive.max_capacity_mb.is_some_and(|mb| size > mb.saturating_mul(1024 * 1024)) {
        return Err("This song is bigger than the Recycle Bin, so Undertone won't delete it.".into());
    }
    Ok(())
}

/// Sends a file to the Recycle Bin. Clears the read-only mark first (downloads have it) and
/// returns whether it was set, so a restore can put it back.
pub fn send_to_bin(path: &Path) -> Result<bool, String> {
    let meta = std::fs::metadata(path).map_err(|_| "The file can no longer be found.".to_string())?;
    check_can_recycle(path, meta.len())?;
    let mut perms = meta.permissions();
    let was_readonly = perms.readonly();
    if was_readonly {
        perms.set_readonly(false);
        std::fs::set_permissions(path, perms).map_err(|e| format!("Can't clear the file's read-only mark: {e}"))?;
    }
    if let Err(e) = trash::delete(path) {
        if was_readonly {
            set_readonly(path);
        }
        log::warn!("recycle {} failed: {e}", path.display());
        return Err("Windows couldn't move the file to the Recycle Bin. It may be open in another app.".into());
    }
    Ok(was_readonly)
}

/// Puts a deleted file back where it was, from the Recycle Bin.
pub fn restore(path: &Path, deleted_at: i64, was_readonly: bool) -> Result<(), String> {
    if path.exists() {
        return Err("A file with the same name is already in that folder.".into());
    }
    let items = trash::os_limited::list().map_err(|e| format!("Can't read the Recycle Bin: {e}"))?;
    let Some(item) = pick_item(items, path, deleted_at) else {
        return Err("It's no longer in the Recycle Bin.".into());
    };
    let parent = path.parent().ok_or("That folder can't be used.")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("Can't recreate the folder: {e}"))?;
    trash::os_limited::restore_all([item]).map_err(|e| match e {
        trash::Error::RestoreCollision { .. } => "A file with the same name is already in that folder.".to_string(),
        e => {
            log::warn!("restore {} failed: {e}", path.display());
            "Windows couldn't restore the file from the Recycle Bin.".to_string()
        }
    })?;
    if was_readonly {
        set_readonly(path);
    }
    Ok(())
}

/// Which of `paths` are still in the Recycle Bin (by their place in `paths`).
pub fn in_bin(paths: &[(PathBuf, i64)]) -> Vec<bool> {
    let Ok(items) = trash::os_limited::list() else { return vec![true; paths.len()] };
    paths.iter().map(|(p, at)| pick_item(items.clone(), p, *at).is_some()).collect()
}

fn set_readonly(path: &Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_readonly(true);
        let _ = std::fs::set_permissions(path, perms);
    }
}

/// The bin item that is the file deleted from `path` around `deleted_at`: same folder and name
/// (the bin may show names without their extension), the one deleted closest to that time. The
/// result carries the file's real name, so restoring brings the extension back too.
fn pick_item(items: Vec<TrashItem>, path: &Path, deleted_at: i64) -> Option<TrashItem> {
    let (parent, name) = (path.parent()?, path.file_name()?);
    let stem = path.file_stem().unwrap_or(name);
    let same = |a: &OsStr, b: &OsStr| a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase();
    items
        .into_iter()
        .filter(|i| same(i.original_parent.as_os_str(), parent.as_os_str()) && (same(&i.name, name) || same(&i.name, stem)))
        .min_by_key(|i| (i.time_deleted - deleted_at).abs())
        .map(|i| TrashItem { name: name.to_owned(), ..i })
}

mod win {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    pub struct Drive {
        pub fixed: bool,
        pub nuke_on_delete: bool,
        pub max_capacity_mb: Option<u64>,
    }

    const DRIVE_FIXED: u32 = 3;
    const BIT_BUCKET: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket";

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(Some(0)).collect()
    }

    fn from_wide(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    fn dword(key: &str, value: &str) -> Option<u32> {
        let (key, value) = (wide(key.as_ref()), wide(value.as_ref()));
        let mut data = 0u32;
        let mut len = std::mem::size_of::<u32>() as u32;
        let res = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(value.as_ptr()),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut data as *mut u32 as *mut _),
                Some(&mut len),
            )
        };
        res.is_ok().then_some(data)
    }

    /// The drive (volume) a file is on, and its Recycle Bin settings for this user.
    pub fn drive(path: &Path) -> Option<Drive> {
        let file = wide(path.as_os_str());
        let mut root = [0u16; 1024];
        unsafe { GetVolumePathNameW(PCWSTR(file.as_ptr()), &mut root) }.ok()?;
        let fixed = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } == DRIVE_FIXED;
        // Per-drive settings live under the volume's GUID ("\\?\Volume{…}\").
        let mut volume = [0u16; 128];
        let guid = unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(root.as_ptr()), &mut volume) }
            .ok()
            .and_then(|_| {
                let v = from_wide(&volume);
                Some(v[v.find('{')?..=v.find('}')?].to_string())
            });
        let per_drive = guid.map(|g| format!(r"{BIT_BUCKET}\Volume\{g}"));
        let read = |value: &str| per_drive.as_deref().and_then(|k| dword(k, value)).or_else(|| dword(BIT_BUCKET, value));
        Some(Drive {
            fixed,
            nuke_on_delete: read("NukeOnDelete") == Some(1),
            max_capacity_mb: read("MaxCapacity").map(u64::from),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(parent: &str, name: &str, at: i64) -> TrashItem {
        TrashItem { id: format!("{parent}\\{name}@{at}").into(), name: name.into(), original_parent: parent.into(), time_deleted: at }
    }

    #[test]
    fn picks_the_deleted_file_from_the_bin() {
        let path = Path::new(r"C:\Music\Artist\Song.flac");
        let items = vec![
            item(r"C:\Music\Other", "Song.flac", 100),
            item(r"C:\Music\Artist", "Song.mp3", 100),
            item(r"c:\music\artist", "Song", 40),
            item(r"C:\Music\Artist", "Song.flac", 95),
            item(r"C:\Music\Artist", "Song.flac", 10),
        ];
        let got = pick_item(items, path, 100).unwrap();
        assert_eq!(got.time_deleted, 95, "same folder and name, deleted closest to our time");
        assert_eq!(got.original_path(), path);
        // A name shown without its extension still matches, and gets it back for the restore.
        let got = pick_item(vec![item(r"C:\MUSIC\Artist", "song", 50)], path, 100).unwrap();
        assert_eq!(got.name, OsStr::new("Song.flac"));
        assert!(pick_item(vec![item(r"C:\Music", "Song.flac", 100)], path, 100).is_none());
        assert!(pick_item(Vec::new(), path, 100).is_none());
    }

    /// Uses this PC's real Recycle Bin, so it only runs when asked:
    /// `cargo test recycle_round_trip -- --ignored`
    #[test]
    #[ignore]
    fn recycle_round_trip() {
        let dir = std::env::temp_dir().join(format!("undertone-recycle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Round Trip.flac");
        std::fs::write(&file, b"not really audio").unwrap();
        let mut perms = std::fs::metadata(&file).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&file, perms).unwrap();

        let at = crate::library::db::now();
        assert!(send_to_bin(&file).unwrap(), "the read-only mark is reported");
        assert!(!file.exists());
        assert_eq!(in_bin(&[(file.clone(), at)]), vec![true]);
        restore(&file, at, true).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"not really audio");
        assert!(std::fs::metadata(&file).unwrap().permissions().readonly(), "read-only again");
        assert_eq!(in_bin(&[(file.clone(), at)]), vec![false]);
        assert!(restore(&file, at, false).is_err(), "nothing left to restore");

        let mut perms = std::fs::metadata(&file).unwrap().permissions();
        perms.set_readonly(false);
        std::fs::set_permissions(&file, perms).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
