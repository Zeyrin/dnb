//! Replays: the presses of a run, saved so it can be watched, raced, or judged
//! again (see `run::rejudge`).

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::run::Press;

/// 2: presses name a lane (rails included) and can be releases.
/// 3: a press can be a WHEEL UP! rewind, which older readers would ignore.
pub const REPLAY_VERSION: u32 = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Replay {
    pub version: u32,
    /// The song's id (`wu_content::songs::BUILTIN`).
    pub song: String,
    pub difficulty: String,
    pub tempo_percent: u32,
    /// No-Fail was on: the vibe hitting zero didn't end the run.
    #[serde(default)]
    pub no_fail: bool,
    /// The selecta bot played, not a person.
    #[serde(default)]
    pub autoplay: bool,
    /// Sudden Death was on: the first miss ended the run.
    #[serde(default)]
    pub sudden_death: bool,
    pub presses: Vec<Press>,
}

impl Replay {
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(io::Error::other)?;
        fs::write(path, text)
    }

    /// Refuses a replay from a newer version of the game: it could hold
    /// something this one can't judge, and would be scored wrong.
    pub fn load(path: &Path) -> io::Result<Replay> {
        let text = fs::read_to_string(path)?;
        let replay: Replay = ron::from_str(&text).map_err(io::Error::other)?;
        if replay.version > REPLAY_VERSION {
            return Err(io::Error::other(format!(
                "replay version {} is newer than this game understands ({REPLAY_VERSION})",
                replay.version
            )));
        }
        Ok(replay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replays_survive_a_round_trip() {
        let replay = Replay {
            version: 1,
            song: "rooftop-transmission".into(),
            difficulty: "Hard".into(),
            tempo_percent: 90,
            no_fail: true,
            autoplay: false,
            sudden_death: false,
            presses: vec![
                Press {
                    lane: 0,
                    ms: 1234.5,
                    up: false,
                    rewind_ms: None,
                },
                Press {
                    lane: 9,
                    ms: 1412.25,
                    up: true,
                    rewind_ms: None,
                },
                Press {
                    lane: 0,
                    ms: 2000.0,
                    up: false,
                    rewind_ms: Some(1500.0),
                },
            ],
        };
        let path = std::env::temp_dir().join(format!("wheelup-replay-{}.ron", std::process::id()));
        replay.save(&path).expect("saved");
        assert_eq!(Replay::load(&path).expect("loaded"), replay);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn version_one_replays_still_load() {
        let text = r#"(version: 1, song: "rooftop-transmission", difficulty: "Hard", tempo_percent: 100,
            presses: [(pad: 6, ms: 1000.0)])"#;
        let replay: Replay = ron::from_str(text).expect("parses");
        assert_eq!(
            replay.presses,
            vec![Press {
                lane: 6,
                ms: 1000.0,
                up: false,
                rewind_ms: None
            }]
        );
        assert!(!replay.no_fail && !replay.autoplay);
    }

    #[test]
    fn a_replay_from_a_newer_game_is_refused() {
        let path = std::env::temp_dir().join(format!("wheelup-future-{}.ron", std::process::id()));
        let future = Replay {
            version: REPLAY_VERSION + 1,
            song: "rooftop-transmission".into(),
            difficulty: "Hard".into(),
            tempo_percent: 100,
            no_fail: false,
            autoplay: false,
            sudden_death: false,
            presses: Vec::new(),
        };
        future.save(&path).expect("saved");
        assert!(Replay::load(&path).is_err());
        let _ = fs::remove_file(path);
    }
}
