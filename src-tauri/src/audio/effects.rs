//! Effects in the render path (Effects in the player bar): playback speed, where pitch follows
//! like a record (0.8 is the "slowed" sound), and reverb. Both are off by default, and then the
//! samples go through untouched.

/// Cubic (Catmull-Rom) interpolation between `p1` (t = 0) and `p2` (t = 1). At t = 0 it returns
/// `p1` exactly.
#[inline]
fn cubic(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
    let a = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
    let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c = -0.5 * p0 + 0.5 * p2;
    ((a * t + b) * t + c) * t + p1
}

/// Plays samples back faster or slower. Holds a few frames of what it read so it can interpolate
/// between them.
pub struct Varispeed {
    channels: usize,
    /// Frames read and not yet used up (interleaved). Frame 0 is the one before `pos`.
    held: Vec<f64>,
    /// Where the next output frame comes from, in frames past `held`'s frame 1.
    pos: f64,
}

impl Varispeed {
    pub fn new(channels: usize) -> Self {
        Self { channels, held: Vec::with_capacity(1 << 15), pos: 0.0 }
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    pub fn clear(&mut self) {
        self.held.clear();
        self.pos = 0.0;
    }

    /// Source frames needed to make `frames` output frames at `speed`, beyond those held.
    pub fn frames_wanted(&self, frames: usize, speed: f64) -> usize {
        let need = (self.pos + frames as f64 * speed).ceil() as usize + 3;
        need.saturating_sub(self.held.len() / self.channels)
    }

    /// Adds frames read from the source.
    pub fn feed(&mut self, samples: &[f64]) {
        self.held.extend_from_slice(samples);
    }

    /// Fills `out` from the held frames at `speed`. Returns the samples written; fewer than asked
    /// when it ran out of source.
    pub fn render(&mut self, out: &mut [f64], speed: f64) -> usize {
        let ch = self.channels;
        let held = self.held.len() / ch;
        let mut written = 0;
        for frame in out.chunks_exact_mut(ch) {
            let at = self.pos + 1.0;
            let i = at.floor() as usize;
            if i + 2 >= held {
                break;
            }
            let t = at - i as f64;
            for (c, s) in frame.iter_mut().enumerate() {
                let h = |k: usize| self.held[k * ch + c];
                *s = cubic(h(i - 1), h(i), h(i + 1), h(i + 2), t);
            }
            self.pos += speed;
            written += ch;
        }
        let used = (self.pos.floor() as usize).min(held);
        self.held.drain(..used * ch);
        self.pos -= used as f64;
        written
    }

    /// Hands back the frames still held, unchanged, when going back to normal speed (rounding to
    /// the nearest frame, which can't be heard). Returns the samples written; once everything
    /// held is out, it's empty again.
    pub fn drain_into(&mut self, out: &mut [f64]) -> usize {
        let ch = self.channels;
        let start = (((self.pos.round() as usize) + 1) * ch).min(self.held.len());
        let n = (self.held.len() - start).min(out.len() / ch * ch);
        out[..n].copy_from_slice(&self.held[start..start + n]);
        // The last frame handed out stays as frame 0, "the one before".
        self.held.drain(..(start + n).saturating_sub(ch));
        self.pos = 0.0;
        if self.held.len() <= ch {
            self.clear();
        }
        n
    }
}

struct Comb {
    buf: Vec<f64>,
    i: usize,
    store: f64,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(1)], i: 0, store: 0.0 }
    }

    #[inline]
    fn process(&mut self, input: f64, feedback: f64, damp: f64) -> f64 {
        let out = self.buf[self.i];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buf[self.i] = input + self.store * feedback;
        self.i = (self.i + 1) % self.buf.len();
        out
    }
}

struct Allpass {
    buf: Vec<f64>,
    i: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(1)], i: 0 }
    }

    #[inline]
    fn process(&mut self, input: f64) -> f64 {
        let delayed = self.buf[self.i];
        self.buf[self.i] = input + delayed * 0.5;
        self.i = (self.i + 1) % self.buf.len();
        delayed - input
    }
}

/// Freeverb's delay lengths at 44.1 kHz, and the right channel's offset.
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const SPREAD: usize = 23;
/// A large room with soft highs: the "slowed + reverb" sound.
const ROOM: f64 = 0.86;
const DAMP: f64 = 0.25;

/// A stereo hall reverb (Freeverb). Processes the first two channels; others stay dry.
pub struct Reverb {
    left: (Vec<Comb>, Vec<Allpass>),
    right: (Vec<Comb>, Vec<Allpass>),
}

impl Reverb {
    pub fn new(rate: u32) -> Self {
        let scale = |n: usize| (n as f64 * rate as f64 / 44_100.0).round() as usize;
        let side = |extra: usize| {
            (
                COMBS.iter().map(|&n| Comb::new(scale(n + extra))).collect(),
                ALLPASSES.iter().map(|&n| Allpass::new(scale(n + extra))).collect(),
            )
        };
        Self { left: side(0), right: side(SPREAD) }
    }

    /// Clears the tail (when reverb is turned back on).
    pub fn reset(&mut self) {
        for (combs, passes) in [&mut self.left, &mut self.right] {
            for c in combs.iter_mut() {
                c.buf.fill(0.0);
                c.store = 0.0;
            }
            for a in passes.iter_mut() {
                a.buf.fill(0.0);
            }
        }
    }

    fn side((combs, passes): &mut (Vec<Comb>, Vec<Allpass>), input: f64) -> f64 {
        let mut out = combs.iter_mut().map(|c| c.process(input, ROOM, DAMP)).sum::<f64>();
        for a in passes.iter_mut() {
            out = a.process(out);
        }
        out
    }

    /// Mixes reverb into interleaved `samples`; `mix` 0..1.
    pub fn process(&mut self, samples: &mut [f64], channels: usize, mix: f64) {
        let wet = 3.0 * mix;
        let dry = 1.0 - 0.5 * mix;
        for frame in samples.chunks_exact_mut(channels) {
            let (l, r) = if channels >= 2 { (frame[0], frame[1]) } else { (frame[0], frame[0]) };
            let input = (l + r) * 0.015;
            let wl = Self::side(&mut self.left, input);
            let wr = Self::side(&mut self.right, input);
            if channels >= 2 {
                frame[0] = l * dry + wl * wet;
                frame[1] = r * dry + wr * wet;
            } else {
                frame[0] = l * dry + (wl + wr) * 0.5 * wet;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cubic_hits_the_samples() {
        assert_eq!(cubic(0.1, 0.25, -0.5, 0.9, 0.0), 0.25);
        assert!((cubic(0.0, 1.0, 2.0, 3.0, 0.5) - 1.5).abs() < 1e-12, "a straight line stays straight");
    }

    #[test]
    fn half_speed_uses_half_the_source() {
        let mut v = Varispeed::new(2);
        let src: Vec<f64> = (0..400).map(|i| (i / 2) as f64).collect(); // ramp, both channels equal
        let want = v.frames_wanted(100, 0.5);
        v.feed(&src[..want * 2]);
        let mut out = vec![0.0; 200];
        let n = v.render(&mut out, 0.5);
        assert_eq!(n, 200, "100 frames out");
        // A ramp slowed down is a gentler ramp: each output frame moves half a source frame.
        assert!((out[2] - out[0] - 0.5).abs() < 1e-9 && (out[198] - out[196] - 0.5).abs() < 1e-9);
        assert!(want <= 54, "about 50 source frames for 100 out at half speed, got {want}");
    }

    #[test]
    fn normal_speed_through_varispeed_is_exact() {
        let mut v = Varispeed::new(1);
        let src: Vec<f64> = (0..64).map(|i| (i as f64 * 0.37).sin()).collect();
        v.feed(&src);
        let mut out = vec![0.0; 32];
        assert_eq!(v.render(&mut out, 1.0), 32);
        assert_eq!(&out[..], &src[1..33], "speed 1 hands the samples back unchanged");
    }

    #[test]
    fn going_back_to_normal_speed_hands_back_what_was_held() {
        let mut v = Varispeed::new(1);
        let src: Vec<f64> = (0..20).map(f64::from).collect();
        v.feed(&src);
        let mut out = vec![0.0; 4];
        v.render(&mut out, 2.0); // frames 1, 3, 5, 7
        let mut rest = vec![0.0; 64];
        let n = v.drain_into(&mut rest);
        assert_eq!(&rest[..n], &src[9..], "carries on from where it was, one frame each");
        assert!(v.is_empty());
    }

    #[test]
    fn reverb_adds_a_tail() {
        let mut r = Reverb::new(44_100);
        let mut buf = vec![0.0; 2 * 8000];
        buf[0] = 1.0;
        buf[1] = 1.0;
        r.process(&mut buf, 2, 1.0);
        let tail: f64 = buf[2 * 2000..].iter().map(|s| s.abs()).sum();
        assert!(tail > 0.01, "the impulse rings on");
        assert!(buf.iter().all(|s| s.is_finite() && s.abs() < 4.0));
    }
}
