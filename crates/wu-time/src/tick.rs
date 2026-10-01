//! Positions and durations in musical time.

use std::fmt;
use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

/// Ticks per beat (quarter note).
pub const PPQ: i64 = 960;
/// Beats per bar. WHEEL UP! is 4/4 throughout: drum & bass and jungle never leave it.
pub const BEATS_PER_BAR: i64 = 4;
/// Ticks in one 16th-note step, the grid patterns are written on.
pub const TICKS_PER_STEP: i64 = PPQ / 4;
/// Ticks in one bar.
pub const TICKS_PER_BAR: i64 = PPQ * BEATS_PER_BAR;
/// 16th-note steps in one bar.
pub const STEPS_PER_BAR: i64 = TICKS_PER_BAR / TICKS_PER_STEP;

/// A position or a duration in musical time, in ticks of 1/960 of a beat.
///
/// Negative ticks are allowed: they are the count-in before the song's first beat.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tick(pub i64);

impl Tick {
    pub const ZERO: Tick = Tick(0);

    pub const fn from_beats(beats: i64) -> Tick {
        Tick(beats * PPQ)
    }

    pub const fn from_steps(steps: i64) -> Tick {
        Tick(steps * TICKS_PER_STEP)
    }

    pub const fn from_bars(bars: i64) -> Tick {
        Tick(bars * TICKS_PER_BAR)
    }

    /// Zero-based bar this tick falls in. Count-in ticks give negative bars.
    pub const fn bar(self) -> i64 {
        self.0.div_euclid(TICKS_PER_BAR)
    }

    /// Zero-based beat within its bar.
    pub const fn beat_in_bar(self) -> i64 {
        self.0.rem_euclid(TICKS_PER_BAR) / PPQ
    }

    /// Zero-based 16th-note step within its bar.
    pub const fn step_in_bar(self) -> i64 {
        self.0.rem_euclid(TICKS_PER_BAR) / TICKS_PER_STEP
    }

    pub fn as_beats(self) -> f64 {
        self.0 as f64 / PPQ as f64
    }

    /// Pushes odd 16th-note steps late by `amount` of a step: 0 is straight,
    /// 1/3 is a triplet feel, and the amount is clamped to 0–0.5. Positions off
    /// the 16th grid are left where they are.
    pub fn swung(self, amount: f64) -> Tick {
        let amount = if amount.is_finite() {
            amount.clamp(0.0, 0.5)
        } else {
            0.0
        };
        if self.0.rem_euclid(2 * TICKS_PER_STEP) == TICKS_PER_STEP {
            Tick(self.0 + (amount * TICKS_PER_STEP as f64).round() as i64)
        } else {
            self
        }
    }
}

/// `bar.beat.tick`, one-based like a DAW's position display.
impl fmt::Display for Tick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let in_beat = self.0.rem_euclid(PPQ);
        write!(f, "{}.{}.{}", self.bar() + 1, self.beat_in_bar() + 1, in_beat)
    }
}

impl Add for Tick {
    type Output = Tick;
    fn add(self, rhs: Tick) -> Tick {
        Tick(self.0 + rhs.0)
    }
}

impl AddAssign for Tick {
    fn add_assign(&mut self, rhs: Tick) {
        self.0 += rhs.0;
    }
}

impl Sub for Tick {
    type Output = Tick;
    fn sub(self, rhs: Tick) -> Tick {
        Tick(self.0 - rhs.0)
    }
}

impl SubAssign for Tick {
    fn sub_assign(&mut self, rhs: Tick) {
        self.0 -= rhs.0;
    }
}

impl Neg for Tick {
    type Output = Tick;
    fn neg(self) -> Tick {
        Tick(-self.0)
    }
}

impl Mul<i64> for Tick {
    type Output = Tick;
    fn mul(self, rhs: i64) -> Tick {
        Tick(self.0 * rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_constants_fit_together() {
        assert_eq!(TICKS_PER_STEP, 240);
        assert_eq!(TICKS_PER_BAR, 3840);
        assert_eq!(STEPS_PER_BAR, 16);
    }

    #[test]
    fn bar_beat_and_step_are_zero_based() {
        let t = Tick::from_bars(1) + Tick::from_beats(1) + Tick::from_steps(1);
        assert_eq!((t.bar(), t.beat_in_bar(), t.step_in_bar()), (1, 1, 5));
        assert_eq!(t.to_string(), "2.2.240");
    }

    #[test]
    fn count_in_ticks_land_in_negative_bars() {
        let t = Tick(-1);
        assert_eq!(t.bar(), -1);
        assert_eq!(t.beat_in_bar(), 3);
        assert_eq!(t.step_in_bar(), 15);
    }

    #[test]
    fn swing_moves_only_odd_steps() {
        assert_eq!(Tick::from_steps(1).swung(1.0 / 3.0), Tick(320));
        assert_eq!(Tick::from_steps(2).swung(1.0 / 3.0), Tick::from_steps(2));
        assert_eq!(Tick(250).swung(0.3), Tick(250));
        assert_eq!(Tick::from_steps(-1).swung(0.5), Tick(-120));
    }

    #[test]
    fn swing_amount_is_clamped() {
        assert_eq!(Tick::from_steps(1).swung(2.0), Tick(360));
        assert_eq!(Tick::from_steps(1).swung(-1.0), Tick::from_steps(1));
        assert_eq!(Tick::from_steps(1).swung(f64::NAN), Tick::from_steps(1));
    }
}
