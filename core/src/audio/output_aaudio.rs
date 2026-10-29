//! Android output through AAudio (docs/android-design.md §3). Same API as the Windows
//! `output.rs`. One shared stream in power-saving mode (larger buffers, fewer wakeups with the
//! screen off), opened at the device's own rate; the engine resamples to it. There is no
//! exclusive mode and no device choice: Android routes to the speaker, headphones or Bluetooth.
//!
//! When the route changes (headphones unplugged, Bluetooth connected) AAudio disconnects the
//! stream. That is reported through `take_error`, and the engine reopens on the new route.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};
use ndk::audio::{
    AudioCallbackResult, AudioContentType, AudioDirection, AudioError, AudioFormat, AudioPerformanceMode,
    AudioSharingMode, AudioStreamBuilder, AudioUsage,
};
use parking_lot::Mutex;
use rtrb::{Producer, RingBuffer};
use serde::Serialize;

use super::dsp::{EngineShared, Renderer};
use super::types::{OutputFormat, OutputMode, SourceFormat};

/// Seconds of audio buffered between the decoder and the device.
const RING_SECONDS: f64 = 0.5;
/// How often underruns are written to the log (the dropout check reads them from logcat).
const XRUN_LOG_EVERY: Duration = Duration::from_secs(60);

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// Android picks the device; the settings show only "System default".
pub fn list_output_devices() -> Result<Vec<DeviceInfo>> {
    Ok(Vec::new())
}

/// A running output stream. Dropping it stops the stream and releases the device.
pub struct OutputHandle {
    stop_tx: Sender<()>,
    thread: Option<JoinHandle<()>>,
    error: Arc<Mutex<Option<String>>>,
    pub format: OutputFormat,
    pub negotiated_for: Option<(u32, Option<u32>, bool, usize)>,
}

impl OutputHandle {
    pub fn take_error(&self) -> Option<String> {
        self.error.lock().take()
    }
}

impl Drop for OutputHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Error type that lets the engine distinguish "format not supported" (exclusive mode only,
/// which Android doesn't have).
#[derive(Debug)]
pub struct ExclusiveUnsupported(pub String);
impl std::fmt::Display for ExclusiveUnsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ExclusiveUnsupported {}

pub fn open(
    _mode: OutputMode,
    _device_id: Option<String>,
    _src: &SourceFormat,
    shared: Arc<EngineShared>,
) -> Result<(OutputHandle, Producer<f64>)> {
    let (init_tx, init_rx) = bounded::<Result<(OutputFormat, Producer<f64>)>>(1);
    let (stop_tx, stop_rx): (Sender<()>, Receiver<()>) = bounded(1);
    let error = Arc::new(Mutex::new(None));
    let error_cb = error.clone();

    // The stream lives on its own thread (AAudio streams aren't Send) and is closed there, never
    // from the error callback, which AAudio forbids.
    let thread = std::thread::Builder::new().name("audio-out-aaudio".into()).spawn(move || {
        // The ring is created once the stream reports its rate, then moved into the callback.
        let consumer: Arc<Mutex<Option<Renderer>>> = Arc::new(Mutex::new(None));
        let cb_renderer = consumer.clone();
        let mut scratch: Vec<f64> = vec![0.0; 16384];
        let disconnected = Arc::new(AtomicBool::new(false));
        let gone = disconnected.clone();

        let build = AudioStreamBuilder::new().map(|b| {
            b.direction(AudioDirection::Output)
                .sharing_mode(AudioSharingMode::Shared)
                .performance_mode(AudioPerformanceMode::PowerSaving)
                .usage(AudioUsage::Media)
                .content_type(AudioContentType::Music)
                .format(AudioFormat::PCM_Float)
                .channel_count(2)
                .data_callback(Box::new(move |_stream, data, frames| {
                    let samples = frames.max(0) as usize * 2;
                    // SAFETY: AAudio hands us `frames` frames of 2-channel f32 to fill.
                    let out = unsafe { std::slice::from_raw_parts_mut(data as *mut f32, samples) };
                    if scratch.len() < samples {
                        scratch.resize(samples, 0.0);
                    }
                    let buf = &mut scratch[..samples];
                    match cb_renderer.try_lock().as_deref_mut() {
                        Some(Some(r)) => r.render(buf),
                        _ => buf.fill(0.0),
                    }
                    for (o, s) in out.iter_mut().zip(buf.iter()) {
                        *o = *s as f32;
                    }
                    AudioCallbackResult::Continue
                }))
                .error_callback(Box::new(move |_stream, e| {
                    let msg = if matches!(e, AudioError::Disconnected) {
                        "The audio output changed (headphones or Bluetooth).".to_string()
                    } else {
                        format!("Audio device error: {e:?}")
                    };
                    log::warn!("aaudio: {msg}");
                    gone.store(true, Ordering::Relaxed);
                    *error_cb.lock() = Some(msg);
                }))
        });
        let stream = match build.and_then(|b| b.open_stream()) {
            Ok(s) => s,
            Err(e) => {
                let _ = init_tx.send(Err(anyhow!("cannot open the audio output: {e:?}")));
                return;
            }
        };
        let rate = stream.sample_rate().max(1) as u32;
        let channels = stream.channel_count().max(1) as usize;
        if channels != 2 || stream.format() != AudioFormat::PCM_Float {
            let _ = init_tx.send(Err(anyhow!(
                "the audio output opened as {channels} channels, {:?}; expected stereo float",
                stream.format()
            )));
            return;
        }
        let cap = ((rate as f64 * RING_SECONDS) as usize * channels).max(16384);
        let (prod, cons) = RingBuffer::new(cap);
        *consumer.lock() = Some(Renderer::new(cons, shared, channels, rate));
        if let Err(e) = stream.request_start() {
            let _ = init_tx.send(Err(anyhow!("cannot start the audio output: {e:?}")));
            return;
        }
        log::info!(
            "aaudio: {rate} Hz, burst {} frames, buffer {} frames, mode {:?}",
            stream.frames_per_burst(),
            stream.buffer_size_in_frames(),
            stream.performance_mode()
        );
        let fmt = OutputFormat {
            mode: OutputMode::Shared,
            device_id: None,
            device_name: "Android audio".into(),
            sample_rate: rate,
            channels,
            bits: 32,
            container_bits: 32,
            float: true,
        };
        let _ = init_tx.send(Ok((fmt, prod)));

        let mut last_xruns = 0;
        loop {
            match stop_rx.recv_timeout(XRUN_LOG_EVERY) {
                Err(RecvTimeoutError::Timeout) if !disconnected.load(Ordering::Relaxed) => {
                    let xruns = stream.x_run_count();
                    if xruns != last_xruns {
                        log::warn!("aaudio: {} underrun(s) in the last minute, {xruns} in all", xruns - last_xruns);
                        last_xruns = xruns;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                _ => break,
            }
        }
        log::info!("aaudio: stream closed, {} underrun(s) in all", stream.x_run_count());
        let _ = stream.request_stop();
    })?;

    let (format, prod) = init_rx
        .recv_timeout(Duration::from_secs(8))
        .map_err(|_| anyhow!("timed out opening the audio output"))??;
    Ok((OutputHandle { stop_tx, thread: Some(thread), error, format, negotiated_for: None }, prod))
}
