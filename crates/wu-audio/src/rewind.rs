//! A selector's rewind: the record pulled back by hand. The engine keeps the
//! last seconds it played; when a WHEEL UP! cuts the tune, those seconds play
//! backwards the way the record turns under the hand. It slows to a stop,
//! spins back, fast, and slows down again where the tune drops in.

use std::f64::consts::PI;

/// Seconds of what was played, kept to be pulled back through.
const HISTORY_S: f64 = 6.0;
/// The share of the rewind spent stopping the record.
const BRAKE: f64 = 0.12;
/// How fast the record turns back at its fastest, against its normal speed.
const PULL_SPEED: f64 = 4.0;
/// How loud the rewind plays.
const GAIN: f32 = 0.8;
/// The share of the rewind it fades out over, so it stops without a click.
const FADE: f64 = 0.15;
/// A record spun fast sounds muffled, not shrill: everything above this goes.
const TOP_HZ: f64 = 7_000.0;

/// The rewind's place in what was played: frames back from the newest one.
#[derive(Clone, Copy, Debug)]
struct Pull {
    /// The frame of the current block it starts on.
    from: usize,
    elapsed: u32,
    frames: u32,
    back: f64,
}

#[derive(Clone, Debug)]
pub struct Rewind {
    /// What was played, interleaved stereo, round and round.
    history: Vec<f32>,
    capacity: usize,
    /// The next frame to write.
    write: usize,
    /// Frames kept so far, up to `capacity`.
    kept: usize,
    pull: Option<Pull>,
    /// The lowpass's per-sample step and its state, per channel.
    smooth: f32,
    state: [f32; 2],
}

impl Rewind {
    /// Allocates the history: build it off the audio thread.
    pub fn new(sample_rate: u32) -> Rewind {
        let capacity = (HISTORY_S * f64::from(sample_rate)) as usize;
        Rewind {
            history: vec![0.0; capacity * 2],
            capacity,
            write: 0,
            kept: 0,
            pull: None,
            smooth: (1.0 - (-2.0 * PI * TOP_HZ / f64::from(sample_rate)).exp()) as f32,
            state: [0.0; 2],
        }
    }

    /// Pulls the record back from frame `from` of the next block, for `frames`.
    pub fn start(&mut self, from: usize, frames: u32) {
        if frames == 0 {
            return;
        }
        // The hand slows the record from full speed to a stop over the brake,
        // turning it half as far as full speed would: it starts that far back,
        // so the stop lands on what played last.
        self.pull = Some(Pull {
            from,
            elapsed: 0,
            frames,
            back: 0.5 * BRAKE * f64::from(frames),
        });
        self.state = [0.0; 2];
    }

    /// Takes in the block just played (interleaved stereo), adding the rewind
    /// where it plays; what plays outside it is kept for the next one.
    pub fn process(&mut self, out: &mut [f32]) {
        for (i, frame) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            match self.pull {
                Some(pull) if i >= pull.from => {
                    let [left, right] = self.pulled(pull);
                    frame[0] = (frame[0] + left).clamp(-1.0, 1.0);
                    frame[1] = (frame[1] + right).clamp(-1.0, 1.0);
                }
                _ => self.keep(frame[0], frame[1]),
            }
        }
        if let Some(pull) = self.pull.as_mut() {
            pull.from = 0;
        }
    }

    fn keep(&mut self, left: f32, right: f32) {
        self.history[self.write * 2] = left;
        self.history[self.write * 2 + 1] = right;
        self.write = (self.write + 1) % self.capacity;
        self.kept = (self.kept + 1).min(self.capacity);
    }

    /// The frame `back` frames before the newest kept one; silence past the start.
    fn frame(&self, back: usize) -> [f32; 2] {
        if back >= self.kept {
            return [0.0; 2];
        }
        let at = (self.write + self.capacity - 1 - back) % self.capacity;
        [self.history[at * 2], self.history[at * 2 + 1]]
    }

    /// The next frame of the rewind, and the record moved on by one.
    fn pulled(&mut self, mut pull: Pull) -> [f32; 2] {
        let t = f64::from(pull.elapsed) / f64::from(pull.frames);
        // Forward, slowing to a stop; then back, fast, and slowing again.
        let speed = if t < BRAKE {
            1.0 - t / BRAKE
        } else {
            let u = (t - BRAKE) / (1.0 - BRAKE);
            -PULL_SPEED * (PI * u).sin().powi(2)
        };
        let from = pull.back;
        pull.back = (pull.back - speed).max(0.0);
        // Everything the record turned through since the last frame, averaged:
        // fast, it blurs instead of aliasing.
        let (near, far) = (from.min(pull.back), from.max(pull.back));
        let (first, last) = (near.ceil() as usize, far.floor() as usize);
        let mut sound = if last > first {
            let mut sum = [0.0f32; 2];
            for back in first..=last {
                let [l, r] = self.frame(back);
                sum[0] += l;
                sum[1] += r;
            }
            let n = (last - first + 1) as f32;
            [sum[0] / n, sum[1] / n]
        } else {
            let at = pull.back.floor() as usize;
            let share = (pull.back - pull.back.floor()) as f32;
            let [a, b] = [self.frame(at), self.frame(at + 1)];
            [a[0] + (b[0] - a[0]) * share, a[1] + (b[1] - a[1]) * share]
        };
        for (channel, state) in sound.iter_mut().zip(self.state.iter_mut()) {
            *state += self.smooth * (*channel - *state);
            *channel = *state;
        }
        let fade = ((1.0 - t) / FADE).min(1.0) as f32;
        let gain = GAIN * fade;
        pull.elapsed += 1;
        // Done: what plays next is kept again, from where the tune drops in.
        self.pull = (pull.elapsed < pull.frames).then_some(pull);
        [sound[0] * gain, sound[1] * gain]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// A second of a 440 Hz tone, then the rewind's frames, as played.
    fn rewound(frames: u32) -> Vec<f32> {
        let mut rewind = Rewind::new(RATE);
        let mut played: Vec<f32> = (0..RATE)
            .flat_map(|i| {
                let x = (2.0 * PI * 440.0 * f64::from(i) / f64::from(RATE)).sin() as f32 * 0.5;
                [x, x]
            })
            .collect();
        rewind.process(&mut played);
        rewind.start(0, frames);
        let mut gap = vec![0.0f32; frames as usize * 2];
        rewind.process(&mut gap);
        assert!(rewind.pull.is_none(), "done by the end of its frames");
        gap
    }

    #[test]
    fn the_record_plays_back_what_was_played_then_falls_silent() {
        let gap = rewound(RATE / 2);
        let loudness = |part: &[f32]| part.iter().map(|x| x.abs()).sum::<f32>() / part.len() as f32;
        let quarter = gap.len() / 4;
        assert!(loudness(&gap[quarter..2 * quarter]) > 0.05, "the pull is heard");
        assert!(gap.iter().all(|x| x.abs() <= 1.0 && x.is_finite()));
        let tail = &gap[gap.len() - 40..];
        assert!(loudness(tail) < 0.01, "it fades out instead of clicking");
    }

    #[test]
    fn nothing_played_pulls_back_silence() {
        let mut rewind = Rewind::new(RATE);
        rewind.start(0, 1_000);
        let mut gap = vec![0.0f32; 2_000];
        rewind.process(&mut gap);
        assert!(gap.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn what_plays_after_the_rewind_is_kept_again() {
        let mut rewind = Rewind::new(RATE);
        rewind.start(10, 20);
        let mut block = vec![0.25f32; 100];
        rewind.process(&mut block);
        // 10 frames before it and 20 after: kept; the 20 of the rewind: not.
        assert_eq!(rewind.kept, 30);
    }
}
