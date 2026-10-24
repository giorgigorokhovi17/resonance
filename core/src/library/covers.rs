//! Album art cache: content-addressed JPEG thumbnails + large versions on disk.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};

const THUMB_PX: u32 = 400;
const FULL_PX: u32 = 1400;

pub struct CoverCache {
    dir: PathBuf,
}

/// Image file names (without extension) commonly used for folder art, in priority order.
const FOLDER_ART_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart", "albumartlarge", "art"];
const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif"];

impl CoverCache {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Self { dir }
    }

    pub fn file(&self, id: &str, full: bool) -> Option<PathBuf> {
        // ids are hex only; reject anything else to avoid path tricks.
        if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let p = self.dir.join(if full { format!("{id}_full.jpg") } else { format!("{id}.jpg") });
        if p.exists() {
            Some(p)
        } else if full {
            // Fall back to the thumbnail if the large version is missing.
            let t = self.dir.join(format!("{id}.jpg"));
            t.exists().then_some(t)
        } else {
            None
        }
    }

    /// Store image bytes; returns the cover id. Thumbnails are generated once per unique image.
    pub fn store(&self, data: &[u8]) -> Option<String> {
        if data.len() < 16 {
            return None;
        }
        let id = blake3::hash(data).to_hex()[..24].to_string();
        let thumb = self.dir.join(format!("{id}.jpg"));
        if thumb.exists() {
            return Some(id);
        }
        let img = match image::load_from_memory(data) {
            Ok(i) => i,
            Err(e) => {
                log::debug!("cover decode failed: {e}");
                return None;
            }
        };
        let full_path = self.dir.join(format!("{id}_full.jpg"));
        let is_jpeg = image::guess_format(data).map(|f| f == ImageFormat::Jpeg).unwrap_or(false);
        let full_bytes = if is_jpeg && img.width().max(img.height()) <= FULL_PX && data.len() < 3_000_000 {
            Some(data.to_vec())
        } else {
            encode_jpeg(&img.resize(FULL_PX, FULL_PX, FilterType::Lanczos3), 90)
        };
        if let Some(b) = full_bytes {
            write_atomic(&full_path, &b);
        }
        let t = img.resize(THUMB_PX, THUMB_PX, FilterType::Triangle);
        if let Some(b) = encode_jpeg(&t, 86) {
            write_atomic(&thumb, &b);
            Some(id)
        } else {
            None
        }
    }
}

fn encode_jpeg(img: &DynamicImage, quality: u8) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let rgb = img.to_rgb8();
    JpegEncoder::new_with_quality(&mut Cursor::new(&mut out), quality)
        .encode_image(&rgb)
        .ok()?;
    Some(out)
}

fn write_atomic(path: &Path, data: &[u8]) {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&tmp, data).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Reads a folder's picture when the file system shows none. Android hides pictures from apps
/// that only have the audio permission; the app reads them through the folder picker's grant.
static FOLDER_IMAGE_READER: OnceLock<fn(&Path) -> Option<Vec<u8>>> = OnceLock::new();

pub fn set_folder_image_reader(read: fn(&Path) -> Option<Vec<u8>>) {
    let _ = FOLDER_IMAGE_READER.set(read);
}

/// The bytes of `dir`'s folder art, from the file system or else the platform's reader.
pub fn read_folder_image(dir: &Path) -> Option<Vec<u8>> {
    if let Some(bytes) = find_folder_image(dir).and_then(|img| std::fs::read(img).ok()) {
        return Some(bytes);
    }
    FOLDER_IMAGE_READER.get().and_then(|read| read(dir))
}

/// Finds folder art (cover.jpg, folder.png, ...) in `dir`.
pub fn find_folder_image(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(usize, PathBuf)> = None;
    let mut any_image: Option<PathBuf> = None;
    for e in entries.flatten() {
        let p = e.path();
        let Some(ext) = p.extension().and_then(|x| x.to_str()).map(|x| x.to_ascii_lowercase()) else { continue };
        if !IMAGE_EXTS.contains(&ext.as_str()) {
            continue;
        }
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        if let Some(rank) = FOLDER_ART_NAMES.iter().position(|n| stem == *n) {
            if best.as_ref().map(|(r, _)| rank < *r).unwrap_or(true) {
                best = Some((rank, p.clone()));
            }
        } else if any_image.is_none() {
            any_image = Some(p);
        }
    }
    best.map(|(_, p)| p).or(any_image)
}
