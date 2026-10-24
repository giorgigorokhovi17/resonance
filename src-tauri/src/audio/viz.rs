//! The visualizer: the renderer copies what's playing (after effects, before volume, mixed to
//! mono) into a small lock-free buffer, and the analyzer turns the latest of it into a few
//! frequency-band levels for the Dynamic Island's bars.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

/// Samples kept for analysis; the analyzer looks at the newest `FFT_LEN`.
const TAP_LEN: usize = 4096;
const FFT_LEN: usize = 2048;
/// Number of band levels, low to high, spaced evenly in pitch between these frequencies.
pub const BANDS: usize = 8;
const LOW_HZ: f64 = 45.0;
const HIGH_HZ: f64 = 14_000.0;
/// Levels below this (dB from full scale) show as nothing, and the range above it fills the bar.
const FLOOR_DB: f32 = -58.0;
const RANGE_DB: f32 = 50.0;
/// How quickly a bar falls back each update (it jumps up at once).
const FALL: f32 = 0.82;

pub struct VizTap {
    buf: Box<[AtomicU32]>,
    write: AtomicUsize,
    rate: AtomicU32,
    /// Someone is looking: only then does the renderer copy samples.
    pub enabled: AtomicBool,
}

impl Default for VizTap {
    fn default() -> Self {
        Self {
            buf: (0..TAP_LEN).map(|_| AtomicU32::new(0)).collect(),
            write: AtomicUsize::new(0),
            rate: AtomicU32::new(44_100),
            enabled: AtomicBool::new(false),
        }
    }
}

impl VizTap {
    /// Called by the renderer with interleaved samples. Never blocks.
    pub fn push(&self, samples: &[f64], channels: usize, rate: u32) {
        if !self.enabled.load(Ordering::Relaxed) || channels == 0 {
            return;
        }
        self.rate.store(rate, Ordering::Relaxed);
        let mut w = self.write.load(Ordering::Relaxed);
        for frame in samples.chunks_exact(channels) {
            let mono = frame.iter().sum::<f64>() / channels as f64;
            self.buf[w % TAP_LEN].store((mono as f32).to_bits(), Ordering::Relaxed);
            w = w.wrapping_add(1);
        }
        self.write.store(w, Ordering::Release);
    }

    /// The newest `out.len()` samples, oldest first. A sample being overwritten while it's read
    /// only makes one bar flicker, so there's no lock.
    fn latest(&self, out: &mut [f32]) -> u32 {
        let w = self.write.load(Ordering::Acquire);
        let n = out.len();
        for (i, s) in out.iter_mut().enumerate() {
            let at = w.wrapping_sub(n).wrapping_add(i) % TAP_LEN;
            *s = f32::from_bits(self.buf[at].load(Ordering::Relaxed));
        }
        self.rate.load(Ordering::Relaxed)
    }
}

pub struct Analyzer {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    levels: [f32; BANDS],
}

impl Default for Analyzer {
    fn default() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_LEN);
        let window = (0..FFT_LEN)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT_LEN as f32).cos())
            .collect();
        let spectrum = fft.make_output_vec();
        Self { fft, window, input: vec![0.0; FFT_LEN], spectrum, levels: [0.0; BANDS] }
    }
}

impl Analyzer {
    /// Band levels 0..1 for what's playing now.
    pub fn levels(&mut self, tap: &VizTap) -> [f32; BANDS] {
        let rate = tap.latest(&mut self.input) as f64;
        for (s, w) in self.input.iter_mut().zip(&self.window) {
            *s *= w;
        }
        if self.fft.process(&mut self.input, &mut self.spectrum).is_err() {
            return self.levels;
        }
        let bin_hz = rate / FFT_LEN as f64;
        // A full-scale sine peaks at FFT_LEN / 4 with the Hann window.
        let full = FFT_LEN as f32 / 4.0;
        let ratio = (HIGH_HZ / LOW_HZ).powf(1.0 / BANDS as f64);
        for (b, level) in self.levels.iter_mut().enumerate() {
            let lo = ((LOW_HZ * ratio.powi(b as i32)) / bin_hz).floor() as usize;
            let hi = ((LOW_HZ * ratio.powi(b as i32 + 1)) / bin_hz).ceil() as usize;
            let (lo, hi) = (lo.max(1), hi.clamp(lo.max(1) + 1, self.spectrum.len()));
            let peak = self.spectrum[lo..hi].iter().map(|c| c.norm()).fold(0.0f32, f32::max) / full;
            let db = 20.0 * peak.max(1e-9).log10();
            // Higher bands carry less energy in music; lift them a little so all bars move.
            let tilt = b as f32 * 1.5;
            let target = ((db + tilt - FLOOR_DB) / RANGE_DB).clamp(0.0, 1.0);
            *level = if target > *level { target } else { *level * FALL + target * (1.0 - FALL) };
        }
        self.levels
    }

    /// Lets the bars fall to nothing (paused or stopped).
    pub fn quiet(&mut self) -> [f32; BANDS] {
        for l in self.levels.iter_mut() {
            *l *= FALL;
            if *l < 0.01 {
                *l = 0.0;
            }
        }
        self.levels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_low_tone_lights_the_low_bars() {
        let tap = VizTap::default();
        tap.enabled.store(true, Ordering::Relaxed);
        let rate = 44_100u32;
        let tone: Vec<f64> = (0..TAP_LEN).map(|i| 0.5 * (2.0 * std::f64::consts::PI * 80.0 * i as f64 / rate as f64).sin()).collect();
        tap.push(&tone, 1, rate);
        let levels = Analyzer::default().levels(&tap);
        assert!(levels[0] > 0.5, "80 Hz shows in the lowest bar: {levels:?}");
        assert!(levels[BANDS - 1] < 0.2, "and not in the highest: {levels:?}");
    }

    #[test]
    fn nothing_is_copied_unless_someone_looks() {
        let tap = VizTap::default();
        tap.push(&[1.0; 64], 2, 44_100);
        assert_eq!(tap.write.load(Ordering::Relaxed), 0);
    }
}
