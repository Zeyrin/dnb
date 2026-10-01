//! Level and pitch conversions, and saturation.

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.max(1e-12).log10()
}

/// Frequency of a MIDI note number (69 = A4 = 440 Hz).
pub fn midi_to_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// A smooth saturator that behaves like `tanh` near zero and never exceeds ±1.
pub fn soft_clip(x: f32) -> f32 {
    if x <= -3.0 {
        -1.0
    } else if x >= 3.0 {
        1.0
    } else {
        // Padé approximant of tanh, monotonic, joining the clamp smoothly at ±3;
        // within 0.025 of tanh everywhere.
        // Clamped too: f32 rounding can overshoot ±1 by an ulp just inside ±3.
        (x * (27.0 + x * x) / (27.0 + 9.0 * x * x)).clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibels_round_trip() {
        for db in [-60.0, -12.0, -6.0, 0.0, 6.0] {
            assert!((gain_to_db(db_to_gain(db)) - db).abs() < 1e-4);
        }
        assert!((db_to_gain(-6.0) - 0.501).abs() < 1e-3);
    }

    #[test]
    fn a4_is_440() {
        assert!((midi_to_hz(69.0) - 440.0).abs() < 1e-3);
        assert!((midi_to_hz(29.0) - 43.65).abs() < 0.01, "F1, sub territory");
    }

    #[test]
    fn soft_clip_is_bounded_monotonic_and_near_tanh() {
        let mut prev = -2.0;
        for i in -500..=500 {
            let x = i as f32 / 100.0;
            let y = soft_clip(x);
            assert!((-1.0..=1.0).contains(&y));
            assert!(y >= prev);
            prev = y;
            assert!((y - x.tanh()).abs() < 0.03, "x = {x}");
        }
    }
}
