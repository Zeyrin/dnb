//! What each difficulty allows, and what a thumb can do.

use serde::{Deserialize, Serialize};
use wu_instruments::Pad;

/// Same-thumb gaps shorter than this need the shoulder button's help: they
/// only appear inside a roll, where L1 or R1 can take every other stroke.
pub const ROLL_GAP_MS: f64 = 110.0;
/// The fewest notes a roll has: a beat of 16ths.
pub const MIN_ROLL_NOTES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Difficulty {
    Beginner,
    Easy,
    Medium,
    Hard,
    Junglist,
}

impl Difficulty {
    pub const ALL: [Difficulty; 5] = [
        Difficulty::Beginner,
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
        Difficulty::Junglist,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Beginner => "Beginner",
            Difficulty::Easy => "Easy",
            Difficulty::Medium => "Medium",
            Difficulty::Hard => "Hard",
            Difficulty::Junglist => "Junglist",
        }
    }

    /// Starting values from the brief; tune by playtesting (log changes in DECISIONS.md).
    pub fn rules(self) -> Rules {
        use Pad::*;
        match self {
            Difficulty::Beginner => Rules {
                pads: &[P1, P2],
                min_velocity: 0.7,
                min_same_thumb_ms: 400.0,
                max_chord: 1,
                max_notes_per_second: 1.5,
                rolls: false,
            },
            Difficulty::Easy => Rules {
                pads: &[P1, P2, P7],
                min_velocity: 0.6,
                min_same_thumb_ms: 250.0,
                max_chord: 2,
                max_notes_per_second: 3.0,
                rolls: false,
            },
            Difficulty::Medium => Rules {
                pads: &[P1, P2, P3, P4, P7],
                min_velocity: 0.5,
                min_same_thumb_ms: 170.0,
                max_chord: 2,
                max_notes_per_second: 5.0,
                rolls: false,
            },
            Difficulty::Hard => Rules {
                pads: &[P1, P2, P3, P4, P5, P6, P7, P8],
                min_velocity: 0.5,
                min_same_thumb_ms: 120.0,
                max_chord: 3,
                max_notes_per_second: 8.0,
                rolls: false,
            },
            Difficulty::Junglist => Rules {
                pads: &[P1, P2, P3, P4, P5, P6, P7, P8],
                min_velocity: 0.0,
                min_same_thumb_ms: 85.0,
                max_chord: 4,
                max_notes_per_second: 12.0,
                rolls: true,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    /// The lanes in play.
    pub pads: &'static [Pad],
    /// Quieter hits (ghost notes) are left to the backing.
    pub min_velocity: f32,
    /// The shortest gap between two presses of the same thumb.
    pub min_same_thumb_ms: f64,
    /// Most notes at one instant.
    pub max_chord: usize,
    /// Peak density, averaged over any two bars.
    pub max_notes_per_second: f64,
    /// Whether fast runs on one lane are kept as rolls (see `ROLL_GAP_MS`).
    pub rolls: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Thumb {
    Left,
    Right,
}

pub fn thumb(pad: Pad) -> Thumb {
    if pad.index() < 4 { Thumb::Left } else { Thumb::Right }
}

/// Opposite buttons under one thumb: never a chord.
pub fn opposite(a: Pad, b: Pad) -> bool {
    use Pad::*;
    matches!(
        (a, b),
        (P1, P2) | (P2, P1) | (P3, P4) | (P4, P3) | (P5, P7) | (P7, P5) | (P6, P8) | (P8, P6)
    )
}

/// Which hits survive thinning first: the backbone of the beat, then the hats,
/// then colour. Lower is more important.
pub fn priority(pad: Pad) -> u8 {
    match pad {
        Pad::P1 => 0,
        Pad::P2 => 1,
        Pad::P5 => 2,
        Pad::P7 => 3,
        Pad::P4 => 4,
        Pad::P8 => 5,
        Pad::P6 => 6,
        Pad::P3 => 7,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbs_and_opposites_follow_the_controller() {
        assert_eq!(thumb(Pad::P4), Thumb::Left);
        assert_eq!(thumb(Pad::P5), Thumb::Right);
        assert!(opposite(Pad::P1, Pad::P2) && opposite(Pad::P8, Pad::P6));
        assert!(!opposite(Pad::P1, Pad::P3) && !opposite(Pad::P5, Pad::P6));
    }

    #[test]
    fn difficulties_get_strictly_more_permissive() {
        for pair in Difficulty::ALL.windows(2) {
            let (easier, harder) = (pair[0].rules(), pair[1].rules());
            assert!(harder.pads.len() >= easier.pads.len());
            assert!(harder.min_same_thumb_ms < easier.min_same_thumb_ms);
            assert!(harder.max_notes_per_second > easier.max_notes_per_second);
            assert!(harder.max_chord >= easier.max_chord);
        }
    }
}
