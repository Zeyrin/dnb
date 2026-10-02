//! The settings file, loaded once at startup and saved when something changes.

use std::path::PathBuf;

use bevy::prelude::*;
use wu_content::settings::{AudioMode, Calibration, Language, Settings};
use wu_input::Layout;

#[derive(Resource, Debug)]
pub struct SettingsStore {
    pub settings: Settings,
    path: Option<PathBuf>,
    /// The system's language, when the settings leave it to the system.
    system_language: Language,
}

/// French if the system speaks it, English otherwise.
fn system_language() -> Language {
    match sys_locale::get_locale() {
        Some(locale) if locale.to_lowercase().starts_with("fr") => Language::French,
        _ => Language::English,
    }
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
        SettingsStore {
            settings,
            path,
            system_language: system_language(),
        }
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

    pub fn note_speed(&self) -> f32 {
        self.settings.note_speed
    }

    pub fn set_note_speed(&mut self, speed: f32) {
        self.settings.note_speed = speed;
        self.save();
    }

    /// The language chosen, or the system's.
    pub fn language(&self) -> Language {
        self.settings.language.unwrap_or(self.system_language)
    }

    pub fn set_language(&mut self, language: Language) {
        self.settings.language = Some(language);
        self.save();
    }

    /// How bright a WHEEL UP!'s flare is, 0–1.
    pub fn flare(&self) -> f32 {
        self.settings.flare.clamp(0.0, 1.0)
    }

    pub fn set_flare(&mut self, flare: f32) {
        self.settings.flare = flare;
        self.save();
    }

    pub fn reduced_motion(&self) -> bool {
        self.settings.reduced_motion
    }

    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.settings.reduced_motion = reduced;
        self.save();
    }

    pub fn bass_on_triggers(&self) -> bool {
        self.settings.bass_on_triggers
    }

    pub fn set_bass_on_triggers(&mut self, on: bool) {
        self.settings.bass_on_triggers = on;
        self.save();
    }
}
