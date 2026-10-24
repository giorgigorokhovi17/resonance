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
    /// Crossfade between songs, in seconds (0 = off: songs follow each other gaplessly).
    pub crossfade: f64,
    /// "dark" | "light" | "system"
    pub theme: String,
    pub close_to_tray: bool,
    pub watch_folders: bool,
    /// The Dynamic Island (src/island.rs).
    pub island: bool,
    pub island_options: IslandOptions,
    pub eq: EqSettings,
    pub appearance: Appearance,
    pub soulseek: SoulseekSettings,
    pub youtube: YouTubeSettings,
}

/// YouTube downloads (sources/youtube.rs). Off until the user turns them on.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct YouTubeSettings {
    pub enabled: bool,
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
    /// Playlist page design: "classic" (header above the songs) | "split" (cover, details and the
    /// playing song's lyrics on the left, songs on the right).
    pub playlist_layout: String,
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
            playlist_layout: "classic".into(),
        }
    }
}

/// Where the Dynamic Island sits and what it shows. Changed from its right-click menu and from
/// Settings; the island window (src/views/Island.tsx) applies them.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct IslandOptions {
    /// "topLeft" | "topCenter" | "topRight" | "middleLeft" | "middleRight" | "bottomLeft" |
    /// "bottomCenter" | "bottomRight", or "custom" (where it was dragged to: `x`, `y`).
    pub position: String,
    /// Top-left corner of the collapsed island, in screen (physical) pixels, for "custom".
    pub x: Option<f64>,
    pub y: Option<f64>,
    /// It can't be dragged.
    pub locked: bool,
    /// Clicks go to whatever is underneath (undone from Settings or the tray menu).
    pub click_through: bool,
    /// Only shows while Undertone is minimised or in the tray.
    pub only_when_minimized: bool,
    /// Steps aside while a game or video is full screen.
    pub hide_in_fullscreen: bool,
    pub show_title: bool,
    pub show_artist: bool,
    /// Grows into controls when pointed at.
    pub expand_on_hover: bool,
    /// "cover" (from the playing song's artwork), a built-in palette id, or "custom".
    pub colors: String,
    /// Three `#rrggbb` colours for "custom".
    pub custom_colors: Vec<String>,
}

pub const ISLAND_POSITIONS: [&str; 9] =
    ["topLeft", "topCenter", "topRight", "middleLeft", "middleRight", "bottomLeft", "bottomCenter", "bottomRight", "custom"];

impl Default for IslandOptions {
    fn default() -> Self {
        Self {
            position: "topCenter".into(),
            x: None,
            y: None,
            locked: false,
            click_through: false,
            only_when_minimized: true,
            hide_in_fullscreen: true,
            show_title: true,
            show_artist: true,
            expand_on_hover: true,
            colors: "cover".into(),
            custom_colors: vec!["#5a3cc8".into(), "#c83c8c".into(), "#2864c8".into()],
        }
    }
}

impl IslandOptions {
    pub fn normalized(mut self) -> Self {
        let d = Self::default();
        let finite = |v: Option<f64>| v.filter(|v| v.is_finite() && v.abs() < 100_000.0);
        self.x = finite(self.x);
        self.y = finite(self.y);
        let custom_ok = self.x.is_some() && self.y.is_some();
        if !ISLAND_POSITIONS.contains(&self.position.as_str()) || (self.position == "custom" && !custom_ok) {
            self.position = d.position;
        }
        if self.colors.is_empty() || self.colors.len() > 32 {
            self.colors = d.colors;
        }
        let colors: Vec<String> = self.custom_colors.into_iter().filter_map(|c| hex_color(Some(c))).take(3).collect();
        self.custom_colors = if colors.len() == 3 { colors } else { d.custom_colors };
        self
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
        if !matches!(self.playlist_layout.as_str(), "classic" | "split") {
            self.playlist_layout = d.playlist_layout;
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
            crossfade: 0.0,
            theme: "dark".into(),
            close_to_tray: false,
            watch_folders: true,
            island: false,
            island_options: IslandOptions::default(),
            eq: EqSettings::default(),
            appearance: Appearance::default(),
            soulseek: SoulseekSettings::default(),
            youtube: YouTubeSettings::default(),
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
                s.island_options = s.island_options.clone().normalized();
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
        assert_eq!(s.island_options, IslandOptions::default());
        assert_eq!(s.soulseek, SoulseekSettings::default());
        assert!(!s.youtube.enabled, "YouTube downloads are off until turned on");
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
    fn playlist_layout_setting() {
        let s: Settings = serde_json::from_str(r#"{"appearance":{"preset":"ocean"}}"#).unwrap();
        assert_eq!(s.appearance.playlist_layout, "classic", "settings from before keep today's design");
        let s: Settings = serde_json::from_str(r#"{"appearance":{"playlistLayout":"split"}}"#).unwrap();
        assert_eq!(s.appearance.normalized().playlist_layout, "split");
        let a = Appearance { playlist_layout: "grid".into(), ..Appearance::default() }.normalized();
        assert_eq!(a.playlist_layout, "classic");
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
    fn island_options() {
        // Settings from 0.7.x: the old mini player switch is ignored, the island keeps its defaults.
        let s: Settings = serde_json::from_str(r#"{"minimizeToMini":true,"island":true}"#).unwrap();
        assert!(s.island);
        let o = &s.island_options;
        assert_eq!((o.position.as_str(), o.colors.as_str()), ("topCenter", "cover"));
        assert!(o.only_when_minimized && o.hide_in_fullscreen && o.show_title && o.show_artist && o.expand_on_hover);
        assert!(!o.locked && !o.click_through);

        let o: IslandOptions =
            serde_json::from_str(r##"{"position":"custom","x":120.5,"y":40,"customColors":["#AABBCC","#000000","#ffffff"]}"##).unwrap();
        let o = o.normalized();
        assert_eq!((o.position.as_str(), o.x, o.y), ("custom", Some(120.5), Some(40.0)));
        assert_eq!(o.custom_colors, ["#aabbcc", "#000000", "#ffffff"]);

        // A custom position without coordinates, unknown positions and bad colours fall back.
        let o = IslandOptions { position: "custom".into(), x: Some(f64::NAN), y: Some(3.0), ..IslandOptions::default() }.normalized();
        assert_eq!((o.position.as_str(), o.x), ("topCenter", None));
        let o = IslandOptions { position: "sideways".into(), custom_colors: vec!["red".into()], ..IslandOptions::default() }.normalized();
        assert_eq!(o.position, "topCenter");
        assert_eq!(o.custom_colors, IslandOptions::default().custom_colors);
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
