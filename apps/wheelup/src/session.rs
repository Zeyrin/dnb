//! What the player picked on the song screen, and how the last run went.

use bevy::prelude::*;
use wu_chart::Difficulty;
use wu_game::run::Press;
use wu_game::score::Score;

/// Quickplay's modifiers, one at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Modifier {
    #[default]
    Off,
    /// Notes vanish on their way down: the last of the highway is played by ear.
    Hidden,
    /// The first miss after the warm-up pulls the plug.
    SuddenDeath,
}

impl Modifier {
    pub const ALL: [Modifier; 3] = [Modifier::Off, Modifier::Hidden, Modifier::SuddenDeath];

    pub fn name(self) -> &'static str {
        match self {
            Modifier::Off => "off",
            Modifier::Hidden => "Hidden",
            Modifier::SuddenDeath => "Sudden Death",
        }
    }
}

#[derive(Resource, Clone, Debug)]
pub struct Session {
    /// Index into the `SongLibrary`: the built-in songs, then the imported ones.
    pub song: usize,
    pub difficulty: Difficulty,
    /// Practice tempo, 50–150 %: a real tempo change, the songs are sequenced.
    pub tempo_percent: u32,
    pub autoplay: bool,
    pub no_fail: bool,
    /// Practice: the section looped (an index into the song's sections), or
    /// `None` to play the song through.
    pub practice: Option<usize>,
    /// Practice's Wait mode: the song stands still on a note until it is hit.
    pub wait: bool,
    pub modifier: Modifier,
    /// A kit to play the tune on instead of its own, by id: heard in the
    /// preview at once, played once a dubplate has pressed it.
    pub kit: Option<&'static str>,
    /// A tour stop whose stage the tune plays in front of instead of its own.
    pub stage: Option<String>,
    /// Picked on the tour: the run goes back there.
    pub from_tour: bool,
}

impl Default for Session {
    fn default() -> Session {
        Session {
            song: 0,
            difficulty: Difficulty::Easy,
            tempo_percent: 100,
            autoplay: false,
            no_fail: false,
            practice: None,
            wait: false,
            modifier: Modifier::Off,
            kit: None,
            stage: None,
            from_tour: false,
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
    pub sudden_death: bool,
    /// A lesson: it sets no record.
    pub lesson: bool,
    pub score: Score,
    pub failed: bool,
    pub presses: Vec<Press>,
    pub notes: usize,
}
