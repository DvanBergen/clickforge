use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage::{
    StorageError, read_json_bounded, resolve_default_config_dir, write_json_atomically,
};

/// File name for persisted workbench and system tray settings.
pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// Maximum settings file size accepted when loading from disk (64 KiB).
pub const MAX_SETTINGS_FILE_BYTES: u64 = 65_536;

/// Default workbench window width in logical pixels.
pub const DEFAULT_WINDOW_WIDTH_PX: f32 = 1280.0;

/// Default workbench window height in logical pixels.
pub const DEFAULT_WINDOW_HEIGHT_PX: f32 = 820.0;

/// Default telemetry drawer height in logical pixels.
pub const DEFAULT_TELEMETRY_DRAWER_HEIGHT_PX: f32 = 176.0;

const MIN_WINDOW_WIDTH_PX: f32 = 800.0;
const MAX_WINDOW_WIDTH_PX: f32 = 7680.0;
const MIN_WINDOW_HEIGHT_PX: f32 = 560.0;
const MAX_WINDOW_HEIGHT_PX: f32 = 4320.0;
const MIN_DRAWER_HEIGHT_PX: f32 = 100.0;
const MAX_DRAWER_HEIGHT_PX: f32 = 600.0;

/// User preferences persisted under `~/.config/clickforge/settings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub minimize_to_tray_on_close: bool,
    pub start_minimized_to_tray: bool,
    pub show_telemetry_drawer: bool,
    pub telemetry_drawer_height_px: f32,
    pub window_width_px: f32,
    pub window_height_px: f32,
    pub last_selected_script_id: Option<String>,
    pub show_gaussian_histogram: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            minimize_to_tray_on_close: true,
            start_minimized_to_tray: false,
            show_telemetry_drawer: false,
            telemetry_drawer_height_px: DEFAULT_TELEMETRY_DRAWER_HEIGHT_PX,
            window_width_px: DEFAULT_WINDOW_WIDTH_PX,
            window_height_px: DEFAULT_WINDOW_HEIGHT_PX,
            last_selected_script_id: None,
            show_gaussian_histogram: true,
        }
    }
}

impl AppSettings {
    /// Clamps persisted window and panel dimensions to safe display ranges.
    pub fn clamped(mut self) -> Self {
        self.window_width_px = clamp_finite_f32(
            self.window_width_px,
            MIN_WINDOW_WIDTH_PX,
            MAX_WINDOW_WIDTH_PX,
            DEFAULT_WINDOW_WIDTH_PX,
        );
        self.window_height_px = clamp_finite_f32(
            self.window_height_px,
            MIN_WINDOW_HEIGHT_PX,
            MAX_WINDOW_HEIGHT_PX,
            DEFAULT_WINDOW_HEIGHT_PX,
        );
        self.telemetry_drawer_height_px = clamp_finite_f32(
            self.telemetry_drawer_height_px,
            MIN_DRAWER_HEIGHT_PX,
            MAX_DRAWER_HEIGHT_PX,
            DEFAULT_TELEMETRY_DRAWER_HEIGHT_PX,
        );
        self
    }
}

/// Atomic JSON store for `AppSettings`.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    settings_path: PathBuf,
}

impl SettingsStore {
    /// Opens the default settings file at `~/.config/clickforge/settings.json`.
    pub fn new_default() -> Result<Self, StorageError> {
        let default_directory = resolve_default_config_dir()?;
        Ok(Self::new_with_base_dir(default_directory))
    }

    /// Opens the settings file rooted inside `base_directory`.
    pub fn new_with_base_dir(base_directory: PathBuf) -> Self {
        Self {
            settings_path: base_directory.join(SETTINGS_FILE_NAME),
        }
    }

    /// Returns the full path to `settings.json`.
    pub fn settings_path(&self) -> &Path {
        &self.settings_path
    }

    /// Loads `AppSettings` from disk, returning defaults if the file does not exist yet.
    pub fn load(&self) -> Result<AppSettings, StorageError> {
        if !self.settings_path.exists() {
            return Ok(AppSettings::default());
        }
        let loaded_settings: AppSettings =
            read_json_bounded(&self.settings_path, MAX_SETTINGS_FILE_BYTES)?;
        Ok(loaded_settings.clamped())
    }

    /// Clamps numeric fields and atomically saves `AppSettings` to disk.
    pub fn save(&self, settings: &AppSettings) -> Result<(), StorageError> {
        let sanitized_settings = settings.clone().clamped();
        write_json_atomically(&self.settings_path, &sanitized_settings)
    }
}

fn clamp_finite_f32(raw_value: f32, min_value: f32, max_value: f32, fallback_value: f32) -> f32 {
    if !raw_value.is_finite() {
        return fallback_value;
    }
    raw_value.clamp(min_value, max_value)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::models::ScriptDocument;

    #[test]
    fn settings_store_loads_defaults_and_clamps_roundtrip() -> Result<(), Box<dyn std::error::Error>>
    {
        let timestamp_ms = ScriptDocument::current_unix_timestamp_ms();
        let random_suffix: u32 = rand::random();
        let temp_dir = std::env::temp_dir().join(format!(
            "clickforge-settings-test-{timestamp_ms}-{random_suffix:08x}"
        ));
        fs::create_dir_all(&temp_dir)?;

        let store = SettingsStore::new_with_base_dir(temp_dir.clone());
        assert_eq!(store.load()?, AppSettings::default());

        let custom_settings = AppSettings {
            minimize_to_tray_on_close: false,
            window_width_px: 100.0,
            telemetry_drawer_height_px: 5000.0,
            last_selected_script_id: Some("script-1".to_string()),
            ..AppSettings::default()
        };
        store.save(&custom_settings)?;

        let reloaded = store.load()?;
        assert!(!reloaded.minimize_to_tray_on_close);
        assert_eq!(reloaded.window_width_px, MIN_WINDOW_WIDTH_PX);
        assert_eq!(reloaded.telemetry_drawer_height_px, MAX_DRAWER_HEIGHT_PX);
        assert_eq!(
            reloaded.last_selected_script_id.as_deref(),
            Some("script-1")
        );

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }
}
