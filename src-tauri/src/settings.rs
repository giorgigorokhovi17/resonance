//! Persistent user settings (JSON in the app config dir).

use std::path::PathBuf;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::audio::eq::EqSettings;
use crate::audio::types::{OutputMode, ReplayGainMode};
use crate::library::media::Backdrop;

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
    /// The minimize button switches to the mini player instead of hiding the window.
    pub minimize_to_mini: bool,
    pub eq: EqSettings,
    pub appearance: Appearance,
    pub soulseek: SoulseekSettings,
}

/// Soulseek options. The account itself is in Windows Credential Manager, not here.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SoulseekSettings {
    /// Where downloads go. `None` means a `Soulseek` folder inside the first library folder.
    pub download_folder: Option<String>,
    /// Accept incoming connections from other users (needed for some results and downloads).
    pub listen: bool,
    pub listen_port: u16,
}

impl Default for SoulseekSettings {
    fn default() -> Self {
        Self { download_folder: None, listen: true, listen_port: 2234 }
    }
}

/// Look-and-feel options from the Visual settings tab. Only stored here; the frontend applies them.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    /// Built-in palette id ("ember", "ocean", ...). Unknown ids fall back to the default palette.
    pub preset: String,
    /// Custom accent colours (`#rrggbb`) that override the preset's.
    pub accent: Option<String>,
    pub accent2: Option<String>,
    /// Take the accent from the playing track's album art.
    pub cover_accent: bool,
    /// True black backgrounds in dark mode.
    pub pure_black: bool,
    /// Album-art colour wash behind the library views.
    pub ambient: bool,
    /// Strength of the album-art colour wash and the Now Playing tint, 0..1.
    pub ambient_intensity: f64,
    /// Blur of the Now Playing artwork backdrop, in px.
    pub backdrop_blur: f64,
    /// "sharp" | "default" | "round"
    pub corners: String,
    /// Interface zoom, 0.8..1.5.
    pub ui_scale: f64,
    /// Custom photo, GIF or video behind the app (a media id with display options).
    pub background: Option<Backdrop>,
    /// Where the custom background shows: "app" (behind everything) | "nowPlaying".
    pub background_scope: String,
    /// Where GIF and video artwork moves: "everywhere" | "nowPlaying" | "off" (the default: it
    /// costs CPU, GPU and battery, so it's opt-in).
    pub animated_art: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            preset: "ember".into(),
            accent: None,
            accent2: None,
            cover_accent: false,
            pure_black: false,
            ambient: true,
            ambient_intensity: 1.0,
            backdrop_blur: 90.0,
            corners: "default".into(),
            ui_scale: 1.0,
            background: None,
            background_scope: "app".into(),
            animated_art: "off".into(),
        }
    }
}

fn hex_color(c: Option<String>) -> Option<String> {
    let c = c?.trim().to_ascii_lowercase();
    let ok = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
    ok.then_some(c)
}

impl Appearance {
    /// Clamps numbers into range and drops values the frontend could not use.
    pub fn normalized(mut self) -> Self {
        let d = Self::default();
        if self.preset.is_empty() || self.preset.len() > 32 {
            self.preset = d.preset;
        }
        self.accent = hex_color(self.accent);
        self.accent2 = hex_color(self.accent2);
        self.ambient_intensity = if self.ambient_intensity.is_finite() { self.ambient_intensity.clamp(0.0, 1.0) } else { d.ambient_intensity };
        self.backdrop_blur = if self.backdrop_blur.is_finite() { self.backdrop_blur.clamp(0.0, 160.0) } else { d.backdrop_blur };
        if !matches!(self.corners.as_str(), "sharp" | "default" | "round") {
            self.corners = d.corners;
        }
        self.ui_scale = if self.ui_scale.is_finite() { self.ui_scale.clamp(0.8, 1.5) } else { d.ui_scale };
        self.background = self.background.and_then(Backdrop::normalized);
        if !matches!(self.background_scope.as_str(), "app" | "nowPlaying") {
            self.background_scope = d.background_scope;
        }
        if !matches!(self.animated_art.as_str(), "everywhere" | "nowPlaying" | "off") {
            self.animated_art = d.animated_art;
        }
        self
    }
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
            minimize_to_mini: false,
            eq: EqSettings::default(),
            appearance: Appearance::default(),
            soulseek: SoulseekSettings::default(),
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
                s.appearance = s.appearance.clone().normalized();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_without_appearance_load_defaults() {
        let s: Settings = serde_json::from_str(r#"{"theme":"light","closeToTray":true}"#).unwrap();
        assert_eq!(s.theme, "light");
        assert!(s.close_to_tray);
        assert_eq!(s.appearance, Appearance::default());
        assert!(!s.minimize_to_mini);
        assert_eq!(s.soulseek, SoulseekSettings::default());
    }

    #[test]
    fn partial_soulseek_settings_keep_defaults() {
        let s: Settings = serde_json::from_str(r#"{"soulseek":{"downloadFolder":"D:\\Music\\In"}}"#).unwrap();
        assert_eq!(s.soulseek.download_folder.as_deref(), Some(r"D:\Music\In"));
        assert!(s.soulseek.listen);
        assert_eq!(s.soulseek.listen_port, 2234);
    }

    #[test]
    fn partial_appearance_keeps_other_defaults() {
        let s: Settings = serde_json::from_str(r#"{"appearance":{"preset":"ocean","pureBlack":true}}"#).unwrap();
        assert_eq!(s.appearance.preset, "ocean");
        assert!(s.appearance.pure_black);
        assert_eq!(s.appearance.backdrop_blur, 90.0);
        assert!(s.appearance.ambient);
    }

    #[test]
    fn appearance_normalization() {
        let a = Appearance {
            preset: String::new(),
            accent: Some(" #FFAA00 ".into()),
            accent2: Some("red".into()),
            ambient_intensity: 4.0,
            backdrop_blur: f64::NAN,
            corners: "wobbly".into(),
            ui_scale: 0.1,
            ..Appearance::default()
        }
        .normalized();
        assert_eq!(a.preset, "ember");
        assert_eq!(a.accent.as_deref(), Some("#ffaa00"));
        assert_eq!(a.accent2, None);
        assert_eq!(a.ambient_intensity, 1.0);
        assert_eq!(a.backdrop_blur, 90.0);
        assert_eq!(a.corners, "default");
        assert_eq!(a.ui_scale, 0.8);
    }

    #[test]
    fn background_and_animation_settings() {
        // Settings saved before custom backgrounds existed.
        let s: Settings = serde_json::from_str(r#"{"appearance":{"preset":"ocean"}}"#).unwrap();
        assert_eq!((s.appearance.background.as_ref(), s.appearance.background_scope.as_str()), (None, "app"));
        assert_eq!(s.appearance.animated_art, "off", "GIFs and videos don't move until turned on");

        let s: Settings = serde_json::from_str(
            r#"{"appearance":{"background":{"mediaId":"ab12","fit":"contain","dim":0.9,"blur":80},
                "backgroundScope":"nowPlaying","animatedArt":"off"}}"#,
        )
        .unwrap();
        let a = s.appearance.normalized();
        let bg = a.background.unwrap();
        assert_eq!((bg.media_id.as_str(), bg.fit.as_str(), bg.dim, bg.blur), ("ab12", "contain", Some(0.8), 40.0));
        assert_eq!((a.background_scope.as_str(), a.animated_art.as_str()), ("nowPlaying", "off"));

        let a = Appearance {
            background: Some(Backdrop { media_id: "not hex!".into(), ..Backdrop::default() }),
            background_scope: "moon".into(),
            animated_art: "sometimes".into(),
            ..Appearance::default()
        }
        .normalized();
        assert_eq!((a.background, a.background_scope.as_str(), a.animated_art.as_str()), (None, "app", "off"));
    }

    #[test]
    fn store_round_trips_appearance() {
        let dir = std::env::temp_dir().join(format!("resonance-settings-test-{}", std::process::id()));
        let path = dir.join("settings.json");
        let store = SettingsStore::load(path.clone());
        store.update(|s| {
            s.appearance.preset = "nord".into();
            s.appearance.accent = Some("#123456".into());
        });
        let again = SettingsStore::load(path);
        assert_eq!(again.get().appearance.preset, "nord");
        assert_eq!(again.get().appearance.accent.as_deref(), Some("#123456"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
