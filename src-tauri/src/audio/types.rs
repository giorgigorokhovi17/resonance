use serde::{Deserialize, Serialize};

/// How the output device is driven.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    /// Regular WASAPI shared mode through the Windows mixer (works with Equalizer APO).
    #[default]
    Shared,
    /// WASAPI exclusive mode, device opened at the file's native format (bit-perfect capable).
    Exclusive,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReplayGainMode {
    #[default]
    Off,
    Track,
    Album,
}

/// Properties of the decoded source stream.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceFormat {
    pub codec: String,
    pub sample_rate: u32,
    /// Bit depth of the source PCM (None for lossy/float codecs).
    pub bits_per_sample: Option<u32>,
    pub channels: usize,
    /// Average bitrate of the file in kbps.
    pub bitrate_kbps: Option<u32>,
    pub lossless: bool,
    /// The decoder produces floating-point samples (lossy codecs).
    pub float: bool,
}

/// What the output device was actually opened with.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OutputFormat {
    pub mode: OutputMode,
    pub device_id: Option<String>,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: usize,
    /// Valid bits per sample sent to the device.
    pub bits: u32,
    /// Container size in bits (e.g. 24-bit audio in a 32-bit container).
    pub container_bits: u32,
    pub float: bool,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum PlayState {
    #[default]
    Stopped,
    Playing,
    Paused,
}
