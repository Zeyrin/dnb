//! The settings file, loaded once at startup and saved when something changes.

use std::path::PathBuf;

use bevy::prelude::*;
use wu_content::settings::{AudioMode, Calibration, Settings};
use wu_input::Layout;

#[derive(Resource, Debug)]
pub struct SettingsStore {
    pub settings: Settings,
    path: Option<PathBuf>,
}

impl SettingsStore {
    pub fn load() -> SettingsStore {
        let path = Settings::default_path();
        let settings = match path.as_deref().map(Settings::load) {
            Some(Ok(settings)) => settings,
            Some(Err(error)) => {
                warn!("settings unreadable, using defaults: {error}");
                Settings::default()
            }
            None => Settings::default(),
        };
        SettingsStore { settings, path }
    }

    pub fn save(&self) {
        let Some(path) = &self.path else {
            warn!("no config directory: settings not saved");
            return;
        };
        if let Err(error) = self.settings.save(path) {
            warn!("settings not saved: {error}");
        }
    }

    pub fn layout(&self) -> Layout {
        Layout::ALL
            .into_iter()
            .find(|l| l.name() == self.settings.layout)
            .unwrap_or_default()
    }

    pub fn calibration(&self, output_device: &str) -> Calibration {
        self.settings.calibration_for(output_device)
    }

    pub fn set_calibration(&mut self, output_device: &str, calibration: Calibration) {
        self.settings.calibration.insert(output_device.to_owned(), calibration);
        self.save();
    }

    pub fn audio_mode(&self) -> AudioMode {
        self.settings.audio_mode
    }

    pub fn set_audio_mode(&mut self, mode: AudioMode) {
        self.settings.audio_mode = mode;
        self.save();
    }
}
