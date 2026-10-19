//! Streaming high-quality sample-rate conversion (shared mode only; exclusive mode never resamples).

use anyhow::{anyhow, Result};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};

const CHUNK: usize = 1024;

pub struct StreamResampler {
    inner: Async<f64>,
    channels: usize,
    ratio: f64,
    /// Interleaved input frames waiting for a full chunk.
    pending: Vec<f64>,
    out: Vec<f64>,
    /// Output frames still to discard (the filter's group delay) at stream start.
    delay_left: usize,
    in_total: u64,
    out_total: u64,
}

impl StreamResampler {
    pub fn new(from: u32, to: u32, channels: usize) -> Result<Self> {
        let ratio = to as f64 / from as f64;
        let params = SincInterpolationParameters::new(256, WindowFunction::BlackmanHarris2)
            .oversampling_factor(256)
            .interpolation(SincInterpolationType::Cubic);
        let inner = Async::<f64>::new_sinc(ratio, 1.0, &params, CHUNK, channels, FixedAsync::Input)
            .map_err(|e| anyhow!("resampler: {e}"))?;
        let delay_left = inner.output_delay();
        let max_out = inner.output_frames_max();
        Ok(Self {
            inner,
            channels,
            ratio,
            pending: Vec::with_capacity(CHUNK * channels * 2),
            out: vec![0.0; max_out * channels],
            delay_left,
            in_total: 0,
            out_total: 0,
        })
    }

    /// Feed interleaved input; append resampled interleaved output to `dst`.
    pub fn process(&mut self, input: &[f64], dst: &mut Vec<f64>) -> Result<()> {
        self.pending.extend_from_slice(input);
        self.in_total += (input.len() / self.channels) as u64;
        loop {
            let need = self.inner.input_frames_next();
            if self.pending.len() / self.channels < need {
                break;
            }
            self.run(need, None, dst)?;
        }
        Ok(())
    }

    /// Flush the remaining input and the filter tail so the output length matches the input
    /// duration exactly (important for gapless transitions).
    pub fn finish(&mut self, dst: &mut Vec<f64>) -> Result<()> {
        let expected = (self.in_total as f64 * self.ratio).round() as u64;
        let mut guard = 0;
        while self.out_total < expected && guard < 64 {
            let need = self.inner.input_frames_next();
            let have = self.pending.len() / self.channels;
            if have >= need {
                self.run(need, None, dst)?;
            } else {
                self.run(need, Some(have), dst)?;
                self.pending.clear();
            }
            guard += 1;
        }
        // Drop any overshoot produced by the final padded chunk.
        if self.out_total > expected {
            let extra = (self.out_total - expected) as usize * self.channels;
            let keep = dst.len().saturating_sub(extra);
            dst.truncate(keep);
            self.out_total = expected;
        }
        Ok(())
    }

    fn run(&mut self, need: usize, partial: Option<usize>, dst: &mut Vec<f64>) -> Result<()> {
        let ch = self.channels;
        let avail = self.pending.len() / ch;
        if avail < need {
            self.pending.resize(need * ch, 0.0);
        }
        let consumed_frames = match partial {
            Some(n) => n,
            None => need,
        };
        let max_out = self.inner.output_frames_max();
        if self.out.len() < max_out * ch {
            self.out.resize(max_out * ch, 0.0);
        }
        let (n_out, _n_in) = {
            let input = InterleavedSlice::new(&self.pending[..need * ch], ch, need)
                .map_err(|e| anyhow!("resampler input: {e:?}"))?;
            let mut output = InterleavedSlice::new_mut(&mut self.out[..max_out * ch], ch, max_out)
                .map_err(|e| anyhow!("resampler output: {e:?}"))?;
            let idx = Indexing { partial_len: partial, ..Default::default() };
            let (n_in, n_out) = self
                .inner
                .process_into_buffer(&input, &mut output, Some(&idx))
                .map_err(|e| anyhow!("resample: {e}"))?;
            (n_out, n_in)
        };
        let drain = (consumed_frames * ch).min(self.pending.len());
        self.pending.drain(..drain);

        let mut start = 0;
        if self.delay_left > 0 {
            let skip = self.delay_left.min(n_out);
            self.delay_left -= skip;
            start = skip;
        }
        if n_out > start {
            dst.extend_from_slice(&self.out[start * ch..n_out * ch]);
            self.out_total += (n_out - start) as u64;
        }
        Ok(())
    }
}
