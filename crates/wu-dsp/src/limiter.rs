//! A look-ahead true-peak limiter, the master's last stage. The audio is
//! delayed by the look-ahead, so the limiter sees each peak coming, turns the
//! gain down smoothly before it arrives, and lets it back up slowly. Nothing
//! crosses the ceiling, between samples included, and nothing clicks.

use std::collections::VecDeque;

use crate::shape::{db_to_gain, gain_to_db};
use crate::truepeak::TruePeak;

#[derive(Clone, Debug)]
pub struct Limiter {
    ceiling: f32,
    lookahead: usize,
    detectors: [TruePeak; 2],
    frame: u64,
    /// Sliding minimum of the gains each frame wants, over the look-ahead:
    /// (frame, gain), gains rising from front to back.
    minima: VecDeque<(u64, f32)>,
    /// The last `lookahead` minima, averaged so the gain glides down.
    held: Vec<f32>,
    held_pos: usize,
    held_sum: f64,
    release: f32,
    gain: f32,
    delay: Vec<[f32; 2]>,
    delay_pos: usize,
}

impl Limiter {
    pub fn new(sample_rate: u32, ceiling_db: f32, lookahead_s: f32, release_s: f32) -> Limiter {
        let sr = sample_rate as f32;
        let lookahead = ((lookahead_s * sr).round() as usize).max(1);
        Limiter {
            ceiling: db_to_gain(ceiling_db),
            lookahead,
            detectors: [TruePeak::new(), TruePeak::new()],
            frame: 0,
            minima: VecDeque::with_capacity(lookahead),
            held: vec![1.0; lookahead],
            held_pos: 0,
            held_sum: lookahead as f64,
            release: 1.0 - (-1.0 / (release_s.max(1e-4) * sr)).exp(),
            gain: 1.0,
            delay: vec![[0.0; 2]; TruePeak::LATENCY + lookahead - 1],
            delay_pos: 0,
        }
    }

    /// Frames the audio comes out later than it went in.
    pub fn latency(&self) -> usize {
        self.delay.len()
    }

    /// How far the gain is turned down right now, in dB (0 when idle).
    pub fn reduction_db(&self) -> f32 {
        -gain_to_db(self.gain)
    }

    /// Takes one stereo frame, returns the frame `latency()` before it, limited.
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        // The detectors report the peak around the frame `TruePeak::LATENCY` back.
        let peak = self.detectors[0].push(left).max(self.detectors[1].push(right));
        let wanted = if peak > self.ceiling { self.ceiling / peak } else { 1.0 };

        let frame = self.frame;
        self.frame += 1;
        let horizon = self.lookahead as u64;
        while self.minima.front().is_some_and(|&(at, _)| at + horizon <= frame) {
            self.minima.pop_front();
        }
        while self.minima.back().is_some_and(|&(_, gain)| gain >= wanted) {
            self.minima.pop_back();
        }
        // At most `lookahead` entries remain, so this never reallocates.
        self.minima.push_back((frame, wanted));
        let floor = self.minima.front().map_or(1.0, |&(_, gain)| gain);

        // Averaging the floor over the look-ahead reaches each peak's gain exactly
        // when that peak leaves the delay line, and never jumps.
        let oldest = std::mem::replace(&mut self.held[self.held_pos], floor);
        self.held_pos = (self.held_pos + 1) % self.lookahead;
        self.held_sum += f64::from(floor) - f64::from(oldest);
        let smooth = (self.held_sum / self.lookahead as f64) as f32;

        self.gain = if smooth < self.gain {
            smooth
        } else if smooth >= 1.0 && self.gain > 1.0 - 1e-6 {
            // Settle on exactly 1, so quiet audio passes bit for bit.
            1.0
        } else {
            self.gain + (smooth - self.gain) * self.release
        };

        let [l, r] = std::mem::replace(&mut self.delay[self.delay_pos], [left, right]);
        self.delay_pos = (self.delay_pos + 1) % self.delay.len();
        (l * self.gain, r * self.gain)
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use proptest::prelude::*;

    use super::*;
    use crate::rng::Rng;

    const SR: u32 = 48_000;

    fn limiter() -> Limiter {
        Limiter::new(SR, -1.0, 0.0015, 0.1)
    }

    /// Runs mono audio through both channels; returns the output, latency removed.
    fn run(limiter: &mut Limiter, input: &[f32]) -> Vec<f32> {
        let latency = limiter.latency();
        let mut out = Vec::with_capacity(input.len());
        for i in 0..input.len() + latency {
            let x = input.get(i).copied().unwrap_or(0.0);
            let (l, _) = limiter.process(x, x);
            if i >= latency {
                out.push(l);
            }
        }
        out
    }

    fn true_peak(audio: &[f32]) -> f32 {
        let mut detector = TruePeak::new();
        let peak = audio.iter().fold(0.0f32, |p, &x| p.max(detector.push(x)));
        peak.max(detector.flush())
    }

    #[test]
    fn quiet_audio_passes_untouched_after_the_latency() {
        let mut limiter = limiter();
        let input: Vec<f32> = (0..4_800)
            .map(|i| 0.5 * (TAU * 440.0 * i as f32 / 48_000.0).sin())
            .collect();
        assert_eq!(run(&mut limiter, &input), input);
        assert_eq!(limiter.reduction_db(), 0.0);
        assert_eq!(
            limiter.latency(),
            TruePeak::LATENCY + 72 - 1,
            "1.5 ms of look-ahead at 48 kHz"
        );
    }

    #[test]
    fn a_hot_drum_hit_is_held_under_the_ceiling_without_a_jump() {
        let mut rng = Rng::new(7);
        // Silence, a decaying noise burst peaking 12 dB over full scale, then half a
        // second of silence for the gain to come back.
        let input: Vec<f32> = (0..33_600)
            .map(|i| {
                if !(2_000..9_600).contains(&i) {
                    0.0
                } else {
                    4.0 * (rng.next_f32() * 2.0 - 1.0) * (-((i - 2_000) as f32) / 1_500.0).exp()
                }
            })
            .collect();
        let mut limiter = limiter();
        let output = run(&mut limiter, &input);
        let ceiling = db_to_gain(-1.0);
        assert!(output.iter().all(|x| x.abs() <= ceiling * 1.000_01));
        assert!(
            true_peak(&output) <= db_to_gain(-0.9),
            "true peak {}",
            true_peak(&output)
        );
        assert!(limiter.reduction_db() < 0.5, "the gain comes back up afterwards");
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        #[test]
        fn nothing_ever_crosses_the_ceiling(seed in 1u64..u64::MAX, level in 0.1f32..8.0, sparse in 0usize..4) {
            let mut rng = Rng::new(seed);
            let input: Vec<f32> = (0..6_000)
                .map(|i| if sparse > 0 && i % (sparse * 97) != 0 { 0.0 } else { level * (rng.next_f32() * 2.0 - 1.0) })
                .collect();
            let mut limiter = limiter();
            let output = run(&mut limiter, &input);
            let ceiling = db_to_gain(-1.0);
            prop_assert!(output.iter().all(|x| x.abs() <= ceiling * 1.000_01));
        }
    }
}
