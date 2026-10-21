use serde::{Deserialize, Serialize};

/// A track as stored in the library and sent to the UI.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    /// Library row id; 0 for files played from outside the library.
    pub id: i64,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub genre: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_no: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disc_no: Option<u32>,
    /// Seconds.
    pub duration: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<u32>,
    /// kbps
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u32>,
    pub codec: String,
    pub file_size: u64,
    pub mtime: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rg_track_gain: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rg_track_peak: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rg_album_gain: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rg_album_peak: Option<f64>,
    pub added_at: i64,
    /// The user chose to hide this track's artwork (cover_id is then None).
    #[serde(default)]
    pub cover_hidden: bool,
    /// The artwork is one the user picked for this song (not from the file or its folder).
    #[serde(default)]
    pub custom_cover: bool,
    /// ISRC from the tags, only when valid (12 characters, upper case, no hyphens).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
    /// MusicBrainz recording id from the tags (lower-case UUID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mbid: Option<String>,
    /// A GIF or video the user picked as this song's artwork (`cover_id` is then its still poster).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_id: Option<String>,
}

impl Track {
    /// Artist used for album grouping.
    pub fn album_artist_or_artist(&self) -> &str {
        if self.album_artist.is_empty() {
            &self.artist
        } else {
            &self.album_artist
        }
    }
}

/// Audio file extensions the player can decode.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "wav", "wave", "mp3", "m4a", "mp4", "aac", "alac", "ogg", "oga", "opus", "aif", "aiff", "aifc", "caf", "mka",
];

pub fn is_audio_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}
