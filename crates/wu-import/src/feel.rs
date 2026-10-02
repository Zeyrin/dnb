//! The feel: where a tune's drums really sound against its straight grid. A
//! break played by a drummer, or swung by its producer, puts some sixteenths
//! a few milliseconds late, every bar the same; the listener hears each hit on
//! its sixteenth, then measures, per drum and per sixteenth of the bar, how far
//! off the line its hits land. Charts timed by the feel put each note where its
//! hit sounds, so playing with the drums is playing in time.
//!
//! One hit's attack is a rough measure; the median of every hit of a drum on
//! the same sixteenth is a steady one. A drum programmed on the grid comes out
//! on it: what is closer than [`STILL_MS`] stays on the line.

use serde::{Deserialize, Serialize};

use crate::hits::{Drum, Heard};
use crate::spectrum::Band;
use crate::tempo::{Grid, band_envelope};

/// Sixteenths in a bar.
pub const SIXTEENTHS: usize = 16;
/// Fewer hits than this on a sixteenth and it stays on the line.
const FEWEST: usize = 8;
/// Hits on a sixteenth that disagree by more than this (the middle half of
/// them, in milliseconds) are a roll or a flam smearing together, not a groove.
const MOST_SPREAD_MS: f64 = 6.0;
/// Closer to the line than this is the listener's own wobble, not the groove.
const STILL_MS: f32 = 3.0;
/// No groove pulls a hit further than this share of a sixteenth.
const FURTHEST_STEP: f64 = 0.4;
/// A hit starts where its envelope has climbed this share of the way to its peak.
const RISEN: f32 = 0.2;

/// Per drum and per sixteenth of the bar, how far after its line (before, if
/// negative) the tune's hits sound, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Feel {
    pub kick: [f32; SIXTEENTHS],
    /// Snares and their ghosts.
    pub snare: [f32; SIXTEENTHS],
    pub hat: [f32; SIXTEENTHS],
}

impl Feel {
    /// Where a hit of `drum` on `step` sounds, in milliseconds after its line.
    pub fn offset_ms(&self, drum: Drum, step: i64) -> f32 {
        let at = step.rem_euclid(SIXTEENTHS as i64) as usize;
        match drum {
            Drum::Kick => self.kick[at],
            Drum::Snare | Drum::Ghost => self.snare[at],
            Drum::Hat => self.hat[at],
        }
    }

    /// Whether every hit sits on its line.
    pub fn is_straight(&self) -> bool {
        [self.kick, self.snare, self.hat].iter().flatten().all(|&ms| ms == 0.0)
    }
}

/// Each drum is timed where its attack is sharpest: a kick where its pitch
/// starts falling (its lows swell too slowly to time it by), a snare's crack,
/// the hats' air.
const FAMILIES: [Band; 3] = [Band::Body, Band::Crack, Band::Air];

fn family(drum: Drum) -> usize {
    match drum {
        Drum::Kick => 0,
        Drum::Snare | Drum::Ghost => 1,
        Drum::Hat => 2,
    }
}

/// Where a hit near `around_s` starts: the last point before its peak still
/// under [`RISEN`] of the way up from the quietest point before it.
fn rise(envelope: &[f32], sample_rate: u32, around_s: f64) -> Option<f64> {
    let sr = f64::from(sample_rate);
    let from = ((around_s - 0.03) * sr).max(0.0) as usize;
    let to = (((around_s + 0.03) * sr) as usize).min(envelope.len());
    let window = envelope.get(from..to)?;
    let (peak_at, &peak) = window.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
    let floor = window[..=peak_at].iter().copied().fold(f32::MAX, f32::min);
    if peak <= 0.0 || peak < 2.0 * floor {
        return None;
    }
    let level = floor + RISEN * (peak - floor);
    let start = window[..=peak_at].iter().rposition(|&x| x < level)? + 1;
    Some((from + start) as f64 / sr)
}

/// Every hit's attack against its line, in milliseconds: per family (kick,
/// snare, hat) and per sixteenth of the bar.
pub fn attack_offsets(mono: &[f32], sample_rate: u32, grid: &Grid, hits: &[Heard]) -> Vec<Vec<Vec<f64>>> {
    let envelopes = FAMILIES.map(|band| band_envelope(mono, sample_rate, band));
    let mut gaps = vec![vec![Vec::new(); SIXTEENTHS]; FAMILIES.len()];
    for hit in hits {
        let f = family(hit.drum);
        let line = grid.time_of_step(hit.step as f64);
        if let Some(at) = rise(&envelopes[f], sample_rate, line) {
            gaps[f][hit.step.rem_euclid(SIXTEENTHS as i64) as usize].push((at - line) * 1000.0);
        }
    }
    gaps
}

/// The value `share` of the way up `sorted`.
fn quantile(sorted: &[f64], share: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * share).round() as usize]
}

/// The middle of `values` if there are enough of them and they agree within
/// `spread_ms` (the middle half of them), less `line`.
fn settled(values: &[f64], spread_ms: f64, line: f64) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    (sorted.len() >= FEWEST && quantile(&sorted, 0.75) - quantile(&sorted, 0.25) <= spread_ms)
        .then(|| quantile(&sorted, 0.5) - line)
}

/// The feel of `hits`, heard on `grid` in `mono`. A sixteenth with hits enough
/// of its own goes by them. An off-sixteenth without goes by the swing: what
/// that drum does on every off-sixteenth, or failing that every drum.
pub fn hear_feel(mono: &[f32], sample_rate: u32, grid: &Grid, hits: &[Heard]) -> Feel {
    let furthest_ms = FURTHEST_STEP * grid.step_s() * 1000.0;
    let offsets = attack_offsets(mono, sample_rate, grid, hits);
    // Each band times its attacks a touch early or late: what a drum does on
    // the eighths, where nobody swings, is its line.
    let lines: Vec<f64> = offsets
        .iter()
        .map(|gaps| {
            let mut on_eighths: Vec<f64> = gaps.iter().step_by(2).flatten().copied().collect();
            if on_eighths.len() < FEWEST {
                on_eighths = gaps.iter().flatten().copied().collect();
            }
            on_eighths.sort_by(f64::total_cmp);
            if on_eighths.is_empty() {
                0.0
            } else {
                quantile(&on_eighths, 0.5)
            }
        })
        .collect();
    let off_sixteenths = |f: usize| -> Vec<f64> {
        offsets[f]
            .iter()
            .skip(1)
            .step_by(2)
            .flatten()
            .map(|ms| ms - lines[f])
            .collect()
    };
    // Swing pulls whole off-sixteenths together, so they may disagree more.
    let swing_spread = 2.0 * MOST_SPREAD_MS;
    let every_drum: Vec<f64> = (0..FAMILIES.len()).flat_map(off_sixteenths).collect();
    let swing_of_all = settled(&every_drum, swing_spread, 0.0);
    let mut feel = [[0.0f32; SIXTEENTHS]; 3];
    for (f, gaps) in offsets.iter().enumerate() {
        let swing = settled(&off_sixteenths(f), swing_spread, 0.0).or(swing_of_all);
        for (at, gaps) in gaps.iter().enumerate() {
            let own = settled(gaps, MOST_SPREAD_MS, lines[f]);
            let off = match own {
                Some(off) => off,
                None if at % 2 == 1 => swing.unwrap_or(0.0),
                None => 0.0,
            };
            if off.abs() >= f64::from(STILL_MS) {
                feel[f][at] = off.clamp(-furthest_ms, furthest_ms) as f32;
            }
        }
    }
    Feel {
        kick: feel[0],
        snare: feel[1],
        hat: feel[2],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drum_sounds_where_its_sixteenth_does() {
        let mut feel = Feel::default();
        assert!(feel.is_straight());
        feel.hat[3] = 12.0;
        feel.snare[7] = -4.0;
        assert_eq!(feel.offset_ms(Drum::Hat, 19), 12.0, "the fourth sixteenth of bar two");
        assert_eq!(feel.offset_ms(Drum::Ghost, 7), -4.0, "ghosts go with the snare");
        assert_eq!(feel.offset_ms(Drum::Kick, 3), 0.0);
        assert!(!feel.is_straight());
    }
}
