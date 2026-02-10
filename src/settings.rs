use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::render::{ColorTheme, DisplayMode};
use crate::ScopeApp;

/// Returns the path to the settings file: `~/.config/scope-rs/settings.json`
fn settings_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("scope-rs");
    path.push("settings.json");
    path
}

/// Persisted application settings.
///
/// Serialized as JSON to the platform config directory.
/// Fields use `#[serde(default)]` so that adding new settings
/// won't break existing config files.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    // Display
    pub display_mode: DisplayMode,
    pub color_theme: ColorTheme,
    pub line_width: f32,
    pub intensity: f32,
    pub persistence: f32,
    pub zoom: f32,

    // Channel controls
    pub swap_xy: bool,
    pub invert_x: bool,
    pub invert_y: bool,
    pub dc_offset_x: f32,
    pub dc_offset_y: f32,

    // Audio input
    pub gain: f32,

    // File playback
    pub volume: f32,
    pub speed: f32,
    pub loop_enabled: bool,

    // Window
    pub show_settings: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            display_mode: DisplayMode::default(),
            color_theme: ColorTheme::default(),
            line_width: 1.5,
            intensity: 1.0,
            persistence: 0.85,
            zoom: 1.0,

            swap_xy: false,
            invert_x: false,
            invert_y: false,
            dc_offset_x: 0.0,
            dc_offset_y: 0.0,

            gain: 1.0,

            volume: 1.0,
            speed: 1.0,
            loop_enabled: false,

            show_settings: false,
        }
    }
}

impl AppSettings {
    /// Load settings from disk, falling back to defaults on any error.
    pub fn load() -> Self {
        let path = settings_path();
        match std::fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str(&contents) {
                Ok(settings) => {
                    log::info!("Loaded settings from {}", path.display());
                    settings
                }
                Err(e) => {
                    log::warn!("Failed to parse settings ({}), using defaults", e);
                    Self::default()
                }
            },
            Err(e) => {
                log::info!("No settings file found ({}), using defaults", e);
                Self::default()
            }
        }
    }

    /// Save settings to disk as pretty JSON.
    pub fn save(&self) {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::warn!("Failed to create config directory: {}", e);
                return;
            }
        }
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    log::warn!("Failed to write settings: {}", e);
                }
            }
            Err(e) => {
                log::warn!("Failed to serialize settings: {}", e);
            }
        }
    }

    /// Extract current settings from the running application.
    pub fn from_app(app: &ScopeApp) -> Self {
        Self {
            display_mode: app.oscilloscope.settings.display_mode,
            color_theme: app.oscilloscope.settings.theme,
            line_width: app.oscilloscope.settings.line_width,
            intensity: app.oscilloscope.settings.intensity,
            persistence: app.oscilloscope.settings.persistence,
            zoom: app.oscilloscope.settings.zoom,

            swap_xy: app.oscilloscope.settings.swap_xy,
            invert_x: app.oscilloscope.settings.invert_x,
            invert_y: app.oscilloscope.settings.invert_y,
            dc_offset_x: app.oscilloscope.settings.dc_offset_x,
            dc_offset_y: app.oscilloscope.settings.dc_offset_y,

            gain: app.audio.gain,

            volume: app.file_player.volume,
            speed: app.file_player.speed,
            loop_enabled: app.file_player.loop_playback,

            show_settings: app.show_settings,
        }
    }

    /// Apply loaded settings to the running application.
    pub fn apply(&self, app: &mut ScopeApp) {
        app.oscilloscope.settings.display_mode = self.display_mode;
        app.oscilloscope.settings.apply_theme(self.color_theme);
        app.oscilloscope.settings.line_width = self.line_width;
        app.oscilloscope.settings.intensity = self.intensity;
        app.oscilloscope.settings.persistence = self.persistence;
        app.oscilloscope.settings.zoom = self.zoom;

        app.oscilloscope.settings.swap_xy = self.swap_xy;
        app.oscilloscope.settings.invert_x = self.invert_x;
        app.oscilloscope.settings.invert_y = self.invert_y;
        app.oscilloscope.settings.dc_offset_x = self.dc_offset_x;
        app.oscilloscope.settings.dc_offset_y = self.dc_offset_y;

        app.audio.gain = self.gain;
        app.audio.sync_gain();

        app.file_player.volume = self.volume;
        app.file_player.speed = self.speed;
        app.file_player.loop_playback = self.loop_enabled;

        app.show_settings = self.show_settings;
    }
}
