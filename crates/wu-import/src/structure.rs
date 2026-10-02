//! The tune's shape: its sections, and which of them are drops (the game's
//! hype phrases). Drum & bass is built in blocks of four and eight bars, and
//! its sections differ in what plays far more than in anything subtler: an
//! intro of drums and pads, the drop with the bass in and the snare driving
//! the backbeat, a breakdown with the drums out or half-time. So the tune is
//! cut into four-bar blocks, each block is called full or not by how much of
//! that it plays, and runs of the same make the sections, named by where
//! they fall around the drops.

use std::ops::Range;

use wu_audio::Note;
use wu_time::TICKS_PER_STEP;

use crate::hits::{Drum, Heard};
use crate::spectrum::Spectrogram;
use crate::tempo::Grid;

/// Sections are told apart in blocks of this many bars…
const BLOCK_BARS: i64 = 4;
/// …and, but for the first and the last, are at least this long.
const SHORTEST_BARS: i64 = 8;
/// A block has drums when it has this share of the hits the busiest blocks have.
const DRUMS_AT: f64 = 0.35;
/// A block's loudness counts from this many decibels under the loudest blocks.
const LOUD_RANGE_DB: f64 = 12.0;
/// A block is full when it plays this share of what the fullest block does.
const FULL_AT: f64 = 0.75;

/// A stretch of the tune.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    /// Bars from the first bar line.
    pub bars: Range<i64>,
    /// Whether it is a drop: drums, bass and all, as loud as the tune gets.
    pub drop: bool,
}

/// The sections of the first `bars` bars, from what was heard in them.
pub fn find_sections(spec: &Spectrogram, grid: &Grid, bars: i64, hits: &[Heard], bass: &[Note]) -> Vec<Section> {
    let blocks = (bars + BLOCK_BARS - 1) / BLOCK_BARS;
    if blocks == 0 {
        return Vec::new();
    }
    let block_bars = |b: i64| (b * BLOCK_BARS)..((b + 1) * BLOCK_BARS).min(bars);
    let steps = |b: i64| {
        let bars = block_bars(b);
        (bars.start * 16)..(bars.end * 16)
    };
    // How loud each block is: the energy of every band, per frame, in decibels.
    let loudness: Vec<f64> = (0..blocks)
        .map(|b| {
            let range = steps(b);
            let rate = spec.rate();
            let from = (grid.time_of_step(range.start as f64) * rate).max(0.0) as usize;
            let to = ((grid.time_of_step(range.end as f64) * rate) as usize).min(spec.frames);
            let frames = from..to.max(from);
            let energy: f64 = spec
                .energy
                .iter()
                .map(|band| {
                    band.get(frames.clone())
                        .map_or(0.0, |e| e.iter().map(|&x| f64::from(x)).sum::<f64>())
                })
                .sum();
            10.0 * (energy / frames.len().max(1) as f64).max(1e-12).log10()
        })
        .collect();
    // How busy the drums are: kicks and snares count whole, ghosts half, hats a quarter.
    let drums: Vec<f64> = (0..blocks)
        .map(|b| {
            let range = steps(b);
            hits.iter()
                .filter(|h| range.contains(&h.step))
                .map(|h| match h.drum {
                    Drum::Kick | Drum::Snare => 1.0,
                    Drum::Ghost => 0.5,
                    Drum::Hat => 0.25,
                })
                .sum::<f64>()
                / (range.end - range.start).max(1) as f64
        })
        .collect();
    // On how many of its steps the bass sounds.
    let mut sounding = vec![false; (bars * 16).max(0) as usize];
    for note in bass {
        let start = note.tick.0 / TICKS_PER_STEP;
        for step in start..start + note.length.0 / TICKS_PER_STEP {
            if let Some(s) = usize::try_from(step).ok().and_then(|s| sounding.get_mut(s)) {
                *s = true;
            }
        }
    }
    let bassy: Vec<f64> = (0..blocks)
        .map(|b| {
            let range = steps(b);
            range.clone().filter(|&s| sounding[s as usize]).count() as f64 / (range.end - range.start).max(1) as f64
        })
        .collect();
    // Bars with a snare on two or four: a drop's drive, which a breakdown's
    // half-time beat and an intro's hats lack.
    let backbeat: Vec<f64> = (0..blocks)
        .map(|b| {
            let bars = block_bars(b);
            let driven = bars
                .clone()
                .filter(|bar| {
                    hits.iter()
                        .any(|h| h.drum == Drum::Snare && (h.step == bar * 16 + 4 || h.step == bar * 16 + 12))
                })
                .count();
            driven as f64 / (bars.end - bars.start).max(1) as f64
        })
        .collect();
    let loud_end = |values: &[f64]| {
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        sorted[(sorted.len() - 1) * 9 / 10]
    };
    let (busiest, loudest) = (loud_end(&drums), loud_end(&loudness));
    let has_drums = |b: usize| busiest > 0.0 && drums[b] >= DRUMS_AT * busiest;
    // How much a block plays: its drums, its bass, its loudness, its drive, each 0–1.
    let intensity: Vec<f64> = (0..blocks as usize)
        .map(|b| {
            let drums = if busiest > 0.0 {
                (drums[b] / busiest).min(1.0)
            } else {
                0.0
            };
            let loud = (1.0 - (loudest - loudness[b]) / LOUD_RANGE_DB).clamp(0.0, 1.0);
            drums + bassy[b] + loud + backbeat[b]
        })
        .collect();
    let fullest = intensity.iter().copied().fold(0.0, f64::max);
    let full: Vec<bool> = (0..blocks as usize)
        .map(|b| has_drums(b) && intensity[b] >= FULL_AT * fullest)
        .collect();
    shape(&full, bars)
}

/// Sections from which four-bar blocks are full: runs of the same, those too
/// short to stand alone (but for the first and the last) made part of the runs
/// either side, named by where they fall around the drops.
fn shape(full: &[bool], bars: i64) -> Vec<Section> {
    // (first block, blocks, full)
    let mut runs: Vec<(usize, usize, bool)> = Vec::new();
    for (b, &full) in full.iter().enumerate() {
        match runs.last_mut() {
            Some((_, length, last)) if *last == full => *length += 1,
            _ => runs.push((b, 1, full)),
        }
    }
    let shortest = (SHORTEST_BARS / BLOCK_BARS) as usize;
    while let Some(i) = (1..runs.len().saturating_sub(1)).find(|&i| runs[i].1 < shortest) {
        let (_, length, _) = runs.remove(i);
        let (_, after, _) = runs.remove(i);
        runs[i - 1].1 += length + after;
    }
    let drops = runs.iter().filter(|r| r.2).count();
    let mut seen = 0;
    runs.iter()
        .enumerate()
        .map(|(i, &(first, length, drop))| {
            let name = if drop {
                seen += 1;
                if seen == 1 {
                    "Drop".to_string()
                } else {
                    format!("Drop {seen}")
                }
            } else if seen > 0 && seen == drops {
                "Outro".to_string()
            } else if seen > 0 {
                "Breakdown".to_string()
            } else if i == 0 {
                "Intro".to_string()
            } else {
                "Build".to_string()
            };
            Section {
                name,
                bars: (first as i64 * BLOCK_BARS)..((first + length) as i64 * BLOCK_BARS).min(bars),
                drop,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(full: &str, bars: i64) -> Vec<(String, Range<i64>)> {
        let full: Vec<bool> = full.chars().map(|c| c == 'F').collect();
        shape(&full, bars).into_iter().map(|s| (s.name, s.bars)).collect()
    }

    #[test]
    fn sections_are_named_by_where_they_fall_around_the_drops() {
        let shape = names("....FFFF..FFFF..", 64);
        let expected = [
            ("Intro", 0..16),
            ("Drop", 16..32),
            ("Breakdown", 32..40),
            ("Drop 2", 40..56),
            ("Outro", 56..64),
        ];
        assert_eq!(shape, expected.map(|(n, b)| (n.to_string(), b)));
    }

    #[test]
    fn four_bars_out_of_a_drop_are_still_the_drop() {
        // A fill of four quieter bars mid-drop, and a tune ending on a drop
        // two bars short of its last block.
        let shape = names("..FFF.FFF", 34);
        assert_eq!(shape, [("Intro".to_string(), 0..8), ("Drop".to_string(), 8..34)]);
    }

    #[test]
    fn a_tune_with_nothing_full_has_no_drop() {
        assert_eq!(names("....", 16), [("Intro".to_string(), 0..16)]);
    }
}
