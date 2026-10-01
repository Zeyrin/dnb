//! The player's settings, kept in a RON file in the OS config directory.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SETTINGS_VERSION: u32 = 1;

/// Offsets measured by the calibration wizard, for one audio output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Calibration {
    /// How late taps land after the sound: subtracted from every tap before judging.
    pub audio_ms: f64,
    /// How late taps land after a flash on screen.
    pub video_ms: f64,
}

impl Calibration {
    /// How far ahead visuals must run so that tapping along to them lands on the sound.
    pub fn visual_lead_ms(&self) -> f64 {
        self.video_ms - self.audio_ms
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    /// Controller layout preset by name: "Reel" or "Drummer".
    pub layout: String,
    /// Calibration per audio output, by device name: Bluetooth headphones and
    /// a wired interface need very different offsets.
    pub calibration: BTreeMap<String, Calibration>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            version: SETTINGS_VERSION,
            layout: "Reel".to_owned(),
            calibration: BTreeMap::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("reading or writing {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("{path} is not valid settings: {message}")]
    Parse { path: PathBuf, message: String },
}

impl Settings {
    /// `<config dir>/wheelup/settings.ron`, when the OS has a config directory.
    pub fn default_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "wheelup").map(|dirs| dirs.config_dir().join("settings.ron"))
    }

    /// Reads `path`; a missing file gives the defaults.
    pub fn load(path: &Path) -> Result<Settings, SettingsError> {
        match fs::read_to_string(path) {
            Ok(text) => ron::from_str(&text).map_err(|e| SettingsError::Parse {
                path: path.to_owned(),
                message: e.to_string(),
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(source) => Err(SettingsError::Io {
                path: path.to_owned(),
                source,
            }),
        }
    }

    /// Writes to a temporary file, then renames it over `path`, so a crash
    /// mid-write never leaves half a file behind.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        let io_error = |source| SettingsError::Io {
            path: path.to_owned(),
            source,
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(io_error)?;
        }
        let text =
            ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(|e| SettingsError::Parse {
                path: path.to_owned(),
                message: e.to_string(),
            })?;
        let temporary = path.with_extension("ron.tmp");
        fs::write(&temporary, text).map_err(io_error)?;
        fs::rename(&temporary, path).map_err(io_error)
    }

    pub fn calibration_for(&self, output_device: &str) -> Calibration {
        self.calibration.get(output_device).copied().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wheelup-settings-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("nested").join("settings.ron")
    }

    #[test]
    fn a_missing_file_gives_defaults() {
        let settings = Settings::load(&scratch("missing")).expect("defaults");
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.calibration_for("anything"), Calibration::default());
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let path = scratch("round-trip");
        let mut settings = Settings {
            layout: "Drummer".to_owned(),
            ..Settings::default()
        };
        settings.calibration.insert(
            "USB Audio".to_owned(),
            Calibration {
                audio_ms: 21.5,
                video_ms: 38.0,
            },
        );
        settings.save(&path).expect("saved");
        let loaded = Settings::load(&path).expect("loaded");
        assert_eq!(loaded, settings);
        assert!((loaded.calibration_for("USB Audio").visual_lead_ms() - 16.5).abs() < 1e-9);
        assert!(!path.with_extension("ron.tmp").exists());
    }

    #[test]
    fn older_files_missing_fields_still_load() {
        let path = scratch("partial");
        fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        fs::write(&path, "(layout: \"Drummer\")").expect("written");
        let loaded = Settings::load(&path).expect("loaded");
        assert_eq!(loaded.layout, "Drummer");
        assert!(loaded.calibration.is_empty());
    }

    #[test]
    fn garbage_is_an_error_not_a_crash() {
        let path = scratch("garbage");
        fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        fs::write(&path, "{{{").expect("written");
        assert!(matches!(Settings::load(&path), Err(SettingsError::Parse { .. })));
    }
}
