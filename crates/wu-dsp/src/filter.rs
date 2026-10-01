//! Filters.

use std::f32::consts::PI;

/// The three simultaneous outputs of a state-variable filter.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SvfOut {
    pub low: f32,
    pub band: f32,
    pub high: f32,
}

/// Zero-delay-feedback state-variable filter (Andrew Simper's trapezoidal form):
/// stable under fast cutoff sweeps, which the filter-sweep FX and wobble basses need.
#[derive(Clone, Debug)]
pub struct Svf {
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    ic1: f32,
    ic2: f32,
}

impl Svf {
    pub fn new(cutoff_hz: f32, q: f32, sample_rate: u32) -> Svf {
        let mut svf = Svf {
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            ic1: 0.0,
            ic2: 0.0,
        };
        svf.set(cutoff_hz, q, sample_rate);
        svf
    }

    /// Retunes without clearing the state, so sweeps stay click-free.
    pub fn set(&mut self, cutoff_hz: f32, q: f32, sample_rate: u32) {
        let nyquist_guard = 0.49 * sample_rate as f32;
        let fc = cutoff_hz.clamp(5.0, nyquist_guard);
        let g = (PI * fc / sample_rate as f32).tan();
        self.k = 1.0 / q.max(0.05);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    pub fn process(&mut self, x: f32) -> SvfOut {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        SvfOut {
            low: v2,
            band: v1,
            high: x - self.k * v1 - v2,
        }
    }

    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

/// A one-pole low-pass; `high` is its complement.
#[derive(Clone, Debug)]
pub struct OnePole {
    a: f32,
    y: f32,
}

impl OnePole {
    pub fn new(cutoff_hz: f32, sample_rate: u32) -> OnePole {
        let mut f = OnePole { a: 0.0, y: 0.0 };
        f.set(cutoff_hz, sample_rate);
        f
    }

    pub fn set(&mut self, cutoff_hz: f32, sample_rate: u32) {
        self.a = 1.0 - (-2.0 * PI * cutoff_hz.max(0.0) / sample_rate as f32).exp();
    }

    pub fn low(&mut self, x: f32) -> f32 {
        self.y += self.a * (x - self.y);
        self.y
    }

    pub fn high(&mut self, x: f32) -> f32 {
        x - self.low(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// RMS of the filter's response to a sine, after it settles.
    fn response(freq: f32, mut filter: impl FnMut(f32) -> f32) -> f32 {
        let n = SR as usize;
        let mut sum = 0.0f64;
        for i in 0..n {
            let x = (2.0 * PI * freq * i as f32 / SR as f32).sin();
            let y = filter(x);
            if i > n / 2 {
                sum += f64::from(y * y);
            }
        }
        ((sum / (n / 2) as f64).sqrt() * std::f64::consts::SQRT_2) as f32
    }

    #[test]
    fn svf_low_pass_keeps_lows_and_cuts_highs() {
        let mut svf = Svf::new(1000.0, 0.707, SR);
        assert!((response(100.0, |x| svf.process(x).low) - 1.0).abs() < 0.02);
        let mut svf = Svf::new(1000.0, 0.707, SR);
        assert!(response(10_000.0, |x| svf.process(x).low) < 0.02);
    }

    #[test]
    fn svf_high_pass_cuts_lows() {
        let mut svf = Svf::new(5000.0, 0.707, SR);
        assert!(response(200.0, |x| svf.process(x).high) < 0.01);
        let mut svf = Svf::new(5000.0, 0.707, SR);
        assert!((response(15_000.0, |x| svf.process(x).high) - 1.0).abs() < 0.1);
    }

    #[test]
    fn svf_survives_cutoff_at_nyquist() {
        let mut svf = Svf::new(1e9, 10.0, SR);
        for i in 0..1000 {
            assert!(svf.process(if i % 2 == 0 { 1.0 } else { -1.0 }).low.is_finite());
        }
    }

    #[test]
    fn one_pole_passes_dc_and_its_complement_blocks_it() {
        let mut lp = OnePole::new(100.0, SR);
        let mut hp = OnePole::new(100.0, SR);
        let (mut l, mut h) = (0.0, 0.0);
        for _ in 0..SR {
            l = lp.low(1.0);
            h = hp.high(1.0);
        }
        assert!((l - 1.0).abs() < 1e-4);
        assert!(h.abs() < 1e-4);
    }
}
