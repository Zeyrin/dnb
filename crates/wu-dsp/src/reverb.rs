//! A stereo reverb: a feedback delay network of eight lines mixed by a
//! Hadamard matrix, behind a pre-delay and four diffusing all-passes. Each line
//! loses level so the tail falls 60 dB in the decay time, and high frequencies
//! a little faster, like a real room.

use std::f32::consts::FRAC_1_SQRT_2;

use crate::filter::OnePole;

/// Line lengths in milliseconds: mutually prime at 48 kHz, so their echoes
/// never pile up on the same frames.
const LINES_MS: [f32; 8] = [29.73, 37.02, 41.10, 43.73, 53.27, 59.98, 67.73, 73.15];
/// The input diffusers, in milliseconds, and how much each smears.
const DIFFUSERS_MS: [f32; 4] = [4.77, 3.59, 12.73, 9.30];
const DIFFUSION: f32 = 0.62;
/// Lows under this stay out of the tail, so it never muddies the bass.
const LOW_CUT_HZ: f32 = 180.0;

#[derive(Clone, Debug)]
struct Line {
    buffer: Vec<f32>,
    pos: usize,
}

impl Line {
    fn new(frames: usize) -> Line {
        Line {
            buffer: vec![0.0; frames.max(1)],
            pos: 0,
        }
    }

    /// The sample written a whole line length ago.
    fn read(&self) -> f32 {
        self.buffer[self.pos]
    }

    fn write(&mut self, x: f32) {
        self.buffer[self.pos] = x;
        self.pos += 1;
        if self.pos == self.buffer.len() {
            self.pos = 0;
        }
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
    }
}

/// A Schroeder all-pass: flat in level, smeared in time.
#[derive(Clone, Debug)]
struct AllPass {
    line: Line,
    gain: f32,
}

impl AllPass {
    fn process(&mut self, x: f32) -> f32 {
        let delayed = self.line.read();
        let v = x + self.gain * delayed;
        self.line.write(v);
        delayed - self.gain * v
    }
}

/// In-place fast Walsh–Hadamard transform, scaled to keep energy: the
/// network's mixing matrix.
fn hadamard(x: &mut [f32; 8]) {
    let mut span = 1;
    while span < 8 {
        for start in (0..8).step_by(span * 2) {
            for i in start..start + span {
                let (a, b) = (x[i], x[i + span]);
                x[i] = (a + b) * FRAC_1_SQRT_2;
                x[i + span] = (a - b) * FRAC_1_SQRT_2;
            }
        }
        span *= 2;
    }
}

#[derive(Clone, Debug)]
pub struct Reverb {
    sample_rate: u32,
    predelay: Line,
    low_cut: OnePole,
    diffusers: [AllPass; 4],
    lines: [Line; 8],
    damping: [OnePole; 8],
    gains: [f32; 8],
}

impl Reverb {
    /// `decay_s` is the time the tail takes to fall 60 dB; highs above
    /// `damping_hz` die away sooner.
    pub fn new(sample_rate: u32, predelay_ms: f32, decay_s: f32, damping_hz: f32) -> Reverb {
        let frames = |ms: f32| (ms * sample_rate as f32 / 1000.0).round() as usize;
        let mut reverb = Reverb {
            sample_rate,
            predelay: Line::new(frames(predelay_ms)),
            low_cut: OnePole::new(LOW_CUT_HZ, sample_rate),
            diffusers: DIFFUSERS_MS.map(|ms| AllPass {
                line: Line::new(frames(ms)),
                gain: DIFFUSION,
            }),
            lines: LINES_MS.map(|ms| Line::new(frames(ms))),
            damping: [(); 8].map(|_| OnePole::new(damping_hz, sample_rate)),
            gains: [0.0; 8],
        };
        reverb.set_decay(decay_s);
        reverb
    }

    /// Sets how long the tail takes to fall 60 dB.
    pub fn set_decay(&mut self, decay_s: f32) {
        let decay = decay_s.max(0.05) * self.sample_rate as f32;
        for (gain, line) in self.gains.iter_mut().zip(&self.lines) {
            // Each trip round a line loses its share of the 60 dB.
            *gain = 10f32.powf(-3.0 * line.buffer.len() as f32 / decay);
        }
    }

    /// Takes a stereo sample in, returns the reverb only (no dry signal).
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let input = 0.5 * (left + right);
        let delayed = self.predelay.read();
        self.predelay.write(input);
        let mut x = self.low_cut.high(delayed);
        for diffuser in &mut self.diffusers {
            x = diffuser.process(x);
        }
        let mut taps = [0.0f32; 8];
        for (tap, line) in taps.iter_mut().zip(&self.lines) {
            *tap = line.read();
        }
        // Two different sums of the same lines: a wide, uncorrelated pair.
        let out_l = taps[0] - taps[2] + taps[4] - taps[6] + taps[1] - taps[5];
        let out_r = taps[1] - taps[3] + taps[5] - taps[7] + taps[2] - taps[4];
        let mut feedback = taps;
        hadamard(&mut feedback);
        for (i, line) in self.lines.iter_mut().enumerate() {
            let damped = self.damping[i].low(feedback[i] * self.gains[i]);
            line.write(damped + x);
        }
        (0.35 * out_l, 0.35 * out_r)
    }

    /// Silences the tail at once.
    pub fn clear(&mut self) {
        self.predelay.clear();
        for diffuser in &mut self.diffusers {
            diffuser.line.clear();
        }
        for line in &mut self.lines {
            line.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// RMS of the left output over each 100 ms after an impulse.
    fn tail(reverb: &mut Reverb, windows: usize) -> Vec<f32> {
        let mut levels = Vec::new();
        let window = SR as usize / 10;
        let mut first = true;
        for _ in 0..windows {
            let mut sum = 0.0f32;
            for _ in 0..window {
                let input = if first { 1.0 } else { 0.0 };
                first = false;
                let (l, _) = reverb.process(input, input);
                assert!(l.is_finite());
                sum += l * l;
            }
            levels.push((sum / window as f32).sqrt());
        }
        levels
    }

    #[test]
    fn the_tail_falls_sixty_db_in_the_decay_time() {
        let mut reverb = Reverb::new(SR, 10.0, 1.5, 20_000.0);
        let levels = tail(&mut reverb, 25);
        // Once the network is full (after 0.2 s), each 100 ms falls about 4 dB.
        let db = |x: f32| 20.0 * x.max(1e-12).log10();
        let slope = (db(levels[15]) - db(levels[5])) / 1.0;
        assert!((slope + 40.0).abs() < 8.0, "{slope} dB per second, wanted −40");
    }

    #[test]
    fn the_sides_differ_and_nothing_blows_up() {
        let mut reverb = Reverb::new(SR, 20.0, 8.0, 6_000.0);
        let mut differ = 0.0f32;
        for i in 0..SR * 4 {
            let x = if i % 4_800 == 0 { 1.0 } else { 0.0 };
            let (l, r) = reverb.process(x, x);
            assert!(l.abs() < 4.0 && r.abs() < 4.0);
            differ = differ.max((l - r).abs());
        }
        assert!(differ > 0.01, "a stereo tail");
    }

    #[test]
    fn the_hadamard_mix_keeps_energy() {
        let mut x = [1.0, -2.0, 0.5, 3.0, 0.0, 1.5, -1.0, 2.0];
        let before: f32 = x.iter().map(|v| v * v).sum();
        hadamard(&mut x);
        let after: f32 = x.iter().map(|v| v * v).sum();
        assert!((before - after).abs() < 1e-4);
    }
}
