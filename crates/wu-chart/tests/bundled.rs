//! Every chart of every song that ships is playable, and the difficulties climb.

use wu_chart::{Difficulty, auto_chart, validate};
use wu_content::songs::BUILTIN;

#[test]
fn every_bundled_chart_is_playable_and_harder_charts_have_more_notes() {
    for song in BUILTIN {
        let compiled = song.load().unwrap_or_else(|e| panic!("{}: {e}", song.id));
        let mut previous = 0;
        for difficulty in Difficulty::ALL {
            let chart = auto_chart(&compiled.drums, &compiled.bass, &compiled.tempo, difficulty);
            let problems = validate(&chart, &compiled.tempo);
            assert!(problems.is_empty(), "{} {difficulty:?}: {problems:?}", song.id);
            assert!(chart.notes.len() > previous, "{} {difficulty:?} adds notes", song.id);
            previous = chart.notes.len();
        }
    }
}

#[test]
fn backing_plus_chart_is_the_whole_song() {
    for song in BUILTIN {
        let compiled = song.load().unwrap_or_else(|e| panic!("{}: {e}", song.id));
        let whole = compiled
            .program(48_000, &compiled.tempo, 0, |_, _| false, |_, _| false)
            .events()
            .len();
        for difficulty in Difficulty::ALL {
            let chart = auto_chart(&compiled.drums, &compiled.bass, &compiled.tempo, difficulty);
            let backing = compiled.program(
                48_000,
                &compiled.tempo,
                0,
                |tick, pad| chart.contains(tick, pad),
                |tick, key| chart.holds_note(tick, key),
            );
            assert_eq!(
                backing.events().len() + chart.notes.len() + chart.holds.len(),
                whole,
                "{} {difficulty:?}",
                song.id
            );
        }
    }
}
