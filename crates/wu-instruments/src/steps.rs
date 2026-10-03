//! Step notation: one character per 16th note, `|` between bars.
//!
//! | Symbol | Meaning |
//! |---|---|
//! | `X` | accent (velocity 1.0) |
//! | `x` | hit (0.8) |
//! | `o` | ghost (0.45) |
//! | `2` `3` `4` | a ratchet: that many hits (0.8) evenly inside the step, a snare roll's stutter |
//! | `.` | rest |
//! | `\|` | bar line: every bar must hold exactly 16 steps |
//!
//! Spaces are ignored, so long patterns can be grouped by beat: `"x... x... | ..."`.

use wu_time::STEPS_PER_BAR;

pub const ACCENT: f32 = 1.0;
pub const HIT: f32 = 0.8;
pub const GHOST: f32 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Rest,
    Hit(f32),
    /// `strokes` hits evenly inside the step, each at `velocity`.
    Ratchet {
        velocity: f32,
        strokes: u8,
    },
}

impl Step {
    /// Its hits, as (where in the step, 0–1, and how hard).
    pub fn strokes(self) -> impl Iterator<Item = (f64, f32)> {
        let (velocity, count) = match self {
            Step::Rest => (0.0, 0),
            Step::Hit(velocity) => (velocity, 1),
            Step::Ratchet { velocity, strokes } => (velocity, strokes),
        };
        (0..count).map(move |i| (f64::from(i) / f64::from(count), velocity))
    }

    /// How hard its first hit is, if it has one.
    pub fn velocity(self) -> Option<f32> {
        match self {
            Step::Rest => None,
            Step::Hit(velocity) | Step::Ratchet { velocity, .. } => Some(velocity),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StepError {
    #[error("unknown step symbol '{symbol}' at position {position} (use X x o 2 3 4 . |)")]
    UnknownSymbol { symbol: char, position: usize },
    #[error("bar {bar} has {steps} steps; every bar needs {STEPS_PER_BAR}")]
    BarLength { bar: usize, steps: usize },
}

/// Parses a step string into one `Step` per 16th note.
pub fn parse_steps(text: &str) -> Result<Vec<Step>, StepError> {
    let mut steps = Vec::with_capacity(text.len());
    let mut bar_start = 0;
    let mut bar = 1;
    let check_bar = |steps: &Vec<Step>, bar_start: usize, bar: usize| {
        let len = steps.len() - bar_start;
        if len == STEPS_PER_BAR as usize {
            Ok(())
        } else {
            Err(StepError::BarLength { bar, steps: len })
        }
    };
    for (position, symbol) in text.chars().enumerate() {
        match symbol {
            'X' => steps.push(Step::Hit(ACCENT)),
            'x' => steps.push(Step::Hit(HIT)),
            'o' => steps.push(Step::Hit(GHOST)),
            '2'..='4' => steps.push(Step::Ratchet {
                velocity: HIT,
                strokes: symbol as u8 - b'0',
            }),
            '.' => steps.push(Step::Rest),
            '|' => {
                check_bar(&steps, bar_start, bar)?;
                bar_start = steps.len();
                bar += 1;
            }
            c if c.is_whitespace() => {}
            symbol => return Err(StepError::UnknownSymbol { symbol, position }),
        }
    }
    check_bar(&steps, bar_start, bar)?;
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn symbols_map_to_velocities() {
        let steps = parse_steps("Xxo. .... .... ....").expect("valid");
        assert_eq!(
            &steps[..4],
            &[Step::Hit(ACCENT), Step::Hit(HIT), Step::Hit(GHOST), Step::Rest]
        );
        assert_eq!(steps.len(), 16);
    }

    #[test]
    fn digits_are_ratchets_inside_their_step() {
        let steps = parse_steps("x3.. .... .... ...4").expect("valid");
        assert_eq!(
            steps[1],
            Step::Ratchet {
                velocity: HIT,
                strokes: 3
            }
        );
        let strokes: Vec<(f64, f32)> = steps[15].strokes().collect();
        assert_eq!(strokes, vec![(0.0, HIT), (0.25, HIT), (0.5, HIT), (0.75, HIT)]);
        assert_eq!(steps[0].strokes().count(), 1);
        assert_eq!(steps[2].strokes().count(), 0);
        assert!(parse_steps("5...............").is_err(), "four at most");
    }

    #[test]
    fn every_bar_must_be_complete() {
        assert_eq!(
            parse_steps("x...............|x..."),
            Err(StepError::BarLength { bar: 2, steps: 4 })
        );
        assert_eq!(parse_steps("x..."), Err(StepError::BarLength { bar: 1, steps: 4 }));
        assert!(parse_steps("x...............|................").is_ok());
    }

    #[test]
    fn unknown_symbols_are_reported_with_their_position() {
        assert_eq!(
            parse_steps("x..?"),
            Err(StepError::UnknownSymbol {
                symbol: '?',
                position: 3
            })
        );
    }

    proptest! {
        #[test]
        fn any_whole_bars_parse_to_sixteen_steps_each(bars in prop::collection::vec("[Xxo.2-4]{16}", 1..8)) {
            let text = bars.join("|");
            let steps = parse_steps(&text).expect("whole bars");
            prop_assert_eq!(steps.len(), bars.len() * 16);
        }
    }
}
