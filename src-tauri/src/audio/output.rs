//! Output backends. Shared mode uses cpal (WASAPI shared, through the Windows mixer so system
//! effects like Equalizer APO apply). Exclusive mode talks to WASAPI directly through the `wasapi`
//! crate because cpal does not implement exclusive mode: the device is opened at the source's
//! sample rate and bit depth with no mixer and no resampling.

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use rtrb::{Producer, RingBuffer};
use serde::Serialize;

use super::dsp::{Dither, EngineShared, Renderer};
use super::types::{OutputFormat, OutputMode, SourceFormat};

/// Seconds of audio buffered between the decoder and the device.
const RING_SECONDS: f64 = 0.5;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

fn werr(e: wasapi::WasapiError) -> anyhow::Error {
    anyhow!("{e}")
}

/// Lists active render endpoints (WASAPI endpoint IDs are shared by both backends).
pub fn list_output_devices() -> Result<Vec<DeviceInfo>> {
    std::thread::spawn(|| -> Result<Vec<DeviceInfo>> {
        let _ = wasapi::initialize_mta();
        let enumerator = wasapi::DeviceEnumerator::new().map_err(werr)?;
        let default_id = enumerator
            .get_default_device(&wasapi::Direction::Render)
            .ok()
            .and_then(|d| d.get_id().ok());
        let coll = enumerator
            .get_device_collection(&wasapi::Direction::Render)
            .map_err(werr)?;
        let n = coll.get_nbr_devices().map_err(werr)?;
        let mut out = Vec::new();
        for i in 0..n {
            let Ok(d) = coll.get_device_at_index(i) else { continue };
            let Ok(id) = d.get_id() else { continue };
            let name = d.get_friendlyname().unwrap_or_else(|_| "Unknown device".into());
            out.push(DeviceInfo { is_default: Some(&id) == default_id.as_ref(), id, name });
        }
        Ok(out)
    })
    .join()
    .map_err(|_| anyhow!("device enumeration panicked"))?
}

/// A running output stream. Dropping it stops the stream and releases the device.
pub struct OutputHandle {
    stop_tx: Sender<()>,
    thread: Option<JoinHandle<()>>,
    error: Arc<Mutex<Option<String>>>,
    pub format: OutputFormat,
    /// What the exclusive stream was negotiated for (rate, bits, float, channels).
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

fn ring_for(rate: u32, channels: usize) -> (Producer<f64>, rtrb::Consumer<f64>) {
    let cap = ((rate as f64 * RING_SECONDS) as usize * channels).max(16384);
    RingBuffer::new(cap)
}

fn boost_thread_priority() {
    use windows::core::w;
    use windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW;
    let mut task_index = 0u32;
    unsafe {
        let _ = AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task_index);
    }
}

pub fn open(
    mode: OutputMode,
    device_id: Option<String>,
    src: &SourceFormat,
    shared: Arc<EngineShared>,
) -> Result<(OutputHandle, Producer<f64>)> {
    match mode {
        OutputMode::Shared => open_shared(device_id, shared),
        OutputMode::Exclusive => open_exclusive(device_id, src, shared),
    }
}

// ---------------------------------------------------------------------------------------------
// Shared mode (cpal)
// ---------------------------------------------------------------------------------------------

fn open_shared(device_id: Option<String>, shared: Arc<EngineShared>) -> Result<(OutputHandle, Producer<f64>)> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let (init_tx, init_rx) = bounded::<Result<(OutputFormat, Producer<f64>)>>(1);
    let (stop_tx, stop_rx): (Sender<()>, Receiver<()>) = bounded(1);
    let error = Arc::new(Mutex::new(None));
    let error_cb = error.clone();

    let thread = std::thread::Builder::new()
        .name("audio-out-shared".into())
        .spawn(move || {
            let build = || -> Result<(cpal::Stream, OutputFormat, Producer<f64>)> {
                let host = cpal::default_host();
                let device = match &device_id {
                    Some(id) => host
                        .output_devices()
                        .context("cannot enumerate devices")?
                        .find(|d| d.id().map(|i| i.id() == id.as_str()).unwrap_or(false))
                        .or_else(|| host.default_output_device()),
                    None => host.default_output_device(),
                }
                .ok_or_else(|| anyhow!("no audio output device available"))?;
                let name = device
                    .description()
                    .map(|d| d.name().to_string())
                    .unwrap_or_else(|_| "Default device".into());
                let actual_id = device.id().ok().map(|i| i.id().to_string());
                let def = device
                    .default_output_config()
                    .map_err(|e| anyhow!("cannot query device format: {e}"))?;
                let rate = def.sample_rate();
                // Music is (almost always) stereo: render 2 channels and let Windows map them
                // onto multichannel endpoints instead of processing silent channels ourselves.
                let channels = (def.channels() as usize).min(2);
                let config = cpal::StreamConfig {
                    channels: channels as u16,
                    sample_rate: rate,
                    buffer_size: cpal::BufferSize::Default,
                };
                let (prod, cons) = ring_for(rate, channels);
                let mut renderer = Renderer::new(cons, shared.clone(), channels, rate);
                let mut scratch: Vec<f64> = vec![0.0; 16384];
                let err_slot = error_cb.clone();
                let stream = device
                    .build_output_stream::<f32, _, _>(
                        config,
                        move |data: &mut [f32], _info| {
                            if scratch.len() < data.len() {
                                scratch.resize(data.len(), 0.0);
                            }
                            let buf = &mut scratch[..data.len()];
                            renderer.render(buf);
                            for (o, s) in data.iter_mut().zip(buf.iter()) {
                                *o = *s as f32;
                            }
                        },
                        move |e| {
                            log::error!("shared output error: {e}");
                            *err_slot.lock() = Some(format!("Audio device error: {e}"));
                        },
                        None,
                    )
                    .map_err(|e| anyhow!("cannot open {name} in shared mode: {e}"))?;
                stream.play().map_err(|e| anyhow!("cannot start stream: {e}"))?;
                let fmt = OutputFormat {
                    mode: OutputMode::Shared,
                    device_id: actual_id,
                    device_name: name,
                    sample_rate: rate,
                    channels,
                    bits: 32,
                    container_bits: 32,
                    float: true,
                };
                Ok((stream, fmt, prod))
            };
            match build() {
                Ok((stream, fmt, prod)) => {
                    let _ = init_tx.send(Ok((fmt, prod)));
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = init_tx.send(Err(e));
                }
            }
        })?;

    let (format, prod) = init_rx
        .recv_timeout(Duration::from_secs(8))
        .map_err(|_| anyhow!("timed out opening the audio device"))??;
    Ok((OutputHandle { stop_tx, thread: Some(thread), error, format, negotiated_for: None }, prod))
}

// ---------------------------------------------------------------------------------------------
// Exclusive mode (raw WASAPI)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SampleKind {
    I16,
    I24Packed,
    I24In32,
    I32,
    F32,
}

impl SampleKind {
    fn spec(self) -> (usize, usize, wasapi::SampleType) {
        match self {
            SampleKind::I16 => (16, 16, wasapi::SampleType::Int),
            SampleKind::I24Packed => (24, 24, wasapi::SampleType::Int),
            SampleKind::I24In32 => (32, 24, wasapi::SampleType::Int),
            SampleKind::I32 => (32, 32, wasapi::SampleType::Int),
            SampleKind::F32 => (32, 32, wasapi::SampleType::Float),
        }
    }
    fn bytes(self) -> usize {
        self.spec().0 / 8
    }
}

/// Device formats to try, best match for the source first. Integer formats at or above the
/// source depth keep the stream bit-perfect (samples are zero-padded, never altered).
fn candidates(src: &SourceFormat) -> Vec<SampleKind> {
    use SampleKind::*;
    if src.float {
        return vec![I32, I24In32, I24Packed, F32, I16];
    }
    match src.bits_per_sample.unwrap_or(16) {
        0..=16 => vec![I16, I24Packed, I24In32, I32, F32],
        17..=24 => vec![I24Packed, I24In32, I32, F32, I16],
        _ => vec![I32, F32, I24In32, I24Packed, I16],
    }
}

const AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED: u32 = 0x8889_0019;
const AUDCLNT_E_DEVICE_IN_USE: u32 = 0x8889_000A;
const AUDCLNT_E_UNSUPPORTED_FORMAT: u32 = 0x8889_0008;
const AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED: u32 = 0x8889_000E;

fn hresult(e: &wasapi::WasapiError) -> Option<u32> {
    match e {
        wasapi::WasapiError::Windows(w) => Some(w.code().0 as u32),
        _ => None,
    }
}

fn explain(e: &wasapi::WasapiError) -> String {
    match hresult(e) {
        Some(AUDCLNT_E_DEVICE_IN_USE) => "the device is in use by another application in exclusive mode".into(),
        Some(AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED) => {
            "exclusive mode is disabled for this device (Sound settings > Device properties > Advanced)".into()
        }
        Some(AUDCLNT_E_UNSUPPORTED_FORMAT) => "the device does not support this format".into(),
        _ => e.to_string(),
    }
}

/// Error type that lets the engine distinguish "format not supported" (fall back to shared).
#[derive(Debug)]
pub struct ExclusiveUnsupported(pub String);
impl std::fmt::Display for ExclusiveUnsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ExclusiveUnsupported {}

fn open_exclusive(
    device_id: Option<String>,
    src: &SourceFormat,
    shared: Arc<EngineShared>,
) -> Result<(OutputHandle, Producer<f64>)> {
    let (init_tx, init_rx) = bounded::<Result<(OutputFormat, Producer<f64>)>>(1);
    let (stop_tx, stop_rx): (Sender<()>, Receiver<()>) = bounded(1);
    let error = Arc::new(Mutex::new(None));
    let error_slot = error.clone();
    let src = src.clone();
    let negotiated_for = Some((src.sample_rate, src.bits_per_sample, src.float, src.channels));

    let thread = std::thread::Builder::new()
        .name("audio-out-exclusive".into())
        .spawn(move || {
            let _ = wasapi::initialize_mta();
            boost_thread_priority();
            let setup = || -> Result<_> {
                let enumerator = wasapi::DeviceEnumerator::new().map_err(werr)?;
                let device = match &device_id {
                    Some(id) => enumerator.get_device(id).or_else(|_| enumerator.get_default_device(&wasapi::Direction::Render)),
                    None => enumerator.get_default_device(&wasapi::Direction::Render),
                }
                .map_err(|e| anyhow!("no output device: {e}"))?;
                let name = device.get_friendlyname().unwrap_or_else(|_| "Output device".into());
                let actual_id = device.get_id().ok();
                let mut client = device.get_iaudioclient().map_err(werr)?;

                let rate = src.sample_rate as usize;
                let mut chosen = None;
                let mut channel_options = vec![src.channels];
                if src.channels != 2 {
                    channel_options.push(2);
                }
                'outer: for &ch in &channel_options {
                    for kind in candidates(&src) {
                        let (store, valid, st) = kind.spec();
                        let wf = wasapi::WaveFormat::new(store, valid, &st, rate, ch, None);
                        if let Ok(accepted) = client.is_supported_exclusive_with_quirks(&wf) {
                            chosen = Some((accepted, kind, ch));
                            break 'outer;
                        }
                    }
                }
                let Some((wf, kind, channels)) = chosen else {
                    return Err(anyhow::Error::new(ExclusiveUnsupported(format!(
                        "{name} does not support {} kHz in exclusive mode",
                        src.sample_rate as f64 / 1000.0
                    ))));
                };

                // 20 ms period, aligned to 128 bytes (Intel HDA requirement).
                let period = client
                    .calculate_aligned_period_near(200_000, Some(128), &wf)
                    .map_err(werr)?;
                let mode = wasapi::StreamMode::EventsExclusive { period_hns: period };
                if let Err(e) = client.initialize_client(&wf, &wasapi::Direction::Render, &mode) {
                    if hresult(&e) == Some(AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED) {
                        let frames = client.get_buffer_size().map_err(werr)?;
                        let aligned = wasapi::calculate_period_100ns(frames as i64, rate as i64);
                        client = device.get_iaudioclient().map_err(werr)?;
                        client
                            .initialize_client(
                                &wf,
                                &wasapi::Direction::Render,
                                &wasapi::StreamMode::EventsExclusive { period_hns: aligned },
                            )
                            .map_err(|e| anyhow!("cannot open {name} in exclusive mode: {}", explain(&e)))?;
                    } else if hresult(&e) == Some(AUDCLNT_E_UNSUPPORTED_FORMAT) {
                        return Err(anyhow::Error::new(ExclusiveUnsupported(format!(
                            "{name} rejected {} kHz in exclusive mode",
                            src.sample_rate as f64 / 1000.0
                        ))));
                    } else {
                        return Err(anyhow!("cannot open {name} in exclusive mode: {}", explain(&e)));
                    }
                }
                let event = client.set_get_eventhandle().map_err(werr)?;
                let render = client.get_audiorenderclient().map_err(werr)?;
                let buffer_frames = client.get_buffer_size().map_err(werr)? as usize;
                let (store, valid, _) = kind.spec();
                let fmt = OutputFormat {
                    mode: OutputMode::Exclusive,
                    device_id: actual_id,
                    device_name: name,
                    sample_rate: src.sample_rate,
                    channels,
                    bits: valid as u32,
                    container_bits: store as u32,
                    float: kind == SampleKind::F32,
                };
                Ok((client, event, render, buffer_frames, kind, channels, fmt))
            };

            let (client, event, render, buffer_frames, kind, channels, fmt) = match setup() {
                Ok(v) => v,
                Err(e) => {
                    let _ = init_tx.send(Err(e));
                    return;
                }
            };
            let (prod, cons) = ring_for(fmt.sample_rate, channels);
            let mut renderer = Renderer::new(cons, shared.clone(), channels, fmt.sample_rate);
            let _ = init_tx.send(Ok((fmt, prod)));

            let mut fbuf = vec![0.0f64; buffer_frames * channels];
            let mut bytes = vec![0u8; buffer_frames * channels * kind.bytes()];
            let mut dither = Dither::new();

            let mut fill = |frames: usize, render_client: &wasapi::AudioRenderClient| -> Result<()> {
                let samples = frames * channels;
                if fbuf.len() < samples {
                    fbuf.resize(samples, 0.0);
                    bytes.resize(samples * kind.bytes(), 0);
                }
                renderer.render(&mut fbuf[..samples]);
                let identity = shared.identity.load(std::sync::atomic::Ordering::Relaxed);
                encode(&fbuf[..samples], &mut bytes[..samples * kind.bytes()], kind, identity, &mut dither);
                render_client
                    .write_to_device(frames, &bytes[..samples * kind.bytes()], None)
                    .map_err(werr)
            };

            // Pre-fill one buffer so playback starts with real audio, then start the clock.
            let mut run = || -> Result<()> {
                let avail = client.get_available_space_in_frames().map_err(werr)? as usize;
                fill(avail, &render)?;
                client.start_stream().map_err(werr)?;
                let mut timeouts = 0;
                loop {
                    match stop_rx.try_recv() {
                        Ok(()) | Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                        Err(crossbeam_channel::TryRecvError::Empty) => {}
                    }
                    if event.wait_for_event(500).is_err() {
                        timeouts += 1;
                        if timeouts > 6 {
                            return Err(anyhow!("audio device stopped responding"));
                        }
                        continue;
                    }
                    timeouts = 0;
                    let avail = client.get_available_space_in_frames().map_err(werr)? as usize;
                    if avail > 0 {
                        fill(avail, &render)?;
                    }
                }
                Ok(())
            };
            if let Err(e) = run() {
                log::error!("exclusive output error: {e}");
                *error_slot.lock() = Some(format!("Audio device error: {e}"));
                // Keep the thread alive until the engine drops the handle.
                let _ = stop_rx.recv();
            }
            let _ = client.stop_stream();
        })?;

    let (format, prod) = init_rx
        .recv_timeout(Duration::from_secs(8))
        .map_err(|_| anyhow!("timed out opening the audio device"))??;
    Ok((OutputHandle { stop_tx, thread: Some(thread), error, format, negotiated_for }, prod))
}

/// Convert processed f64 samples into the device's PCM format. When the chain is bit-transparent
/// (`identity`), samples are scaled by an exact power of two and never dithered, so integer
/// sources reach the DAC unchanged.
fn encode(src: &[f64], dst: &mut [u8], kind: SampleKind, identity: bool, dither: &mut Dither) {
    match kind {
        SampleKind::I16 => {
            for (i, &x) in src.iter().enumerate() {
                let mut v = x * 32768.0;
                if !identity {
                    v += dither.tpdf();
                }
                let v = v.round().clamp(-32768.0, 32767.0) as i16;
                dst[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
            }
        }
        SampleKind::I24Packed => {
            for (i, &x) in src.iter().enumerate() {
                let mut v = x * 8_388_608.0;
                if !identity {
                    v += dither.tpdf();
                }
                let v = v.round().clamp(-8_388_608.0, 8_388_607.0) as i32;
                let b = v.to_le_bytes();
                dst[i * 3..i * 3 + 3].copy_from_slice(&b[..3]);
            }
        }
        SampleKind::I24In32 => {
            for (i, &x) in src.iter().enumerate() {
                let mut v = x * 8_388_608.0;
                if !identity {
                    v += dither.tpdf();
                }
                let v = (v.round().clamp(-8_388_608.0, 8_388_607.0) as i32) << 8;
                dst[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        SampleKind::I32 => {
            for (i, &x) in src.iter().enumerate() {
                let v = (x * 2_147_483_648.0).round().clamp(-2_147_483_648.0, 2_147_483_647.0) as i32;
                dst[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        SampleKind::F32 => {
            for (i, &x) in src.iter().enumerate() {
                dst[i * 4..i * 4 + 4].copy_from_slice(&(x as f32).to_le_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_encode_is_bit_exact_24() {
        let mut d = Dither::new();
        for s in [-8_388_608i32, -1, 0, 1, 123_456, 8_388_607] {
            let x = s as f64 / 8_388_608.0;
            let mut out = [0u8; 3];
            encode(&[x], &mut out, SampleKind::I24Packed, true, &mut d);
            let back = i32::from_le_bytes([out[0], out[1], out[2], if out[2] & 0x80 != 0 { 0xFF } else { 0 }]);
            assert_eq!(back, s);
        }
    }

    #[test]
    fn identity_encode_16_into_24in32_is_padded() {
        let mut d = Dither::new();
        let s: i16 = -12345;
        let x = s as f64 / 32768.0;
        let mut out = [0u8; 4];
        encode(&[x], &mut out, SampleKind::I24In32, true, &mut d);
        assert_eq!(i32::from_le_bytes(out), (s as i32) << 16);
    }
}
