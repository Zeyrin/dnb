//! Charts and the auto-charter.

use std::collections::BTreeMap;

use wu_audio::Hit;
use wu_instruments::Pad;
use wu_time::{PPQ, TICKS_PER_BAR, TICKS_PER_STEP, TempoMap, Tick};

use crate::rules::{Difficulty, Rules, Thumb, opposite, priority, thumb};

/// A note to play: a pad at a tick. (Holds, rolls and rails arrive with M4.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChartNote {
    pub tick: Tick,
    pub pad: Pad,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chart {
    pub difficulty: Difficulty,
    /// Sorted by tick, then pad; no duplicates.
    pub notes: Vec<ChartNote>,
}

impl Chart {
    pub fn contains(&self, tick: Tick, pad: Pad) -> bool {
        self.notes.binary_search(&ChartNote { tick, pad }).is_ok()
    }
}

/// How strong a position is in the bar: 0 for the downbeat, up to 5 off the grid.
/// Thinning keeps strong positions first, so an easier chart still sounds like
/// the beat rather than a random subset of it.
pub(crate) fn metric_level(tick: Tick) -> u8 {
    let t = tick.0.rem_euclid(TICKS_PER_BAR);
    if t == 0 {
        0
    } else if t % (2 * PPQ) == 0 {
        1
    } else if t % PPQ == 0 {
        2
    } else if t % (2 * TICKS_PER_STEP) == 0 {
        3
    } else if t % TICKS_PER_STEP == 0 {
        4
    } else {
        5
    }
}

/// Thins a drum part down to what `difficulty` allows.
pub fn auto_chart(hits: &[Hit], tempo: &TempoMap, difficulty: Difficulty) -> Chart {
    let rules = difficulty.rules();
    let mut candidates: Vec<ChartNote> = hits
        .iter()
        .filter(|h| rules.pads.contains(&h.pad) && h.velocity >= rules.min_velocity)
        .map(|h| ChartNote {
            tick: h.tick,
            pad: h.pad,
        })
        .collect();
    candidates.sort();
    candidates.dedup();
    // Strongest positions and most important pads are placed first.
    candidates.sort_by_key(|n| (metric_level(n.tick), priority(n.pad), n.tick));

    let ms_at = |tick: Tick| tempo.seconds_at(tick.0 as f64) * 1000.0;
    let mut by_tick: BTreeMap<Tick, Vec<Pad>> = BTreeMap::new();
    let mut thumbs: [BTreeMap<Tick, f64>; 2] = [BTreeMap::new(), BTreeMap::new()];
    for note in candidates {
        let side = thumb_index(thumb(note.pad));
        let at = ms_at(note.tick);
        let accepted_thumb = &thumbs[side];
        let too_close = |neighbour: Option<(&Tick, &f64)>| {
            neighbour.is_some_and(|(&tick, &ms)| tick != note.tick && (at - ms).abs() < rules.min_same_thumb_ms)
        };
        if too_close(accepted_thumb.range(..note.tick).next_back())
            || too_close(accepted_thumb.range(note.tick..).find(|(t, _)| **t != note.tick))
        {
            continue;
        }
        let chord = by_tick.entry(note.tick).or_default();
        let same_thumb: Vec<Pad> = chord.iter().copied().filter(|&p| thumb(p) == thumb(note.pad)).collect();
        if chord.len() >= rules.max_chord || same_thumb.len() >= 2 || same_thumb.iter().any(|&p| opposite(p, note.pad))
        {
            continue;
        }
        chord.push(note.pad);
        thumbs[side].insert(note.tick, at);
    }

    let mut notes: Vec<ChartNote> = by_tick
        .into_iter()
        .flat_map(|(tick, pads)| pads.into_iter().map(move |pad| ChartNote { tick, pad }))
        .collect();
    notes.sort();
    thin_density(&mut notes, tempo, &rules);
    Chart { difficulty, notes }
}

pub(crate) fn thumb_index(thumb: Thumb) -> usize {
    match thumb {
        Thumb::Left => 0,
        Thumb::Right => 1,
    }
}

/// Two-bar windows, one bar apart, each with its start and end tick.
pub(crate) fn density_windows(notes: &[ChartNote]) -> impl Iterator<Item = (Tick, Tick)> {
    let last_bar = notes.last().map_or(0, |n| n.tick.bar());
    let first_bar = notes.first().map_or(0, |n| n.tick.bar());
    (first_bar..=last_bar).map(|bar| (Tick::from_bars(bar), Tick::from_bars(bar + 2)))
}

pub(crate) fn notes_per_second(notes: &[ChartNote], tempo: &TempoMap, (start, end): (Tick, Tick)) -> f64 {
    let count = notes.iter().filter(|n| n.tick >= start && n.tick < end).count();
    let seconds = tempo.seconds_at(end.0 as f64) - tempo.seconds_at(start.0 as f64);
    count as f64 / seconds
}

/// Removes the weakest notes from any two-bar window that is too dense.
fn thin_density(notes: &mut Vec<ChartNote>, tempo: &TempoMap, rules: &Rules) {
    loop {
        let crowded = density_windows(notes).find(|&w| notes_per_second(notes, tempo, w) > rules.max_notes_per_second);
        let Some((start, end)) = crowded else { return };
        let weakest = notes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.tick >= start && n.tick < end)
            .max_by_key(|(_, n)| (metric_level(n.tick), priority(n.pad), n.tick))
            .map(|(i, _)| i);
        match weakest {
            Some(i) => {
                notes.remove(i);
            }
            None => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(step: i64, pad: Pad, velocity: f32) -> Hit {
        Hit {
            tick: Tick::from_steps(step),
            pad,
            velocity,
        }
    }

    /// Two bars of 16th hats, kick and snare on the backbeat, ghosts in between.
    fn busy_bar() -> Vec<Hit> {
        let mut hits = Vec::new();
        for bar in 0..2 {
            let s = bar * 16;
            hits.extend((0..16).map(|i| hit(s + i, Pad::P7, 0.8)));
            hits.extend([hit(s, Pad::P1, 1.0), hit(s + 10, Pad::P1, 0.8)]);
            hits.extend([hit(s + 4, Pad::P2, 1.0), hit(s + 12, Pad::P2, 1.0)]);
            hits.extend([hit(s + 7, Pad::P3, 0.45), hit(s + 9, Pad::P3, 0.45)]);
        }
        hits
    }

    #[test]
    fn metric_levels_rank_the_grid() {
        let levels: Vec<u8> = [0, 1920, 960, 480, 240, 120].map(|t| metric_level(Tick(t))).to_vec();
        assert_eq!(levels, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn easy_keeps_the_backbone_on_strong_beats() {
        let tempo = TempoMap::constant(168.0);
        let chart = auto_chart(&busy_bar(), &tempo, Difficulty::Easy);
        assert!(chart.contains(Tick::ZERO, Pad::P1), "kick on the one");
        assert!(chart.contains(Tick::from_steps(4), Pad::P2), "snare on two");
        assert!(chart.notes.iter().all(|n| n.pad != Pad::P3), "no ghosts");
        // Hats are thinned to strong positions only: never 16ths.
        assert!(
            chart
                .notes
                .iter()
                .filter(|n| n.pad == Pad::P7)
                .all(|n| metric_level(n.tick) <= 3)
        );
    }

    #[test]
    fn hard_keeps_more_than_easy() {
        let tempo = TempoMap::constant(168.0);
        let easy = auto_chart(&busy_bar(), &tempo, Difficulty::Easy);
        let hard = auto_chart(&busy_bar(), &tempo, Difficulty::Hard);
        assert!(hard.notes.len() > easy.notes.len());
        assert!(hard.notes.windows(2).all(|w| w[0] < w[1]), "sorted, no duplicates");
    }
}
