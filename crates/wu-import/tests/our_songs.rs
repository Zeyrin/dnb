//! The listener against songs whose every hit is known: our own, rendered.

use wu_audio::render_offline;
use wu_content::songs::BUILTIN;
use wu_import::bass::hear_bass;
use wu_import::find_grid;
use wu_import::hits::{Drum, hear_drums};
use wu_import::spectrum::analyse;
use wu_import::structure::find_sections;
use wu_instruments::Pad;
use wu_time::{TICKS_PER_STEP, Tick};

const SR: u32 = 48_000;

/// A built-in song as a listener would get it: the whole mix, mono. Without
/// its break, when asked: the break's hits are in no list to check against.
fn rendered(index: usize, without_break: bool) -> (wu_content::project::Song, Vec<f32>) {
    rendered_as(index, |song| {
        if without_break {
            song.tracks
                .retain(|(_, track, _)| !track.instrument.starts_with("break/"));
        }
    })
}

/// A built-in song, changed by `change` (another kit, say), as a listener would get it.
fn rendered_as(
    index: usize,
    change: impl FnOnce(&mut wu_content::project::Song),
) -> (wu_content::project::Song, Vec<f32>) {
    let mut song = BUILTIN[index].load().expect("compiles");
    change(&mut song);
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
    // A fill's snares and toms are now and then taken for a hat.
    assert!(hats.0 > 0.87 && hats.1 > 0.9, "hats {hats:?}");
}

/// How well a bass line was heard: on the steps where the bass sounds, the
/// share heard on the right key; of the steps heard as bass, the share right;
/// of the notes played, the share heard starting on their step; of the notes
/// heard, the share starting where one does.
#[derive(Clone, Copy, Debug)]
struct Heard {
    recall: f64,
    precision: f64,
    starts_found: f64,
    starts_right: f64,
}

fn bass_heard(song: &wu_content::project::Song, mono: &[f32]) -> Heard {
    let spec = analyse(mono, SR);
    let grid = find_grid(mono, &spec).expect("a steady beat");
    let steps = song.length.0 / TICKS_PER_STEP;
    let kicks: Vec<i64> = hear_drums(&spec, &grid, steps)
        .iter()
        .filter(|h| h.drum == Drum::Kick)
        .map(|h| h.step)
        .collect();
    let heard = hear_bass(mono, SR, &grid, steps, &kicks);
    // The key sounding on each step, if any.
    let keyed = |notes: &[wu_audio::Note]| {
        let mut keys = vec![None; steps as usize];
        for n in notes {
            let (start, length) = (n.tick.0 / TICKS_PER_STEP, n.length.0 / TICKS_PER_STEP);
            for s in start..(start + length).min(steps) {
                keys[s as usize] = Some(n.key);
            }
        }
        keys
    };
    let (truth, found) = (keyed(&song.bass), keyed(&heard));
    let sounding = truth.iter().filter(|k| k.is_some()).count() as f64;
    let right = truth.iter().zip(&found).filter(|(t, f)| t.is_some() && t == f).count() as f64;
    let claimed = found.iter().filter(|k| k.is_some()).count() as f64;
    let starts = |notes: &[wu_audio::Note]| notes.iter().map(|n| n.tick).collect::<Vec<_>>();
    let (played, picked) = (starts(&song.bass), starts(&heard));
    let shared = picked.iter().filter(|t| played.contains(t)).count() as f64;
    Heard {
        recall: right / sounding,
        precision: right / claimed.max(1.0),
        starts_found: shared / played.len().max(1) as f64,
        starts_right: shared / picked.len().max(1) as f64,
    }
}

#[test]
fn rooftop_transmissions_bass_line_is_heard_note_for_note() {
    let (song, mono) = rendered(0, false);
    let heard = bass_heard(&song, &mono);
    eprintln!("bass: {heard:?}");
    assert!(heard.recall > 0.95 && heard.precision > 0.95, "{heard:?}");
    assert!(heard.starts_found > 0.9 && heard.starts_right > 0.9, "{heard:?}");
}

/// The bars the song's drops were heard in, against the bars it really
/// plays full (drums and bass) and the bars it marks as hype: the share of
/// hype bars heard as drops, and the share of drop bars that really are full.
fn drops_heard(song: &wu_content::project::Song, mono: &[f32]) -> (f64, f64, Vec<wu_import::structure::Section>) {
    let spec = analyse(mono, SR);
    let grid = find_grid(mono, &spec).expect("a steady beat");
    let steps = song.length.0 / TICKS_PER_STEP;
    let hits = hear_drums(&spec, &grid, steps);
    let kicks: Vec<i64> = hits.iter().filter(|h| h.drum == Drum::Kick).map(|h| h.step).collect();
    let bass = hear_bass(mono, SR, &grid, steps, &kicks);
    let bars = steps / 16;
    let sections = find_sections(&spec, &grid, bars, &hits, &bass);
    let bar_of = |tick: Tick| tick.0 / wu_time::TICKS_PER_BAR;
    let hype: Vec<i64> = song
        .hype
        .iter()
        .flat_map(|&(start, end)| bar_of(start)..bar_of(end))
        .collect();
    // Full: a snare on two or four, and the bass line playing.
    let full: Vec<i64> = (0..bars)
        .filter(|&bar| {
            let (from, to) = (Tick::from_bars(bar), Tick::from_bars(bar + 1));
            let backbeat = [4, 12].map(|step| Tick::from_bars(bar) + Tick::from_steps(step));
            song.drums
                .iter()
                .any(|h| matches!(h.pad, Pad::P2 | Pad::P5) && backbeat.contains(&h.tick))
                && song.bass.iter().any(|n| n.tick < to && from < n.tick + n.length)
        })
        .collect();
    let dropped: Vec<i64> = sections
        .iter()
        .filter(|s| s.drop)
        .flat_map(|s| s.bars.clone())
        .collect();
    let found = hype.iter().filter(|b| dropped.contains(b)).count() as f64 / hype.len().max(1) as f64;
    let right = dropped.iter().filter(|b| full.contains(b)).count() as f64 / dropped.len().max(1) as f64;
    (found, right, sections)
}

#[test]
fn rooftop_transmissions_drops_are_heard_where_they_are() {
    let (song, mono) = rendered(0, false);
    let (found, right, sections) = drops_heard(&song, &mono);
    let shape: Vec<String> = sections.iter().map(|s| format!("{} {:?}", s.name, s.bars)).collect();
    eprintln!("sections: {}", shape.join(", "));
    assert!(
        found > 0.95,
        "only {found:.2} of the hype bars heard as drops: {shape:?}"
    );
    assert!(right > 0.95, "only {right:.2} of the drops really full: {shape:?}");
    // Every section starts on one of the song's own section lines.
    let lines: Vec<i64> = song
        .sections
        .iter()
        .map(|(_, start, _)| start.0 / wu_time::TICKS_PER_BAR)
        .collect();
    for section in &sections {
        assert!(
            lines.contains(&section.bars.start),
            "{} starts on bar {}: {shape:?}",
            section.name,
            section.bars.start
        );
    }
}

/// The same song on every kit, each with its own kick and snare to find the
/// grid and the drops by, and a long, low kick (Halftime Heavy's, Darkside's)
/// right on the sub. Slow: run with `cargo test --release -- --ignored`.
#[test]
#[ignore = "renders the song eight times"]
fn every_kit_is_heard_on_its_grid_with_its_bass_line_and_drops() {
    let mut all = Vec::new();
    for kit in wu_instruments::kits::KITS {
        let (song, mono) = rendered_as(0, |song| song.kit = kit.id.to_string());
        let grid = find_grid(&mono, &analyse(&mono, SR)).expect("a steady beat");
        let bpm = song.tempo.bpm_at(Tick::ZERO);
        assert!(
            (grid.bpm - bpm).abs() < 0.01,
            "{}: {bpm} BPM heard as {}",
            kit.id,
            grid.bpm
        );
        assert!(
            grid.first_bar_s.abs() < 0.003,
            "{}: first bar line heard at {} s",
            kit.id,
            grid.first_bar_s
        );
        let heard = bass_heard(&song, &mono);
        eprintln!("{:>16}: {heard:?}", kit.id);
        all.push(heard);
        let (found, right, sections) = drops_heard(&song, &mono);
        let shape: Vec<String> = sections.iter().map(|s| format!("{} {:?}", s.name, s.bars)).collect();
        eprintln!("{:>16}  drops {found:.2}/{right:.2}: {}", "", shape.join(", "));
        assert!(found > 0.9 && right > 0.9, "{}: drops {found:.2} / {right:.2}", kit.id);
    }
    let mean = |f: fn(&Heard) -> f64| all.iter().map(f).sum::<f64>() / all.len() as f64;
    let worst = |f: fn(&Heard) -> f64| all.iter().map(f).fold(1.0, f64::min);
    let (recall, precision) = (mean(|h| h.recall), mean(|h| h.precision));
    let (starts_found, starts_right) = (mean(|h| h.starts_found), mean(|h| h.starts_right));
    eprintln!("mean: {recall:.3}/{precision:.3}, starts {starts_found:.3}/{starts_right:.3}");
    assert!(recall > 0.95 && precision > 0.95, "mean {recall:.3} / {precision:.3}");
    assert!(
        starts_found > 0.94 && starts_right > 0.94,
        "starts {starts_found:.3} / {starts_right:.3}"
    );
    // Halftime Heavy's kick rings on the sub's own note, and the fills' toms sit in its range.
    assert!(worst(|h| h.recall) > 0.88 && worst(|h| h.precision) > 0.88);
}
