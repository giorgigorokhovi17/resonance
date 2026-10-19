//! File decoding with symphonia. Produces interleaved `f64` samples at the file's native rate.

use std::fs::File;
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};
use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase};

use super::types::SourceFormat;

fn codecs() -> &'static CodecRegistry {
    static REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        // Symphonia has no native Opus decoder; use libopus through the adapter crate.
        registry.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
        registry
    })
}

pub struct ProbeInfo {
    pub source: SourceFormat,
    pub duration: f64,
}

/// Format/duration of a file via the decoder, for files whose tags can't be parsed.
/// When the container has no frame count, the file is decoded to measure it.
pub fn probe_info(path: &Path) -> Option<ProbeInfo> {
    let mut d = Decoder::open(path).ok()?;
    let duration = match d.duration_secs {
        Some(s) if s > 0.0 => s,
        _ => {
            let mut frames = 0usize;
            while let Ok(Some(chunk)) = d.next_chunk() {
                frames += chunk.len() / d.channels();
            }
            frames as f64 / d.sample_rate() as f64
        }
    };
    let mut source = d.source.clone();
    if source.bitrate_kbps.is_none() && duration > 0.5 {
        if let Ok(md) = std::fs::metadata(path) {
            source.bitrate_kbps = Some(((md.len() as f64 * 8.0) / duration / 1000.0).round() as u32);
        }
    }
    Some(ProbeInfo { source, duration })
}

pub struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    pub source: SourceFormat,
    pub duration_secs: Option<f64>,
    channels: usize,
    rate: u32,
    buf: Vec<f64>,
    /// Samples decoded during `open` (used to learn the real output sample format).
    primed: bool,
    /// Frames to discard from the start of the next decoded chunk (sample-accurate seeking).
    skip_frames: usize,
}

fn codec_is_lossless(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("flac") || n.contains("alac") || n.contains("pcm") || n.contains("wav") || n.contains("aiff")
}

fn pretty_codec(short: &str, path: &Path) -> String {
    let s = short.to_ascii_lowercase();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if s.starts_with("pcm") {
        return match ext.as_str() {
            "aif" | "aiff" | "aifc" => "AIFF".into(),
            _ => "WAV".into(),
        };
    }
    match s.as_str() {
        "flac" => "FLAC".into(),
        "alac" => "ALAC".into(),
        "mp3" | "mpa" => "MP3".into(),
        "aac" => "AAC".into(),
        "vorbis" => "Vorbis".into(),
        "opus" => "Opus".into(),
        other => other.to_ascii_uppercase(),
    }
}

impl Decoder {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        let file_len = file.metadata().map(|m| m.len()).ok();
        let mss = MediaSourceStream::new(
            Box::new(file),
            MediaSourceStreamOptions { buffer_len: 256 * 1024 },
        );
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| anyhow!("unsupported or corrupt file ({e})"))?;

        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| anyhow!("no audio track"))?;
        let track_id = track.id;
        let time_base = track.time_base;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| anyhow!("missing audio codec parameters"))?
            .clone();
        let num_frames = track.num_frames;

        let decoder = codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default().gapless(true))
            .map_err(|e| anyhow!("unsupported codec ({e})"))?;

        let codec_short = decoder.codec_info().short_name;
        let rate = params
            .sample_rate
            .ok_or_else(|| anyhow!("unknown sample rate"))?;
        let channels = params.channels.as_ref().map(|c| c.count()).unwrap_or(2).max(1);

        let duration_secs = match (num_frames, time_base) {
            (Some(n), _) => Some(n as f64 / rate as f64),
            _ => None,
        };
        let bitrate_kbps = match (file_len, duration_secs) {
            (Some(len), Some(d)) if d > 0.5 => Some(((len as f64 * 8.0) / d / 1000.0).round() as u32),
            _ => None,
        };

        let lossless = codec_is_lossless(codec_short);
        let source = SourceFormat {
            codec: pretty_codec(codec_short, path),
            sample_rate: rate,
            bits_per_sample: params.bits_per_sample,
            channels,
            bitrate_kbps,
            lossless,
            float: !lossless,
        };

        let mut dec = Decoder {
            format,
            decoder,
            track_id,
            time_base,
            source,
            duration_secs,
            channels,
            rate,
            buf: Vec::with_capacity(8192),
            primed: false,
            skip_frames: 0,
        };
        dec.prime()?;
        Ok(dec)
    }

    /// Decode the first packet so we know the real channel count and sample format.
    fn prime(&mut self) -> Result<()> {
        if self.decode_packet()? {
            self.primed = true;
        }
        Ok(())
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    /// Decodes one packet into `self.buf`. Returns false at end of stream.
    fn decode_packet(&mut self) -> Result<bool> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(false),
                Err(SymError::ResetRequired) => return Ok(false),
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
                Err(e) => return Err(anyhow!("read error: {e}")),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(audio) => {
                    let ch = audio.spec().channels().count().max(1);
                    let int_bits = match &audio {
                        GenericAudioBufferRef::S8(_) | GenericAudioBufferRef::U8(_) => Some(8),
                        GenericAudioBufferRef::S16(_) | GenericAudioBufferRef::U16(_) => Some(16),
                        GenericAudioBufferRef::S24(_) | GenericAudioBufferRef::U24(_) => Some(24),
                        GenericAudioBufferRef::S32(_) | GenericAudioBufferRef::U32(_) => Some(32),
                        _ => None,
                    };
                    if !self.primed {
                        // Trust the decoded buffer over container metadata.
                        self.channels = ch;
                        self.source.channels = ch;
                        match int_bits {
                            Some(b) => {
                                self.source.float = false;
                                if self.source.bits_per_sample.is_none() {
                                    self.source.bits_per_sample = Some(b);
                                }
                            }
                            None => {
                                self.source.float = true;
                                if !self.source.lossless {
                                    self.source.bits_per_sample = None;
                                }
                            }
                        }
                    }
                    if audio.frames() == 0 {
                        continue;
                    }
                    audio.copy_to_vec_interleaved::<f64>(&mut self.buf);
                    if ch != self.channels {
                        // Channel layout changed mid-stream (rare, e.g. chained OGG). Remap naively.
                        let frames = self.buf.len() / ch;
                        let mut remapped = Vec::with_capacity(frames * self.channels);
                        for f in 0..frames {
                            for c in 0..self.channels {
                                remapped.push(self.buf[f * ch + c.min(ch - 1)]);
                            }
                        }
                        self.buf = remapped;
                    }
                    return Ok(true);
                }
                Err(SymError::DecodeError(e)) => {
                    log::warn!("skipping undecodable packet: {e}");
                    continue;
                }
                Err(SymError::IoError(_)) => continue,
                Err(e) => return Err(anyhow!("decode error: {e}")),
            }
        }
    }

    /// Returns the next chunk of interleaved samples, or `None` at end of stream.
    pub fn next_chunk(&mut self) -> Result<Option<&[f64]>> {
        if self.primed {
            self.primed = false;
        } else if !self.decode_packet()? {
            return Ok(None);
        }
        if self.skip_frames > 0 {
            let frames = self.buf.len() / self.channels;
            let skip = self.skip_frames.min(frames);
            self.skip_frames -= skip;
            self.buf.drain(..skip * self.channels);
            if self.buf.is_empty() {
                return self.next_chunk();
            }
        }
        Ok(Some(&self.buf))
    }

    /// Sample-accurate seek. Returns the position actually reached in seconds.
    pub fn seek(&mut self, secs: f64) -> Result<f64> {
        let secs = secs.max(0.0);
        let time = Time::try_from_secs_f64(secs).ok_or_else(|| anyhow!("invalid seek time"))?;
        let seeked = self
            .format
            .seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(self.track_id) })
            .map_err(|e| anyhow!("seek failed: {e}"))?;
        self.decoder.reset();
        self.primed = false;
        self.buf.clear();
        let required = seeked.required_ts.get();
        let actual = seeked.actual_ts.get();
        let delta_ts = (required - actual).max(0) as f64;
        // Convert timestamp delta to frames at the native rate.
        let delta_frames = match self.time_base {
            Some(tb) => delta_ts * tb.numer.get() as f64 / tb.denom.get() as f64 * self.rate as f64,
            None => delta_ts,
        };
        self.skip_frames = delta_frames.round() as usize;
        let reached = match self.time_base {
            Some(tb) => required as f64 * tb.numer.get() as f64 / tb.denom.get() as f64,
            None => secs,
        };
        Ok(reached)
    }
}
