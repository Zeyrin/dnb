//! Tempo maps: where ticks fall in seconds and sample frames.

use serde::{Deserialize, Serialize};

use crate::tick::{PPQ, Tick};

/// Slowest tempo a map accepts. Practice mode scales songs down to 50 %, so the
/// range is far wider than any song needs.
pub const MIN_BPM: f64 = 20.0;
/// Fastest tempo a map accepts.
pub const MAX_BPM: f64 = 999.0;

/// A tempo that holds from `tick` until the next point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TempoPoint {
    pub tick: Tick,
    pub bpm: f64,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum TempoError {
    #[error("a tempo map needs at least one tempo")]
    Empty,
    #[error("the first tempo must start at tick 0, not tick {0}")]
    FirstNotAtZero(i64),
    #[error("tempo changes must be in strictly increasing order (tick {0})")]
    NotIncreasing(i64),
    #[error("tempo {bpm} BPM at tick {tick} is outside {MIN_BPM}–{MAX_BPM} BPM")]
    OutOfRange { tick: i64, bpm: f64 },
}

/// Step tempo changes from tick 0 on. Ticks before 0 (the count-in) use the
/// first tempo.
///
/// Each segment stores the second it starts at, computed once from the exact
/// lengths of the segments before it, so converting any tick costs one binary
/// search and one multiplication, with no accumulated rounding.
#[derive(Clone, Debug, PartialEq)]
pub struct TempoMap {
    segments: Vec<Segment>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Segment {
    tick: i64,
    bpm: f64,
    start_seconds: f64,
}

impl Segment {
    fn seconds_per_tick(&self) -> f64 {
        60.0 / (self.bpm * PPQ as f64)
    }
}

impl TempoMap {
    /// One tempo for the whole song. Out-of-range values are clamped, and a
    /// non-finite one falls back to 120 BPM.
    pub fn constant(bpm: f64) -> TempoMap {
        let bpm = if bpm.is_finite() {
            bpm.clamp(MIN_BPM, MAX_BPM)
        } else {
            120.0
        };
        TempoMap {
            segments: vec![Segment {
                tick: 0,
                bpm,
                start_seconds: 0.0,
            }],
        }
    }

    pub fn new(points: &[TempoPoint]) -> Result<TempoMap, TempoError> {
        let first = points.first().ok_or(TempoError::Empty)?;
        if first.tick != Tick::ZERO {
            return Err(TempoError::FirstNotAtZero(first.tick.0));
        }
        let mut segments: Vec<Segment> = Vec::with_capacity(points.len());
        for point in points {
            if !(MIN_BPM..=MAX_BPM).contains(&point.bpm) {
                return Err(TempoError::OutOfRange {
                    tick: point.tick.0,
                    bpm: point.bpm,
                });
            }
            let start_seconds = match segments.last() {
                None => 0.0,
                Some(prev) if point.tick.0 <= prev.tick => {
                    return Err(TempoError::NotIncreasing(point.tick.0));
                }
                Some(prev) => prev.start_seconds + (point.tick.0 - prev.tick) as f64 * prev.seconds_per_tick(),
            };
            segments.push(Segment {
                tick: point.tick.0,
                bpm: point.bpm,
                start_seconds,
            });
        }
        Ok(TempoMap { segments })
    }

    pub fn points(&self) -> impl Iterator<Item = TempoPoint> + '_ {
        self.segments.iter().map(|s| TempoPoint {
            tick: Tick(s.tick),
            bpm: s.bpm,
        })
    }

    /// Every tempo multiplied by `factor`: practice mode's tempo slider.
    pub fn scaled(&self, factor: f64) -> Result<TempoMap, TempoError> {
        let points: Vec<TempoPoint> = self
            .points()
            .map(|p| TempoPoint {
                tick: p.tick,
                bpm: p.bpm * factor,
            })
            .collect();
        TempoMap::new(&points)
    }

    pub fn bpm_at(&self, tick: Tick) -> f64 {
        self.segment_for_tick(tick.0 as f64).bpm
    }

    /// Seconds from tick 0 to `tick`, which may be fractional or negative.
    pub fn seconds_at(&self, tick: f64) -> f64 {
        let seg = self.segment_for_tick(tick);
        seg.start_seconds + (tick - seg.tick as f64) * seg.seconds_per_tick()
    }

    /// The (fractional) tick sounding `seconds` after tick 0.
    pub fn tick_at_seconds(&self, seconds: f64) -> f64 {
        let idx = self
            .segments
            .partition_point(|s| s.start_seconds <= seconds)
            .saturating_sub(1);
        let seg = &self.segments[idx];
        seg.tick as f64 + (seconds - seg.start_seconds) / seg.seconds_per_tick()
    }

    /// The sample frame `tick` starts on, counted from tick 0 at frame 0.
    pub fn frame_at(&self, tick: Tick, sample_rate: u32) -> i64 {
        (self.seconds_at(tick.0 as f64) * f64::from(sample_rate)).round() as i64
    }

    /// The (fractional) tick sounding at `frame`.
    pub fn tick_at_frame(&self, frame: f64, sample_rate: u32) -> f64 {
        self.tick_at_seconds(frame / f64::from(sample_rate))
    }

    fn segment_for_tick(&self, tick: f64) -> &Segment {
        let idx = self
            .segments
            .partition_point(|s| s.tick as f64 <= tick)
            .saturating_sub(1);
        &self.segments[idx]
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::tick::TICKS_PER_BAR;

    fn two_tempos() -> TempoMap {
        TempoMap::new(&[
            TempoPoint {
                tick: Tick::ZERO,
                bpm: 120.0,
            },
            TempoPoint {
                tick: Tick::from_beats(4),
                bpm: 60.0,
            },
        ])
        .expect("valid map")
    }

    #[test]
    fn a_beat_at_174_bpm_lands_on_the_expected_sample() {
        let map = TempoMap::constant(174.0);
        assert_eq!(map.frame_at(Tick::from_beats(1), 48_000), 16_552);
        assert!((map.seconds_at(960.0) - 60.0 / 174.0).abs() < 1e-12);
    }

    #[test]
    fn tempo_changes_take_effect_at_their_tick() {
        let map = two_tempos();
        assert_eq!(map.seconds_at(Tick::from_beats(4).0 as f64), 2.0);
        assert_eq!(map.seconds_at(Tick::from_beats(5).0 as f64), 3.0);
        assert_eq!(map.tick_at_seconds(3.0), Tick::from_beats(5).0 as f64);
        assert_eq!(map.bpm_at(Tick::from_beats(3)), 120.0);
        assert_eq!(map.bpm_at(Tick::from_beats(4)), 60.0);
    }

    #[test]
    fn count_in_uses_the_first_tempo() {
        let map = two_tempos();
        assert_eq!(map.seconds_at(-960.0), -0.5);
        assert_eq!(map.tick_at_seconds(-0.5), -960.0);
        assert_eq!(map.frame_at(Tick(-960), 48_000), -24_000);
    }

    #[test]
    fn an_hour_lands_on_the_exact_sample() {
        let map = TempoMap::constant(174.0);
        let one_hour = Tick::from_beats(174 * 60);
        assert_eq!(map.frame_at(one_hour, 48_000), 3600 * 48_000);
        assert_eq!(map.frame_at(one_hour, 44_100), 3600 * 44_100);
    }

    #[test]
    fn scaling_changes_every_tempo() {
        let half = two_tempos().scaled(0.5).expect("in range");
        assert_eq!(half.seconds_at(Tick::from_beats(4).0 as f64), 4.0);
        assert_eq!(half.bpm_at(Tick::from_bars(10)), 30.0);
    }

    #[test]
    fn invalid_maps_are_rejected() {
        assert_eq!(TempoMap::new(&[]), Err(TempoError::Empty));
        assert_eq!(
            TempoMap::new(&[TempoPoint {
                tick: Tick(10),
                bpm: 170.0
            }]),
            Err(TempoError::FirstNotAtZero(10))
        );
        assert_eq!(
            TempoMap::new(&[
                TempoPoint {
                    tick: Tick::ZERO,
                    bpm: 170.0
                },
                TempoPoint {
                    tick: Tick::ZERO,
                    bpm: 172.0
                },
            ]),
            Err(TempoError::NotIncreasing(0))
        );
        assert!(matches!(
            TempoMap::new(&[TempoPoint {
                tick: Tick::ZERO,
                bpm: 5000.0
            }]),
            Err(TempoError::OutOfRange { .. })
        ));
        assert!(TempoMap::constant(10.0).scaled(0.5).is_err());
    }

    #[test]
    fn constant_clamps_bad_input() {
        assert_eq!(TempoMap::constant(5.0).bpm_at(Tick::ZERO), MIN_BPM);
        assert_eq!(TempoMap::constant(f64::NAN).bpm_at(Tick::ZERO), 120.0);
    }

    proptest! {
        #[test]
        fn ticks_survive_a_round_trip_through_seconds(
            bpms in prop::collection::vec(60.0f64..250.0, 1..6),
            tick in -TICKS_PER_BAR..(TICKS_PER_BAR * 400),
        ) {
            let points: Vec<TempoPoint> = bpms
                .iter()
                .enumerate()
                .map(|(i, &bpm)| TempoPoint { tick: Tick::from_bars(i as i64 * 8), bpm })
                .collect();
            let map = TempoMap::new(&points).expect("valid map");
            let back = map.tick_at_seconds(map.seconds_at(tick as f64));
            prop_assert!((back - tick as f64).abs() < 1e-6);
        }

        #[test]
        fn frames_never_run_backwards(bpm in 60.0f64..250.0, tick in 0i64..(TICKS_PER_BAR * 400)) {
            let map = TempoMap::constant(bpm);
            prop_assert!(map.frame_at(Tick(tick + 1), 48_000) >= map.frame_at(Tick(tick), 48_000));
        }
    }
}
