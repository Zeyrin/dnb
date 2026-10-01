//! Oscillators.

/// A phase accumulator in [0, 1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Phase(pub f64);

impl Phase {
    /// Returns the current phase, then advances by `freq / sample_rate`.
    pub fn tick(&mut self, freq: f64, sample_rate: f64) -> f64 {
        let current = self.0;
        self.0 = (self.0 + freq / sample_rate).rem_euclid(1.0);
        current
    }
}

/// PolyBLEP correction around a discontinuity at phase 0.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// Band-limited sawtooth: phase `t` in [0, 1), `dt` = frequency / sample rate.
pub fn polyblep_saw(t: f32, dt: f32) -> f32 {
    2.0 * t - 1.0 - poly_blep(t, dt)
}

/// Band-limited square, 50 % duty.
pub fn polyblep_square(t: f32, dt: f32) -> f32 {
    let naive = if t < 0.5 { 1.0 } else { -1.0 };
    naive + poly_blep(t, dt) - poly_blep((t + 0.5).fract(), dt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_wraps() {
        let mut p = Phase::default();
        for _ in 0..10 {
            p.tick(12_345.0, 48_000.0);
        }
        assert!((0.0..1.0).contains(&p.0));
    }

    #[test]
    fn band_limited_waves_are_bounded_and_centred() {
        let (freq, sr) = (440.0f64, 48_000.0f64);
        let dt = (freq / sr) as f32;
        let mut p = Phase::default();
        let (mut sum_saw, mut sum_sq) = (0.0f64, 0.0f64);
        let n = 48_000;
        for _ in 0..n {
            let t = p.tick(freq, sr) as f32;
            let (saw, sq) = (polyblep_saw(t, dt), polyblep_square(t, dt));
            assert!(saw.abs() <= 1.1 && sq.abs() <= 1.1);
            sum_saw += f64::from(saw);
            sum_sq += f64::from(sq);
        }
        assert!((sum_saw / f64::from(n)).abs() < 0.01);
        assert!((sum_sq / f64::from(n)).abs() < 0.01);
    }
}
