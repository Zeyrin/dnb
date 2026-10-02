//! The records file: loaded once at startup, saved after every run that sets one.

use std::path::PathBuf;

use bevy::prelude::*;
use wu_chart::Difficulty;
use wu_game::records::{Best, Conditions, Outcome, Records};

use crate::session::LastRun;

#[derive(Resource, Debug)]
pub struct RecordsStore {
    records: Records,
    path: Option<PathBuf>,
}

impl RecordsStore {
    pub fn load() -> RecordsStore {
        let path = Records::default_path();
        let records = match path.as_deref().map(Records::load) {
            Some(Ok(records)) => records,
            Some(Err(error)) => {
                warn!("records unreadable, starting afresh (the file is left as it is): {error}");
                // Never overwrite a file we couldn't read: it may be a newer game's.
                return RecordsStore {
                    records: Records::default(),
                    path: None,
                };
            }
            None => Records::default(),
        };
        RecordsStore { records, path }
    }

    pub fn best(&self, song: &str, difficulty: Difficulty) -> Option<&Best> {
        self.records.best(song, difficulty)
    }

    /// No record set yet, on anything: a new player, most likely.
    pub fn is_empty(&self) -> bool {
        self.records.best.is_empty()
    }

    /// Enters a finished run, and saves when it set a record.
    pub fn submit(&mut self, run: &LastRun) -> Outcome {
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let conditions = Conditions {
            tempo_percent: run.tempo_percent,
            autoplay: run.autoplay,
            no_fail: run.no_fail,
            failed: run.failed,
        };
        let outcome = self
            .records
            .submit(&run.song, run.difficulty, &run.score, conditions, at);
        if matches!(outcome, Outcome::First | Outcome::Beaten(_)) {
            match &self.path {
                Some(path) => {
                    if let Err(error) = self.records.save(path) {
                        warn!("record not saved: {error}");
                    }
                }
                None => warn!("record not saved: no data folder, or an unreadable records file"),
            }
        }
        outcome
    }
}

/// A record in a line: grade, points, accuracy, and FC for a full combo.
pub fn describe(best: &Best) -> String {
    format!(
        "{} · {} · {:.1} %{}",
        best.grade,
        best.points,
        best.accuracy * 100.0,
        if best.full_combo { " · FULL COMBO" } else { "" }
    )
}
