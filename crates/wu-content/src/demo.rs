//! The demo groove: two bars on the Ragga '93 kit. A two-step bar, then a
//! jungle edit in the style of the classic chopped breaks.

use wu_audio::{Hit, Program};
use wu_instruments::{Kit, Pad};
use wu_time::{STEPS_PER_BAR, TempoMap, Tick};

use crate::steps::{Step, StepError, parse_steps};

pub const DEMO_BPM: f64 = 174.0;
pub const DEMO_BARS: i64 = 2;

/// Pad by pad. P1 kick, P2 snare, P3 ghost, P5 jungle snare, P7 closed hat, P8 open hat.
pub const DEMO_PATTERN: [(Pad, &str); 6] = [
    (Pad::P1, "X... .... ..X. .... | X.x. .... ..xx ...."),
    (Pad::P2, ".... X... .... X... | .... .... .... ...."),
    (Pad::P3, ".... ...o .o.. ...o | .... .... .... ...."),
    (Pad::P5, ".... .... .... .... | .... X..o .o.. X..o"),
    (Pad::P7, "x.x. x.x. x.x. x.x. | x.x. x.x. x.x. x..."),
    (Pad::P8, ".... .... .... .... | .... .... .... ..x."),
];

/// Turns pad step strings into hits, repeating the pattern for `bars` bars.
pub fn hits_from_steps(pattern: &[(Pad, &str)], bars: i64) -> Result<Vec<Hit>, StepError> {
    let mut hits = Vec::new();
    for &(pad, text) in pattern {
        let steps = parse_steps(text)?;
        let pattern_steps = steps.len() as i64;
        for step in 0..bars.max(0) * STEPS_PER_BAR {
            if let Step::Hit(velocity) = steps[(step % pattern_steps) as usize] {
                hits.push(Hit {
                    tick: Tick::from_steps(step),
                    pad,
                    velocity,
                });
            }
        }
    }
    hits.sort_by_key(|h| (h.tick, h.pad));
    Ok(hits)
}

/// The demo, ready for the engine: `bars` bars at `bpm`, looped if `looped`.
pub fn demo_program(sample_rate: u32, bpm: f64, bars: i64, looped: bool) -> Program {
    let hits = hits_from_steps(&DEMO_PATTERN, bars).unwrap_or_default();
    let program = Program::new(sample_rate, TempoMap::constant(bpm), Kit::ragga_93(sample_rate)).with_hits(hits);
    if looped {
        program.with_loop(Tick::ZERO, Tick::from_bars(bars))
    } else {
        program
    }
}

/// A click on every beat (accented on the one), looped over a bar: the
/// calibration metronome. Plays the rim, `Pad::P4`.
pub fn metronome_program(sample_rate: u32, bpm: f64) -> Program {
    let hits = (0..4).map(|beat| Hit {
        tick: Tick::from_beats(beat),
        pad: Pad::P4,
        velocity: if beat == 0 { 1.0 } else { 0.75 },
    });
    Program::new(sample_rate, TempoMap::constant(bpm), Kit::ragga_93(sample_rate))
        .with_hits(hits)
        .with_loop(Tick::ZERO, Tick::from_bars(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_pattern_is_valid_and_two_bars_long() {
        for (pad, text) in DEMO_PATTERN {
            let steps = parse_steps(text).unwrap_or_else(|e| panic!("{pad}: {e}"));
            assert_eq!(steps.len() as i64, DEMO_BARS * STEPS_PER_BAR, "{pad}");
        }
    }

    #[test]
    fn hits_repeat_the_pattern() {
        let two = hits_from_steps(&DEMO_PATTERN, 2).expect("valid");
        let four = hits_from_steps(&DEMO_PATTERN, 4).expect("valid");
        assert_eq!(four.len(), two.len() * 2);
        let kick_on_one = Hit {
            tick: Tick::ZERO,
            pad: Pad::P1,
            velocity: 1.0,
        };
        assert_eq!(two[0], kick_on_one);
        assert!(four.contains(&Hit {
            tick: Tick::from_bars(2),
            ..kick_on_one
        }));
    }

    #[test]
    fn the_metronome_clicks_every_beat() {
        let program = metronome_program(48_000, 120.0);
        assert_eq!(program.events().len(), 4);
        assert!(program.events().iter().all(|e| e.pad() == Some(Pad::P4)));
        assert_eq!(program.events()[1].frame, 24_000);
    }

    #[test]
    fn the_looped_demo_loops_over_its_bars() {
        let program = demo_program(48_000, DEMO_BPM, DEMO_BARS, true);
        let range = program.loop_range().expect("looped");
        assert_eq!((range.start, range.end), (Tick::ZERO, Tick::from_bars(2)));
    }
}
