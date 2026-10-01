//! Loudness as broadcasters and streaming services measure it (ITU-R BS.1770-4,
//! EBU R128): K-weighted, gated, in LUFS.
//!
//! [`LoudnessMeter`] is an offline tool: it keeps one number per 100 ms it has
//! measured, so unlike the rest of this crate it allocates as it goes.

use std::f64::consts::PI;

use crate::shape::gain_to_db;
use crate::truepeak::TruePeak;

/// A second-order section (transposed direct form II), in `f64`: the
/// K-weighting high-pass sits at 38 Hz, where `f32` coefficients lose too much.
#[derive(Clone, Debug)]
pub struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    /// `b` is the numerator, `a` the denominator without its leading 1.
    pub fn new(b: [f64; 3], a: [f64; 2]) -> Biquad {
        Biquad { b, a, z: [0.0; 2] }
    }

    pub fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// The K-weighting curve: a +4 dB shelf above about 1.5 kHz (the head), then a
/// high-pass at 38 Hz (what ears make of the lowest lows).
#[derive(Clone, Debug)]
pub struct KWeighting {
    shelf: Biquad,
    high_pass: Biquad,
}

impl KWeighting {
    /// The two filters of BS.1770, derived for any sample rate (the analogue
    /// parameters are the ones libebur128 fits to the standard's 48 kHz coefficients).
    pub fn new(sample_rate: u32) -> KWeighting {
        let fs = f64::from(sample_rate);
        let (f0, gain_db, q) = (1_681.974_450_955_533, 3.999_843_853_973_347, 0.707_175_236_955_419_6);
        let k = (PI * f0 / fs).tan();
        let vh = 10f64.powf(gain_db / 20.0);
        let vb = vh.powf(0.499_666_774_154_541_6);
        let a0 = 1.0 + k / q + k * k;
        let shelf = Biquad::new(
            [
                (vh + vb * k / q + k * k) / a0,
                2.0 * (k * k - vh) / a0,
                (vh - vb * k / q + k * k) / a0,
            ],
            [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        );
        let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
        let k = (PI * f0 / fs).tan();
        let a0 = 1.0 + k / q + k * k;
        let high_pass = Biquad::new([1.0, -2.0, 1.0], [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0]);
        KWeighting { shelf, high_pass }
    }

    pub fn process(&mut self, x: f64) -> f64 {
        self.high_pass.process(self.shelf.process(x))
    }
}

/// Below this, a block is silence and doesn't count.
const ABSOLUTE_GATE_LUFS: f64 = -70.0;
/// Blocks this far below the song's level don't count either: fades and gaps
/// would drag the number down.
const RELATIVE_GATE_LU: f64 = -10.0;
/// Measurement steps of 100 ms: a 400 ms block is four, a 3 s window thirty.
const BLOCK_STEPS: usize = 4;
const SHORT_TERM_STEPS: usize = 30;

/// Loudness of a mean square summed over the channels.
fn lufs(power: f64) -> f64 {
    -0.691 + 10.0 * power.max(1e-20).log10()
}

/// Measures stereo audio: integrated loudness (gated), the loudest moments, and
/// the true and sample peaks.
#[derive(Clone, Debug)]
pub struct LoudnessMeter {
    weighting: [KWeighting; 2],
    detectors: [TruePeak; 2],
    step_frames: usize,
    in_step: usize,
    step_sum: [f64; 2],
    /// Mean square of each complete 100 ms step, summed over the channels.
    steps: Vec<f64>,
    true_peak: f32,
    sample_peak: f32,
}

impl LoudnessMeter {
    pub fn new(sample_rate: u32) -> LoudnessMeter {
        LoudnessMeter {
            weighting: [KWeighting::new(sample_rate), KWeighting::new(sample_rate)],
            detectors: [TruePeak::new(), TruePeak::new()],
            step_frames: (sample_rate as usize / 10).max(1),
            in_step: 0,
            step_sum: [0.0; 2],
            steps: Vec::new(),
            true_peak: 0.0,
            sample_peak: 0.0,
        }
    }

    /// Measures more interleaved stereo.
    pub fn process(&mut self, interleaved: &[f32]) {
        let (frames, _) = interleaved.as_chunks::<2>();
        for frame in frames {
            for (channel, &x) in frame.iter().enumerate() {
                self.sample_peak = self.sample_peak.max(x.abs());
                self.true_peak = self.true_peak.max(self.detectors[channel].push(x));
                let y = self.weighting[channel].process(f64::from(x));
                self.step_sum[channel] += y * y;
            }
            self.in_step += 1;
            if self.in_step == self.step_frames {
                let power = (self.step_sum[0] + self.step_sum[1]) / self.step_frames as f64;
                self.steps.push(power);
                self.step_sum = [0.0; 2];
                self.in_step = 0;
            }
        }
    }

    /// Mean squares of every window of `steps` steps, one step apart.
    fn windows(&self, steps: usize) -> impl Iterator<Item = f64> + '_ {
        self.steps
            .windows(steps)
            .map(move |w| w.iter().sum::<f64>() / steps as f64)
    }

    /// Integrated loudness in LUFS, gated as BS.1770-4 says; `None` until there
    /// is a 400 ms block above silence.
    pub fn integrated(&self) -> Option<f64> {
        let mean = |powers: &[f64]| powers.iter().sum::<f64>() / powers.len() as f64;
        let audible: Vec<f64> = self
            .windows(BLOCK_STEPS)
            .filter(|&p| lufs(p) > ABSOLUTE_GATE_LUFS)
            .collect();
        if audible.is_empty() {
            return None;
        }
        let gate = lufs(mean(&audible)) + RELATIVE_GATE_LU;
        let counted: Vec<f64> = audible.into_iter().filter(|&p| lufs(p) > gate).collect();
        // Never empty: the loudest block is louder than the mean.
        Some(lufs(mean(&counted)))
    }

    /// The loudest 400 ms block, in LUFS.
    pub fn max_momentary(&self) -> Option<f64> {
        self.windows(BLOCK_STEPS).map(lufs).reduce(f64::max)
    }

    /// The loudest 3 s window, in LUFS.
    pub fn max_short_term(&self) -> Option<f64> {
        self.windows(SHORT_TERM_STEPS).map(lufs).reduce(f64::max)
    }

    /// The highest true peak so far, in dBTP (decibels relative to full scale).
    pub fn true_peak_db(&self) -> f32 {
        // The detectors still hold the last few samples: draw them as if silence followed.
        let tail = self
            .detectors
            .clone()
            .iter_mut()
            .fold(0.0f32, |peak, d| peak.max(d.flush()));
        gain_to_db(self.true_peak.max(tail))
    }

    pub fn sample_peak_db(&self) -> f32 {
        gain_to_db(self.sample_peak)
    }

    /// Seconds measured, in whole 100 ms steps.
    pub fn seconds(&self) -> f64 {
        self.steps.len() as f64 / 10.0
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::shape::db_to_gain;

    const SR: u32 = 48_000;

    /// A stereo 1 kHz sine at `db` (peak) in each channel, `seconds` long.
    fn tone(db: f32, seconds: f32) -> Vec<f32> {
        let amplitude = db_to_gain(db);
        let frames = (seconds * SR as f32) as usize;
        (0..frames)
            .flat_map(|i| {
                let x = amplitude * (TAU * 1_000.0 * i as f32 / SR as f32).sin();
                [x, x]
            })
            .collect()
    }

    fn integrated(parts: &[(f32, f32)]) -> f64 {
        let mut meter = LoudnessMeter::new(SR);
        for &(db, seconds) in parts {
            meter.process(&tone(db, seconds));
        }
        meter.integrated().expect("audible")
    }

    #[test]
    fn the_k_weighting_matches_the_standard_at_48k() {
        let k = KWeighting::new(48_000);
        let expected_shelf = (
            [1.535_124_859_586_97, -2.691_696_189_406_38, 1.198_392_810_852_85],
            [-1.690_659_293_182_41, 0.732_480_774_215_85],
        );
        for (got, want) in k.shelf.b.iter().zip(expected_shelf.0) {
            assert!((got - want).abs() < 1e-6, "{got} vs {want}");
        }
        for (got, want) in k.shelf.a.iter().zip(expected_shelf.1) {
            assert!((got - want).abs() < 1e-6, "{got} vs {want}");
        }
        let expected_high_pass = [-1.990_047_454_833_98, 0.990_072_250_366_21];
        for (got, want) in k.high_pass.a.iter().zip(expected_high_pass) {
            assert!((got - want).abs() < 1e-6, "{got} vs {want}");
        }
    }

    // The EBU Tech 3341 test signals for integrated loudness.

    #[test]
    fn a_minus_23_dbfs_tone_reads_minus_23_lufs() {
        assert!((integrated(&[(-23.0, 20.0)]) + 23.0).abs() < 0.1);
        assert!((integrated(&[(-33.0, 20.0)]) + 33.0).abs() < 0.1);
    }

    #[test]
    fn the_relative_gate_ignores_quiet_passages() {
        let level = integrated(&[(-36.0, 10.0), (-23.0, 60.0), (-36.0, 10.0)]);
        assert!((level + 23.0).abs() < 0.1, "{level}");
    }

    #[test]
    fn the_absolute_gate_ignores_near_silence() {
        let level = integrated(&[
            (-72.0, 10.0),
            (-36.0, 10.0),
            (-23.0, 60.0),
            (-36.0, 10.0),
            (-72.0, 10.0),
        ]);
        assert!((level + 23.0).abs() < 0.1, "{level}");
    }

    #[test]
    fn silence_has_no_loudness() {
        let mut meter = LoudnessMeter::new(SR);
        meter.process(&vec![0.0; 2 * SR as usize]);
        assert_eq!(meter.integrated(), None);
        assert!(meter.true_peak_db() < -200.0);
    }

    #[test]
    fn peaks_and_the_loudest_moments_are_reported() {
        let mut meter = LoudnessMeter::new(SR);
        meter.process(&tone(-30.0, 5.0));
        meter.process(&tone(-10.0, 5.0));
        assert!((meter.sample_peak_db() + 10.0).abs() < 0.01);
        assert!((meter.true_peak_db() + 10.0).abs() < 0.05);
        assert!((meter.max_momentary().expect("blocks") + 10.0).abs() < 0.1);
        assert!((meter.max_short_term().expect("windows") + 10.0).abs() < 0.1);
        assert!((meter.seconds() - 10.0).abs() < 1e-9);
    }
}
