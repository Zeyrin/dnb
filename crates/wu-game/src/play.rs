//! Setting up a run of a song: its chart, timing windows, and notes in song
//! time at the practice tempo. The game and `wheelup-cli replay` share this, so
//! a replay is judged against exactly the notes it was played on.

use wu_chart::{Chart, Difficulty, auto_chart};
use wu_content::project::Song;
use wu_time::TempoMap;

use crate::judge::{TimedNote, Windows};
use crate::replay::Replay;
use crate::run::rejudge;
use crate::score::{Score, ScoreRules};

/// How a run at `difficulty` is scored: from Hard up, pressing with no note in
/// reach costs vibe.
pub fn score_rules(difficulty: Difficulty, no_fail: bool) -> ScoreRules {
    let strict = matches!(difficulty, Difficulty::Hard | Difficulty::Junglist);
    ScoreRules {
        overhit_penalty: if strict { 0.02 } else { 0.0 },
        no_fail,
    }
}

pub fn windows(difficulty: Difficulty) -> Windows {
    match difficulty {
        Difficulty::Beginner | Difficulty::Easy => Windows::LOOSE,
        Difficulty::Medium => Windows::MEDIUM,
        Difficulty::Hard | Difficulty::Junglist => Windows::TIGHT,
    }
}

/// The song's tempo map at `percent` of its speed (50–150).
pub fn practice_tempo(song: &Song, percent: u32) -> TempoMap {
    let factor = f64::from(percent.clamp(50, 150)) / 100.0;
    song.tempo.scaled(factor).unwrap_or_else(|_| song.tempo.clone())
}

/// The chart for `difficulty`, cut at the song's own tempo (practice speed
/// doesn't change which notes there are).
pub fn chart(song: &Song, difficulty: Difficulty) -> Chart {
    auto_chart(&song.drums, &song.tempo, difficulty)
}

/// The chart's notes in song milliseconds at `tempo`.
pub fn timed_notes(chart: &Chart, tempo: &TempoMap) -> Vec<TimedNote> {
    chart
        .notes
        .iter()
        .map(|n| TimedNote {
            ms: tempo.seconds_at(n.tick.0 as f64) * 1000.0,
            pad: n.pad,
        })
        .collect()
}

/// Judges a saved replay again; `None` if it names a difficulty that doesn't exist.
pub fn replay_score(song: &Song, replay: &Replay) -> Option<Score> {
    let difficulty = Difficulty::ALL
        .into_iter()
        .find(|d| d.name().eq_ignore_ascii_case(&replay.difficulty))?;
    let notes = timed_notes(&chart(song, difficulty), &practice_tempo(song, replay.tempo_percent));
    let rules = score_rules(difficulty, replay.no_fail);
    Some(rejudge(notes, windows(difficulty), rules, &replay.presses))
}

#[cfg(test)]
mod tests {
    use wu_content::songs::BUILTIN;

    use super::*;
    use crate::judge::Judgement;
    use crate::run::Press;

    #[test]
    fn a_perfect_replay_of_the_bundled_song_scores_all_wicked() {
        let song = BUILTIN[0].load().expect("compiles");
        let notes = timed_notes(&chart(&song, Difficulty::Hard), &practice_tempo(&song, 150));
        let mut replay = Replay {
            version: 1,
            song: BUILTIN[0].id.into(),
            difficulty: "Hard".into(),
            tempo_percent: 150,
            no_fail: false,
            autoplay: true,
            presses: notes
                .iter()
                .map(|n| Press {
                    pad: n.pad.index() as u8,
                    ms: n.ms,
                })
                .collect(),
        };
        let score = replay_score(&song, &replay).expect("known difficulty");
        assert_eq!(score.counts[Judgement::Wicked.index()] as usize, notes.len());
        assert_eq!(score.max_combo as usize, notes.len());
        assert!(!score.failed);

        replay.difficulty = "Impossible".into();
        assert_eq!(replay_score(&song, &replay), None);
    }

    #[test]
    fn a_replay_fails_or_not_as_its_run_did() {
        let song = BUILTIN[0].load().expect("compiles");
        // Nothing pressed: the vibe drains to zero within a few misses.
        let mut replay = Replay {
            version: 1,
            song: BUILTIN[0].id.into(),
            difficulty: "Easy".into(),
            tempo_percent: 100,
            no_fail: false,
            autoplay: false,
            presses: Vec::new(),
        };
        assert!(replay_score(&song, &replay).expect("known difficulty").failed);
        replay.no_fail = true;
        assert!(!replay_score(&song, &replay).expect("known difficulty").failed);
    }

    #[test]
    fn practice_tempo_scales_note_times() {
        let song = BUILTIN[0].load().expect("compiles");
        let chart = chart(&song, Difficulty::Easy);
        let normal = timed_notes(&chart, &practice_tempo(&song, 100));
        let slow = timed_notes(&chart, &practice_tempo(&song, 50));
        let last = normal.len() - 1;
        assert!((slow[last].ms - 2.0 * normal[last].ms).abs() < 1e-6);
    }
}
