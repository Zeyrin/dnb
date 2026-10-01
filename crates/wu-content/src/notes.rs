//! Note lines, for bass and melodic parts: `pitch:length` tokens on the 16th grid.
//!
//! ```text
//! F1:6 . . Ab1:4 C2:2 Bb1:2 | F1:16
//! F3+Ab3+C4:16 | Db3+F3+Ab3:16
//! ```
//!
//! - `F1:6` plays F1 for six 16th steps. Pitches are scientific (C4 = 60), so
//!   F1 is 43.7 Hz: sub territory.
//! - `F3+Ab3+C4:16` plays a chord: the three pitches together, for 16 steps.
//! - `.` rests for one step.
//! - `|` marks a bar line, and must fall on one: it catches miscounted bars.
//!   A note may run on across bar lines (`F1:32 |` holds for two bars).

use wu_time::STEPS_PER_BAR;

/// One note, in 16th steps from the start of the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteStep {
    pub step: i64,
    pub length: i64,
    /// MIDI key.
    pub key: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NoteError {
    #[error("\"{0}\" is not a note: write pitch:steps, like F1:4")]
    Token(String),
    #[error("\"{0}\" is not a pitch: a letter A–G, an optional # or b, an octave (C4 = middle C)")]
    Pitch(String),
    #[error("bar line after {steps} steps: bars are {STEPS_PER_BAR} steps long")]
    BarLine { steps: i64 },
}

/// Parses a note line; returns the notes and the total length in steps.
pub fn parse_notes(text: &str) -> Result<(Vec<NoteStep>, i64), NoteError> {
    let mut notes = Vec::new();
    let mut position = 0i64;
    for token in text.split_whitespace() {
        match token {
            "." => position += 1,
            "|" => {
                if position % STEPS_PER_BAR != 0 {
                    return Err(NoteError::BarLine { steps: position });
                }
            }
            _ => {
                let (pitches, length) = token
                    .split_once(':')
                    .ok_or_else(|| NoteError::Token(token.to_owned()))?;
                let length: i64 = length
                    .parse()
                    .ok()
                    .filter(|&l| l > 0)
                    .ok_or_else(|| NoteError::Token(token.to_owned()))?;
                for pitch in pitches.split('+') {
                    let key = parse_pitch(pitch).ok_or_else(|| NoteError::Pitch(pitch.to_owned()))?;
                    notes.push(NoteStep {
                        step: position,
                        length,
                        key,
                    });
                }
                position += length;
            }
        }
    }
    Ok((notes, position))
}

/// `"Ab1"` → 32. Scientific pitch: C4 = 60, so C-1 = 0.
pub fn parse_pitch(text: &str) -> Option<u8> {
    let mut chars = text.chars();
    let class = match chars.next()? {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest: String = chars.collect();
    let (accidental, octave) = match rest.as_bytes().first() {
        Some(b'#') => (1, &rest[1..]),
        Some(b'b') => (-1, &rest[1..]),
        _ => (0, rest.as_str()),
    };
    let octave: i32 = octave.parse().ok()?;
    u8::try_from(12 * (octave + 1) + class + accidental)
        .ok()
        .filter(|&key| key <= 127)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitches_are_scientific() {
        assert_eq!(parse_pitch("C4"), Some(60));
        assert_eq!(parse_pitch("A4"), Some(69));
        assert_eq!(parse_pitch("F1"), Some(29));
        assert_eq!(parse_pitch("Ab1"), Some(32));
        assert_eq!(parse_pitch("C#-1"), Some(1));
        assert_eq!(parse_pitch("Cb-1"), None);
        assert_eq!(parse_pitch("H2"), None);
        assert_eq!(parse_pitch("G9"), Some(127));
        assert_eq!(parse_pitch("A9"), None);
    }

    #[test]
    fn a_line_parses_into_notes_and_rests() {
        let (notes, steps) = parse_notes("F1:6 . . Ab1:4 C2:2 Bb1:2 | F1:16").expect("valid");
        assert_eq!(steps, 32);
        assert_eq!(
            notes[0],
            NoteStep {
                step: 0,
                length: 6,
                key: 29
            }
        );
        assert_eq!(
            notes[1],
            NoteStep {
                step: 8,
                length: 4,
                key: 32
            }
        );
        assert_eq!(
            notes[4],
            NoteStep {
                step: 16,
                length: 16,
                key: 29
            }
        );
    }

    #[test]
    fn notes_may_run_across_bar_lines_but_bar_lines_must_land_on_bars() {
        assert!(parse_notes("F1:32 | Eb1:16").is_ok());
        assert_eq!(parse_notes("F1:6 . | F1:16"), Err(NoteError::BarLine { steps: 7 }));
        assert_eq!(parse_notes("F1"), Err(NoteError::Token("F1".into())));
        assert_eq!(parse_notes("F1:0"), Err(NoteError::Token("F1:0".into())));
        assert_eq!(parse_notes("Q1:4"), Err(NoteError::Pitch("Q1".into())));
    }

    #[test]
    fn chords_sound_their_pitches_together() {
        let (notes, steps) = parse_notes("F3+Ab3+C4:8 . . . . . . . . | Db3+F3+Ab3:16").expect("valid");
        assert_eq!(steps, 32);
        let keys: Vec<(i64, u8)> = notes.iter().map(|n| (n.step, n.key)).collect();
        assert_eq!(keys, vec![(0, 53), (0, 56), (0, 60), (16, 49), (16, 53), (16, 56)]);
        assert!(notes.iter().all(|n| n.length == 8 || n.length == 16));
        assert_eq!(parse_notes("F3+:4"), Err(NoteError::Pitch(String::new())));
    }
}
