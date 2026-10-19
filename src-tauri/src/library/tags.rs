//! Tag + audio property reading with lofty.

use std::path::Path;
use std::time::UNIX_EPOCH;

use anyhow::Result;
use lofty::config::ParseOptions;
use lofty::file::FileType;
use lofty::probe::Probe;
use lofty::picture::PictureType;
use lofty::prelude::*;

use super::model::Track;

pub struct EmbeddedPicture {
    pub data: Vec<u8>,
}

fn parse_rg(s: &str) -> Option<f64> {
    let t = s.trim().trim_end_matches(|c: char| c.is_alphabetic() || c.is_whitespace());
    t.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A MusicBrainz id is a UUID; anything else in the field is ignored.
fn normalize_mbid(s: &str) -> Option<String> {
    let s = s.trim().to_ascii_lowercase();
    let ok = s.len() == 36
        && s.char_indices().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_hexdigit() });
    ok.then_some(s)
}

fn codec_name(ft: FileType, path: &Path, has_bit_depth: bool) -> String {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ft {
        FileType::Flac => "FLAC".into(),
        FileType::Mpeg => "MP3".into(),
        FileType::Opus => "Opus".into(),
        FileType::Vorbis => "Vorbis".into(),
        FileType::Wav => "WAV".into(),
        FileType::Aiff => "AIFF".into(),
        FileType::Aac => "AAC".into(),
        FileType::Mp4 => {
            if has_bit_depth {
                "ALAC".into()
            } else {
                "AAC".into()
            }
        }
        _ => ext.to_ascii_uppercase(),
    }
}

/// Reads tags and audio properties. Falls back to the file name when tags are missing.
pub fn read_track(path: &Path, want_picture: bool) -> Result<(Track, Option<EmbeddedPicture>)> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();

    let mut track = Track {
        path: path.to_string_lossy().to_string(),
        title: stem.clone(),
        file_size: meta.len(),
        mtime,
        ..Default::default()
    };

    // Skip decoding embedded pictures unless asked: it dominates scan time on large libraries.
    let opts = ParseOptions::new().read_cover_art(want_picture);
    let probed = Probe::open(path)
        .and_then(|p| p.options(opts).guess_file_type().map_err(Into::into))
        .and_then(|p| p.read());
    let tagged = match probed {
        Ok(t) => t,
        Err(e) => {
            // Unreadable tags: still list the file, taking duration/format from the decoder.
            log::debug!("tag read failed for {}: {e}", path.display());
            match crate::audio::decoder::probe_info(path) {
                Some(info) => {
                    track.duration = info.duration;
                    track.sample_rate = Some(info.source.sample_rate);
                    track.bit_depth = if info.source.lossless { info.source.bits_per_sample } else { None };
                    track.channels = Some(info.source.channels as u32);
                    track.bitrate = info.source.bitrate_kbps;
                    track.codec = info.source.codec;
                }
                None => {
                    track.codec = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_uppercase();
                }
            }
            return Ok((track, None));
        }
    };

    let props = tagged.properties();
    track.duration = props.duration().as_secs_f64();
    track.sample_rate = props.sample_rate();
    track.bit_depth = props.bit_depth().map(|b| b as u32);
    track.channels = props.channels().map(|c| c as u32);
    track.bitrate = props.audio_bitrate().or(props.overall_bitrate());
    track.codec = codec_name(tagged.file_type(), path, props.bit_depth().is_some());

    let mut picture = None;
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        if let Some(v) = tag.title() {
            let v = v.trim();
            if !v.is_empty() {
                track.title = v.to_string();
            }
        }
        track.artist = tag.artist().map(|s| s.trim().to_string()).unwrap_or_default();
        track.album = tag.album().map(|s| s.trim().to_string()).unwrap_or_default();
        track.genre = tag.genre().map(|s| s.trim().to_string()).unwrap_or_default();
        track.album_artist = tag
            .get_string(ItemKey::AlbumArtist)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        track.track_no = tag.track();
        track.disc_no = tag.disk();
        track.year = tag
            .date()
            .map(|d| d.year as i32)
            .or_else(|| {
                tag.get_string(ItemKey::Year)
                    .and_then(|s| s.get(..4))
                    .and_then(|s| s.parse().ok())
            })
            .filter(|y| *y > 0);
        track.rg_track_gain = tag.get_string(ItemKey::ReplayGainTrackGain).and_then(parse_rg);
        track.rg_track_peak = tag.get_string(ItemKey::ReplayGainTrackPeak).and_then(parse_rg);
        track.rg_album_gain = tag.get_string(ItemKey::ReplayGainAlbumGain).and_then(parse_rg);
        track.rg_album_peak = tag.get_string(ItemKey::ReplayGainAlbumPeak).and_then(parse_rg);

        // Identifiers for playlist matching. Any tag may hold them (ID3v2 + APE, …). ISRC fields
        // sometimes hold barcodes or catalogue numbers; only valid values are kept.
        track.isrc = tagged
            .tags()
            .iter()
            .filter_map(|t| t.get_string(ItemKey::Isrc))
            .find_map(super::interchange::normalize_isrc);
        track.mbid = tagged
            .tags()
            .iter()
            .filter_map(|t| t.get_string(ItemKey::MusicBrainzRecordingId))
            .find_map(normalize_mbid);

        if want_picture {
            let pics = tag.pictures();
            let best = pics
                .iter()
                .find(|p| p.pic_type() == PictureType::CoverFront)
                .or_else(|| pics.first());
            if let Some(p) = best {
                if !p.data().is_empty() {
                    picture = Some(EmbeddedPicture { data: p.data().to_vec() });
                }
            }
        }
    }
    if want_picture && picture.is_none() {
        // Some files carry pictures in a secondary tag (e.g. ID3v2 + APE).
        for tag in tagged.tags() {
            if let Some(p) = tag.pictures().first() {
                picture = Some(EmbeddedPicture { data: p.data().to_vec() });
                break;
            }
        }
    }
    Ok((track, picture))
}

#[cfg(test)]
mod tests {
    use super::{normalize_mbid, parse_rg};

    #[test]
    fn mbid_values() {
        assert_eq!(normalize_mbid(" 6C4E3B79-5345-4854-AC55-82AD1EC2D928 ").as_deref(), Some("6c4e3b79-5345-4854-ac55-82ad1ec2d928"));
        assert_eq!(normalize_mbid("6c4e3b79534548 54ac5582ad1ec2d928"), None);
        assert_eq!(normalize_mbid(""), None);
    }

    #[test]
    fn rg_values() {
        assert_eq!(parse_rg("-6.54 dB"), Some(-6.54));
        assert_eq!(parse_rg("+1.20 dB"), Some(1.2));
        assert_eq!(parse_rg("0.988"), Some(0.988));
        assert_eq!(parse_rg("garbage"), None);
    }
}
