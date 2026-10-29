//! Output on platforms other than Windows and Android, which have no backend.
//! Same API as `output.rs`; opening always fails, so the engine reports an error and stays stopped.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use rtrb::Producer;
use serde::Serialize;

use super::dsp::EngineShared;
use super::types::{OutputFormat, OutputMode, SourceFormat};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub fn list_output_devices() -> Result<Vec<DeviceInfo>> {
    Ok(Vec::new())
}

pub struct OutputHandle {
    pub format: OutputFormat,
    pub negotiated_for: Option<(u32, Option<u32>, bool, usize)>,
}

impl OutputHandle {
    pub fn take_error(&self) -> Option<String> {
        None
    }
}

pub fn open(
    _mode: OutputMode,
    _device_id: Option<String>,
    _src: &SourceFormat,
    _shared: Arc<EngineShared>,
) -> Result<(OutputHandle, Producer<f64>)> {
    Err(anyhow!("No audio output on this platform yet."))
}

#[derive(Debug)]
pub struct ExclusiveUnsupported(pub String);
impl std::fmt::Display for ExclusiveUnsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ExclusiveUnsupported {}
