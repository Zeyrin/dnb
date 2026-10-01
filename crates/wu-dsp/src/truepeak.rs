//! True peak: how far a signal swings between its samples once a converter
//! draws the continuous wave (ITU-R BS.1770-4, Annex 2), estimated by 4×
//! oversampling. A bright snare or a crushed break can peak a decibel or more
//! above its highest sample.

use std::f64::consts::PI;

/// Taps per interpolated point. With a Blackman window, 16 keeps the estimate
/// within a few hundredths of a decibel up to a quarter of the sample rate.
const TAPS: usize = 16;
/// Points interpolated between two samples: 4× oversampling.
const POINTS: usize = 3;

#[derive(Clone, Debug)]
pub struct TruePeak {
    /// Windowed-sinc kernels for the points ¼, ½ and ¾ of the way to the next sample.
    kernels: [[f32; TAPS]; POINTS],
    /// The last `TAPS` samples, written twice so they always sit in one slice.
    history: [f32; 2 * TAPS],
    pos: usize,
}

impl Default for TruePeak {
    fn default() -> TruePeak {
        TruePeak::new()
    }
}

impl TruePeak {
    /// How many samples behind the input the reported peak is: the kernels need
    /// that many samples after a point to draw it.
    pub const LATENCY: usize = TAPS / 2;

    pub fn new() -> TruePeak {
        let half = (TAPS / 2) as f64;
        let kernels = std::array::from_fn(|point| {
            let mu = (point + 1) as f64 / (POINTS + 1) as f64;
            let kernel: [f64; TAPS] = std::array::from_fn(|tap| {
                // The point sits between taps TAPS/2 - 1 and TAPS/2.
                let d = tap as f64 - (half - 1.0 + mu);
                let sinc = (PI * d).sin() / (PI * d);
                let window = 0.42 + 0.5 * (PI * d / half).cos() + 0.08 * (2.0 * PI * d / half).cos();
                sinc * window
            });
            // Unity gain at DC, whatever the window did to the sum.
            let sum: f64 = kernel.iter().sum();
            kernel.map(|k| (k / sum) as f32)
        });
        TruePeak {
            kernels,
            history: [0.0; 2 * TAPS],
            pos: 0,
        }
    }

    /// Takes the next sample. Returns the highest absolute level from the sample
    /// `LATENCY` back up to, but not including, the one after it.
    pub fn push(&mut self, x: f32) -> f32 {
        self.history[self.pos] = x;
        self.history[self.pos + TAPS] = x;
        self.pos = (self.pos + 1) % TAPS;
        let window = &self.history[self.pos..self.pos + TAPS];
        let mut peak = window[TAPS / 2 - 1].abs();
        for kernel in &self.kernels {
            let y: f32 = kernel.iter().zip(window).map(|(k, s)| k * s).sum();
            peak = peak.max(y.abs());
        }
        peak
    }

    /// The peaks of the last `LATENCY` samples, as if silence followed them.
    pub fn flush(&mut self) -> f32 {
        (0..Self::LATENCY).fold(0.0f32, |peak, _| peak.max(self.push(0.0)))
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_4, TAU};

    use super::*;

    const SR: f64 = 48_000.0;

    fn measure(freq: f64, phase: f64, amplitude: f32) -> (f32, f32) {
        let mut detector = TruePeak::new();
        let (mut sample_peak, mut true_peak) = (0.0f32, 0.0f32);
        for i in 0..4_800 {
            // In f64: an f32 phase drifts by thousandths of a radian over 4 800 samples.
            let x = amplitude * (TAU * freq * i as f64 / SR + phase).sin() as f32;
            sample_peak = sample_peak.max(x.abs());
            // Skip the start-up: the history begins as silence.
            let peak = detector.push(x);
            if i > 2 * TAPS {
                true_peak = true_peak.max(peak);
            }
        }
        (sample_peak, true_peak)
    }

    #[test]
    fn finds_the_peak_a_quarter_rate_sine_hides_between_samples() {
        // At fs/4 with a 45° phase, every sample lands at ±0.707 of the crest.
        let (sample_peak, true_peak) = measure(12_000.0, FRAC_PI_4, 0.5);
        assert!((sample_peak - 0.5 * FRAC_PI_4.cos() as f32).abs() < 1e-4);
        assert!((true_peak - 0.5).abs() < 0.5 * 0.01, "true peak {true_peak}");
    }

    #[test]
    fn never_reads_much_above_the_real_crest() {
        for freq in [50.0, 997.0, 5_000.0, 9_000.0, 12_000.0] {
            for phase in [0.0, 0.3, 1.1, 2.0] {
                let (_, true_peak) = measure(freq, phase, 0.8);
                // Within 0.1 dB above, and at most the 4× grid's 0.17 dB below.
                assert!(true_peak < 0.8 * 1.012, "{freq} Hz: {true_peak}");
                assert!(true_peak > 0.8 * 0.98, "{freq} Hz: {true_peak}");
            }
        }
    }

    #[test]
    fn a_lone_click_peaks_at_its_own_height_and_flush_finds_it() {
        let mut detector = TruePeak::new();
        assert!(detector.push(0.6) < 0.01, "reported LATENCY samples later");
        assert!((detector.flush() - 0.6).abs() < 1e-6);
    }
}
