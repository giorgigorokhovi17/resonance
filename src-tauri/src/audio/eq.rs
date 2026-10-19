//! 10-band parametric EQ (RBJ biquads, 64-bit), preamp, automatic headroom and AutoEQ import.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

pub const NUM_BANDS: usize = 10;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FilterType {
    Peaking,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EqBand {
    pub enabled: bool,
    pub filter: FilterType,
    pub freq: f64,
    pub gain: f64,
    pub q: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct EqSettings {
    /// When false the EQ (and its preamp) is bypassed entirely.
    pub enabled: bool,
    /// Manual preamp in dB.
    pub preamp: f64,
    /// Automatically lower the gain so that EQ boosts can never clip.
    pub auto_headroom: bool,
    pub bands: Vec<EqBand>,
    pub preset_name: Option<String>,
}

impl Default for EqSettings {
    fn default() -> Self {
        let freqs = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
        let bands = freqs
            .iter()
            .enumerate()
            .map(|(i, &f)| EqBand {
                enabled: true,
                filter: if i == 0 {
                    FilterType::LowShelf
                } else if i == NUM_BANDS - 1 {
                    FilterType::HighShelf
                } else {
                    FilterType::Peaking
                },
                freq: f,
                gain: 0.0,
                q: if i == 0 || i == NUM_BANDS - 1 { 0.707 } else { 1.41 },
            })
            .collect();
        Self { enabled: false, preamp: 0.0, auto_headroom: true, bands, preset_name: None }
    }
}

impl EqSettings {
    pub fn normalized(mut self) -> Self {
        self.bands.truncate(NUM_BANDS);
        while self.bands.len() < NUM_BANDS {
            self.bands.push(EqBand { enabled: false, filter: FilterType::Peaking, freq: 1000.0, gain: 0.0, q: 1.0 });
        }
        for b in &mut self.bands {
            b.freq = b.freq.clamp(10.0, 24000.0);
            b.gain = b.gain.clamp(-30.0, 30.0);
            b.q = b.q.clamp(0.05, 20.0);
        }
        self.preamp = self.preamp.clamp(-30.0, 30.0);
        self
    }

    fn band_active(b: &EqBand) -> bool {
        if !b.enabled {
            return false;
        }
        match b.filter {
            FilterType::LowPass | FilterType::HighPass => true,
            _ => b.gain.abs() > 1e-9,
        }
    }

    /// True when processing would change the signal at all.
    pub fn is_active(&self) -> bool {
        self.enabled && (self.preamp.abs() > 1e-9 || self.bands.iter().any(Self::band_active))
    }

    /// Effective preamp (dB) including automatic headroom for the given sample rate.
    pub fn effective_preamp_db(&self, rate: u32) -> f64 {
        if !self.enabled {
            return 0.0;
        }
        if !self.auto_headroom {
            return self.preamp;
        }
        let peak = max_response_db(self, rate);
        // Total peak gain = preamp + peak; keep it at or below 0 dB.
        let total = self.preamp + peak;
        if total > 0.0 {
            self.preamp - total
        } else {
            self.preamp
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    pub fn design(band: &EqBand, rate: u32) -> Biquad {
        let fs = rate as f64;
        let f0 = band.freq.min(fs * 0.49).max(1.0);
        let w0 = 2.0 * PI * f0 / fs;
        let (sn, cs) = w0.sin_cos();
        let q = band.q.max(0.01);
        let alpha = sn / (2.0 * q);
        let a = 10f64.powf(band.gain / 40.0);
        let (b0, b1, b2, a0, a1, a2) = match band.filter {
            FilterType::Peaking => (
                1.0 + alpha * a,
                -2.0 * cs,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cs,
                1.0 - alpha / a,
            ),
            FilterType::LowShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cs + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cs),
                    a * ((a + 1.0) - (a - 1.0) * cs - s),
                    (a + 1.0) + (a - 1.0) * cs + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cs),
                    (a + 1.0) + (a - 1.0) * cs - s,
                )
            }
            FilterType::HighShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cs + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cs),
                    a * ((a + 1.0) + (a - 1.0) * cs - s),
                    (a + 1.0) - (a - 1.0) * cs + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cs),
                    (a + 1.0) - (a - 1.0) * cs - s,
                )
            }
            FilterType::LowPass => (
                (1.0 - cs) / 2.0,
                1.0 - cs,
                (1.0 - cs) / 2.0,
                1.0 + alpha,
                -2.0 * cs,
                1.0 - alpha,
            ),
            FilterType::HighPass => (
                (1.0 + cs) / 2.0,
                -(1.0 + cs),
                (1.0 + cs) / 2.0,
                1.0 + alpha,
                -2.0 * cs,
                1.0 - alpha,
            ),
        };
        Biquad { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    /// Magnitude response in dB at frequency `f`.
    pub fn response_db(&self, f: f64, rate: u32) -> f64 {
        let w = 2.0 * PI * f / rate as f64;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        // H(e^jw) = (b0 + b1 e^-jw + b2 e^-2jw) / (1 + a1 e^-jw + a2 e^-2jw)
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        let num = nr * nr + ni * ni;
        let den = (dr * dr + di * di).max(1e-300);
        10.0 * (num / den).max(1e-300).log10()
    }
}

/// Peak of the combined band response (excluding preamp), sampled on a log grid.
pub fn max_response_db(settings: &EqSettings, rate: u32) -> f64 {
    let filters: Vec<Biquad> = settings
        .bands
        .iter()
        .filter(|b| EqSettings::band_active(b))
        .map(|b| Biquad::design(b, rate))
        .collect();
    if filters.is_empty() {
        return 0.0;
    }
    let hi = (rate as f64 * 0.49).min(22000.0);
    let lo: f64 = 10.0;
    let n = 600;
    let mut peak = f64::NEG_INFINITY;
    for i in 0..n {
        let f = lo * (hi / lo).powf(i as f64 / (n - 1) as f64);
        let db: f64 = filters.iter().map(|q| q.response_db(f, rate)).sum();
        peak = peak.max(db);
    }
    peak
}

/// Combined response curve for UI display: (frequency, dB) pairs including the effective preamp.
pub fn response_curve(settings: &EqSettings, rate: u32, points: usize) -> Vec<(f64, f64)> {
    let filters: Vec<Biquad> = settings
        .bands
        .iter()
        .filter(|b| EqSettings::band_active(b))
        .map(|b| Biquad::design(b, rate))
        .collect();
    let pre = settings.effective_preamp_db(rate);
    let points = points.max(2);
    (0..points)
        .map(|i| {
            let f = 20.0 * (20000.0f64 / 20.0).powf(i as f64 / (points - 1) as f64);
            let db: f64 = filters.iter().map(|q| q.response_db(f, rate)).sum::<f64>() + pre;
            (f, db)
        })
        .collect()
}

/// Real-time EQ processor (Transposed Direct Form II, f64 state per channel).
pub struct EqProcessor {
    filters: Vec<Biquad>,
    /// state[channel][filter] = (z1, z2)
    state: Vec<Vec<(f64, f64)>>,
    active: bool,
    preamp_linear: f64,
}

impl EqProcessor {
    pub fn new(settings: &EqSettings, rate: u32, channels: usize) -> Self {
        let mut p = EqProcessor { filters: Vec::new(), state: Vec::new(), active: false, preamp_linear: 1.0 };
        p.update(settings, rate, channels);
        p
    }

    /// Recompute coefficients. Filter state is kept when the band layout is unchanged so that
    /// tweaking a slider does not click.
    pub fn update(&mut self, settings: &EqSettings, rate: u32, channels: usize) {
        let filters: Vec<Biquad> = if settings.enabled {
            settings
                .bands
                .iter()
                .filter(|b| EqSettings::band_active(b))
                .map(|b| Biquad::design(b, rate))
                .collect()
        } else {
            Vec::new()
        };
        if self.state.len() != channels || self.state.first().map(|s| s.len()) != Some(filters.len()) {
            self.state = vec![vec![(0.0, 0.0); filters.len()]; channels];
        }
        self.filters = filters;
        self.active = settings.is_active();
        self.preamp_linear = if settings.enabled {
            10f64.powf(settings.effective_preamp_db(rate) / 20.0)
        } else {
            1.0
        };
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn preamp_linear(&self) -> f64 {
        self.preamp_linear
    }

    pub fn process(&mut self, buf: &mut [f64], channels: usize) {
        if !self.active || self.filters.is_empty() {
            return;
        }
        for (ch, state) in self.state.iter_mut().enumerate().take(channels) {
            for (fi, f) in self.filters.iter().enumerate() {
                let (mut z1, mut z2) = state[fi];
                let mut i = ch;
                while i < buf.len() {
                    let x = buf[i];
                    let y = f.b0 * x + z1;
                    z1 = f.b1 * x - f.a1 * y + z2;
                    z2 = f.b2 * x - f.a2 * y;
                    buf[i] = y;
                    i += channels;
                }
                // Flush denormals.
                if z1.abs() < 1e-30 {
                    z1 = 0.0;
                }
                if z2.abs() < 1e-30 {
                    z2 = 0.0;
                }
                state[fi] = (z1, z2);
            }
        }
    }
}

/// Parse an AutoEQ `ParametricEQ.txt` (Equalizer APO format).
///
/// ```text
/// Preamp: -6.2 dB
/// Filter 1: ON LSC Fc 105 Hz Gain 5.5 dB Q 0.70
/// Filter 2: ON PK Fc 200 Hz Gain -1.2 dB Q 0.50
/// ```
pub fn parse_autoeq(text: &str) -> Result<(EqSettings, Vec<String>), String> {
    let mut preamp = 0.0;
    let mut bands = Vec::new();
    let mut warnings = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("preamp") {
            if let Some(v) = line.split(':').nth(1).and_then(|s| first_number(s)) {
                preamp = v;
            }
            continue;
        }
        if !lower.starts_with("filter") {
            continue;
        }
        let Some(rest) = line.split_once(':').map(|(_, r)| r) else { continue };
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        if tokens.len() < 2 {
            continue;
        }
        let enabled = tokens[0].eq_ignore_ascii_case("on");
        let filter = match tokens[1].to_ascii_uppercase().as_str() {
            "PK" | "PEQ" | "MODAL" => FilterType::Peaking,
            "LS" | "LSC" | "LSQ" | "LS 6DB" | "LS 12DB" => FilterType::LowShelf,
            "HS" | "HSC" | "HSQ" | "HS 6DB" | "HS 12DB" => FilterType::HighShelf,
            "LP" | "LPQ" => FilterType::LowPass,
            "HP" | "HPQ" => FilterType::HighPass,
            other => {
                warnings.push(format!("Unsupported filter type '{other}' skipped"));
                continue;
            }
        };
        let mut freq = None;
        let mut gain = 0.0;
        let mut q = match filter {
            FilterType::Peaking => 1.0,
            _ => 0.707,
        };
        let mut i = 2;
        while i < tokens.len() {
            let key = tokens[i].to_ascii_lowercase();
            let val = tokens.get(i + 1).and_then(|v| v.parse::<f64>().ok());
            match (key.as_str(), val) {
                ("fc", Some(v)) => freq = Some(v),
                ("gain", Some(v)) => gain = v,
                ("q", Some(v)) => q = v,
                ("bw", Some(v)) => {
                    // Bandwidth in octaves -> Q
                    let two = 2f64.powf(v);
                    q = two.sqrt() / (two - 1.0);
                }
                _ => {}
            }
            i += 1;
        }
        let Some(freq) = freq else { continue };
        bands.push(EqBand { enabled, filter, freq, gain, q });
    }
    if bands.is_empty() {
        return Err("No filters found. Expected an AutoEQ ParametricEQ.txt file.".into());
    }
    if bands.len() > NUM_BANDS {
        warnings.push(format!(
            "File has {} filters; only the first {NUM_BANDS} were imported.",
            bands.len()
        ));
    }
    let settings = EqSettings { enabled: true, preamp, auto_headroom: true, bands, preset_name: None }.normalized();
    Ok((settings, warnings))
}

fn first_number(s: &str) -> Option<f64> {
    s.split_whitespace().find_map(|t| t.parse::<f64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_autoeq() {
        let txt = "Preamp: -6.2 dB\nFilter 1: ON LSC Fc 105 Hz Gain 5.5 dB Q 0.70\nFilter 2: ON PK Fc 200 Hz Gain -1.2 dB Q 0.50\nFilter 3: OFF HSC Fc 10000 Hz Gain 2.0 dB Q 0.70\n";
        let (s, w) = parse_autoeq(txt).unwrap();
        assert!(w.is_empty());
        assert_eq!(s.preamp, -6.2);
        assert_eq!(s.bands[0].filter, FilterType::LowShelf);
        assert_eq!(s.bands[1].gain, -1.2);
        assert!(!s.bands[2].enabled);
        assert_eq!(s.bands.len(), NUM_BANDS);
    }

    #[test]
    fn peaking_response_matches_gain() {
        let b = EqBand { enabled: true, filter: FilterType::Peaking, freq: 1000.0, gain: 6.0, q: 1.0 };
        let bq = Biquad::design(&b, 48000);
        assert!((bq.response_db(1000.0, 48000) - 6.0).abs() < 0.01);
        assert!(bq.response_db(20.0, 48000).abs() < 0.1);
    }

    #[test]
    fn auto_headroom_prevents_boost() {
        let mut s = EqSettings::default();
        s.enabled = true;
        s.bands[5].gain = 8.0;
        let pre = s.effective_preamp_db(48000);
        assert!(pre <= -7.9 && pre >= -8.1, "{pre}");
    }

    /// Runs a real 1 kHz sine through a +6 dB peaking band and checks the steady-state gain.
    #[test]
    fn processor_applies_gain_to_signal() {
        let mut s = EqSettings::default();
        s.enabled = true;
        s.auto_headroom = false;
        s.bands[5] = EqBand { enabled: true, filter: FilterType::Peaking, freq: 1000.0, gain: 6.0, q: 1.0 };
        let rate = 48000;
        let mut p = EqProcessor::new(&s, rate, 1);
        let n = rate as usize; // 1 s
        let mut buf: Vec<f64> = (0..n)
            .map(|i| 0.25 * (2.0 * PI * 1000.0 * i as f64 / rate as f64).sin())
            .collect();
        p.process(&mut buf, 1);
        let peak = buf[n / 2..].iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let gain_db = 20.0 * (peak / 0.25).log10();
        assert!((gain_db - 6.0).abs() < 0.05, "measured {gain_db} dB");
    }

    #[test]
    fn auto_headroom_keeps_boosted_signal_below_full_scale() {
        let mut s = EqSettings::default();
        s.enabled = true;
        s.bands[0] = EqBand { enabled: true, filter: FilterType::LowShelf, freq: 120.0, gain: 9.0, q: 0.7 };
        let rate = 44100;
        let mut p = EqProcessor::new(&s, rate, 1);
        let pre = p.preamp_linear();
        // Full-scale 50 Hz sine: boosted by ~9 dB, then the auto preamp pulls it back under 0 dBFS.
        let n = rate as usize;
        let mut buf: Vec<f64> = (0..n).map(|i| (2.0 * PI * 50.0 * i as f64 / rate as f64).sin()).collect();
        p.process(&mut buf, 1);
        let peak = buf[n / 2..].iter().fold(0.0f64, |m, v| m.max(v.abs())) * pre;
        assert!(peak <= 1.0005, "peak after headroom = {peak}");
        assert!(peak > 0.9, "headroom should not over-attenuate: {peak}");
    }

    #[test]
    fn flat_is_identity() {
        let mut s = EqSettings::default();
        s.enabled = true;
        assert!(!s.is_active());
        let mut p = EqProcessor::new(&s, 44100, 2);
        let mut buf = vec![0.25, -0.5, 0.125, 1.0];
        p.process(&mut buf, 2);
        assert_eq!(buf, vec![0.25, -0.5, 0.125, 1.0]);
        assert_eq!(p.preamp_linear(), 1.0);
    }
}
