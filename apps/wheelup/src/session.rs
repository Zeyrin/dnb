//! What the player picked on the song screen, and how the last run went.

use bevy::prelude::*;
use wu_chart::Difficulty;
use wu_game::run::Press;
use wu_game::score::Score;

#[derive(Resource, Clone, Debug)]
pub struct Session {
    /// Index into the `SongLibrary`: the built-in songs, then the imported ones.
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

/// The difficulties on the song screen.
pub const PLAYABLE: [Difficulty; 5] = Difficulty::ALL;

impl Session {
    /// The selecta bot never fails: it would only fail on a bug.
    pub fn no_fail(&self) -> bool {
        self.no_fail || self.autoplay
    }
}

/// The last finished run, for the results screen.
#[derive(Resource, Clone, Debug)]
pub struct LastRun {
    /// The song's id, for the replay.
    pub song: String,
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
