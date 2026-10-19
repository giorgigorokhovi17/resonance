//! Persistent user settings (JSON in the app config dir).

use std::path::PathBuf;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::audio::eq::EqSettings;
use crate::audio::types::{OutputMode, ReplayGainMode};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub output_mode: OutputMode,
    pub device_id: Option<String>,
    pub volume: f64,
    pub replay_gain: ReplayGainMode,
    pub replay_gain_preamp: f64,
    /// "dark" | "light" | "system"
    pub theme: String,
    pub close_to_tray: bool,
    pub watch_folders: bool,
    pub eq: EqSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_mode: OutputMode::Shared,
            device_id: None,
            volume: 0.8,
            replay_gain: ReplayGainMode::Off,
            replay_gain_preamp: 0.0,
            theme: "dark".into(),
            close_to_tray: false,
            watch_folders: true,
            eq: EqSettings::default(),
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
    data: Mutex<Settings>,
}

impl SettingsStore {
    pub fn load(path: PathBuf) -> Self {
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .map(|mut s| {
                s.eq = s.eq.clone().normalized();
                s
            })
            .unwrap_or_default();
        Self { path, data: Mutex::new(data) }
    }

    pub fn get(&self) -> Settings {
        self.data.lock().clone()
    }

    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> Settings {
        let snapshot = {
            let mut s = self.data.lock();
            f(&mut s);
            s.clone()
        };
        if let Err(e) = self.save(&snapshot) {
            log::error!("failed to save settings: {e}");
        }
        snapshot
    }

    fn save(&self, s: &Settings) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(s).unwrap_or_default())?;
        std::fs::rename(&tmp, &self.path)
    }
}
