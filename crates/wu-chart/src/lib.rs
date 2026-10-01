//! Charts: which of a song's drum hits the player plays, per difficulty.
//!
//! The auto-charter starts from the song's own drum part, so a chart is always
//! the music, thinned out to what a pair of thumbs can play at that level. The
//! hits a chart leaves out keep playing as backing, so the groove stays whole.
//!
//! Hands follow the controller: pads P1–P4 sit on the D-pad (left thumb), P5–P8
//! on the face buttons (right thumb). Opposite buttons (↑↓, ←→, △✕, □○) can't be
//! pressed together by one thumb; neighbours can. This holds for every layout
//! preset, which only swap pads within a hand.

#![forbid(unsafe_code)]

mod chart;
mod rules;
mod validate;

pub use chart::{Chart, ChartNote, auto_chart};
pub use rules::{Difficulty, Rules, Thumb, opposite, priority, thumb};
pub use validate::{Violation, validate};
