//! Records: the best run on each song at each difficulty, the one to beat.
//!
//! Only a real run sets one: the song at its own tempo, played by a person,
//! with the plug left in, and No-Fail off.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use wu_chart::Difficulty;

use crate::score::Score;

/// 2: the dubplates pressed.
pub const RECORDS_VERSION: u32 = 2;

/// The best run on one song at one difficulty.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Best {
    pub points: u64,
    /// 0–1.
    pub accuracy: f64,
    /// Its grade, as shown ("S+", "S", …).
    pub grade: String,
    pub max_combo: u32,
    /// Every note hit, none missed.
    pub full_combo: bool,
    /// When, in seconds since the Unix epoch.
    pub at: u64,
}

impl Best {
    pub fn of(score: &Score, at: u64) -> Best {
        Best {
            points: score.points,
            accuracy: score.accuracy(),
            grade: score.grade().label().to_owned(),
            max_combo: score.max_combo,
            full_combo: score.full_combo(),
            at,
        }
    }
}

/// What a run did to the records.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// It doesn't count: practice tempo, the selecta bot, No-Fail, or the plug pulled.
    NotCounted,
    /// The first run there.
    First,
    /// It beat this one.
    Beaten(Best),
    /// This one still stands.
    Kept(Best),
}

/// How a run was played, for whether it counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conditions {
    pub tempo_percent: u32,
    pub autoplay: bool,
    pub no_fail: bool,
    pub failed: bool,
}

impl Conditions {
    pub fn count(self) -> bool {
        self.tempo_percent == 100 && !self.autoplay && !self.no_fail && !self.failed
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Records {
    pub version: u32,
    /// By song id, then difficulty.
    pub best: BTreeMap<String, BTreeMap<Difficulty, Best>>,
    /// What the player's dubplates pressed: see [`crate::dubplates`].
    #[serde(default)]
    pub pressed: BTreeSet<String>,
}

impl Default for Records {
    fn default() -> Records {
        Records {
            version: RECORDS_VERSION,
            best: BTreeMap::new(),
            pressed: BTreeSet::new(),
        }
    }
}

impl Records {
    /// `<data dir>/wheelup/records.ron`, when the OS has a data directory.
    pub fn default_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "wheelup").map(|dirs| dirs.data_dir().join("records.ron"))
    }

    pub fn best(&self, song: &str, difficulty: Difficulty) -> Option<&Best> {
        self.best.get(song).and_then(|by| by.get(&difficulty))
    }

    /// Enters a finished run; a higher score than the record replaces it.
    pub fn submit(
        &mut self,
        song: &str,
        difficulty: Difficulty,
        score: &Score,
        conditions: Conditions,
        at: u64,
    ) -> Outcome {
        if !conditions.count() {
            return Outcome::NotCounted;
        }
        let run = Best::of(score, at);
        let slot = self.best.entry(song.to_owned()).or_default();
        match slot.get(&difficulty) {
            None => {
                slot.insert(difficulty, run);
                Outcome::First
            }
            Some(best) if run.points > best.points => {
                let beaten = best.clone();
                slot.insert(difficulty, run);
                Outcome::Beaten(beaten)
            }
            Some(best) => Outcome::Kept(best.clone()),
        }
    }

    /// Reads `path`; a missing file is no records yet. Refuses records from a
    /// newer game, which might hold what this one would throw away on saving.
    pub fn load(path: &Path) -> io::Result<Records> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Records::default()),
            Err(error) => return Err(error),
        };
        let records: Records = ron::from_str(&text).map_err(io::Error::other)?;
        if records.version > RECORDS_VERSION {
            return Err(io::Error::other(format!(
                "records version {} is newer than this game understands ({RECORDS_VERSION})",
                records.version
            )));
        }
        Ok(records)
    }

    /// Writes to a temporary file, then renames it over `path`, so a crash
    /// mid-write never loses the records.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(io::Error::other)?;
        let temporary = path.with_extension("ron.tmp");
        fs::write(&temporary, text)?;
        fs::rename(&temporary, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::ScoreRules;

    fn score(points: u64) -> Score {
        let mut score = Score::new(ScoreRules {
            overhit_penalty: 0.0,
            no_fail: false,
        });
        score.points = points;
        score
    }

    const REAL: Conditions = Conditions {
        tempo_percent: 100,
        autoplay: false,
        no_fail: false,
        failed: false,
    };

    #[test]
    fn a_higher_score_takes_the_record_and_a_lower_one_does_not() {
        let mut records = Records::default();
        assert_eq!(
            records.submit("rooftop", Difficulty::Hard, &score(1000), REAL, 1),
            Outcome::First
        );
        let Outcome::Kept(kept) = records.submit("rooftop", Difficulty::Hard, &score(900), REAL, 2) else {
            panic!("a lower score must not take the record")
        };
        assert_eq!(kept.points, 1000);
        let Outcome::Beaten(beaten) = records.submit("rooftop", Difficulty::Hard, &score(1200), REAL, 3) else {
            panic!("a higher score takes the record")
        };
        assert_eq!(beaten.points, 1000);
        assert_eq!(records.best("rooftop", Difficulty::Hard).map(|b| b.points), Some(1200));
        // Each difficulty keeps its own.
        assert_eq!(records.best("rooftop", Difficulty::Easy), None);
    }

    #[test]
    fn practice_the_selecta_bot_no_fail_and_a_pulled_plug_set_no_record() {
        let mut records = Records::default();
        for conditions in [
            Conditions {
                tempo_percent: 80,
                ..REAL
            },
            Conditions { autoplay: true, ..REAL },
            Conditions { no_fail: true, ..REAL },
            Conditions { failed: true, ..REAL },
        ] {
            assert_eq!(
                records.submit("rooftop", Difficulty::Hard, &score(5000), conditions, 1),
                Outcome::NotCounted
            );
        }
        assert!(records.best.is_empty());
    }

    #[test]
    fn records_survive_the_disk_and_a_newer_file_is_refused() {
        let dir = std::env::temp_dir().join(format!("wu-records-{}", std::process::id()));
        let path = dir.join("records.ron");
        let mut records = Records::default();
        records.submit("underpass", Difficulty::Junglist, &score(777), REAL, 42);
        records.save(&path).expect("saves");
        assert_eq!(Records::load(&path).expect("loads"), records);
        assert_eq!(
            Records::load(&dir.join("missing.ron")).expect("none yet"),
            Records::default()
        );
        let newer = Records {
            version: RECORDS_VERSION + 1,
            ..records
        };
        newer.save(&path).expect("saves");
        assert!(Records::load(&path).is_err());
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }
}
