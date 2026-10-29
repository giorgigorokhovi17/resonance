//! CSV playlists (RFC 4180), written with ISRC and MusicBrainz ids for use in spreadsheets and by
//! other tools.
//!
//! Columns: Position, Title, Artist, Album, Duration (ms), ISRC, MusicBrainz Recording ID,
//! Location, Added At (RFC 3339, UTC). The file is UTF-8 with a BOM so Excel detects the encoding.
//! `Artist` is one string, as in M3U.
//!
//! Spreadsheet formula injection: a text cell starting with `=`, `+`, `-`, `@`, tab or CR is
//! written with a leading `'`, which Excel and LibreOffice treat as "this is text" and hide.
//! Uploader titles really do start with these. The reader removes the quote again, and a value
//! that itself starts with `'` before one of those characters gets a second quote, so the mapping
//! is exact both ways.
//!
//! Reading accepts our own files. Exportify CSVs (header starts with `Track URI`) are recognised
//! and refused for now: that importer waits for a real sample.

use std::path::Path;

use anyhow::{bail, Context, Result};

use super::interchange::{relative_to, Playlist, PlaylistItem, TrackRef};

pub const HEADER: [&str; 9] =
    ["Position", "Title", "Artist", "Album", "Duration (ms)", "ISRC", "MusicBrainz Recording ID", "Location", "Added At"];

// ---- RFC 4180 ---------------------------------------------------------------------------------

/// Split CSV text into records. Quoted fields may contain commas, quotes (`""`) and line breaks.
/// Lines may end in CRLF or LF. Blank lines are skipped.
pub fn read_records(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let end_record = |record: &mut Vec<String>, field: &mut String, records: &mut Vec<Vec<String>>| {
        record.push(std::mem::take(field));
        let r = std::mem::take(record);
        if !(r.len() == 1 && r[0].is_empty()) {
            records.push(r);
        }
    };
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            ',' => record.push(std::mem::take(&mut field)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => end_record(&mut record, &mut field, &mut records),
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        end_record(&mut record, &mut field, &mut records);
    }
    records
}

fn quote(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) || s.starts_with(' ') || s.ends_with(' ') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn formula_trigger(s: &str) -> bool {
    s.starts_with(['=', '+', '-', '@', '\t', '\r']) || (s.starts_with('\'') && formula_trigger(&s[1..]))
}

/// Text cell as written: guarded against formula injection, then quoted if needed.
pub(crate) fn text_cell(s: &str) -> String {
    if formula_trigger(s) {
        quote(&format!("'{s}"))
    } else {
        quote(s)
    }
}

fn unguard(s: &str) -> String {
    match s.strip_prefix('\'') {
        Some(rest) if formula_trigger(rest) => rest.to_string(),
        _ => s.to_string(),
    }
}

// ---- RFC 3339 timestamps (UTC), without a date library ----------------------------------------

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's civil-from-days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (if m > 2 { m - 3 } else { m + 9 }) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn format_rfc3339(secs: i64) -> String {
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    format!("{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z", s / 3600, s / 60 % 60, s % 60)
}

/// `YYYY-MM-DD[T ]HH:MM:SS[.fff](Z|±HH:MM)?`; a missing offset is taken as UTC. Fractions are dropped.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let s = s.trim();
    let num = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<i64>().ok());
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if s.as_bytes().get(4) != Some(&b'-') || s.as_bytes().get(7) != Some(&b'-') || !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let (h, mi, se) = if s.len() > 10 { (num(11, 13)?, num(14, 16)?, num(17, 19)?) } else { (0, 0, 0) };
    let mut rest = s.get(19..).unwrap_or("");
    if let Some(r) = rest.strip_prefix('.') {
        rest = r.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        o if o.len() == 6 && (o.starts_with('+') || o.starts_with('-')) => {
            let v = o[1..3].parse::<i64>().ok()? * 3600 + o[4..6].parse::<i64>().ok()? * 60;
            if o.starts_with('-') { -v } else { v }
        }
        _ => return None,
    };
    Some(days_from_civil(y, mo as u32, d as u32) * 86_400 + h * 3600 + mi * 60 + se - offset)
}

// ---- playlists --------------------------------------------------------------------------------

/// CSV text for `pl`. Locations inside `base` are written relative to it.
pub fn write_str(pl: &Playlist, base: Option<&Path>) -> String {
    let mut s = HEADER.join(",");
    s.push_str("\r\n");
    for (i, item) in pl.items.iter().enumerate() {
        let t = &item.track;
        let cells = [
            (i + 1).to_string(),
            text_cell(&t.title),
            text_cell(&t.artists.join("; ")),
            text_cell(&t.album),
            t.duration_ms.map(|d| d.to_string()).unwrap_or_default(),
            t.isrc.clone().unwrap_or_default(),
            t.mbid.clone().unwrap_or_default(),
            text_cell(&t.location.as_deref().map(|l| relative_to(l, base)).unwrap_or_default()),
            item.added_at.map(format_rfc3339).unwrap_or_default(),
        ];
        s.push_str(&cells.join(","));
        s.push_str("\r\n");
    }
    s
}

pub fn write_file(file: &Path, pl: &Playlist) -> Result<()> {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(write_str(pl, file.parent()).as_bytes());
    std::fs::write(file, bytes).with_context(|| format!("cannot write {}", file.display()))?;
    Ok(())
}

pub fn parse_str(text: &str, default_name: &str) -> Result<Playlist> {
    let records = read_records(text);
    let Some((header, rows)) = records.split_first() else { bail!("the CSV file is empty") };
    let col = |name: &str| header.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
    if col("Track URI").is_some() {
        bail!("Exportify CSV files can't be imported yet (that importer waits for a real sample file)");
    }
    let (Some(title_c), Some(loc_c)) = (col("Title"), col("Location")) else {
        bail!("not a Undertone playlist CSV (needs Title and Location columns)");
    };
    let (artist_c, album_c, dur_c, isrc_c, mbid_c, added_c) =
        (col("Artist"), col("Album"), col("Duration (ms)"), col("ISRC"), col("MusicBrainz Recording ID"), col("Added At"));
    let items = rows
        .iter()
        .map(|r| {
            let get = |c: Option<usize>| c.and_then(|c| r.get(c)).map(|v| unguard(v)).unwrap_or_default();
            let opt = |c: Option<usize>| Some(get(c)).filter(|v| !v.trim().is_empty());
            let artist = get(artist_c);
            PlaylistItem {
                track: TrackRef {
                    title: get(Some(title_c)),
                    artists: if artist.is_empty() { Vec::new() } else { vec![artist] },
                    album: get(album_c),
                    duration_ms: opt(dur_c).and_then(|d| d.trim().parse().ok()),
                    isrc: opt(isrc_c).and_then(|v| super::interchange::normalize_isrc(&v)),
                    mbid: opt(mbid_c).map(|v| v.trim().to_ascii_lowercase()),
                    location: opt(Some(loc_c)).and_then(|l| super::interchange::canonical_location(&l)),
                    ..Default::default()
                },
                added_at: opt(added_c).and_then(|v| parse_rfc3339(&v)),
            }
        })
        .collect();
    Ok(Playlist { name: default_name.to_string(), source: "csv".into(), source_id: None, items })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::interchange::testgen;
    use proptest::prelude::*;

    #[test]
    fn rfc4180_quoting() {
        let recs = read_records("\u{feff}a,\"b,c\",\"say \"\"hi\"\"\"\r\n\"two\r\nlines\",,x\n\nlast");
        assert_eq!(recs, vec![vec!["a", "b,c", "say \"hi\""], vec!["two\r\nlines", "", "x"], vec!["last"]]);
    }

    #[test]
    fn formula_injection_is_guarded_and_reversible() {
        for s in ["=HYPERLINK(\"x\")", "-ish", "+1", "@me", "'=already quoted", "'plain", "normal"] {
            let cell = text_cell(s);
            let back = unguard(&read_records(&cell)[0][0]);
            assert_eq!(back, s, "cell {cell}");
            if s.starts_with(['=', '-', '+', '@']) {
                assert!(cell.trim_start_matches('"').starts_with('\''), "{cell} must not start with a formula character");
            }
        }
    }

    #[test]
    fn timestamps() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(parse_rfc3339("2000-02-29T00:00:00Z"), Some(951_782_400));
        assert_eq!(parse_rfc3339("2000-02-29T02:00:00.123+02:00"), Some(951_782_400));
        assert_eq!(parse_rfc3339("2000-02-29"), Some(951_782_400));
        assert_eq!(parse_rfc3339("29/02/2000"), None);
    }

    #[test]
    fn writes_isrc_and_refuses_exportify() {
        let pl = Playlist {
            items: vec![PlaylistItem {
                track: TrackRef { title: "Into You".into(), artists: vec!["Ariana Grande".into()], isrc: Some("USUM71601827".into()), location: Some(r"C:\m\a, b.flac".into()), ..Default::default() },
                added_at: Some(0),
            }],
            ..Default::default()
        };
        let s = write_str(&pl, None);
        assert!(s.starts_with("Position,Title,Artist,Album,Duration (ms),ISRC,"));
        assert!(s.contains("1,Into You,Ariana Grande,,,USUM71601827,,\"C:\\m\\a, b.flac\",1970-01-01T00:00:00Z\r\n"), "{s}");
        assert!(parse_str("Track URI,Track Name\nspotify:track:x,y\n", "x").unwrap_err().to_string().contains("Exportify"));
        assert!(parse_str("a,b\n1,2\n", "x").is_err());
    }

    /// What our CSV carries: everything except service ids, with artists as one string.
    fn project(mut pl: Playlist) -> Playlist {
        pl.name = "stem".into();
        pl.source = "csv".into();
        for it in &mut pl.items {
            let t = &mut it.track;
            let artists = t.artists.join("; ");
            t.artists = if artists.is_empty() { Vec::new() } else { vec![artists] };
            t.service_ids.clear();
        }
        pl
    }

    proptest! {
        #[test]
        fn parse_write_roundtrip(pl in testgen::playlist("csv")) {
            prop_assert_eq!(parse_str(&write_str(&pl, None), "stem").unwrap(), project(pl));
        }

        #[test]
        fn timestamps_roundtrip(secs in -2_000_000_000i64..4_000_000_000) {
            prop_assert_eq!(parse_rfc3339(&format_rfc3339(secs)), Some(secs));
        }
    }
}
