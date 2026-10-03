//! Setting up a run of a song: its chart, timing windows, and notes in song
//! time at the practice tempo. The game and `wheelup-cli replay` share this, so
//! a replay is judged against exactly the notes it was played on.

use wu_audio::Hit;
use wu_chart::{Chart, ChartNote, Difficulty, auto_chart};
use wu_content::project::{Groove, Song};
use wu_instruments::Pad;
use wu_time::{TICKS_PER_STEP, TempoMap, Tick};

use crate::judge::{HoldSpan, Lane, TimedNote, Windows};
use crate::replay::Replay;
use crate::run::{Press, Run, rejudge};
use crate::score::{Score, ScoreRules};

/// How a run at `difficulty` is scored: from Hard up, pressing with no note in
/// reach costs vibe.
/// Bars from the first note that are a warm-up: misses there cost no vibe.
pub const WARM_UP_BARS: i64 = 8;

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
    if song.recording.is_some() && difficulty == Difficulty::Junglist {
        return auto_chart(&whole_kit(song), &song.bass, &song.tempo, difficulty);
    }
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

/// An imported tune's drums over the whole kit, every button in play as in the
/// game's own tunes. The kick and the snare stay where they always are; the
/// rest spreads out. The hats go hand to hand, as a drummer's do: with the
/// kick or the snare on ✕, on the offbeat on ○, and wherever the other hand is
/// free (a beat with neither, the sixteenths between) on →. A phrase's last
/// two beats (its fill) go round the tom. The recording plays
/// every sound: the pads only say which button.
fn whole_kit(song: &Song) -> Vec<Hit> {
    let phrase = Tick::from_bars(8).0;
    let fill = Tick::from_steps(8).0;
    let ticks_of = |pads: &[Pad]| -> std::collections::BTreeSet<Tick> {
        song.drums
            .iter()
            .filter(|h| pads.contains(&h.pad))
            .map(|h| h.tick)
            .collect()
    };
    let backbone = ticks_of(&[Pad::P1, Pad::P2]);
    // A ghost's neighbours stay on the hat's own button: the rim would crowd it.
    let ghosts = ticks_of(&[Pad::P3]);
    let near_ghost = |tick: Tick| [-1, 1].iter().any(|&d| ghosts.contains(&(tick + Tick::from_steps(d))));
    // Phrases run eight bars from each section's start, the last cut short by its end.
    let in_fill = |tick: Tick| {
        song.sections.iter().any(|&(_, start, end)| {
            start <= tick && tick < end && (phrase - (tick - start).0 % phrase <= fill || (end - tick).0 <= fill)
        })
    };
    song.drums
        .iter()
        .map(|&hit| {
            let step = hit.tick.0.div_euclid(TICKS_PER_STEP).rem_euclid(16);
            let pad = match hit.pad {
                Pad::P3 | Pad::P5 | Pad::P7 if in_fill(hit.tick) => Pad::P6,
                Pad::P7 if step % 4 == 2 => Pad::P8,
                Pad::P7 if (step % 2 == 1 || !backbone.contains(&hit.tick)) && !near_ghost(hit.tick) => Pad::P4,
                pad => pad,
            };
            Hit { pad, ..hit }
        })
        .collect()
}

/// The part of `chart` from `start` to `end`, for practice to loop. A hold or a
/// roll running past the end is cut there, as the loop cuts it.
pub fn chart_between(mut chart: Chart, start: Tick, end: Tick) -> Chart {
    let inside = |tick: Tick| start <= tick && tick < end;
    chart.notes.retain(|n| inside(n.tick));
    chart.rolls.retain(|r| inside(r.start));
    chart.holds.retain(|h| inside(h.start));
    for roll in &mut chart.rolls {
        roll.end = roll.end.min(end);
    }
    for hold in &mut chart.holds {
        hold.end = hold.end.min(end);
    }
    chart
}

/// The chart's notes in song milliseconds at `tempo`, each tap where its hit
/// really sounds if the song's recording has a `groove`.
pub fn timed_notes(chart: &Chart, tempo: &TempoMap, groove: Option<&Groove>) -> Vec<TimedNote> {
    let ms_at = |tick: Tick| tempo.seconds_at(tick.0 as f64) * 1000.0;
    let sounds_at = |n: &ChartNote| groove.map_or(n.tick, |groove| groove.place(n.pad, n.tick));
    let taps = chart.notes.iter().map(|n| TimedNote::tap(ms_at(sounds_at(n)), n.pad));
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
    // On Junglist an import's hats spread over the open hat and the rim: they
    // keep the hats' feel there.
    let groove = song.recording.as_ref().map(|recording| {
        let mut groove = recording.groove.clone();
        if chart.difficulty == Difficulty::Junglist {
            for pad in [Pad::P4, Pad::P8] {
                groove.ticks[pad.index()] = groove.ticks[Pad::P7.index()];
            }
        }
        groove
    });
    let first = [
        chart.notes.first().map(|n| n.tick),
        chart.rolls.iter().map(|r| r.start).min(),
        chart.holds.iter().map(|h| h.start).min(),
    ]
    .into_iter()
    .flatten()
    .min();
    let run = Run::new(timed_notes(chart, tempo, groove.as_ref()), windows, rules)
        .with_hype(song.hype.iter().map(|&(start, end)| (ms_at(start), ms_at(end))));
    match first {
        Some(first) => run.with_warm_up(ms_at(first + Tick::from_bars(WARM_UP_BARS))),
        None => run,
    }
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
    fn a_recordings_groove_puts_each_tap_where_its_hit_sounds() {
        use wu_content::project::Recording;
        use wu_instruments::Pad;
        use wu_time::TICKS_PER_STEP;

        let mut song = BUILTIN[0].load().expect("compiles");
        let chart = chart(&song, Difficulty::Junglist);
        let tempo = practice_tempo(&song, 100);
        let straight = timed_notes(&chart, &tempo, None);
        // The hats on the and of every bar's first beat a quarter of a step late.
        let mut groove = Groove::default();
        groove.ticks[Pad::P7.index()][2] = TICKS_PER_STEP / 4;
        song.recording = Some(Recording {
            path: "tune.wav".into(),
            first_bar_s: 0.0,
            gain_db: 0.0,
            groove,
        });
        let run = new_run(&song, &chart, &tempo, false);
        let ms_at = |tick: Tick| tempo.seconds_at(tick.0 as f64) * 1000.0;
        let late_ms = ms_at(Tick(TICKS_PER_STEP / 4));
        let mut moved = 0;
        let mut expected: Vec<(f64, Lane)> = chart
            .notes
            .iter()
            .map(|note| {
                let on_the_and = note.tick.0.div_euclid(TICKS_PER_STEP).rem_euclid(16) == 2;
                let late = note.pad == Pad::P7 && on_the_and;
                moved += usize::from(late);
                (ms_at(note.tick) + if late { late_ms } else { 0.0 }, Lane::Pad(note.pad))
            })
            .collect();
        let mut heard: Vec<(f64, Lane)> = run
            .judge()
            .notes()
            .iter()
            .filter(|n| n.hold.is_none())
            .map(|n| (n.ms, n.lane))
            .collect();
        expected.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        heard.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        assert_eq!(expected.len(), heard.len());
        for (want, got) in expected.iter().zip(&heard) {
            assert!((want.0 - got.0).abs() < 1e-6 && want.1 == got.1, "{want:?} {got:?}");
        }
        assert_eq!(straight.len(), run.judge().notes().len());
        assert!(moved > 0, "the chart has hats on the and of one");
    }

    #[test]
    fn a_perfect_replay_of_the_bundled_song_scores_all_wicked() {
        let song = BUILTIN[0].load().expect("compiles");
        let notes = timed_notes(&chart(&song, Difficulty::Hard), &practice_tempo(&song, 150), None);
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
    fn junglist_plays_every_button() {
        for builtin in BUILTIN.iter().filter(|song| song.id != "first-steps") {
            let song = builtin.load().expect("compiles");
            let chart = chart(&song, Difficulty::Junglist);
            for pad in Pad::ALL {
                assert!(
                    chart.notes.iter().any(|n| n.pad == pad),
                    "{} never plays {pad:?}",
                    builtin.id
                );
            }
        }
    }

    #[test]
    fn an_imported_tune_takes_the_whole_kit() {
        // What the listener hears: kicks, snares, ghosts and hats.
        let mut song = BUILTIN
            .iter()
            .find(|song| song.id == "dubplate-pressure")
            .expect("built in")
            .load()
            .expect("compiles");
        song.drums
            .retain(|h| matches!(h.pad, Pad::P1 | Pad::P2 | Pad::P3 | Pad::P5 | Pad::P7));
        let kit = whole_kit(&song);
        let pads: std::collections::BTreeSet<Pad> = kit.iter().map(|h| h.pad).collect();
        assert_eq!(pads.len(), Pad::ALL.len(), "{pads:?}");
        let step = |tick: Tick| tick.0.div_euclid(TICKS_PER_STEP).rem_euclid(16);
        assert!(
            kit.iter().all(|h| h.pad != Pad::P7 || step(h.tick) % 4 != 2),
            "offbeat hats open"
        );
        assert_eq!(kit.len(), song.drums.len(), "every hit kept");
        for (heard, played) in song.drums.iter().zip(&kit) {
            if matches!(heard.pad, Pad::P1 | Pad::P2) {
                assert_eq!(heard.pad, played.pad, "the kick and the snare stay put");
            }
        }
    }

    #[test]
    fn nobody_fails_in_the_warm_up() {
        let song = BUILTIN
            .iter()
            .find(|song| song.id == "dubplate-pressure")
            .expect("built in")
            .load()
            .expect("compiles");
        let chart = chart(&song, Difficulty::Junglist);
        let warm_up_ends = song
            .tempo
            .seconds_at((chart.notes[0].tick + Tick::from_bars(WARM_UP_BARS)).0 as f64)
            * 1000.0;
        let mut run = new_run(&song, &chart, &song.tempo, false);
        run.settle(warm_up_ends - 1.0);
        assert!(run.score().judged() > 8, "every note of the warm-up missed");
        assert!(!run.score().failed, "and the plug still in");
        run.finish();
        assert!(run.score().failed, "missing on after it pulls it");
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
        // The drums are the lesson: no bass to hold. The rolls lesson rolls.
        assert!(lesson_chart.holds.is_empty() && !lesson_chart.rolls.is_empty());
        // A lesson can't be failed: nothing pressed at all, and the run lives on.
        let score = rejudge(new_run(&lesson, &lesson_chart, &lesson.tempo, false), &[]);
        assert!(!score.failed);
    }

    #[test]
    fn practice_charts_only_its_section() {
        let song = BUILTIN[0].load().expect("compiles");
        let full = chart(&song, Difficulty::Junglist);
        let (_, start, end) = song.sections[1].clone();
        let part = chart_between(full.clone(), start, end);
        assert!(!part.notes.is_empty() && part.notes.len() < full.notes.len());
        assert!(part.notes.iter().all(|n| start <= n.tick && n.tick < end));
        assert!(part.holds.iter().all(|h| start <= h.start && h.end <= end));
        assert!(part.rolls.iter().all(|r| start <= r.start && r.end <= end));
        // What the section has, it keeps.
        let inside = full.notes.iter().filter(|n| start <= n.tick && n.tick < end).count();
        assert_eq!(part.notes.len(), inside);
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
        let normal = timed_notes(&chart, &practice_tempo(&song, 100), None);
        let slow = timed_notes(&chart, &practice_tempo(&song, 50), None);
        let last = normal.len() - 1;
        assert!((slow[last].ms - 2.0 * normal[last].ms).abs() < 1e-6);
    }
}
