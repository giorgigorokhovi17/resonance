//! Custom photos, GIFs and short videos used as artwork and backgrounds.
//!
//! Every animated item keeps a still poster in the cover cache (`cover_id`), so lists, colour
//! extraction, the media overlay, the tray and backups keep working with images only. The moving
//! file is a separate motion asset stored here by content hash (`<id>.<ext>`), and only the views
//! that animate load it, through the `media` URI scheme with HTTP Range support.
//!
//! There's no decoder for video here: the backend checks magic bytes, the container's codec,
//! the extension and the size; the frontend reads duration and resolution from a hidden `<video>`
//! and makes the poster from the first frame.

use std::collections::HashSet;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use serde::{Deserialize, Serialize};

const MB: u64 = 1024 * 1024;
/// Still images larger than this on the long side are scaled down when stored as a background.
pub const MAX_IMAGE_PX: u32 = 3840;
/// Open-ended Range requests ("bytes=0-") are answered in pieces of at most this size.
pub const RANGE_CHUNK: u64 = 4 * MB;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum MediaKind {
    /// A still photo (backgrounds only; still covers go to the cover cache).
    Image,
    Gif,
    WebpAnimated,
    Video,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MediaKind::Image => "image",
            MediaKind::Gif => "gif",
            MediaKind::WebpAnimated => "webp-animated",
            MediaKind::Video => "video",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "image" => MediaKind::Image,
            "gif" => MediaKind::Gif,
            "webp-animated" => MediaKind::WebpAnimated,
            "video" => MediaKind::Video,
            _ => return None,
        })
    }

    pub fn animated(self) -> bool {
        self != MediaKind::Image
    }
}

/// What the file is for. Covers are small and short; backgrounds may be larger and longer.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Purpose {
    Cover,
    Background,
}

pub struct Limits {
    pub image_bytes: u64,
    pub animated_bytes: u64,
    pub video_bytes: u64,
    pub video_ms: u64,
    /// Long and short side of a video, in pixels.
    pub video_long_px: u32,
    pub video_short_px: u32,
}

impl Purpose {
    pub fn limits(self) -> Limits {
        match self {
            Purpose::Cover => Limits {
                image_bytes: 25 * MB,
                animated_bytes: 15 * MB,
                video_bytes: 15 * MB,
                video_ms: 10_000,
                video_long_px: 1080,
                video_short_px: 1080,
            },
            Purpose::Background => Limits {
                image_bytes: 25 * MB,
                animated_bytes: 30 * MB,
                video_bytes: 100 * MB,
                video_ms: 60_000,
                video_long_px: 1920,
                video_short_px: 1080,
            },
        }
    }
}

/// A supported file, recognised from its contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Jpeg,
    Png { animated: bool },
    Webp { animated: bool },
    Gif { animated: bool },
    Mp4,
    WebM,
}

impl Format {
    pub fn kind(self) -> MediaKind {
        match self {
            Format::Gif { animated: true } => MediaKind::Gif,
            Format::Webp { animated: true } => MediaKind::WebpAnimated,
            Format::Mp4 | Format::WebM => MediaKind::Video,
            _ => MediaKind::Image,
        }
    }

    /// Extension of the stored copy.
    pub fn ext(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png { .. } => "png",
            Format::Webp { .. } => "webp",
            Format::Gif { .. } => "gif",
            Format::Mp4 => "mp4",
            Format::WebM => "webm",
        }
    }

    pub fn mime(self) -> &'static str {
        mime_for_ext(self.ext()).unwrap_or("application/octet-stream")
    }

    /// Extensions a file of this format may have.
    fn extensions(self) -> &'static [&'static str] {
        match self {
            Format::Jpeg => &["jpg", "jpeg", "jfif"],
            Format::Png { .. } => &["png"],
            Format::Webp { .. } => &["webp"],
            Format::Gif { .. } => &["gif"],
            Format::Mp4 => &["mp4", "m4v"],
            Format::WebM => &["webm"],
        }
    }
}

/// Extensions of stored files, and what they're served as.
const STORED: &[(&str, &str)] = &[
    ("mp4", "video/mp4"),
    ("webm", "video/webm"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("jpg", "image/jpeg"),
    ("png", "image/png"),
];

pub fn mime_for_ext(ext: &str) -> Option<&'static str> {
    STORED.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

pub fn ext_for_mime(mime: &str) -> Option<&'static str> {
    STORED.iter().find(|(_, m)| *m == mime).map(|(e, _)| *e)
}

const HINT_VIDEO: &str = "Save it as MP4 (H.264) or WebM (VP9) instead.";

/// Plain refusal for extensions that are never accepted, before the file is read.
fn refused_extension(ext: &str) -> Option<String> {
    Some(match ext {
        "mov" | "qt" => format!("MOV videos don't play reliably in Undertone. {HINT_VIDEO}"),
        "mkv" => format!("MKV videos aren't supported. {HINT_VIDEO}"),
        "avi" | "wmv" | "flv" | "mpg" | "mpeg" | "ts" | "m2ts" | "mts" | "3gp" | "ogv" => {
            format!("That video format isn't supported. {HINT_VIDEO}")
        }
        "heic" | "heif" => "HEIC photos aren't supported. Save it as JPG or PNG instead.".into(),
        "bmp" | "tif" | "tiff" | "avif" | "svg" | "ico" | "jxl" => "Use a JPG, PNG or WebP picture.".into(),
        _ => return None,
    })
}

fn unsupported() -> String {
    "That file isn't a supported picture or video. Use JPG, PNG, WebP, GIF, MP4 (H.264) or WebM (VP9 or AV1).".into()
}

/// Recognises a file from its first bytes and, for videos, the codec its container declares.
pub fn sniff(data: &[u8]) -> Result<Format, String> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok(Format::Jpeg);
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(Format::Png { animated: png_is_animated(data) });
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return match gif_frames(data, 2) {
            0 => Err("That GIF is damaged and can't be read.".into()),
            n => Ok(Format::Gif { animated: n > 1 }),
        };
    }
    if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Ok(Format::Webp { animated: webp_is_animated(data) });
    }
    if data.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return check_matroska(data).map(|_| Format::WebM);
    }
    if data.len() >= 8 && is_bmff_box(&data[4..8]) {
        return check_mp4(data).map(|_| Format::Mp4);
    }
    Err(unsupported())
}

// ---- images -------------------------------------------------------------------------------------

/// Counts GIF frames (image descriptors), stopping at `stop`. 0 means the file is unreadable.
pub fn gif_frames(data: &[u8], stop: usize) -> usize {
    let mut i = 13; // header + logical screen descriptor
    if data.len() < i {
        return 0;
    }
    let packed = data[10];
    if packed & 0x80 != 0 {
        i += 3 << ((packed & 7) + 1);
    }
    let mut frames = 0;
    // Skips data sub-blocks; returns the index after the terminator.
    let skip_blocks = |mut j: usize| -> Option<usize> {
        loop {
            let n = *data.get(j)? as usize;
            j += 1;
            if n == 0 {
                return Some(j);
            }
            j += n;
        }
    };
    while let Some(&b) = data.get(i) {
        match b {
            0x2C => {
                frames += 1;
                if frames >= stop {
                    return frames;
                }
                let Some(&p) = data.get(i + 9) else { return frames };
                i += 10;
                if p & 0x80 != 0 {
                    i += 3 << ((p & 7) + 1);
                }
                i += 1; // LZW minimum code size
                match skip_blocks(i) {
                    Some(j) => i = j,
                    None => return frames,
                }
            }
            0x21 => match skip_blocks(i + 2) {
                Some(j) => i = j,
                None => return frames,
            },
            _ => return frames, // 0x3B trailer, or junk after the last frame
        }
    }
    frames
}

/// The animation flag of a VP8X (extended) WebP.
fn webp_is_animated(data: &[u8]) -> bool {
    data.len() >= 21 && &data[12..16] == b"VP8X" && data[20] & 0x02 != 0
}

/// APNG: an `acTL` chunk before the first image data.
fn png_is_animated(data: &[u8]) -> bool {
    let mut i = 8;
    while i + 8 <= data.len() {
        let len = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        match &data[i + 4..i + 8] {
            b"acTL" => return true,
            b"IDAT" => return false,
            _ => {}
        }
        i = i.saturating_add(12).saturating_add(len);
    }
    false
}

/// A still image ready for the background store: scaled down to `MAX_IMAGE_PX`, and re-encoded
/// as JPEG when it had to be scaled or was an animated PNG (so it can never animate on its own).
pub fn fit_still_image(data: Vec<u8>, format: Format) -> Result<(Vec<u8>, Format, u32, u32), String> {
    let unreadable = || "That picture can't be read.".to_string();
    let (w, h) = image::ImageReader::new(Cursor::new(&data))
        .with_guessed_format()
        .map_err(|_| unreadable())?
        .into_dimensions()
        .map_err(|_| unreadable())?;
    let apng = format == Format::Png { animated: true };
    if w.max(h) <= MAX_IMAGE_PX && !apng {
        return Ok((data, format, w, h));
    }
    let img = image::load_from_memory(&data).map_err(|_| unreadable())?;
    let img = if w.max(h) > MAX_IMAGE_PX { img.resize(MAX_IMAGE_PX, MAX_IMAGE_PX, FilterType::CatmullRom) } else { img };
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut Cursor::new(&mut out), 90)
        .encode_image(&img.to_rgb8())
        .map_err(|_| unreadable())?;
    Ok((out, Format::Jpeg, img.width(), img.height()))
}

// ---- MP4 ----------------------------------------------------------------------------------------

fn is_bmff_box(t: &[u8]) -> bool {
    matches!(t, b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"skip" | b"pnot")
}

/// Iterates ISO-BMFF boxes in `data`: (type, payload).
fn boxes(data: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut i = 0usize;
    std::iter::from_fn(move || {
        if i + 8 > data.len() {
            return None;
        }
        let size = u32::from_be_bytes(data[i..i + 4].try_into().ok()?) as u64;
        let kind: [u8; 4] = data[i + 4..i + 8].try_into().ok()?;
        let (header, size) = match size {
            0 => (8, (data.len() - i) as u64),
            1 => {
                let large = u64::from_be_bytes(data.get(i + 8..i + 16)?.try_into().ok()?);
                (16, large)
            }
            s => (8, s),
        };
        if size < header as u64 {
            return None;
        }
        // A box cut short by the end of the data still yields what's there.
        let end = i.saturating_add(usize::try_from(size).unwrap_or(usize::MAX)).min(data.len());
        let payload = &data[i + header..end];
        i = end;
        Some((kind, payload))
    })
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data).find(|(k, _)| k == kind).map(|(_, p)| p)
}

/// Accepts MP4 with H.264 video; refuses QuickTime (MOV), HEVC and other codecs with a hint.
fn check_mp4(data: &[u8]) -> Result<(), String> {
    let mov = || format!("MOV videos don't play reliably in Undertone. {HINT_VIDEO}");
    let Some(ftyp) = child(data, b"ftyp") else { return Err(mov()) };
    match ftyp.get(..4).unwrap_or_default() {
        b"qt  " => return Err(mov()),
        b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"mif1" | b"msf1" => {
            return Err("HEIC photos aren't supported. Save it as JPG or PNG instead.".into())
        }
        b"avif" | b"avis" => return Err("Use a JPG, PNG or WebP picture.".into()),
        _ => {}
    }
    let Some(moov) = child(data, b"moov") else {
        return Err("That video can't be read (it may be incomplete).".into());
    };
    let mut codecs = Vec::new();
    for (k, trak) in boxes(moov) {
        if &k != b"trak" {
            continue;
        }
        let Some(mdia) = child(trak, b"mdia") else { continue };
        let is_video = child(mdia, b"hdlr").and_then(|h| h.get(8..12)) == Some(&b"vide"[..]);
        if !is_video {
            continue;
        }
        let stsd = child(mdia, b"minf").and_then(|m| child(m, b"stbl")).and_then(|s| child(s, b"stsd"));
        if let Some(entries) = stsd.and_then(|s| s.get(8..)) {
            codecs.extend(boxes(entries).map(|(k, _)| k));
        }
    }
    if codecs.is_empty() {
        return Err("That file has no video in it.".into());
    }
    for c in &codecs {
        match c {
            b"avc1" | b"avc3" => {}
            b"hvc1" | b"hev1" | b"dvh1" | b"dvhe" => {
                return Err(format!("This video uses HEVC (H.265), which doesn't play reliably in Undertone. {HINT_VIDEO}"))
            }
            b"encv" => return Err("That video is copy-protected and can't be played.".into()),
            _ => return Err(format!("This MP4 doesn't use H.264. {HINT_VIDEO}")),
        }
    }
    Ok(())
}

// ---- WebM ---------------------------------------------------------------------------------------

/// Reads an EBML element id (marker bits kept) and returns (id, length).
fn ebml_id(data: &[u8], i: usize) -> Option<(u32, usize)> {
    let b = *data.get(i)?;
    let len = b.leading_zeros() as usize + 1;
    if len > 4 {
        return None;
    }
    let mut id = 0u32;
    for k in 0..len {
        id = (id << 8) | *data.get(i + k)? as u32;
    }
    Some((id, len))
}

/// Reads an EBML size; `None` in the value means "unknown size" (all ones).
fn ebml_size(data: &[u8], i: usize) -> Option<(Option<u64>, usize)> {
    let b = *data.get(i)?;
    let len = b.leading_zeros() as usize + 1;
    if len > 8 {
        return None;
    }
    let mut v = (b as u64) & (0xFF >> len);
    let mut all_ones = v == (0xFF >> len) as u64;
    for k in 1..len {
        let x = *data.get(i + k)?;
        all_ones &= x == 0xFF;
        v = (v << 8) | x as u64;
    }
    Some((if all_ones { None } else { Some(v) }, len))
}

/// Iterates EBML elements in `data`: (id, payload). An unknown size runs to the end of `data`.
fn elements(data: &[u8]) -> impl Iterator<Item = (u32, &[u8])> {
    let mut i = 0usize;
    std::iter::from_fn(move || {
        let (id, il) = ebml_id(data, i)?;
        let (size, sl) = ebml_size(data, i + il)?;
        let start = i + il + sl;
        if start > data.len() {
            return None;
        }
        let end = match size {
            Some(s) => start.saturating_add(usize::try_from(s).unwrap_or(usize::MAX)).min(data.len()),
            None => data.len(),
        };
        i = end;
        Some((id, &data[start..end]))
    })
}

const EBML_DOCTYPE: u32 = 0x4282;
const MKV_SEGMENT: u32 = 0x1853_8067;
const MKV_TRACKS: u32 = 0x1654_AE6B;
const MKV_CLUSTER: u32 = 0x1F43_B675;
const MKV_TRACK_ENTRY: u32 = 0xAE;
const MKV_TRACK_TYPE: u32 = 0x83;
const MKV_CODEC_ID: u32 = 0x86;

/// Accepts WebM with VP9 or AV1 video; refuses MKV and other codecs with a hint.
fn check_matroska(data: &[u8]) -> Result<(), String> {
    let unreadable = || "That video can't be read (it may be incomplete).".to_string();
    let mut doc_type = None;
    let mut codecs = Vec::new();
    for (id, payload) in elements(data) {
        match id {
            0x1A45_DFA3 => {
                doc_type = elements(payload).find(|(i, _)| *i == EBML_DOCTYPE).map(|(_, p)| p.to_vec());
            }
            MKV_SEGMENT => {
                for (id, seg) in elements(payload) {
                    if id == MKV_TRACKS {
                        for (id, entry) in elements(seg) {
                            if id != MKV_TRACK_ENTRY {
                                continue;
                            }
                            let field = |want: u32| elements(entry).find(|(i, _)| *i == want).map(|(_, p)| p);
                            if field(MKV_TRACK_TYPE) == Some(&[1][..]) {
                                if let Some(c) = field(MKV_CODEC_ID) {
                                    codecs.push(String::from_utf8_lossy(c).trim_end_matches('\0').to_string());
                                }
                            }
                        }
                        break;
                    }
                    if id == MKV_CLUSTER {
                        break; // tracks always come before the media data
                    }
                }
                break;
            }
            _ => {}
        }
    }
    match doc_type.as_deref() {
        Some(b"webm") => {}
        Some(b"matroska") => return Err(format!("MKV videos aren't supported. {HINT_VIDEO}")),
        _ => return Err(unreadable()),
    }
    if codecs.is_empty() {
        return Err("That file has no video in it.".into());
    }
    for c in &codecs {
        match c.as_str() {
            "V_VP9" | "V_AV1" => {}
            "V_MPEGH/ISO/HEVC" => {
                return Err(format!("This video uses HEVC (H.265), which doesn't play reliably in Undertone. {HINT_VIDEO}"))
            }
            _ => return Err(format!("This WebM doesn't use VP9 or AV1. {HINT_VIDEO}")),
        }
    }
    Ok(())
}

// ---- checks -------------------------------------------------------------------------------------

fn mb(bytes: u64) -> String {
    format!("{} MB", (bytes as f64 / MB as f64).round() as u64)
}

/// Refuses a file of this format and size for `purpose`, with a plain reason.
pub fn check_size(format: Format, purpose: Purpose, bytes: u64) -> Result<(), String> {
    let l = purpose.limits();
    let what = if purpose == Purpose::Cover { "cover" } else { "background" };
    match format.kind() {
        MediaKind::Image if bytes > l.image_bytes => Err(format!("Pictures can be up to {}. This one is {}.", mb(l.image_bytes), mb(bytes))),
        MediaKind::Gif if bytes > l.animated_bytes => Err(format!(
            "GIFs can be up to {} as a {what}. This one is {}. GIFs are large for what they show: save it as an MP4 video instead, which is usually 5 to 10 times smaller.",
            mb(l.animated_bytes),
            mb(bytes)
        )),
        MediaKind::WebpAnimated if bytes > l.animated_bytes => Err(format!(
            "Animated WebP pictures can be up to {} as a {what}. This one is {}. Saving it as an MP4 video makes it much smaller.",
            mb(l.animated_bytes),
            mb(bytes)
        )),
        MediaKind::Video if bytes > l.video_bytes => Err(format!(
            "{} videos can be up to {}. This one is {}. Shorten it or lower its resolution.",
            if purpose == Purpose::Cover { "Cover" } else { "Background" },
            mb(l.video_bytes),
            mb(bytes)
        )),
        _ => Ok(()),
    }
}

/// Refuses a video whose length or resolution is over the limits for `purpose`.
pub fn check_video(purpose: Purpose, width: u32, height: u32, duration_ms: u64) -> Result<(), String> {
    let l = purpose.limits();
    let (long, short) = (width.max(height), width.min(height));
    if width == 0 || height == 0 {
        return Err("That video can't be read.".into());
    }
    if duration_ms > l.video_ms + 50 {
        return Err(format!(
            "{} videos can be up to {} seconds long. This one is {} s.",
            if purpose == Purpose::Cover { "Cover" } else { "Background" },
            l.video_ms / 1000,
            (duration_ms as f64 / 1000.0).round()
        ));
    }
    match purpose {
        Purpose::Cover if long > l.video_long_px => Err(format!(
            "Cover videos can be up to {} px on the long side (720 px is plenty). This one is {width}×{height}.",
            l.video_long_px
        )),
        Purpose::Background if long > l.video_long_px || short > l.video_short_px => Err(format!(
            "Background videos can be up to 1920×1080. This one is {width}×{height}; larger video costs too much to decode while music plays."
        )),
        _ => Ok(()),
    }
}

/// A file read from disk that passed the format, extension and size checks.
#[derive(Debug)]
pub struct Checked {
    pub format: Format,
    pub data: Vec<u8>,
}

/// Reads `path` and checks it can be used for `purpose`. Never reads more than the largest limit.
pub fn read_checked(path: &Path, purpose: Purpose) -> Result<Checked, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if let Some(msg) = refused_extension(&ext) {
        return Err(msg);
    }
    let len = std::fs::metadata(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?.len();
    let l = purpose.limits();
    let cap = l.image_bytes.max(l.animated_bytes).max(l.video_bytes);
    if len > cap {
        // The precise message depends on what the file is; judge that from its first bytes.
        let mut head = vec![0u8; 64 * 1024];
        let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
        head.truncate(n);
        let format = sniff(&head).unwrap_or(Format::Mp4);
        check_size(format, purpose, len)?;
        return Err(format!("That file is too large ({}).", mb(len)));
    }
    let data = std::fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let format = sniff(&data)?;
    if !ext.is_empty() && !format.extensions().contains(&ext.as_str()) {
        return Err(format!(
            "This file is named .{ext} but it's actually a .{} file. Rename it and try again.",
            format.ext()
        ));
    }
    check_size(format, purpose, data.len() as u64)?;
    Ok(Checked { format, data })
}

// ---- store --------------------------------------------------------------------------------------

/// Motion assets and background photos on disk, by content hash.
pub struct MediaStore {
    dir: PathBuf,
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

impl MediaStore {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Self { dir }
    }

    /// The stored file for `id` and its MIME type.
    pub fn file(&self, id: &str) -> Option<(PathBuf, &'static str)> {
        if !valid_id(id) {
            return None;
        }
        STORED.iter().find_map(|(ext, mime)| {
            let p = self.dir.join(format!("{id}.{ext}"));
            p.is_file().then_some((p, *mime))
        })
    }

    /// Stores `data` as `<id>.<ext>` and returns the id. Storing the same bytes again is a no-op.
    pub fn put(&self, data: &[u8], ext: &str) -> std::io::Result<String> {
        let id = blake3::hash(data).to_hex()[..24].to_string();
        let path = self.dir.join(format!("{id}.{ext}"));
        if !path.is_file() {
            let tmp = self.dir.join(format!("{id}.tmp{}", std::process::id()));
            std::fs::write(&tmp, data)?;
            if let Err(e) = std::fs::rename(&tmp, &path) {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
        }
        Ok(id)
    }

    pub fn remove(&self, id: &str) {
        if let Some((p, _)) = self.file(id) {
            let _ = std::fs::remove_file(p);
        }
    }

    /// Deletes stored files (and leftover temp files) whose id isn't in `keep`. Returns how many
    /// were deleted. Files that don't look like ours are left alone.
    pub fn sweep(&self, keep: &HashSet<String>) -> usize {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return 0 };
        let mut removed = 0;
        for e in entries.flatten() {
            let p = e.path();
            let (Some(stem), Some(ext)) = (p.file_stem().and_then(|s| s.to_str()), p.extension().and_then(|s| s.to_str())) else {
                continue;
            };
            let ours = valid_id(stem) && (mime_for_ext(ext).is_some() || ext.starts_with("tmp"));
            if ours && (ext.starts_with("tmp") || !keep.contains(stem)) && std::fs::remove_file(&p).is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

// ---- serving ------------------------------------------------------------------------------------

/// What part of a file a request asks for (inclusive byte offsets).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ByteRange {
    Full,
    Partial(u64, u64),
    Unsatisfiable,
}

/// Parses a `Range` header for a file of `len` bytes. Only the first range of a list is served,
/// open-ended ranges are capped at `chunk` bytes, and anything malformed is ignored (whole file),
/// as HTTP allows.
pub fn byte_range(header: Option<&str>, len: u64, chunk: u64) -> ByteRange {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else { return ByteRange::Full };
    let first = spec.split(',').next().unwrap_or("").trim();
    let Some((a, b)) = first.split_once('-') else { return ByteRange::Full };
    let (a, b) = (a.trim(), b.trim());
    if a.is_empty() {
        // "bytes=-N": the last N bytes.
        let Ok(n) = b.parse::<u64>() else { return ByteRange::Full };
        if n == 0 || len == 0 {
            return ByteRange::Unsatisfiable;
        }
        return ByteRange::Partial(len - n.min(len), len - 1);
    }
    let Ok(start) = a.parse::<u64>() else { return ByteRange::Full };
    if start >= len {
        return ByteRange::Unsatisfiable;
    }
    let end = if b.is_empty() {
        start.saturating_add(chunk.max(1) - 1).min(len - 1)
    } else {
        match b.parse::<u64>() {
            Ok(e) if e >= start => e.min(len - 1),
            _ => return ByteRange::Full,
        }
    };
    ByteRange::Partial(start, end)
}

pub struct Served {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

/// Answers a request for a stored file: 200 with the whole file, 206 with the requested part, or
/// 416 when the range starts past the end.
pub fn serve(path: &Path, mime: &str, range: Option<&str>) -> std::io::Result<Served> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let mut headers = vec![
        ("Content-Type", mime.to_string()),
        ("Accept-Ranges", "bytes".to_string()),
        ("Cache-Control", "max-age=31536000, immutable".to_string()),
        ("Access-Control-Allow-Origin", "*".to_string()),
    ];
    let (status, body) = match byte_range(range, len, RANGE_CHUNK) {
        ByteRange::Full => {
            let mut body = Vec::with_capacity(len as usize);
            f.read_to_end(&mut body)?;
            (200, body)
        }
        ByteRange::Partial(start, end) => {
            f.seek(SeekFrom::Start(start))?;
            let mut body = vec![0u8; (end - start + 1) as usize];
            f.read_exact(&mut body)?;
            headers.push(("Content-Range", format!("bytes {start}-{end}/{len}")));
            (206, body)
        }
        ByteRange::Unsatisfiable => {
            headers.push(("Content-Range", format!("bytes */{len}")));
            (416, Vec::new())
        }
    };
    headers.push(("Content-Length", body.len().to_string()));
    Ok(Served { status, headers, body })
}

// ---- backdrop options ---------------------------------------------------------------------------

/// A photo, GIF or video shown behind a playlist page or the whole app, with a scrim on top so
/// text stays readable.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Backdrop {
    pub media_id: String,
    /// "cover" (fill, cropping) | "contain" (whole picture)
    pub fit: String,
    /// Scrim strength 0..0.8. `None` uses the theme mode's default (55% dark, 35% light).
    pub dim: Option<f64>,
    /// Blur in px, 0..40.
    pub blur: f64,
}

impl Default for Backdrop {
    fn default() -> Self {
        Self { media_id: String::new(), fit: "cover".into(), dim: None, blur: 0.0 }
    }
}

impl Backdrop {
    /// Clamps the options into range; `None` when there's no valid media id.
    pub fn normalized(mut self) -> Option<Self> {
        if !valid_id(&self.media_id) {
            return None;
        }
        if !matches!(self.fit.as_str(), "cover" | "contain") {
            self.fit = "cover".into();
        }
        self.dim = self.dim.filter(|d| d.is_finite()).map(|d| d.clamp(0.0, 0.8));
        self.blur = if self.blur.is_finite() { self.blur.clamp(0.0, 40.0) } else { 0.0 };
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bmff(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(payload);
        v
    }

    /// A minimal MP4: ftyp + moov with one video track using `codec`.
    fn mp4(brand: &[u8; 4], codec: &[u8; 4]) -> Vec<u8> {
        let mut ftyp = brand.to_vec();
        ftyp.extend_from_slice(&[0, 0, 2, 0]);
        ftyp.extend_from_slice(b"isomiso2");
        let mut hdlr = vec![0u8; 8];
        hdlr.extend_from_slice(b"vide");
        hdlr.extend_from_slice(&[0u8; 12]);
        let mut stsd = vec![0, 0, 0, 0, 0, 0, 0, 1];
        stsd.extend(bmff(codec, &[0u8; 78]));
        let stbl = bmff(b"stbl", &bmff(b"stsd", &stsd));
        let minf = bmff(b"minf", &stbl);
        let mut mdia = bmff(b"hdlr", &hdlr);
        mdia.extend(minf);
        let trak = bmff(b"trak", &bmff(b"mdia", &mdia));
        let mut out = bmff(b"ftyp", &ftyp);
        out.extend(bmff(b"moov", &trak));
        out.extend(bmff(b"mdat", &[1, 2, 3]));
        out
    }

    fn ebml(id: &[u8], payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() < 0x7F);
        let mut v = id.to_vec();
        v.push(0x80 | payload.len() as u8);
        v.extend_from_slice(payload);
        v
    }

    /// A minimal WebM/MKV header with one video track, the way MediaRecorder writes it (segment
    /// and cluster of unknown size).
    fn matroska(doc_type: &str, codec: &str) -> Vec<u8> {
        let mut out = ebml(&[0x1A, 0x45, 0xDF, 0xA3], &ebml(&[0x42, 0x82], doc_type.as_bytes()));
        let mut entry = ebml(&[0xD7], &[1]);
        entry.extend(ebml(&[0x83], &[1]));
        entry.extend(ebml(&[0x86], codec.as_bytes()));
        let tracks = ebml(&[0x16, 0x54, 0xAE, 0x6B], &ebml(&[0xAE], &entry));
        out.extend_from_slice(&[0x18, 0x53, 0x80, 0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        out.extend(tracks);
        out.extend_from_slice(&[0x1F, 0x43, 0xB6, 0x75, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xE7, 0x81, 0x00]);
        out
    }

    fn gif(frames: usize) -> Vec<u8> {
        let mut v = b"GIF89a".to_vec();
        v.extend_from_slice(&[1, 0, 1, 0, 0x80, 0, 0]); // 1x1, global colour table of 2 entries
        v.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
        v.extend_from_slice(&[0x21, 0xFF, 0x0B]);
        v.extend_from_slice(b"NETSCAPE2.0");
        v.extend_from_slice(&[3, 1, 0, 0, 0]);
        for _ in 0..frames {
            v.extend_from_slice(&[0x21, 0xF9, 4, 0, 10, 0, 0, 0]);
            v.extend_from_slice(&[0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
            v.extend_from_slice(&[2, 2, 0x4C, 0x01, 0]);
        }
        v.push(0x3B);
        v
    }

    fn webp(animated: bool) -> Vec<u8> {
        let mut v = b"RIFF\x00\x00\x00\x00WEBPVP8X\x0a\x00\x00\x00".to_vec();
        v.push(if animated { 0x12 } else { 0x10 });
        v.extend_from_slice(&[0u8; 9]);
        v
    }

    #[test]
    fn recognises_supported_formats_by_magic_bytes() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10]), Ok(Format::Jpeg));
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR"), Ok(Format::Png { animated: false }));
        assert_eq!(sniff(&gif(1)), Ok(Format::Gif { animated: false }));
        assert_eq!(sniff(&gif(3)), Ok(Format::Gif { animated: true }));
        assert_eq!(sniff(&webp(false)), Ok(Format::Webp { animated: false }));
        assert_eq!(sniff(&webp(true)), Ok(Format::Webp { animated: true }));
        assert_eq!(sniff(&mp4(b"isom", b"avc1")), Ok(Format::Mp4));
        assert_eq!(sniff(&mp4(b"mp42", b"avc3")), Ok(Format::Mp4));
        assert_eq!(sniff(&matroska("webm", "V_VP9")), Ok(Format::WebM));
        assert_eq!(sniff(&matroska("webm", "V_AV1")), Ok(Format::WebM));
        assert_eq!(Format::Gif { animated: true }.kind(), MediaKind::Gif);
        assert_eq!(Format::Webp { animated: false }.kind(), MediaKind::Image);
    }

    #[test]
    fn refuses_unreliable_and_unknown_formats_with_a_hint() {
        let refused = |d: &[u8]| sniff(d).unwrap_err();
        assert!(refused(&mp4(b"qt  ", b"avc1")).contains("MOV"));
        assert!(refused(&mp4(b"isom", b"hvc1")).contains("HEVC"));
        assert!(refused(&mp4(b"isom", b"hev1")).contains("HEVC"));
        assert!(refused(&mp4(b"isom", b"av01")).contains("doesn't use H.264"));
        assert!(refused(&matroska("matroska", "V_VP9")).contains("MKV"));
        assert!(refused(&matroska("webm", "V_VP8")).contains("VP9 or AV1"));
        assert!(refused(&matroska("webm", "V_MPEGH/ISO/HEVC")).contains("HEVC"));
        assert!(refused(b"BM\x00\x00\x00\x00\x00\x00").contains("isn't a supported"));
        assert!(refused(b"hello world, not media").contains("isn't a supported"));
        let mut heic = bmff(b"ftyp", b"heic\0\0\0\0mif1heic");
        heic.extend(bmff(b"meta", &[0; 8]));
        assert!(refused(&heic).contains("HEIC"));
        // A damaged GIF and an MP4 without its index.
        assert!(refused(b"GIF89a\x01\x00").contains("damaged"));
        assert!(refused(&bmff(b"ftyp", b"isom\0\0\0\0isom")).contains("incomplete"));
    }

    #[test]
    fn gif_frame_count_survives_truncation() {
        let g = gif(5);
        assert_eq!(gif_frames(&g, 100), 5);
        assert_eq!(gif_frames(&g, 2), 2);
        assert_eq!(gif_frames(&g[..g.len() - 20], 100), 4);
        assert_eq!(gif_frames(&g[..12], 100), 0);
    }

    #[test]
    fn size_limits_depend_on_purpose_and_kind() {
        let gif = Format::Gif { animated: true };
        assert!(check_size(gif, Purpose::Cover, 15 * MB).is_ok());
        let e = check_size(gif, Purpose::Cover, 15 * MB + 1).unwrap_err();
        assert!(e.contains("MP4"), "an oversized GIF cover suggests MP4: {e}");
        assert!(check_size(gif, Purpose::Background, 30 * MB).is_ok());
        assert!(check_size(gif, Purpose::Background, 31 * MB).is_err());
        assert!(check_size(Format::Mp4, Purpose::Cover, 15 * MB + 1).is_err());
        assert!(check_size(Format::Mp4, Purpose::Background, 100 * MB).is_ok());
        assert!(check_size(Format::Mp4, Purpose::Background, 100 * MB + 1).is_err());
        assert!(check_size(Format::Jpeg, Purpose::Background, 25 * MB).is_ok());
        assert!(check_size(Format::Png { animated: false }, Purpose::Cover, 26 * MB).is_err());
    }

    #[test]
    fn video_length_and_resolution_limits() {
        assert!(check_video(Purpose::Cover, 720, 720, 10_000).is_ok());
        assert!(check_video(Purpose::Cover, 1080, 1080, 9_000).is_ok());
        assert!(check_video(Purpose::Cover, 720, 720, 12_000).unwrap_err().contains("10 seconds"));
        assert!(check_video(Purpose::Cover, 1920, 1080, 5_000).unwrap_err().contains("1080 px"));
        assert!(check_video(Purpose::Background, 1920, 1080, 60_000).is_ok());
        assert!(check_video(Purpose::Background, 1080, 1920, 30_000).is_ok());
        assert!(check_video(Purpose::Background, 3840, 2160, 30_000).unwrap_err().contains("1920×1080"));
        assert!(check_video(Purpose::Background, 1920, 1200, 30_000).is_err());
        assert!(check_video(Purpose::Background, 1280, 720, 61_000).unwrap_err().contains("60 seconds"));
        assert!(check_video(Purpose::Background, 0, 0, 1_000).is_err());
    }

    #[test]
    fn reading_checks_extension_and_size() {
        let dir = std::env::temp_dir().join(format!("resonance-media-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, data: &[u8]| {
            let p = dir.join(name);
            std::fs::write(&p, data).unwrap();
            p
        };
        let ok = read_checked(&write("loop.mp4", &mp4(b"isom", b"avc1")), Purpose::Cover).unwrap();
        assert_eq!(ok.format, Format::Mp4);
        assert!(read_checked(&write("clip.mov", &mp4(b"isom", b"avc1")), Purpose::Cover).unwrap_err().contains("MOV"));
        assert!(read_checked(&write("pic.bmp", b"BM"), Purpose::Cover).unwrap_err().contains("JPG, PNG or WebP"));
        let e = read_checked(&write("loop.gif", &mp4(b"isom", b"avc1")), Purpose::Cover).unwrap_err();
        assert!(e.contains("actually a .mp4"), "{e}");
        // Too large for a cover, fine as a background; the file isn't read in full either way.
        let padded = |name: &str, data: &[u8], len: u64| {
            let p = write(name, data);
            std::fs::OpenOptions::new().write(true).open(&p).unwrap().set_len(len).unwrap();
            p
        };
        let p = padded("big.gif", &gif(2), 16 * MB);
        assert!(read_checked(&p, Purpose::Cover).unwrap_err().contains("MP4"));
        assert_eq!(read_checked(&p, Purpose::Background).unwrap().format, Format::Gif { animated: true });
        let p = padded("huge.mp4", &mp4(b"isom", b"avc1"), 101 * MB);
        assert!(read_checked(&p, Purpose::Background).unwrap_err().contains("100 MB"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn large_still_backgrounds_are_scaled_down() {
        let img = image::RgbImage::from_pixel(4000, 1000, image::Rgb([200, 30, 30]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let (data, format, w, h) = fit_still_image(png, Format::Png { animated: false }).unwrap();
        assert_eq!((format, w, h), (Format::Jpeg, 3840, 960));
        assert_eq!(sniff(&data), Ok(Format::Jpeg));
        // Small enough: stored as it is.
        let small = image::RgbImage::from_pixel(64, 32, image::Rgb([1, 2, 3]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(small).write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let (data, format, w, h) = fit_still_image(png.clone(), Format::Png { animated: false }).unwrap();
        assert_eq!((format, w, h, data == png), (Format::Png { animated: false }, 64, 32, true));
    }

    #[test]
    fn range_requests() {
        const C: u64 = 1000;
        assert_eq!(byte_range(None, 5000, C), ByteRange::Full);
        assert_eq!(byte_range(Some("bytes=0-99"), 5000, C), ByteRange::Partial(0, 99));
        assert_eq!(byte_range(Some("bytes=4000-9999"), 5000, C), ByteRange::Partial(4000, 4999));
        // Open-ended ranges are answered in chunks.
        assert_eq!(byte_range(Some("bytes=0-"), 5000, C), ByteRange::Partial(0, 999));
        assert_eq!(byte_range(Some("bytes=4500-"), 5000, C), ByteRange::Partial(4500, 4999));
        assert_eq!(byte_range(Some("bytes=-200"), 5000, C), ByteRange::Partial(4800, 4999));
        assert_eq!(byte_range(Some("bytes=-9000"), 5000, C), ByteRange::Partial(0, 4999));
        assert_eq!(byte_range(Some("bytes=10-20, 30-40"), 5000, C), ByteRange::Partial(10, 20));
        // Out of range.
        assert_eq!(byte_range(Some("bytes=5000-"), 5000, C), ByteRange::Unsatisfiable);
        assert_eq!(byte_range(Some("bytes=6000-7000"), 5000, C), ByteRange::Unsatisfiable);
        assert_eq!(byte_range(Some("bytes=-0"), 5000, C), ByteRange::Unsatisfiable);
        assert_eq!(byte_range(Some("bytes=0-"), 0, C), ByteRange::Unsatisfiable);
        // Malformed headers are ignored.
        assert_eq!(byte_range(Some("bytes=20-10"), 5000, C), ByteRange::Full);
        assert_eq!(byte_range(Some("items=0-10"), 5000, C), ByteRange::Full);
        assert_eq!(byte_range(Some("bytes=abc"), 5000, C), ByteRange::Full);
    }

    #[test]
    fn range_responses() {
        let dir = std::env::temp_dir().join(format!("resonance-media-serve-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clip.mp4");
        let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &data).unwrap();
        let header = |s: &Served, name: &str| s.headers.iter().find(|(k, _)| *k == name).map(|(_, v)| v.clone());

        let full = serve(&path, "video/mp4", None).unwrap();
        assert_eq!((full.status, full.body.len()), (200, 10_000));
        assert_eq!(header(&full, "Accept-Ranges").as_deref(), Some("bytes"));
        assert_eq!(header(&full, "Content-Type").as_deref(), Some("video/mp4"));
        assert_eq!(header(&full, "Content-Length").as_deref(), Some("10000"));

        let part = serve(&path, "video/mp4", Some("bytes=100-199")).unwrap();
        assert_eq!(part.status, 206);
        assert_eq!(part.body, &data[100..200]);
        assert_eq!(header(&part, "Content-Range").as_deref(), Some("bytes 100-199/10000"));
        assert_eq!(header(&part, "Content-Length").as_deref(), Some("100"));

        let tail = serve(&path, "video/mp4", Some("bytes=9990-")).unwrap();
        assert_eq!((tail.status, tail.body.as_slice()), (206, &data[9990..]));

        let out = serve(&path, "video/mp4", Some("bytes=10000-")).unwrap();
        assert_eq!((out.status, out.body.len()), (416, 0));
        assert_eq!(header(&out, "Content-Range").as_deref(), Some("bytes */10000"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_and_sweep() {
        let dir = std::env::temp_dir().join(format!("resonance-media-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = MediaStore::new(dir.clone());
        let a = store.put(b"first video bytes", "mp4").unwrap();
        let b = store.put(b"second gif bytes", "gif").unwrap();
        assert_eq!(store.put(b"first video bytes", "mp4").unwrap(), a, "same bytes, same id");
        assert_eq!(store.file(&a).map(|(_, m)| m), Some("video/mp4"));
        assert_eq!(store.file(&b).map(|(_, m)| m), Some("image/gif"));
        assert!(store.file("../../etc").is_none());
        std::fs::write(dir.join(format!("{a}.tmp123")), b"partial").unwrap();
        std::fs::write(dir.join("notes.txt"), b"not ours").unwrap();

        let keep: HashSet<String> = [a.clone()].into_iter().collect();
        assert_eq!(store.sweep(&keep), 2, "the unreferenced gif and the temp file");
        assert!(store.file(&a).is_some());
        assert!(store.file(&b).is_none());
        assert!(dir.join("notes.txt").exists(), "unrelated files are left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backdrop_normalization() {
        let b = Backdrop { media_id: "abc123".into(), fit: "stretch".into(), dim: Some(2.0), blur: -5.0 }.normalized().unwrap();
        assert_eq!((b.fit.as_str(), b.dim, b.blur), ("cover", Some(0.8), 0.0));
        assert_eq!(Backdrop { dim: Some(f64::NAN), ..b.clone() }.normalized().unwrap().dim, None);
        assert!(Backdrop { media_id: "../x".into(), ..b }.normalized().is_none());
    }
}
