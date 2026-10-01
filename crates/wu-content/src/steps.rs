//! Step notation: one character per 16th note, `|` between bars.
//!
//! | Symbol | Meaning |
//! |---|---|
//! | `X` | accent (velocity 1.0) |
//! | `x` | hit (0.8) |
//! | `o` | ghost (0.45) |
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
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StepError {
    #[error("unknown step symbol '{symbol}' at position {position} (use X x o . |)")]
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
        fn any_whole_bars_parse_to_sixteen_steps_each(bars in prop::collection::vec("[Xxo.]{16}", 1..8)) {
            let text = bars.join("|");
            let steps = parse_steps(&text).expect("whole bars");
            prop_assert_eq!(steps.len(), bars.len() * 16);
        }
    }
}
