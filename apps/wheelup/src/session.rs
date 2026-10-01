//! What the player picked on the song screen, and how the last run went.

use bevy::prelude::*;
use wu_chart::Difficulty;
use wu_game::run::Press;
use wu_game::score::{Score, ScoreRules};

#[derive(Resource, Clone, Debug)]
pub struct Session {
    /// Index into `wu_content::songs::BUILTIN`.
    pub song: usize,
    pub difficulty: Difficulty,
    /// Practice tempo, 50–150 %: a real tempo change, the songs are sequenced.
    pub tempo_percent: u32,
    pub autoplay: bool,
    pub no_fail: bool,
}

impl Default for Session {
    fn default() -> Session {
        Session {
            song: 0,
            difficulty: Difficulty::Easy,
            tempo_percent: 100,
            autoplay: false,
            no_fail: false,
        }
    }
}

/// The difficulties on offer until roll segments arrive (Junglist needs them).
pub const PLAYABLE: [Difficulty; 4] = [
    Difficulty::Beginner,
    Difficulty::Easy,
    Difficulty::Medium,
    Difficulty::Hard,
];

pub use wu_game::play::windows;

impl Session {
    /// The selecta bot never fails: it would only fail on a bug.
    pub fn no_fail(&self) -> bool {
        self.no_fail || self.autoplay
    }

    pub fn score_rules(&self) -> ScoreRules {
        wu_game::play::score_rules(self.difficulty, self.no_fail())
    }
}

/// The last finished run, for the results screen.
#[derive(Resource, Clone, Debug)]
pub struct LastRun {
    /// The song's id, for the replay.
    pub song: &'static str,
    pub title: String,
    pub difficulty: Difficulty,
    pub tempo_percent: u32,
    pub no_fail: bool,
    pub autoplay: bool,
    pub score: Score,
    pub failed: bool,
    pub presses: Vec<Press>,
    pub notes: usize,
}
