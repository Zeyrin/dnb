//! The listener against songs whose every hit is known: our own, rendered.

use wu_audio::render_offline;
use wu_content::songs::BUILTIN;
use wu_import::find_grid;
use wu_import::hits::{Drum, hear_drums};
use wu_import::spectrum::analyse;
use wu_instruments::Pad;
use wu_time::{TICKS_PER_STEP, Tick};

const SR: u32 = 48_000;

/// A built-in song as a listener would get it: the whole mix, mono. Without
/// its break, when asked: the break's hits are in no list to check against.
fn rendered(index: usize, without_break: bool) -> (wu_content::project::Song, Vec<f32>) {
    let mut song = BUILTIN[index].load().expect("compiles");
    if without_break {
        song.tracks
            .retain(|(_, track, _)| !track.instrument.starts_with("break/"));
    }
    let program = song.whole_program(SR, &song.tempo, 0);
    let frames = program.tempo.frame_at(song.length + Tick::from_bars(1), SR) as usize;
    let render = render_offline(program, frames, 512);
    let (frames, _) = render.audio.as_chunks::<2>();
    let mono = frames.iter().map(|[l, r]| 0.5 * (l + r)).collect();
    (song, mono)
}

#[test]
fn rooftop_transmission_is_heard_at_its_tempo_on_its_bar_lines() {
    let (song, mono) = rendered(0, false);
    let grid = find_grid(&mono, &analyse(&mono, SR)).expect("a steady beat");
    let bpm = song.tempo.bpm_at(Tick::ZERO);
    assert!((grid.bpm - bpm).abs() < 0.01, "{bpm} BPM heard as {}", grid.bpm);
    assert!(
        grid.first_bar_s.abs() < 0.003,
        "the first bar line is at 0 s, heard at {} s",
        grid.first_bar_s
    );
    assert!(grid.fit > 0.7, "fit {}", grid.fit);
}

/// Precision and recall of `heard` steps against `truth` steps.
fn score(heard: &[i64], truth: &[i64]) -> (f64, f64) {
    let hits = heard.iter().filter(|s| truth.contains(s)).count() as f64;
    (hits / heard.len().max(1) as f64, hits / truth.len().max(1) as f64)
}

#[test]
fn rooftop_transmissions_drums_are_heard_hit_for_hit() {
    let (song, mono) = rendered(0, true);
    let spec = analyse(&mono, SR);
    let grid = find_grid(&mono, &spec).expect("a steady beat");
    let steps = song.length.0 / TICKS_PER_STEP;
    let heard = hear_drums(&spec, &grid, steps);
    let step_of = |tick: Tick| tick.0 / TICKS_PER_STEP;
    let truth = |pads: &[Pad], loud: bool| -> Vec<i64> {
        let mut steps: Vec<i64> = song
            .drums
            .iter()
            .filter(|h| pads.contains(&h.pad) && (h.velocity >= 0.6) == loud)
            .map(|h| step_of(h.tick))
            .collect();
        steps.dedup();
        steps
    };
    let found = |drums: &[Drum]| -> Vec<i64> {
        heard
            .iter()
            .filter(|h| drums.contains(&h.drum))
            .map(|h| h.step)
            .collect()
    };
    let kicks = score(&found(&[Drum::Kick]), &truth(&[Pad::P1], true));
    // Rims crack like snares: a snare heard on a rim is no mistake.
    let mut snare_like = truth(&[Pad::P2, Pad::P5, Pad::P4], true);
    snare_like.extend(truth(&[Pad::P3, Pad::P5], false));
    let snares = score(&found(&[Drum::Snare]), &snare_like);
    let snare_recall = score(&found(&[Drum::Snare, Drum::Ghost]), &truth(&[Pad::P2, Pad::P5], true)).1;
    let hats = score(&found(&[Drum::Hat]), &truth(&[Pad::P7, Pad::P8], true));
    eprintln!(
        "kicks {:.2}/{:.2} · snares {:.2}/{snare_recall:.2} · hats {:.2}/{:.2} (precision/recall)",
        kicks.0, kicks.1, snares.0, hats.0, hats.1
    );
    assert!(kicks.0 > 0.9 && kicks.1 > 0.95, "kicks {kicks:?}");
    assert!(
        snares.0 > 0.95 && snare_recall > 0.85,
        "snares {:?} / {snare_recall}",
        snares
    );
    assert!(hats.0 > 0.9 && hats.1 > 0.9, "hats {hats:?}");
}
