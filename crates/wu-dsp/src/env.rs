//! Envelopes.

/// Attack, decay, sustain, release: times in seconds, sustain 0–1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adsr {
    pub attack_s: f32,
    pub decay_s: f32,
    pub sustain: f32,
    pub release_s: f32,
}

impl Adsr {
    pub const fn new(attack_s: f32, decay_s: f32, sustain: f32, release_s: f32) -> Adsr {
        Adsr {
            attack_s,
            decay_s,
            sustain,
            release_s,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Idle,
    Attack,
    Decay,
    Release,
}

/// Below this an envelope in its release is silent: −80 dB.
const SILENT: f32 = 1e-4;
/// Decay and release get within this share of their target in their time (−60 dB).
const SETTLED: f32 = 1e-3;

/// An ADSR envelope's state: a linear attack, then an exponential decay to the
/// sustain level and an exponential release, each settling (to −60 dB of the
/// distance left) in its time, like an analogue envelope.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Envelope {
    stage: Stage,
    level: f32,
    attack_step: f32,
    decay: f32,
    sustain: f32,
    release: f32,
}

/// The per-sample factor that brings a distance down to `SETTLED` of itself in `seconds`.
fn settle_factor(seconds: f32, sample_rate: f32) -> f32 {
    let samples = (seconds * sample_rate).max(1.0);
    (SETTLED.ln() / samples).exp()
}

impl Envelope {
    pub fn new(adsr: &Adsr, sample_rate: u32) -> Envelope {
        let sr = sample_rate as f32;
        Envelope {
            stage: Stage::Idle,
            level: 0.0,
            attack_step: 1.0 / (adsr.attack_s * sr).max(1.0),
            decay: settle_factor(adsr.decay_s, sr),
            sustain: adsr.sustain.clamp(0.0, 1.0),
            release: settle_factor(adsr.release_s, sr),
        }
    }

    /// Starts the attack from wherever the level is.
    pub fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }

    /// Lets go: the release starts from the current level.
    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    /// Advances one sample and returns the level.
    pub fn step(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => self.level = 0.0,
            Stage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => self.level = self.sustain + (self.level - self.sustain) * self.decay,
            Stage::Release => {
                self.level *= self.release;
                if self.level < SILENT {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn run(env: &mut Envelope, samples: usize) -> f32 {
        (0..samples).fold(0.0, |_, _| env.step())
    }

    #[test]
    fn attack_decay_sustain_release() {
        let adsr = Adsr::new(0.01, 0.1, 0.5, 0.2);
        let mut env = Envelope::new(&adsr, SR);
        assert_eq!(env.step(), 0.0, "silent until triggered");
        env.trigger();
        assert!((run(&mut env, 240) - 0.5).abs() < 0.01, "half way up the attack");
        assert!((run(&mut env, 240) - 1.0).abs() < 1e-3, "the top after 10 ms");
        let settled = run(&mut env, 4_800);
        assert!(
            (settled - 0.5).abs() < 0.001,
            "at the sustain after the decay: {settled}"
        );
        // Within f32 rounding of it (the distance stalls a few hundred ulps out, −90 dB).
        assert!((run(&mut env, 48_000) - 0.5).abs() < 1e-4, "holds while the note does");
        env.release();
        assert!(run(&mut env, 9_600) < 0.5 * 1.1e-3, "−60 dB after the release time");
        run(&mut env, 9_600);
        assert!(env.is_idle());
    }

    #[test]
    fn a_release_during_the_attack_falls_from_where_it_got_to() {
        let mut env = Envelope::new(&Adsr::new(0.1, 0.1, 1.0, 0.05), SR);
        env.trigger();
        let level = run(&mut env, 2_400);
        env.release();
        let next = env.step();
        assert!(next < level && next > 0.9 * level, "no jump: {level} → {next}");
    }

    #[test]
    fn a_zero_sustain_decays_away() {
        let mut env = Envelope::new(&Adsr::new(0.0, 0.2, 0.0, 0.1), SR);
        env.trigger();
        assert_eq!(env.step(), 1.0, "an instant attack");
        assert!(run(&mut env, 9_600) < 1.1e-3);
    }
}
