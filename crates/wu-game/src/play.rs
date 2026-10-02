//! Setting up a run of a song: its chart, timing windows, and notes in song
//! time at the practice tempo. The game and `wheelup-cli replay` share this, so
//! a replay is judged against exactly the notes it was played on.

use wu_chart::{Chart, Difficulty, auto_chart};
use wu_content::project::Song;
use wu_time::{TempoMap, Tick};

use crate::judge::{HoldSpan, Lane, TimedNote, Windows};
use crate::replay::Replay;
use crate::run::{Press, Run, rejudge};
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
/// doesn't change which notes there are). A lesson is charted the same at every
/// difficulty: each section's own pads and, where it says so, its bass line,
/// all of them, with rolls where they run fast; the rest plays itself.
pub fn chart(song: &Song, difficulty: Difficulty) -> Chart {
    if !song.is_lesson() {
        return auto_chart(&song.drums, &song.bass, &song.tempo, difficulty);
    }
    let lesson_at = |tick: Tick| song.lessons.iter().find(|l| l.start <= tick && tick < l.end);
    let hits: Vec<_> = song
        .drums
        .iter()
        .copied()
        .filter(|h| lesson_at(h.tick).is_some_and(|l| l.pads.contains(&h.pad)))
        .collect();
    let bass: Vec<_> = song
        .bass
        .iter()
        .copied()
        .filter(|n| lesson_at(n.tick).is_some_and(|l| l.rails))
        .collect();
    auto_chart(&hits, &bass, &song.tempo, Difficulty::Junglist)
}

/// The chart's notes in song milliseconds at `tempo`.
pub fn timed_notes(chart: &Chart, tempo: &TempoMap) -> Vec<TimedNote> {
    let ms_at = |tick: Tick| tempo.seconds_at(tick.0 as f64) * 1000.0;
    let taps = chart.notes.iter().map(|n| TimedNote::tap(ms_at(n.tick), n.pad));
    let holds = chart.holds.iter().map(|h| TimedNote {
        ms: ms_at(h.start),
        lane: Lane::Rail(h.rail),
        hold: Some(HoldSpan {
            end_ms: ms_at(h.end),
            beats: (h.end - h.start).as_beats(),
        }),
    });
    taps.chain(holds).collect()
}

/// What the selecta bot plays: every note dead on time, every hold to its end,
/// in the order the judge would see them.
pub fn perfect_presses(notes: &[TimedNote]) -> Vec<Press> {
    let mut presses: Vec<Press> = notes
        .iter()
        .flat_map(|n| {
            let lane = n.lane.index() as u8;
            let down = Press {
                lane,
                ms: n.ms,
                up: false,
                rewind_ms: None,
            };
            let up = n.hold.map(|span| Press {
                lane,
                ms: span.end_ms,
                up: true,
                rewind_ms: None,
            });
            std::iter::once(down).chain(up)
        })
        .collect();
    presses.sort_by(|a, b| a.ms.total_cmp(&b.ms));
    presses
}

/// A fresh run of `chart` at `tempo`: its notes, timing windows, scoring rules
/// and hype phrases. The game and replays both start here, so they agree. A
/// lesson is judged loosely and can't be failed.
pub fn new_run(song: &Song, chart: &Chart, tempo: &TempoMap, no_fail: bool) -> Run {
    let ms_at = |tick: Tick| tempo.seconds_at(tick.0 as f64) * 1000.0;
    let (windows, rules) = if song.is_lesson() {
        (Windows::LOOSE, score_rules(Difficulty::Beginner, true))
    } else {
        (windows(chart.difficulty), score_rules(chart.difficulty, no_fail))
    };
    Run::new(timed_notes(chart, tempo), windows, rules)
        .with_hype(song.hype.iter().map(|&(start, end)| (ms_at(start), ms_at(end))))
}

/// Judges a saved replay again; `None` if it names a difficulty that doesn't exist.
pub fn replay_score(song: &Song, replay: &Replay) -> Option<Score> {
    let difficulty = Difficulty::ALL
        .into_iter()
        .find(|d| d.name().eq_ignore_ascii_case(&replay.difficulty))?;
    let run = new_run(
        song,
        &chart(song, difficulty),
        &practice_tempo(song, replay.tempo_percent),
        replay.no_fail,
    );
    Some(rejudge(run, &replay.presses))
}

#[cfg(test)]
mod tests {
    use wu_content::songs::BUILTIN;

    use super::*;
    use crate::judge::Judgement;
    use crate::replay::REPLAY_VERSION;

    #[test]
    fn a_perfect_replay_of_the_bundled_song_scores_all_wicked() {
        let song = BUILTIN[0].load().expect("compiles");
        let notes = timed_notes(&chart(&song, Difficulty::Hard), &practice_tempo(&song, 150));
        let holds = notes.iter().filter(|n| n.hold.is_some()).count();
        assert!(holds > 0, "Hard plays the bass on both rails");
        let mut replay = Replay {
            version: REPLAY_VERSION,
            song: BUILTIN[0].id.into(),
            difficulty: "Hard".into(),
            tempo_percent: 150,
            no_fail: false,
            autoplay: true,
            presses: perfect_presses(&notes),
        };
        let score = replay_score(&song, &replay).expect("known difficulty");
        assert_eq!(score.counts[Judgement::Wicked.index()] as usize, notes.len());
        assert_eq!(score.max_combo as usize, notes.len());
        assert_eq!((score.holds_completed as usize, score.holds_dropped), (holds, 0));
        assert!(!score.failed);

        replay.difficulty = "Impossible".into();
        assert_eq!(replay_score(&song, &replay), None);
    }

    #[test]
    fn a_lesson_charts_what_each_section_hands_over_at_every_difficulty() {
        let lesson = BUILTIN
            .iter()
            .find(|song| song.id == "first-steps")
            .expect("the lesson is built in")
            .load()
            .expect("compiles");
        let taught = |tick: Tick| {
            lesson
                .lessons
                .iter()
                .find(|l| l.start <= tick && tick < l.end)
                .expect("every note is in a lesson")
        };
        let lesson_chart = chart(&lesson, Difficulty::Beginner);
        for difficulty in Difficulty::ALL {
            assert_eq!(chart(&lesson, difficulty), lesson_chart, "{difficulty:?}");
        }
        // Each note is a pad its section hands over, and every hit handed over is a note.
        for note in &lesson_chart.notes {
            assert!(taught(note.tick).pads.contains(&note.pad), "{note:?}");
        }
        let handed_over = lesson
            .drums
            .iter()
            .filter(|h| {
                lesson
                    .lessons
                    .iter()
                    .any(|l| l.start <= h.tick && h.tick < l.end && l.pads.contains(&h.pad))
            })
            .count();
        assert_eq!(lesson_chart.notes.len(), handed_over);
        // The bass only where a section hands it over; the rolls lesson rolls.
        assert!(!lesson_chart.holds.is_empty() && !lesson_chart.rolls.is_empty());
        for hold in &lesson_chart.holds {
            assert!(taught(hold.start).rails, "{hold:?}");
        }
        // A lesson can't be failed: nothing pressed at all, and the run lives on.
        let score = rejudge(new_run(&lesson, &lesson_chart, &lesson.tempo, false), &[]);
        assert!(!score.failed);
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
