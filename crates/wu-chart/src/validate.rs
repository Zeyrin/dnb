//! The playability validator: everything the auto-charter promises, checked on
//! any chart, generated or hand-edited.

use std::collections::BTreeMap;

use wu_instruments::Pad;
use wu_time::{TempoMap, Tick};

use crate::chart::{Chart, density_windows, notes_per_second, thumb_index};
use crate::rules::{opposite, thumb};

#[derive(Clone, Debug, PartialEq)]
pub enum Violation {
    Unsorted { index: usize },
    PadNotAllowed { tick: Tick, pad: Pad },
    OppositeChord { tick: Tick, a: Pad, b: Pad },
    ThumbOverloaded { tick: Tick, count: usize },
    ChordTooBig { tick: Tick, size: usize },
    TooFast { tick: Tick, pad: Pad, gap_ms: f64 },
    TooDense { from: Tick, notes_per_second: f64 },
}

pub fn validate(chart: &Chart, tempo: &TempoMap) -> Vec<Violation> {
    let rules = chart.difficulty.rules();
    let mut problems = Vec::new();
    for (index, pair) in chart.notes.windows(2).enumerate() {
        if pair[0] >= pair[1] {
            problems.push(Violation::Unsorted { index: index + 1 });
        }
    }
    let mut chords: BTreeMap<Tick, Vec<Pad>> = BTreeMap::new();
    for note in &chart.notes {
        if !rules.pads.contains(&note.pad) {
            problems.push(Violation::PadNotAllowed {
                tick: note.tick,
                pad: note.pad,
            });
        }
        chords.entry(note.tick).or_default().push(note.pad);
    }
    let ms_at = |tick: Tick| tempo.seconds_at(tick.0 as f64) * 1000.0;
    let mut last: [Option<(Tick, f64)>; 2] = [None, None];
    for (&tick, pads) in &chords {
        if pads.len() > rules.max_chord {
            problems.push(Violation::ChordTooBig { tick, size: pads.len() });
        }
        for (side, last_on_thumb) in last.iter_mut().enumerate() {
            let on_thumb: Vec<Pad> = pads
                .iter()
                .copied()
                .filter(|&p| thumb_index(thumb(p)) == side)
                .collect();
            if on_thumb.len() > 2 {
                problems.push(Violation::ThumbOverloaded {
                    tick,
                    count: on_thumb.len(),
                });
            }
            for (i, &a) in on_thumb.iter().enumerate() {
                for &b in &on_thumb[i + 1..] {
                    if opposite(a, b) {
                        problems.push(Violation::OppositeChord { tick, a, b });
                    }
                }
            }
            if let Some(&pad) = on_thumb.first() {
                let at = ms_at(tick);
                if let Some((_, previous)) = *last_on_thumb {
                    let gap_ms = at - previous;
                    if gap_ms + 1e-6 < rules.min_same_thumb_ms {
                        problems.push(Violation::TooFast { tick, pad, gap_ms });
                    }
                }
                *last_on_thumb = Some((tick, at));
            }
        }
    }
    for window in density_windows(&chart.notes) {
        let nps = notes_per_second(&chart.notes, tempo, window);
        if nps > rules.max_notes_per_second + 1e-9 {
            problems.push(Violation::TooDense {
                from: window.0,
                notes_per_second: nps,
            });
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use wu_audio::Hit;
    use wu_time::TICKS_PER_STEP;

    use super::*;
    use crate::chart::{ChartNote, auto_chart};
    use crate::rules::Difficulty;

    #[test]
    fn broken_charts_are_caught() {
        let tempo = TempoMap::constant(170.0);
        let note = |step: i64, pad: Pad| ChartNote {
            tick: Tick::from_steps(step),
            pad,
        };
        let chart = Chart {
            difficulty: Difficulty::Easy,
            notes: vec![note(0, Pad::P1), note(0, Pad::P2), note(1, Pad::P1), note(2, Pad::P5)],
        };
        let problems = validate(&chart, &tempo);
        assert!(problems.contains(&Violation::OppositeChord {
            tick: Tick::ZERO,
            a: Pad::P1,
            b: Pad::P2
        }));
        assert!(problems.contains(&Violation::PadNotAllowed {
            tick: Tick::from_steps(2),
            pad: Pad::P5
        }));
        assert!(problems.iter().any(|p| matches!(p, Violation::TooFast { .. })));
    }

    fn arbitrary_hits() -> impl Strategy<Value = Vec<Hit>> {
        prop::collection::vec((0i64..128, 0usize..8, 0.2f32..1.0, 0i64..3), 0..400).prop_map(|raw| {
            raw.into_iter()
                .map(|(step, pad, velocity, nudge)| Hit {
                    // Mostly on the 16th grid, sometimes a 32nd off it.
                    tick: Tick(step * TICKS_PER_STEP + nudge * TICKS_PER_STEP / 2),
                    pad: Pad::from_index(pad).unwrap_or(Pad::P1),
                    velocity,
                })
                .collect()
        })
    }

    proptest! {
        #[test]
        fn every_generated_chart_is_playable(hits in arbitrary_hits(), bpm in 150.0f64..180.0) {
            let tempo = TempoMap::constant(bpm);
            for difficulty in Difficulty::ALL {
                let chart = auto_chart(&hits, &tempo, difficulty);
                let problems = validate(&chart, &tempo);
                prop_assert!(problems.is_empty(), "{:?}: {:?}", difficulty, problems);
            }
        }
    }
}
