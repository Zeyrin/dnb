//! Parameter smoothing, so knob moves never click.

/// Moves toward its target exponentially: about 63 % of the way per time constant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Smoothed {
    current: f32,
    target: f32,
    coeff: f32,
}

impl Smoothed {
    pub fn new(value: f32, time_constant_s: f32, sample_rate: u32) -> Smoothed {
        let coeff = 1.0 - (-1.0 / (time_constant_s.max(1e-6) * sample_rate as f32)).exp();
        Smoothed {
            current: value,
            target: value,
            coeff,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Jumps straight to `value`.
    pub fn snap(&mut self, value: f32) {
        self.current = value;
        self.target = value;
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Advances one sample and returns the new value.
    pub fn step(&mut self) -> f32 {
        self.current += (self.target - self.current) * self.coeff;
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaches_the_target_without_overshoot() {
        let mut s = Smoothed::new(0.0, 0.005, 48_000);
        s.set_target(1.0);
        let mut last = 0.0;
        for _ in 0..4_800 {
            let v = s.step();
            assert!(v >= last && v <= 1.0);
            last = v;
        }
        assert!((last - 1.0).abs() < 1e-3);
    }
}
