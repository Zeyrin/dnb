//! A dub delay: a ping-pong echo whose repeats come back darker, thinner and a
//! little saturated each time round, like a tape echo pushed hard, with a slow
//! wow in the tape speed.

use std::f64::consts::TAU;

use crate::filter::OnePole;
use crate::shape::soft_clip;
use crate::smooth::Smoothed;

/// The repeats lose their lows under this and their highs over this.
const REPEAT_LOW_CUT_HZ: f32 = 280.0;
const REPEAT_HIGH_CUT_HZ: f32 = 3_200.0;
/// The tape speed wanders this far, this often.
const WOW_MS: f64 = 0.25;
const WOW_HZ: f64 = 0.6;
/// A new delay time is glided to, like a tape echo's speed knob.
const TIME_GLIDE_S: f32 = 0.15;
/// Each side hears this much of the other: echoes that bounce without
/// leaving one ear empty.
const CROSSFEED: f32 = 0.25;

#[derive(Clone, Debug)]
pub struct DubDelay {
    sample_rate: u32,
    left: Vec<f32>,
    right: Vec<f32>,
    pos: usize,
    /// Delay in frames.
    time: Smoothed,
    feedback: f32,
    low_cut: [OnePole; 2],
    high_cut: [OnePole; 2],
    wow: f64,
}

impl DubDelay {
    /// Room for echoes up to `max_seconds` apart.
    pub fn new(sample_rate: u32, max_seconds: f32, seconds: f32, feedback: f32) -> DubDelay {
        let frames = (max_seconds.max(0.01) * sample_rate as f32).ceil() as usize + 4;
        let mut delay = DubDelay {
            sample_rate,
            left: vec![0.0; frames],
            right: vec![0.0; frames],
            pos: 0,
            time: Smoothed::new(0.0, TIME_GLIDE_S, sample_rate),
            feedback: 0.0,
            low_cut: [(); 2].map(|_| OnePole::new(REPEAT_LOW_CUT_HZ, sample_rate)),
            high_cut: [(); 2].map(|_| OnePole::new(REPEAT_HIGH_CUT_HZ, sample_rate)),
            wow: 0.0,
        };
        delay.set_time(seconds);
        delay.time.snap(delay.time.target());
        delay.set_feedback(feedback);
        delay
    }

    /// Echoes `seconds` apart (as far as the buffer allows), glided to.
    pub fn set_time(&mut self, seconds: f32) {
        let longest = (self.left.len() - 4) as f32;
        let frames = seconds * self.sample_rate as f32;
        self.time.set_target(frames.clamp(1.0, longest));
    }

    /// How much of each echo comes round again, 0–0.95.
    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = feedback.clamp(0.0, 0.95);
    }

    /// Reads `frames` behind the write position, between frames.
    fn read(buffer: &[f32], pos: usize, frames: f64) -> f32 {
        let len = buffer.len();
        let back = frames.clamp(1.0, (len - 2) as f64);
        let whole = back as usize;
        let frac = (back - whole as f64) as f32;
        let a = buffer[(pos + len - whole) % len];
        let b = buffer[(pos + len - whole - 1) % len];
        a + (b - a) * frac
    }

    /// Takes a stereo sample in, returns the echoes only (no dry signal).
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let sr = f64::from(self.sample_rate);
        self.wow = (self.wow + WOW_HZ / sr).fract();
        let wow = WOW_MS / 1000.0 * sr * (TAU * self.wow).sin();
        let frames = f64::from(self.time.step()) + wow;
        let echo_l = Self::read(&self.left, self.pos, frames);
        let echo_r = Self::read(&self.right, self.pos, frames);
        // Each repeat is band-limited and saturated before it goes round again.
        let mut shape = |x: f32, side: usize| {
            let thin = self.low_cut[side].high(x);
            soft_clip(self.high_cut[side].low(thin))
        };
        let back_l = shape(echo_l, 0);
        let back_r = shape(echo_r, 1);
        // Ping-pong: the input enters on the left, each echo crosses over.
        self.left[self.pos] = 0.5 * (left + right) + self.feedback * back_r;
        self.right[self.pos] = self.feedback * back_l;
        self.pos = (self.pos + 1) % self.left.len();
        (echo_l + CROSSFEED * echo_r, echo_r + CROSSFEED * echo_l)
    }

    /// Silences every echo at once.
    pub fn clear(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    #[test]
    fn echoes_alternate_sides_and_fade() {
        let mut delay = DubDelay::new(SR, 1.0, 0.25, 0.6);
        let echo = SR as usize / 4;
        let mut out = Vec::new();
        for i in 0..echo * 5 {
            // A short burst in the delay's passband.
            let x = if i < 48 { (i as f32 * 0.3).sin() } else { 0.0 };
            out.push(delay.process(x, x));
        }
        let energy = |n: usize, side: usize| -> f32 {
            out[n * echo..n * echo + 2_000]
                .iter()
                .map(|&(l, r)| if side == 0 { l * l } else { r * r })
                .sum()
        };
        assert!(energy(0, 0) < 1e-9, "nothing before the first echo");
        assert!(energy(1, 0) > 2.0 * energy(1, 1), "the first echo is on the left");
        assert!(energy(2, 1) > 2.0 * energy(2, 0), "the second on the right");
        assert!(energy(3, 0) < energy(1, 0), "each repeat is quieter");
    }

    #[test]
    fn full_feedback_stays_bounded() {
        let mut delay = DubDelay::new(SR, 0.5, 0.1, 0.95);
        for i in 0..SR * 10 {
            let x = if i < SR { (i as f32 * 0.05).sin() } else { 0.0 };
            let (l, r) = delay.process(x, x);
            // Input plus a saturated echo, at most.
            assert!(l.abs() <= 2.5 && r.abs() <= 2.5, "{l} {r}");
        }
    }

    #[test]
    fn a_new_time_glides() {
        let mut delay = DubDelay::new(SR, 1.0, 0.2, 0.5);
        delay.set_time(0.4);
        delay.process(0.0, 0.0);
        let early = f64::from(delay.time.step());
        assert!(early > 0.2 * f64::from(SR) && early < 0.21 * f64::from(SR));
    }
}
