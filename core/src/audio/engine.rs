//! The player thread: decodes ahead into a lock-free ring buffer, tracks segment boundaries for
//! gapless playback, handles seeking and (re)opens the output device when the format requires it.
//! With crossfade on, the last seconds of each decoded track are held back and mixed with the
//! start of the next one.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;
use rtrb::Producer;
use serde::Serialize;

use super::decoder::Decoder;
use super::dsp::{slider_to_gain, EngineShared};
use super::eq::EqSettings;
use super::output::{self, ExclusiveUnsupported, OutputHandle};
use super::resample::StreamResampler;
use super::types::{OutputFormat, OutputMode, PlayState, ReplayGainMode, SourceFormat};
use crate::library::model::Track;
use crate::queue::{PlayQueue, QueueItem};

pub enum Command {
    /// Start playing a queue item (by uid) at `start` seconds.
    PlayItem { uid: u64, start: f64, paused: bool },
    Play,
    Pause,
    Toggle,
    Stop,
    Seek(f64),
    Next,
    Previous,
    /// The queue was edited; re-validate any track decoded ahead of time.
    QueueChanged,
    SetOutput { mode: OutputMode, device_id: Option<String> },
    SetReplayGain { mode: ReplayGainMode, preamp_db: f64 },
    /// Crossfade length in seconds (0 = off, plain gapless).
    SetCrossfade(f64),
    /// Stop when the playing song ends instead of going on to the next one (sleep timer).
    StopAfterCurrent(bool),
    /// Remember where to resume a restored queue item when playback is next started.
    SetResume { uid: u64, position: f64 },
    /// Send the status now (an effect changed: the UI shows the speed and "bit-perfect").
    EmitStatus,
    Shutdown,
}

pub enum EngineEvent {
    Status(PlayerStatus),
    TrackChanged(Option<QueueItem>),
    Error(String),
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStatus {
    pub state: PlayState,
    pub position: f64,
    pub duration: f64,
    pub current_uid: Option<u64>,
    pub track: Option<Track>,
    pub source: Option<SourceFormat>,
    pub output: Option<OutputFormat>,
    pub output_mode: OutputMode,
    pub bit_perfect: bool,
    pub resampled: bool,
    pub volume: f64,
    pub muted: bool,
    pub replay_gain_db: Option<f64>,
    pub eq_active: bool,
    pub underruns: u64,
    /// Playback speed (1 = normal): the position moves this much faster than the clock.
    pub speed: f64,
    /// Speed or reverb is changing the sound.
    pub effects_active: bool,
}

pub struct EngineConfig {
    pub mode: OutputMode,
    pub device_id: Option<String>,
    pub volume: f64,
    pub eq: EqSettings,
    pub rg_mode: ReplayGainMode,
    pub rg_preamp: f64,
    pub crossfade: f64,
}

/// Longest crossfade, in seconds.
pub const MAX_CROSSFADE: f64 = 10.0;

pub struct Engine {
    tx: Sender<Command>,
    pub shared: Arc<EngineShared>,
    pub queue: Arc<Mutex<PlayQueue>>,
    status: Arc<Mutex<PlayerStatus>>,
    volume_slider: Arc<AtomicU64>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Engine {
    pub fn new(
        cfg: EngineConfig,
        queue: PlayQueue,
        emit: impl Fn(EngineEvent) + Send + 'static,
    ) -> Arc<Engine> {
        let shared = Arc::new(EngineShared::new(slider_to_gain(cfg.volume), cfg.eq.clone()));
        let queue = Arc::new(Mutex::new(queue));
        let status = Arc::new(Mutex::new(PlayerStatus {
            volume: cfg.volume,
            output_mode: cfg.mode,
            speed: 1.0,
            ..Default::default()
        }));
        let volume_slider = Arc::new(AtomicU64::new(cfg.volume.to_bits()));
        let (tx, rx) = unbounded();
        let player = Player {
            rx,
            shared: shared.clone(),
            queue: queue.clone(),
            status: status.clone(),
            volume_slider: volume_slider.clone(),
            emit: Box::new(emit),
            mode: cfg.mode,
            device_id: cfg.device_id,
            rg_mode: cfg.rg_mode,
            rg_preamp: cfg.rg_preamp,
            crossfade: cfg.crossfade.clamp(0.0, MAX_CROSSFADE),
            stop_after: false,
            stop_then: None,
            out: None,
            prod: None,
            dec: None,
            reopen: None,
            segments: VecDeque::new(),
            produced: 0,
            pending: Vec::with_capacity(1 << 16),
            pending_pos: 0,
            scratch: Vec::with_capacity(1 << 15),
            fresh: Vec::with_capacity(1 << 15),
            state: PlayState::Stopped,
            eos: false,
            last_emit: Instant::now(),
            recover_attempts: 0,
            resume: None,
        };
        let thread = std::thread::Builder::new()
            .name("audio-player".into())
            .spawn(move || player.run())
            .expect("spawn player thread");
        Arc::new(Engine {
            tx,
            shared,
            queue,
            status,
            volume_slider,
            thread: Mutex::new(Some(thread)),
        })
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    pub fn status(&self) -> PlayerStatus {
        let mut s = self.status.lock().clone();
        s.volume = self.volume();
        s.muted = self.shared.muted.load(Ordering::Relaxed);
        s.speed = self.shared.speed();
        s.effects_active = self.shared.effects_active();
        s
    }

    /// Playback speed (pitch follows) and reverb mix; both apply at once.
    pub fn set_effects(&self, speed: f64, reverb: f64) {
        self.shared.set_effects(speed, reverb);
        let _ = self.tx.send(Command::EmitStatus);
    }

    pub fn volume(&self) -> f64 {
        f64::from_bits(self.volume_slider.load(Ordering::Relaxed))
    }

    pub fn set_volume(&self, slider: f64) {
        let v = slider.clamp(0.0, 1.0);
        self.volume_slider.store(v.to_bits(), Ordering::Relaxed);
        self.shared.set_volume(slider_to_gain(v));
    }

    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
    }

    pub fn set_eq(&self, eq: EqSettings) {
        *self.shared.eq.lock() = eq;
        self.shared.eq_gen.fetch_add(1, Ordering::AcqRel);
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(t) = self.thread.lock().take() {
            let _ = t.join();
        }
    }
}

struct Decoding {
    item: QueueItem,
    decoder: Decoder,
    resampler: Option<StreamResampler>,
    out_channels: usize,
    rg_gain: f64,
    /// Samples kept back at the end of the track for a crossfade (0 = none).
    hold: usize,
    /// The newest `hold` decoded samples, not yet handed to the output.
    tail: VecDeque<f64>,
    /// Decoding reached the end (and the resampler was flushed).
    ended: bool,
}

struct Segment {
    item: QueueItem,
    /// Value of the `produced` counter where this track's first frame sits.
    start: u64,
    base_secs: f64,
    rate: u32,
    source: SourceFormat,
    duration: f64,
    resampled: bool,
    rg_db: Option<f64>,
}

struct Player {
    rx: Receiver<Command>,
    shared: Arc<EngineShared>,
    queue: Arc<Mutex<PlayQueue>>,
    status: Arc<Mutex<PlayerStatus>>,
    volume_slider: Arc<AtomicU64>,
    emit: Box<dyn Fn(EngineEvent) + Send>,
    mode: OutputMode,
    device_id: Option<String>,
    rg_mode: ReplayGainMode,
    rg_preamp: f64,
    crossfade: f64,
    stop_after: bool,
    /// Where the queue continues after a stop-after-this-song, set when the stop is decided.
    stop_then: Option<QueueItem>,
    out: Option<OutputHandle>,
    prod: Option<Producer<f64>>,
    dec: Option<Decoding>,
    /// Next track already opened but needing a different device format; started after drain.
    reopen: Option<(QueueItem, Decoder)>,
    segments: VecDeque<Segment>,
    /// Total frames handed to the output path (ring + pending), comparable with frames_played.
    produced: u64,
    pending: Vec<f64>,
    pending_pos: usize,
    scratch: Vec<f64>,
    fresh: Vec<f64>,
    state: PlayState,
    /// Decoding finished and nothing follows in the queue.
    eos: bool,
    last_emit: Instant,
    recover_attempts: u32,
    resume: Option<(u64, f64)>,
}

fn rg_gain(track: &Track, mode: ReplayGainMode, preamp: f64) -> (f64, Option<f64>) {
    let (gain, peak) = match mode {
        ReplayGainMode::Off => return (1.0, None),
        ReplayGainMode::Track => (track.rg_track_gain, track.rg_track_peak),
        ReplayGainMode::Album => (
            track.rg_album_gain.or(track.rg_track_gain),
            track.rg_album_peak.or(track.rg_track_peak),
        ),
    };
    let Some(g) = gain else { return (1.0, None) };
    let mut lin = 10f64.powf((g + preamp) / 20.0);
    // Never let ReplayGain push the track's peak above full scale.
    if let Some(p) = peak {
        if p > 0.0 && lin * p > 1.0 {
            lin = 1.0 / p;
        }
    }
    (lin, Some(20.0 * lin.log10()))
}

fn map_channels(input: &[f64], in_ch: usize, out_ch: usize, dst: &mut Vec<f64>) {
    dst.clear();
    if in_ch == out_ch {
        dst.extend_from_slice(input);
        return;
    }
    let frames = input.len() / in_ch;
    dst.reserve(frames * out_ch);
    let k = std::f64::consts::FRAC_1_SQRT_2;
    for f in 0..frames {
        let s = &input[f * in_ch..(f + 1) * in_ch];
        if in_ch == 1 {
            for c in 0..out_ch {
                dst.push(if c < 2 { s[0] } else { 0.0 });
            }
        } else if out_ch == 1 {
            dst.push(s.iter().sum::<f64>() / in_ch as f64);
        } else if out_ch == 2 {
            // Downmix (WAVE order: FL FR FC LFE BL BR SL SR); LFE dropped.
            let (mut l, mut r) = (s[0], s[1]);
            let mut extra = 0.0;
            for (i, v) in s.iter().enumerate().skip(2) {
                match i {
                    2 => {
                        l += v * k;
                        r += v * k;
                        extra += k;
                    }
                    3 => {}
                    _ if i % 2 == 0 => {
                        l += v * k;
                        extra += k / 2.0;
                    }
                    _ => {
                        r += v * k;
                        extra += k / 2.0;
                    }
                }
            }
            let norm = 1.0 / (1.0 + extra);
            dst.push(l * norm);
            dst.push(r * norm);
        } else {
            for c in 0..out_ch {
                dst.push(if c < in_ch { s[c] } else { 0.0 });
            }
        }
    }
}

/// Decodes the next chunk of `dec` into `out`: channel-mapped, resampled and with ReplayGain.
/// Returns whether the track ended, and an error message for the user.
fn decode_into(dec: &mut Decoding, scratch: &mut Vec<f64>, out: &mut Vec<f64>) -> (bool, Option<String>) {
    if dec.ended {
        return (true, None);
    }
    let before = out.len();
    let ch_out = dec.out_channels;
    let ch_in = dec.decoder.channels();
    let mut finished = false;
    let mut error = None;
    match dec.decoder.next_chunk() {
        Ok(Some(chunk)) => {
            map_channels(chunk, ch_in, ch_out, scratch);
            match dec.resampler.as_mut() {
                Some(r) => {
                    if let Err(e) = r.process(scratch, out) {
                        log::error!("{e}");
                        finished = true;
                    }
                }
                None => out.extend_from_slice(scratch),
            }
        }
        Ok(None) => finished = true,
        Err(e) => {
            error = Some(format!("Error while decoding \"{}\": {e}", dec.item.track.title));
            finished = true;
        }
    }
    if finished {
        if let Some(r) = dec.resampler.as_mut() {
            let _ = r.finish(out);
        }
        dec.ended = true;
    }
    if dec.rg_gain != 1.0 {
        let g = dec.rg_gain;
        for s in &mut out[before..] {
            *s *= g;
        }
    }
    (finished, error)
}

/// Equal-power crossfade of `tail` (fading out) with `head` (fading in) into `dst`.
fn mix_crossfade(tail: &VecDeque<f64>, head: &[f64], channels: usize, dst: &mut Vec<f64>) {
    let frames = (tail.len() / channels).max(1) as f64;
    dst.reserve(tail.len());
    for (i, s) in tail.iter().enumerate() {
        let t = (i / channels) as f64 / frames * std::f64::consts::FRAC_PI_2;
        let h = head.get(i).copied().unwrap_or(0.0);
        dst.push(s * t.cos() + h * t.sin());
    }
}

impl Player {
    fn run(mut self) {
        loop {
            let ring_has_room = self.prod.as_ref().map(|p| p.slots() > 4096).unwrap_or(false);
            let busy = self.state != PlayState::Stopped
                && ring_has_room
                && (self.dec.is_some() || self.pending_pos < self.pending.len());
            let timeout = if busy { Duration::from_millis(1) } else { Duration::from_millis(15) };
            match self.rx.recv_timeout(timeout) {
                Ok(cmd) => {
                    if !self.handle(cmd) {
                        break;
                    }
                    let mut stop = false;
                    while let Ok(c) = self.rx.try_recv() {
                        if !self.handle(c) {
                            stop = true;
                            break;
                        }
                    }
                    if stop {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if self.state != PlayState::Stopped {
                self.pump();
            }
            self.check_progress();
            if self.state == PlayState::Playing && self.last_emit.elapsed() >= Duration::from_millis(200) {
                self.emit_status();
            }
        }
        self.hard_stop();
    }

    // -----------------------------------------------------------------------------------------
    // Commands
    // -----------------------------------------------------------------------------------------

    fn handle(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::PlayItem { uid, start, paused } => {
                let item = self.queue.lock().get(uid).cloned();
                if let Some(item) = item {
                    self.start_item_or_skip(item, start, !paused);
                }
            }
            Command::Play => match self.state {
                PlayState::Paused => {
                    self.shared.paused.store(false, Ordering::Relaxed);
                    self.state = PlayState::Playing;
                    if self.out.is_none() {
                        self.resume_from_current(true);
                    }
                    self.emit_status();
                }
                PlayState::Stopped => self.resume_from_current(true),
                PlayState::Playing => {}
            },
            Command::Pause => {
                if self.state == PlayState::Playing {
                    self.shared.paused.store(true, Ordering::Relaxed);
                    self.state = PlayState::Paused;
                    self.emit_status();
                }
            }
            Command::Toggle => {
                let cmd = if self.state == PlayState::Playing { Command::Pause } else { Command::Play };
                return self.handle(cmd);
            }
            Command::Stop => {
                self.hard_stop();
                self.emit_status();
            }
            Command::Seek(secs) => self.seek(secs),
            Command::Next => {
                let cur = self.playing_uid();
                let next = self.queue.lock().next_after(cur, false);
                match next {
                    Some(item) => {
                        let play = self.state != PlayState::Paused;
                        self.start_item_or_skip(item, 0.0, play);
                    }
                    None => {
                        self.hard_stop();
                        self.emit_status();
                    }
                }
            }
            Command::Previous => {
                if self.position() > 3.0 {
                    self.seek(0.0);
                } else {
                    let cur = self.playing_uid();
                    let prev = self.queue.lock().prev_before(cur);
                    if let Some(item) = prev {
                        let play = self.state != PlayState::Paused;
                        self.start_item_or_skip(item, 0.0, play);
                    } else {
                        self.seek(0.0);
                    }
                }
            }
            Command::QueueChanged => self.on_queue_changed(),
            Command::SetOutput { mode, device_id } => {
                self.mode = mode;
                self.device_id = device_id;
                self.status.lock().output_mode = mode;
                if let Some(seg) = self.segments.front() {
                    let item = seg.item.clone();
                    let pos = self.position();
                    let play = self.state == PlayState::Playing;
                    self.close_output();
                    self.start_item(item, pos, play);
                } else {
                    self.close_output();
                }
                self.emit_status();
            }
            Command::SetReplayGain { mode, preamp_db } => {
                self.rg_mode = mode;
                self.rg_preamp = preamp_db;
                if !self.segments.is_empty() {
                    let pos = self.position();
                    self.seek(pos);
                }
            }
            // Applies from the next song decoded (the playing one keeps what it holds back), so
            // dragging the slider never interrupts playback.
            Command::SetCrossfade(secs) => self.crossfade = if secs.is_finite() { secs.clamp(0.0, MAX_CROSSFADE) } else { 0.0 },
            Command::StopAfterCurrent(on) => {
                let decided = self.stop_then.is_some();
                if on != self.stop_after || (!on && decided) {
                    self.stop_after = on;
                    // Whatever was decoded ahead was decided without this: decode it again.
                    let ahead = self.segments.len() > 1 || self.reopen.is_some() || self.eos;
                    if ahead && (on || decided) {
                        let pos = self.position();
                        self.seek(pos);
                    }
                }
            }
            Command::SetResume { uid, position } => self.resume = Some((uid, position)),
            Command::EmitStatus => self.emit_status(),
            Command::Shutdown => return false,
        }
        true
    }

    fn playing_uid(&self) -> Option<u64> {
        self.segments
            .front()
            .map(|s| s.item.uid)
            .or_else(|| self.queue.lock().current_uid())
    }

    fn resume_from_current(&mut self, play: bool) {
        let item = {
            let q = self.queue.lock();
            q.current().cloned().or_else(|| q.next_after(None, false))
        };
        if let Some(item) = item {
            let resume = self.resume.take().filter(|(uid, _)| *uid == item.uid).map(|(_, p)| p);
            let pos = if self.segments.front().map(|s| s.item.uid) == Some(item.uid) {
                self.position()
            } else {
                resume.unwrap_or(0.0)
            };
            self.start_item_or_skip(item, pos, play);
        }
    }

    /// Start `item`; if it cannot be opened, report and try the following items.
    fn start_item_or_skip(&mut self, item: QueueItem, start: f64, play: bool) {
        let mut candidate = Some(item);
        let max = self.queue.lock().len().max(1);
        for _ in 0..max {
            let Some(item) = candidate.take() else { break };
            let uid = item.uid;
            if self.start_item(item, start, play) {
                return;
            }
            candidate = self.queue.lock().next_after(Some(uid), false);
        }
        self.hard_stop();
        self.emit_status();
    }

    fn start_item(&mut self, item: QueueItem, start: f64, play: bool) -> bool {
        self.flush();
        self.dec = None;
        self.reopen = None;
        self.segments.clear();
        self.eos = false;
        self.stop_then = None;

        let mut decoder = match Decoder::open(Path::new(&item.track.path)) {
            Ok(d) => d,
            Err(e) => {
                self.error(format!("Cannot play \"{}\": {e}", item.track.title));
                return false;
            }
        };
        let mut base = 0.0;
        if start > 0.05 {
            match decoder.seek(start) {
                Ok(b) => base = b,
                Err(e) => log::warn!("seek on open failed: {e}"),
            }
        }
        self.shared.paused.store(!play, Ordering::Relaxed);
        if let Err(e) = self.ensure_output(&decoder.source) {
            self.error(format!("Cannot open audio output: {e}"));
            return false;
        }
        let d = match self.make_decoding(item.clone(), decoder) {
            Ok(d) => d,
            Err(e) => {
                self.error(format!("Cannot play \"{}\": {e}", item.track.title));
                return false;
            }
        };
        self.push_segment(&d, base);
        self.dec = Some(d);
        self.state = if play { PlayState::Playing } else { PlayState::Paused };
        self.recover_attempts = 0;
        self.queue.lock().set_current(Some(item.uid));
        (self.emit)(EngineEvent::TrackChanged(Some(item)));
        self.emit_status();
        true
    }

    fn push_segment(&mut self, d: &Decoding, base: f64) {
        let fmt = &self.out.as_ref().expect("output open").format;
        let (_, rg_db) = rg_gain(&d.item.track, self.rg_mode, self.rg_preamp);
        let duration = d
            .decoder
            .duration_secs
            .filter(|v| *v > 0.0)
            .unwrap_or(d.item.track.duration);
        self.segments.push_back(Segment {
            item: d.item.clone(),
            // `produced` already counts samples still waiting in `pending`.
            start: self.produced,
            base_secs: base,
            rate: fmt.sample_rate,
            source: d.decoder.source.clone(),
            duration,
            resampled: d.resampler.is_some(),
            rg_db,
        });
    }

    fn make_decoding(&self, item: QueueItem, decoder: Decoder) -> Result<Decoding> {
        let fmt = &self.out.as_ref().expect("output open").format;
        let resampler = if decoder.sample_rate() != fmt.sample_rate {
            Some(StreamResampler::new(decoder.sample_rate(), fmt.sample_rate, fmt.channels)?)
        } else {
            None
        };
        let (rg, _) = rg_gain(&item.track, self.rg_mode, self.rg_preamp);
        let hold = if self.crossfade > 0.0 && item.track.duration > 2.0 * self.crossfade + 2.0 {
            (self.crossfade * fmt.sample_rate as f64) as usize * fmt.channels
        } else {
            0
        };
        Ok(Decoding {
            item,
            decoder,
            resampler,
            out_channels: fmt.channels,
            rg_gain: rg,
            hold,
            tail: VecDeque::new(),
            ended: false,
        })
    }

    fn seek(&mut self, secs: f64) {
        let Some(seg) = self.segments.front() else { return };
        let item = seg.item.clone();
        let secs = secs.clamp(0.0, (seg.duration - 0.05).max(0.0));
        // Reuse the decoder when it is still on the playing track; otherwise reopen it.
        let decoder = match self.dec.take() {
            Some(d) if d.item.uid == item.uid => Some(d.decoder),
            _ => None,
        };
        let mut decoder = match decoder {
            Some(d) => d,
            None => match Decoder::open(Path::new(&item.track.path)) {
                Ok(d) => d,
                Err(e) => {
                    self.error(format!("Seek failed: {e}"));
                    return;
                }
            },
        };
        self.flush();
        self.reopen = None;
        self.eos = false;
        self.stop_then = None;
        let base = match decoder.seek(secs) {
            Ok(b) => b,
            Err(e) => {
                self.error(format!("Seek failed: {e}"));
                0.0
            }
        };
        if self.out.is_none() {
            if let Err(e) = self.ensure_output(&decoder.source) {
                self.error(format!("Cannot open audio output: {e}"));
                return;
            }
        }
        match self.make_decoding(item, decoder) {
            Ok(d) => {
                self.segments.clear();
                self.push_segment(&d, base);
                self.dec = Some(d);
            }
            Err(e) => self.error(format!("Seek failed: {e}")),
        }
        self.emit_status();
    }

    fn on_queue_changed(&mut self) {
        let playing = self.segments.front().map(|s| s.item.uid);
        let Some(playing) = playing else { return };
        let expected = self.queue.lock().next_after(Some(playing), true).map(|i| i.uid);
        let ahead = if self.segments.len() > 1 {
            Some(self.segments.back().map(|s| s.item.uid))
        } else if let Some((item, _)) = &self.reopen {
            Some(Some(item.uid))
        } else if self.eos {
            Some(None)
        } else {
            None
        };
        if let Some(ahead) = ahead {
            if ahead != expected {
                // Whatever was decoded ahead is stale: re-seek the current track in place.
                let pos = self.position();
                self.seek(pos);
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Output management
    // -----------------------------------------------------------------------------------------

    fn output_compatible(&self, src: &SourceFormat) -> bool {
        let Some(out) = &self.out else { return false };
        match self.mode {
            OutputMode::Shared => {
                out.format.mode == OutputMode::Shared
                    && (self.device_id.is_none() || self.device_id == out.format.device_id)
            }
            OutputMode::Exclusive => {
                out.format.mode == OutputMode::Exclusive
                    && out.negotiated_for == Some((src.sample_rate, src.bits_per_sample, src.float, src.channels))
            }
        }
    }

    fn ensure_output(&mut self, src: &SourceFormat) -> Result<()> {
        if self.output_compatible(src) {
            return Ok(());
        }
        // Exclusive mode falls back to shared mode for formats the device can't take.
        let fallback_from_exclusive = self.mode == OutputMode::Exclusive
            && self.out.as_ref().map(|o| o.format.mode == OutputMode::Shared).unwrap_or(false);
        self.close_output();
        match output::open(self.mode, self.device_id.clone(), src, self.shared.clone()) {
            Ok((h, p)) => {
                self.out = Some(h);
                self.prod = Some(p);
                Ok(())
            }
            Err(e) if self.mode == OutputMode::Exclusive => {
                let reason = if e.downcast_ref::<ExclusiveUnsupported>().is_some() {
                    format!("{e}. Using shared mode for this track.")
                } else {
                    format!("Exclusive mode unavailable: {e}. Using shared mode.")
                };
                if !fallback_from_exclusive {
                    self.error(reason);
                }
                let (h, p) = output::open(OutputMode::Shared, self.device_id.clone(), src, self.shared.clone())?;
                self.out = Some(h);
                self.prod = Some(p);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn close_output(&mut self) {
        self.shared.expect_data.store(false, Ordering::Relaxed);
        self.prod = None;
        self.out = None; // Drop joins the output thread and releases the device.
        self.pending.clear();
        self.pending_pos = 0;
        self.produced = self.shared.frames_played.load(Ordering::Acquire);
    }

    /// Discard everything buffered between the decoder and the speakers.
    fn flush(&mut self) {
        self.shared.expect_data.store(false, Ordering::Relaxed);
        self.pending.clear();
        self.pending_pos = 0;
        if self.out.is_some() {
            let g = self.shared.flush_gen.fetch_add(1, Ordering::AcqRel) + 1;
            let t0 = Instant::now();
            while self.shared.flush_ack.load(Ordering::Acquire) < g && t0.elapsed() < Duration::from_millis(400) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        self.produced = self.shared.frames_played.load(Ordering::Acquire);
    }

    fn hard_stop(&mut self) {
        self.flush();
        self.dec = None;
        self.reopen = None;
        self.segments.clear();
        self.eos = false;
        self.close_output();
        self.state = PlayState::Stopped;
        self.shared.paused.store(true, Ordering::Relaxed);
    }

    // -----------------------------------------------------------------------------------------
    // Decoding
    // -----------------------------------------------------------------------------------------

    /// Push pending samples to the ring. Returns true when everything pending was written.
    fn drain_pending(&mut self) -> bool {
        if self.dec.is_none() {
            self.shared.expect_data.store(false, Ordering::Relaxed);
        }
        let Some(prod) = self.prod.as_mut() else { return false };
        let remaining = self.pending.len() - self.pending_pos;
        if remaining == 0 {
            return true;
        }
        let n = remaining.min(prod.slots());
        if n > 0 {
            if let Ok(mut chunk) = prod.write_chunk(n) {
                let (a, b) = chunk.as_mut_slices();
                let src = &self.pending[self.pending_pos..self.pending_pos + n];
                a.copy_from_slice(&src[..a.len()]);
                b.copy_from_slice(&src[a.len()..]);
                chunk.commit_all();
                self.pending_pos += n;
            }
            // Only arm glitch detection once the ring holds a healthy amount of audio.
            let armed = self.dec.is_some() && prod.slots() < prod.buffer().capacity() / 2;
            if armed {
                self.shared.expect_data.store(true, Ordering::Relaxed);
            }
        }
        if self.pending_pos >= self.pending.len() {
            self.pending.clear();
            self.pending_pos = 0;
            true
        } else {
            false
        }
    }

    fn pump(&mut self) {
        let t0 = Instant::now();
        loop {
            if !self.drain_pending() {
                return;
            }
            if t0.elapsed() > Duration::from_millis(20) {
                return;
            }
            let Some(dec) = self.dec.as_mut() else { return };
            let ch_out = dec.out_channels;
            let before = self.pending.len();
            let (finished, error) = if dec.hold == 0 {
                decode_into(dec, &mut self.scratch, &mut self.pending)
            } else {
                // Keep the newest `hold` samples back for the crossfade; release the rest.
                self.fresh.clear();
                let r = decode_into(dec, &mut self.scratch, &mut self.fresh);
                dec.tail.extend(self.fresh.drain(..));
                let excess = dec.tail.len().saturating_sub(dec.hold);
                self.pending.extend(dec.tail.drain(..excess));
                r
            };
            if let Some(e) = error {
                self.error(e);
            }
            self.produced += ((self.pending.len() - before) / ch_out) as u64;
            if finished {
                let done = self.dec.take().expect("decoder present");
                self.advance_decoder(done.item, done.tail, ch_out);
            }
        }
    }

    /// Hands samples held back for a crossfade to the output unchanged.
    fn release_tail(&mut self, tail: VecDeque<f64>, channels: usize) {
        if tail.is_empty() {
            return;
        }
        self.produced += (tail.len() / channels.max(1)) as u64;
        self.pending.extend(tail);
    }

    /// Crossfade only between different songs that aren't from the same album (albums stay
    /// gapless), and only into songs long enough for it.
    fn crossfades(&self, from: &Track, to: &Track) -> bool {
        let same_album = !from.album.is_empty()
            && from.album == to.album
            && from.album_artist_or_artist() == to.album_artist_or_artist();
        self.crossfade > 0.0 && from.path != to.path && !same_album && to.duration > 2.0 * self.crossfade + 2.0
    }

    /// Mixes the end of the previous track (`tail`) with the start of `d`.
    fn crossfade_into(&mut self, d: &mut Decoding, tail: VecDeque<f64>) {
        let need = tail.len();
        let mut head = Vec::with_capacity(need + (1 << 15));
        while head.len() < need {
            let (finished, error) = decode_into(d, &mut self.scratch, &mut head);
            if let Some(e) = error {
                self.error(e);
            }
            if finished {
                break;
            }
        }
        let ch = d.out_channels;
        let before = self.pending.len();
        mix_crossfade(&tail, &head, ch, &mut self.pending);
        if head.len() > need {
            self.pending.extend_from_slice(&head[need..]);
        }
        self.produced += ((self.pending.len() - before) / ch) as u64;
    }

    /// Current track fully decoded: open the next one for gapless continuation (or a crossfade
    /// into it, with the held-back `tail`).
    fn advance_decoder(&mut self, finished: QueueItem, mut tail: VecDeque<f64>, channels: usize) {
        if self.stop_after {
            // Sleep timer: this song is the last one. Play resumes with the next one.
            self.stop_after = false;
            self.release_tail(tail, channels);
            self.stop_then = self.queue.lock().next_after(Some(finished.uid), true);
            self.eos = true;
            return;
        }
        let mut after = finished.uid;
        let max = self.queue.lock().len().max(1);
        for _ in 0..max {
            let next = self.queue.lock().next_after(Some(after), true);
            let Some(item) = next else { break };
            match Decoder::open(Path::new(&item.track.path)) {
                Ok(decoder) => {
                    if self.output_compatible(&decoder.source) {
                        match self.make_decoding(item.clone(), decoder) {
                            Ok(mut d) => {
                                let tail = std::mem::take(&mut tail);
                                if !tail.is_empty() && self.crossfades(&finished.track, &d.item.track) {
                                    self.push_segment(&d, 0.0);
                                    self.crossfade_into(&mut d, tail);
                                } else {
                                    self.release_tail(tail, channels);
                                    self.push_segment(&d, 0.0);
                                }
                                self.dec = Some(d);
                                return;
                            }
                            Err(e) => self.error(format!("Cannot play \"{}\": {e}", item.track.title)),
                        }
                    } else {
                        let tail = std::mem::take(&mut tail);
                        self.release_tail(tail, channels);
                        self.reopen = Some((item, decoder));
                        return;
                    }
                }
                Err(e) => {
                    self.error(format!("Cannot play \"{}\": {e}", item.track.title));
                }
            }
            if item.uid == after {
                break; // repeat-one on a broken file
            }
            after = item.uid;
        }
        self.release_tail(tail, channels);
        self.eos = true;
    }

    // -----------------------------------------------------------------------------------------
    // Progress
    // -----------------------------------------------------------------------------------------

    fn position(&self) -> f64 {
        let Some(seg) = self.segments.front() else { return 0.0 };
        let played = self.shared.frames_played.load(Ordering::Acquire);
        let rel = played.saturating_sub(seg.start);
        (seg.base_secs + rel as f64 / seg.rate as f64).min(seg.duration.max(0.0))
    }

    fn check_progress(&mut self) {
        let played = self.shared.frames_played.load(Ordering::Acquire);

        // Gapless boundary crossed?
        let mut changed = false;
        while self.segments.len() > 1 && played >= self.segments[1].start {
            self.segments.pop_front();
            changed = true;
        }
        if changed {
            let item = self.segments.front().map(|s| s.item.clone());
            self.queue.lock().set_current(item.as_ref().map(|i| i.uid));
            (self.emit)(EngineEvent::TrackChanged(item));
            self.emit_status();
        }

        // Device failure (unplugged, invalidated, ...): try to resume on the current device.
        if let Some(err) = self.out.as_ref().and_then(|o| o.take_error()) {
            self.error(err);
            self.recover_output();
            return;
        }

        let drained = self.pending_pos >= self.pending.len() && played >= self.produced;
        if !drained || self.state == PlayState::Stopped {
            return;
        }
        if let Some((item, decoder)) = self.reopen.take() {
            // Next track needs a different device format (exclusive mode): reopen now that the
            // previous track has fully played.
            self.close_output();
            if let Err(e) = self.ensure_output(&decoder.source) {
                self.error(format!("Cannot open audio output: {e}"));
                self.hard_stop();
                self.emit_status();
                return;
            }
            match self.make_decoding(item.clone(), decoder) {
                Ok(d) => {
                    self.segments.clear();
                    self.push_segment(&d, 0.0);
                    self.dec = Some(d);
                    self.queue.lock().set_current(Some(item.uid));
                    (self.emit)(EngineEvent::TrackChanged(Some(item)));
                    self.emit_status();
                }
                Err(e) => {
                    self.error(format!("Cannot play \"{}\": {e}", item.track.title));
                    self.advance_decoder(item, VecDeque::new(), 1);
                }
            }
        } else if self.eos && self.dec.is_none() {
            // End of the queue, or the song the sleep timer stops after.
            let then = self.stop_then.take();
            self.hard_stop();
            if let Some(item) = then {
                self.queue.lock().set_current(Some(item.uid));
                (self.emit)(EngineEvent::TrackChanged(Some(item)));
            }
            self.emit_status();
        }
    }

    fn recover_output(&mut self) {
        self.recover_attempts += 1;
        let Some(seg) = self.segments.front() else {
            self.hard_stop();
            return;
        };
        let item = seg.item.clone();
        let pos = self.position();
        let play = self.state == PlayState::Playing;
        self.close_output();
        if self.recover_attempts > 3 {
            self.hard_stop();
            self.emit_status();
            return;
        }
        std::thread::sleep(Duration::from_millis(300));
        if !self.start_item(item, pos, play) {
            self.hard_stop();
            self.emit_status();
        }
    }

    fn error(&self, msg: String) {
        log::warn!("{msg}");
        (self.emit)(EngineEvent::Error(msg));
    }

    fn emit_status(&mut self) {
        self.last_emit = Instant::now();
        let seg = self.segments.front();
        let out = self.out.as_ref().map(|o| o.format.clone());
        let identity = self.shared.identity.load(Ordering::Relaxed);
        let bit_perfect = match (seg, &out) {
            (Some(seg), Some(o)) => {
                o.mode == OutputMode::Exclusive
                    && !seg.resampled
                    && !seg.source.float
                    && !o.float
                    && seg.rg_db.map(|d| d.abs() < 1e-9).unwrap_or(true)
                    && seg.source.channels == o.channels
                    && seg.source.bits_per_sample.map(|b| b <= o.bits).unwrap_or(false)
                    && (identity
                        || self.state != PlayState::Playing
                            && !self.shared.eq.lock().is_active()
                            && !self.shared.effects_active()
                            && self.volume_is_unity())
            }
            _ => false,
        };
        let status = PlayerStatus {
            state: self.state,
            position: self.position(),
            duration: seg.map(|s| s.duration).unwrap_or(0.0),
            current_uid: seg.map(|s| s.item.uid),
            track: seg.map(|s| s.item.track.clone()),
            source: seg.map(|s| s.source.clone()),
            output: out,
            output_mode: self.mode,
            bit_perfect,
            resampled: seg.map(|s| s.resampled).unwrap_or(false),
            volume: f64::from_bits(self.volume_slider.load(Ordering::Relaxed)),
            muted: self.shared.muted.load(Ordering::Relaxed),
            replay_gain_db: seg.and_then(|s| s.rg_db),
            eq_active: self.shared.eq.lock().is_active(),
            underruns: self.shared.underruns.load(Ordering::Relaxed),
            speed: self.shared.speed(),
            effects_active: self.shared.effects_active(),
        };
        *self.status.lock() = status.clone();
        (self.emit)(EngineEvent::Status(status));
    }

    fn volume_is_unity(&self) -> bool {
        self.shared.volume() == 1.0 && !self.shared.muted.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossfade_fades_one_song_out_and_the_next_in() {
        // Two channels, four frames: the old song at 1.0, the new one at 0.5.
        let tail: VecDeque<f64> = std::iter::repeat_n(1.0, 8).collect();
        let head = vec![0.5; 8];
        let mut out = Vec::new();
        mix_crossfade(&tail, &head, 2, &mut out);
        assert_eq!(out.len(), 8);
        assert_eq!((out[0], out[1]), (1.0, 1.0), "starts with only the old song");
        let last = 6;
        let t = 3.0 / 4.0 * std::f64::consts::FRAC_PI_2;
        assert!((out[last] - (t.cos() + 0.5 * t.sin())).abs() < 1e-12);
        // A next song shorter than the fade mixes with silence.
        let mut out = Vec::new();
        mix_crossfade(&tail, &head[..2], 2, &mut out);
        assert!((out[7] - t.cos()).abs() < 1e-12);
    }
}
