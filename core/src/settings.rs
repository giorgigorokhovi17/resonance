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
    /// Shortcuts that work while other apps are in front (hotkeys.rs).
    pub hotkeys: HotkeySettings,
    /// Speed and reverb (Effects in the player bar).
    pub effects: EffectSettings,
    pub eq: EqSettings,
    pub appearance: Appearance,
    pub soulseek: SoulseekSettings,
    pub youtube: YouTubeSettings,
    pub phone_sync: PhoneSyncSettings,
}

/// Phone sync (phone.rs): playlists on the home network for a phone's browser. Off until turned on.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PhoneSyncSettings {
    pub enabled: bool,
    pub port: u16,
    /// Secret part of the link; without it the server answers nothing. Made when first needed.
    pub key: String,
}

pub const DEFAULT_PHONE_PORT: u16 = 8787;

impl Default for PhoneSyncSettings {
    fn default() -> Self {
        Self { enabled: false, port: DEFAULT_PHONE_PORT, key: String::new() }
    }
}

impl PhoneSyncSettings {
    /// A port from 1024 up, and a key of letters and digits (a fresh one when missing or odd).
    pub fn normalized(mut self) -> Self {
        if self.port < 1024 {
            self.port = DEFAULT_PHONE_PORT;
        }
        if self.key.len() < 16 || !self.key.chars().all(|c| c.is_ascii_alphanumeric()) {
            self.key = new_key();
        }
        self
    }
}

/// 32 random hex digits. Windows' random numbers through the standard library's hash keys, which
/// are seeded from them per process; no extra crate for this one use.
pub fn new_key() -> String {
    use std::hash::{BuildHasher, Hasher};
    let part = |salt: u64| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(salt);
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        h.finish()
    };
    format!("{:016x}{:016x}", part(1), part(2))
}

/// YouTube downloads (sources/youtube.rs). Off until the user turns them on.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct YouTubeSettings {
    pub enabled: bool,
}

/// Soulseek options. The account itself is in Windows Credential Manager or Android Keystore, not here.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SoulseekSettings {
    /// Where downloads go. `None` means a `Soulseek` folder inside the first library folder.
    pub download_folder: Option<String>,
    /// Accept incoming connections from other users (needed for some results and downloads).
    pub listen: bool,
    pub listen_port: u16,
    /// On Android, download only on Wi-Fi (unmetered networks) unless turned off.
    pub wifi_only: bool,
}

impl Default for SoulseekSettings {
    fn default() -> Self {
        Self { download_folder: None, listen: true, listen_port: 2234, wifi_only: true }
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

/// Global shortcuts, as key names joined with "+" ("Ctrl+Alt+ArrowRight"). `None` is unset.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct HotkeySettings {
    pub enabled: bool,
    pub play_pause: Option<String>,
    pub next: Option<String>,
    pub previous: Option<String>,
    pub volume_up: Option<String>,
    pub volume_down: Option<String>,
    pub mute: Option<String>,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        let keys = |k: &str| Some(format!("Ctrl+Alt+{k}"));
        Self {
            enabled: true,
            play_pause: keys("Space"),
            next: keys("ArrowRight"),
            previous: keys("ArrowLeft"),
            volume_up: keys("ArrowUp"),
            volume_down: keys("ArrowDown"),
            mute: keys("KeyM"),
        }
    }
}

impl HotkeySettings {
    /// Trims the shortcuts; empty or overlong ones become unset.
    pub fn normalized(mut self) -> Self {
        let clean = |k: Option<String>| k.map(|k| k.trim().to_string()).filter(|k| !k.is_empty() && k.len() <= 64);
        self.play_pause = clean(self.play_pause);
        self.next = clean(self.next);
        self.previous = clean(self.previous);
        self.volume_up = clean(self.volume_up);
        self.volume_down = clean(self.volume_down);
        self.mute = clean(self.mute);
        self
    }
}

/// Playback speed (pitch follows, like a record: 0.8 is "slowed") and reverb.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct EffectSettings {
    pub speed: f64,
    /// How much reverb, 0 (none) to 1.
    pub reverb: f64,
}

pub const MIN_SPEED: f64 = 0.5;
pub const MAX_SPEED: f64 = 1.5;

impl Default for EffectSettings {
    fn default() -> Self {
        Self { speed: 1.0, reverb: 0.0 }
    }
}

impl EffectSettings {
    pub fn normalized(self) -> Self {
        let d = Self::default();
        let speed = if self.speed.is_finite() { self.speed.clamp(MIN_SPEED, MAX_SPEED) } else { d.speed };
        let reverb = if self.reverb.is_finite() { self.reverb.clamp(0.0, 1.0) } else { d.reverb };
        // Snap near-normal speed to exactly 1 so playback stays untouched.
        Self { speed: if (speed - 1.0).abs() < 0.005 { 1.0 } else { speed }, reverb }
    }

    pub fn is_active(&self) -> bool {
        self.speed != 1.0 || self.reverb > 0.0
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
    /// The screen for the corner and edge positions (its Windows name, e.g. `\\.\DISPLAY2`);
    /// `None` for the main screen. A dragged ("custom") spot keeps its own screen.
    pub screen: Option<String>,
    /// It can't be dragged.
    pub locked: bool,
    /// Clicks go to whatever is underneath, except while Alt is held.
    pub click_through: bool,
    /// Only shows while Undertone is minimised or in the tray.
    pub only_when_minimized: bool,
    /// Steps aside while a game or video is full screen.
    pub hide_in_fullscreen: bool,
    /// When not hiding: goes to another screen while a game or video is full screen (a window on
    /// top of a game breaks its vsync), and comes back after.
    pub move_in_fullscreen: bool,
    pub show_title: bool,
    pub show_artist: bool,
    /// Previous, play/pause and next on the small island.
    pub show_controls: bool,
    /// Shuffle and repeat next to them.
    pub show_shuffle_repeat: bool,
    /// A folder button that lists the playlists to play one.
    pub show_playlists: bool,
    /// A seek bar along the bottom of the small island.
    pub show_progress: bool,
    /// Grows into controls when pointed at.
    pub expand_on_hover: bool,
    /// Shows a card with the song for a few seconds when a new one starts.
    pub peek_on_new_song: bool,
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
            screen: None,
            locked: false,
            click_through: false,
            only_when_minimized: true,
            hide_in_fullscreen: true,
            move_in_fullscreen: true,
            show_title: true,
            show_artist: true,
            show_controls: true,
            show_shuffle_repeat: true,
            show_playlists: true,
            show_progress: true,
            expand_on_hover: true,
            peek_on_new_song: true,
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
        self.screen = self.screen.map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s.len() <= 64);
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
            hotkeys: HotkeySettings::default(),
            effects: EffectSettings::default(),
            eq: EqSettings::default(),
            appearance: Appearance::default(),
            soulseek: SoulseekSettings::default(),
            youtube: YouTubeSettings::default(),
            phone_sync: PhoneSyncSettings::default(),
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
                s.hotkeys = s.hotkeys.clone().normalized();
                s.effects = s.effects.normalized();
                s.phone_sync = s.phone_sync.clone().normalized();
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

/// A saved equalizer setting (Settings → EQ presets, kept in backups).
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EqPreset {
    pub name: String,
    pub eq: EqSettings,
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
        assert!(o.show_controls && o.show_shuffle_repeat && o.show_playlists && o.show_progress && o.peek_on_new_song);
        assert!(!o.locked && !o.click_through);
        assert!(o.move_in_fullscreen, "settings from before 0.7.15 move the island off full-screen games");
        assert_eq!(o.screen, None, "the main screen unless one is chosen");
        let o: IslandOptions = serde_json::from_str(r#"{"screen":"  "}"#).unwrap();
        assert_eq!(o.normalized().screen, None);
        let o: IslandOptions = serde_json::from_str(r#"{"screen":" \\\\.\\DISPLAY2 "}"#).unwrap();
        assert_eq!(o.normalized().screen.as_deref(), Some(r"\\.\DISPLAY2"));

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
    fn hotkeys_and_effects() {
        // Settings from before: default shortcuts, no effects.
        let s: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert!(s.hotkeys.enabled);
        assert_eq!(s.hotkeys.next.as_deref(), Some("Ctrl+Alt+ArrowRight"));
        assert_eq!(s.effects, EffectSettings::default());
        assert!(!s.effects.is_active());

        let h = HotkeySettings { play_pause: Some("  Ctrl+Shift+KeyP ".into()), next: Some(" ".into()), ..HotkeySettings::default() };
        let h = h.normalized();
        assert_eq!((h.play_pause.as_deref(), h.next), (Some("Ctrl+Shift+KeyP"), None));

        let e = EffectSettings { speed: 0.1, reverb: 3.0 }.normalized();
        assert_eq!((e.speed, e.reverb), (MIN_SPEED, 1.0));
        let e = EffectSettings { speed: 1.003, reverb: f64::NAN }.normalized();
        assert_eq!((e.speed, e.reverb), (1.0, 0.0), "near-normal speed snaps to untouched playback");
        assert!(EffectSettings { speed: 0.8, reverb: 0.0 }.is_active());
    }

    #[test]
    fn phone_sync_settings() {
        let s: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert!(!s.phone_sync.enabled, "off until turned on");
        assert_eq!(s.phone_sync.port, DEFAULT_PHONE_PORT);

        let p = PhoneSyncSettings { enabled: true, port: 80, key: "short".into() }.normalized();
        assert_eq!(p.port, DEFAULT_PHONE_PORT, "no privileged ports");
        assert_eq!(p.key.len(), 32);
        assert!(p.key.chars().all(|c| c.is_ascii_hexdigit()));
        let again = p.clone().normalized();
        assert_eq!(again.key, p.key, "a good key is kept");
        assert_ne!(new_key(), new_key());
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
