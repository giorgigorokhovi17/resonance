//! Real-time render path shared by both output backends: pulls samples from the ring buffer (at
//! the playback speed), applies EQ, reverb, preamp and volume in 64-bit float, feeds the
//! visualizer, and reports whether the chain was bit-transparent.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use rtrb::Consumer;

use super::effects::{Reverb, Varispeed};
use super::eq::{EqProcessor, EqSettings};
use super::viz::VizTap;

/// State shared between the player thread, the output thread and the UI.
pub struct EngineShared {
    /// Frames taken from the ring buffer by the output (monotonic).
    pub frames_played: AtomicU64,
    pub flush_gen: AtomicU64,
    pub flush_ack: AtomicU64,
    pub paused: AtomicBool,
    /// Linear gain as f64 bits.
    volume_bits: AtomicU64,
    pub muted: AtomicBool,
    pub eq: Mutex<EqSettings>,
    pub eq_gen: AtomicU64,
    /// Set by the renderer: the last callback passed samples through unmodified.
    pub identity: AtomicBool,
    pub underruns: AtomicU64,
    /// The player has more audio coming: an empty ring now is a real glitch (not start/end/seek).
    pub expect_data: AtomicBool,
    /// Playback speed and reverb mix (0..1), as f64 bits.
    speed_bits: AtomicU64,
    reverb_bits: AtomicU64,
    /// What's playing, for the visualizer.
    pub viz: VizTap,
}

impl EngineShared {
    pub fn new(volume: f64, eq: EqSettings) -> Self {
        Self {
            frames_played: AtomicU64::new(0),
            flush_gen: AtomicU64::new(0),
            flush_ack: AtomicU64::new(0),
            paused: AtomicBool::new(true),
            volume_bits: AtomicU64::new(volume.to_bits()),
            muted: AtomicBool::new(false),
            eq: Mutex::new(eq),
            eq_gen: AtomicU64::new(1),
            identity: AtomicBool::new(false),
            underruns: AtomicU64::new(0),
            expect_data: AtomicBool::new(false),
            speed_bits: AtomicU64::new(1.0f64.to_bits()),
            reverb_bits: AtomicU64::new(0.0f64.to_bits()),
            viz: VizTap::default(),
        }
    }

    pub fn speed(&self) -> f64 {
        f64::from_bits(self.speed_bits.load(Ordering::Relaxed))
    }

    pub fn reverb(&self) -> f64 {
        f64::from_bits(self.reverb_bits.load(Ordering::Relaxed))
    }

    pub fn set_effects(&self, speed: f64, reverb: f64) {
        self.speed_bits.store(speed.to_bits(), Ordering::Relaxed);
        self.reverb_bits.store(reverb.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// Speed or reverb changes the sound.
    pub fn effects_active(&self) -> bool {
        self.speed() != 1.0 || self.reverb() > 0.0
    }

    pub fn volume(&self) -> f64 {
        f64::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }

    pub fn set_volume(&self, v: f64) {
        self.volume_bits.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
}

/// Maps the UI volume slider (0..1) to linear gain with a perceptual (cubic) taper.
/// 1.0 maps to exactly unity so full volume stays bit-perfect.
pub fn slider_to_gain(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v >= 1.0 {
        1.0
    } else {
        v * v * v
    }
}

pub struct Renderer {
    cons: Consumer<f64>,
    shared: Arc<EngineShared>,
    channels: usize,
    rate: u32,
    eq: EqProcessor,
    eq_gen: u64,
    flush_ack: u64,
    gain: f64,
    gain_coeff: f64,
    fade: f64,
    fade_step: f64,
    varispeed: Varispeed,
    /// Made the first time reverb is turned on, then kept.
    reverb: Option<Reverb>,
    reverb_on: bool,
}

impl Renderer {
    pub fn new(cons: Consumer<f64>, shared: Arc<EngineShared>, channels: usize, rate: u32) -> Self {
        let eq_settings = shared.eq.lock().clone();
        let eq = EqProcessor::new(&eq_settings, rate, channels);
        let eq_gen = shared.eq_gen.load(Ordering::Acquire);
        let flush_ack = shared.flush_gen.load(Ordering::Acquire);
        shared.flush_ack.store(flush_ack, Ordering::Release);
        let mut r = Self {
            cons,
            shared,
            channels,
            rate,
            eq,
            eq_gen,
            flush_ack,
            gain: 0.0,
            // ~5 ms one-pole smoothing for volume changes.
            gain_coeff: 1.0 - (-1.0 / (0.005 * rate as f64)).exp(),
            fade: 0.0,
            // 8 ms linear fade for pause/resume de-clicking.
            fade_step: 1.0 / (0.008 * rate as f64),
            varispeed: Varispeed::new(channels),
            reverb: None,
            reverb_on: false,
        };
        r.gain = r.target_gain();
        // Start fully open when already playing so the first samples are untouched.
        r.fade = if r.shared.paused.load(Ordering::Relaxed) { 0.0 } else { 1.0 };
        r
    }

    fn target_gain(&self) -> f64 {
        if self.shared.muted.load(Ordering::Relaxed) {
            0.0
        } else {
            self.shared.volume() * self.eq.preamp_linear()
        }
    }

    /// Fill `out` (interleaved, length multiple of `channels`) with processed samples.
    pub fn render(&mut self, out: &mut [f64]) {
        let ch = self.channels;

        // 1. Flush request (seek / stop): discard everything buffered.
        let flush = self.shared.flush_gen.load(Ordering::Acquire);
        if flush != self.flush_ack {
            let n = self.cons.slots();
            if let Ok(chunk) = self.cons.read_chunk(n) {
                chunk.commit_all();
            }
            self.varispeed.clear();
            self.flush_ack = flush;
            self.shared.flush_ack.store(flush, Ordering::Release);
        }

        // 2. Pick up EQ changes without ever blocking.
        let g = self.shared.eq_gen.load(Ordering::Acquire);
        if g != self.eq_gen {
            if let Some(settings) = self.shared.eq.try_lock() {
                self.eq.update(&settings, self.rate, ch);
                self.eq_gen = g;
            }
        }

        let paused = self.shared.paused.load(Ordering::Relaxed);
        let target_fade = if paused { 0.0 } else { 1.0 };
        if paused && self.fade <= 0.0 {
            out.fill(0.0);
            self.shared.identity.store(false, Ordering::Relaxed);
            return;
        }

        // 3. Pull samples: straight from the ring at normal speed, through varispeed otherwise.
        // Frames taken from the ring count as played, so the position stays in song time.
        let want = out.len() / ch * ch;
        let speed = self.shared.speed();
        let mut n = 0;
        if speed == 1.0 && !self.varispeed.is_empty() {
            // Back to normal speed: first what varispeed still holds.
            n = self.varispeed.drain_into(&mut out[..want]);
        }
        if speed == 1.0 {
            n += self.pull(&mut out[n..want]);
        } else {
            let frames = self.varispeed.frames_wanted(want / ch, speed);
            let take = (frames * ch).min(self.cons.slots() / ch * ch);
            if take > 0 {
                if let Ok(chunk) = self.cons.read_chunk(take) {
                    let (a, b) = chunk.as_slices();
                    self.varispeed.feed(a);
                    self.varispeed.feed(b);
                    chunk.commit_all();
                }
                self.shared.frames_played.fetch_add((take / ch) as u64, Ordering::Release);
            }
            n = self.varispeed.render(&mut out[..want], speed);
        }
        if n < out.len() {
            out[n..].fill(0.0);
            if n < want && !paused && self.shared.expect_data.load(Ordering::Relaxed) {
                self.shared.underruns.fetch_add(1, Ordering::Relaxed);
            }
        }

        // 4. EQ, then reverb.
        self.eq.process(&mut out[..n], ch);
        let mix = self.shared.reverb();
        if mix > 0.0 {
            let rate = self.rate;
            let reverb = self.reverb.get_or_insert_with(|| Reverb::new(rate));
            if !self.reverb_on {
                reverb.reset();
            }
            reverb.process(&mut out[..n], ch, mix);
        }
        self.reverb_on = mix > 0.0;

        // What's heard, before the volume, for the visualizer.
        self.shared.viz.push(&out[..n], ch, self.rate);

        // 5. Gain (volume x preamp) with smoothing + pause fade.
        let target = self.target_gain();
        let identity = !self.eq.is_active()
            && speed == 1.0
            && !self.reverb_on
            && target == 1.0
            && self.gain == 1.0
            && self.fade >= 1.0
            && !paused;
        if !identity {
            let mut gain = self.gain;
            let mut fade = self.fade;
            for frame in out[..n].chunks_exact_mut(ch) {
                gain += (target - gain) * self.gain_coeff;
                if (target - gain).abs() < 1e-7 {
                    gain = target;
                }
                if fade < target_fade {
                    fade = (fade + self.fade_step).min(1.0);
                } else if fade > target_fade {
                    fade = (fade - self.fade_step).max(0.0);
                }
                let g = gain * fade;
                for s in frame.iter_mut() {
                    *s = (*s * g).clamp(-1.0, 1.0);
                }
            }
            self.gain = gain;
            self.fade = fade;
        }
        self.shared.identity.store(identity, Ordering::Relaxed);
    }

    /// Copies what the ring has, up to `out.len()`, into `out`. Returns the samples copied.
    fn pull(&mut self, out: &mut [f64]) -> usize {
        let ch = self.channels;
        let n = out.len().min(self.cons.slots() / ch * ch);
        if n > 0 {
            if let Ok(chunk) = self.cons.read_chunk(n) {
                let (a, b) = chunk.as_slices();
                out[..a.len()].copy_from_slice(a);
                out[a.len()..a.len() + b.len()].copy_from_slice(b);
                chunk.commit_all();
            }
        }
        self.shared.frames_played.fetch_add((n / ch) as u64, Ordering::Release);
        n
    }
}

/// Fast TPDF dither generator (xorshift).
pub struct Dither {
    state: u64,
}

impl Dither {
    pub fn new() -> Self {
        Self { state: 0x9E37_79B9_7F4A_7C15 }
    }

    #[inline]
    fn next_unit(&mut self) -> f64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        (x >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    }

    /// Triangular noise in the range (-1, 1) LSB.
    #[inline]
    pub fn tpdf(&mut self) -> f64 {
        self.next_unit() + self.next_unit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::eq::EqSettings;

    fn renderer(volume: f64, eq: EqSettings) -> (rtrb::Producer<f64>, Renderer, Arc<EngineShared>) {
        let shared = Arc::new(EngineShared::new(volume, eq));
        shared.paused.store(false, Ordering::Relaxed);
        let (prod, cons) = rtrb::RingBuffer::new(4096);
        let r = Renderer::new(cons, shared.clone(), 2, 44100);
        (prod, r, shared)
    }

    fn sample_data() -> Vec<f64> {
        // 24-bit integer sample values scaled exactly like the decoder does.
        (0..512).map(|i| ((i * 7919 % 16_777_216) as i64 - 8_388_608) as f64 / 8_388_608.0).collect()
    }

    #[test]
    fn unity_gain_without_eq_is_bit_transparent() {
        let (mut prod, mut r, shared) = renderer(1.0, EqSettings::default());
        let data = sample_data();
        for &s in &data {
            prod.push(s).unwrap();
        }
        let mut out = vec![0.0; data.len()];
        r.render(&mut out);
        assert!(shared.identity.load(Ordering::Relaxed));
        assert_eq!(out, data, "samples must pass through untouched");
        assert_eq!(shared.frames_played.load(Ordering::Relaxed), 256);
    }

    #[test]
    fn flat_but_enabled_eq_is_still_transparent() {
        let mut eq = EqSettings::default();
        eq.enabled = true; // all bands at 0 dB, preamp 0
        let (mut prod, mut r, shared) = renderer(1.0, eq);
        let data = sample_data();
        for &s in &data {
            prod.push(s).unwrap();
        }
        let mut out = vec![0.0; data.len()];
        r.render(&mut out);
        assert!(shared.identity.load(Ordering::Relaxed));
        assert_eq!(out, data);
    }

    #[test]
    fn volume_change_is_not_identity() {
        let (mut prod, mut r, shared) = renderer(0.5, EqSettings::default());
        for &s in &sample_data() {
            prod.push(s).unwrap();
        }
        let mut out = vec![0.0; 512];
        r.render(&mut out);
        assert!(!shared.identity.load(Ordering::Relaxed));
    }

    #[test]
    fn slowed_playback_moves_through_the_song_at_its_speed() {
        let (mut prod, mut r, shared) = renderer(1.0, EqSettings::default());
        shared.set_effects(0.5, 0.0);
        for i in 0..2048 {
            prod.push((i as f64 * 0.01).sin() * 0.5).unwrap();
        }
        let mut out = vec![0.0; 1024]; // 512 frames
        r.render(&mut out);
        let played = shared.frames_played.load(Ordering::Relaxed);
        assert!((256..=260).contains(&played), "512 frames at half speed use ~256 of the song, used {played}");
        assert!(!shared.identity.load(Ordering::Relaxed), "slowed isn't bit-perfect");

        // Back to normal: what was held comes out first, then the ring, with nothing lost.
        shared.set_effects(1.0, 0.0);
        let mut out = vec![0.0; 1024];
        r.render(&mut out);
        assert!(out.iter().all(|s| s.abs() <= 0.5));
        // The 3 look-ahead frames: one only steadied the interpolation, two still play.
        assert_eq!(shared.frames_played.load(Ordering::Relaxed), played + (512 - 2));
    }

    #[test]
    fn flush_discards_buffered_audio() {
        let (mut prod, mut r, shared) = renderer(1.0, EqSettings::default());
        for _ in 0..1000 {
            prod.push(0.5).unwrap();
        }
        shared.flush_gen.fetch_add(1, Ordering::AcqRel);
        let mut out = vec![1.0; 64];
        r.render(&mut out);
        assert!(out.iter().all(|&s| s == 0.0));
        assert_eq!(shared.flush_ack.load(Ordering::Acquire), shared.flush_gen.load(Ordering::Acquire));
        assert_eq!(shared.frames_played.load(Ordering::Relaxed), 0);
    }
}
