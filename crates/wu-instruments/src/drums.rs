//! Drum synthesis. Each `render` produces one full-velocity hit, mono,
//! normalised to the same peak; kits set the balance with pad gains and the
//! sampler scales each hit by its velocity.

use std::f32::consts::TAU;

use wu_dsp::{Rng, SamplerEra, Svf, polyblep_square, soft_clip};

/// Peak level every rendered hit is normalised to.
pub const HIT_PEAK: f32 = 0.95;

fn frames(sample_rate: u32, seconds: f32) -> usize {
    (seconds.max(0.0) * sample_rate as f32).round() as usize
}

/// Fades the last `ms` milliseconds to silence so no hit ends on a step.
fn fade_tail(samples: &mut [f32], sample_rate: u32, ms: f32) {
    let n = frames(sample_rate, ms / 1000.0).min(samples.len());
    let start = samples.len() - n;
    for (i, s) in samples[start..].iter_mut().enumerate() {
        *s *= 1.0 - (i as f32 + 1.0) / n as f32;
    }
}

fn normalise(mut samples: Vec<f32>) -> Vec<f32> {
    let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if peak > 0.0 {
        let gain = HIT_PEAK / peak;
        samples.iter_mut().for_each(|s| *s *= gain);
    }
    samples
}

/// `drive` into the saturator, scaled back so a full-scale input stays near full scale.
fn saturate(x: f32, drive: f32) -> f32 {
    soft_clip(drive * x) / soft_clip(drive)
}

fn finish(mut samples: Vec<f32>, sample_rate: u32, era: Option<SamplerEra>) -> Vec<f32> {
    fade_tail(&mut samples, sample_rate, 15.0);
    if let Some(era) = era {
        samples = era.apply(&samples, sample_rate);
    }
    normalise(samples)
}

/// A sine that falls in pitch, with a click on top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kick {
    pub start_hz: f32,
    pub end_hz: f32,
    pub pitch_decay_s: f32,
    pub amp_decay_s: f32,
    pub click: f32,
    pub drive: f32,
    pub length_s: f32,
}

impl Kick {
    /// Short and punchy, so it leaves room for the sub.
    pub const DNB: Kick = Kick {
        start_hz: 190.0,
        end_hz: 52.0,
        pitch_decay_s: 0.032,
        amp_decay_s: 0.15,
        click: 0.35,
        drive: 2.2,
        length_s: 0.45,
    };

    pub fn render(&self, sample_rate: u32, seed: u64) -> Vec<f32> {
        let sr = sample_rate as f32;
        let mut rng = Rng::new(seed);
        let mut click_filter = Svf::new(3000.0, 0.7, sample_rate);
        let mut phase = 0.0f32;
        let out = (0..frames(sample_rate, self.length_s))
            .map(|i| {
                let t = i as f32 / sr;
                let freq = self.end_hz + (self.start_hz - self.end_hz) * (-t / self.pitch_decay_s).exp();
                let body = (TAU * phase).sin() * (-t / self.amp_decay_s).exp();
                phase = (phase + freq / sr).fract();
                let click = click_filter.process(rng.noise()).high * (-t / 0.0025).exp() * self.click;
                saturate(body, self.drive) + click
            })
            .collect();
        finish(out, sample_rate, None)
    }
}

/// Two tuned drum-head modes plus a band of noise for the wires.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snare {
    pub tone_hz: f32,
    /// Second head mode, as a ratio of the first.
    pub tone_ratio: f32,
    pub tone_decay_s: f32,
    /// How far the pitch overshoots at the strike (0 = none).
    pub snap: f32,
    pub noise_hp_hz: f32,
    pub noise_lp_hz: f32,
    pub noise_decay_s: f32,
    pub tone_level: f32,
    pub noise_level: f32,
    pub drive: f32,
    pub length_s: f32,
    pub era: Option<SamplerEra>,
}

impl Snare {
    /// The two-step backbeat snare.
    pub const DNB: Snare = Snare {
        tone_hz: 185.0,
        tone_ratio: 1.65,
        tone_decay_s: 0.06,
        snap: 0.35,
        noise_hp_hz: 1100.0,
        noise_lp_hz: 9500.0,
        noise_decay_s: 0.15,
        tone_level: 0.6,
        noise_level: 0.85,
        drive: 1.6,
        length_s: 0.42,
        era: None,
    };

    /// Quiet, short in-between hits.
    pub const GHOST: Snare = Snare {
        tone_hz: 200.0,
        tone_ratio: 1.6,
        tone_decay_s: 0.03,
        snap: 0.2,
        noise_hp_hz: 1500.0,
        noise_lp_hz: 8000.0,
        noise_decay_s: 0.06,
        tone_level: 0.5,
        noise_level: 0.8,
        drive: 1.2,
        length_s: 0.2,
        era: None,
    };

    /// Tuned up, noisier, crunched through a 12-bit drum machine: a chopped-break snare.
    pub const JUNGLE: Snare = Snare {
        tone_hz: 240.0,
        tone_ratio: 1.5,
        tone_decay_s: 0.05,
        snap: 0.5,
        noise_hp_hz: 1600.0,
        noise_lp_hz: 11_000.0,
        noise_decay_s: 0.2,
        tone_level: 0.5,
        noise_level: 1.0,
        drive: 2.4,
        length_s: 0.45,
        era: Some(SamplerEra::DRUM_MACHINE),
    };

    pub fn render(&self, sample_rate: u32, seed: u64) -> Vec<f32> {
        let sr = sample_rate as f32;
        let mut rng = Rng::new(seed);
        let mut hp = Svf::new(self.noise_hp_hz, 0.7, sample_rate);
        let mut lp = Svf::new(self.noise_lp_hz, 0.7, sample_rate);
        let (mut phase1, mut phase2) = (0.0f32, 0.0f32);
        let out = (0..frames(sample_rate, self.length_s))
            .map(|i| {
                let t = i as f32 / sr;
                let pitch = 1.0 + self.snap * (-t / 0.006).exp();
                let f1 = self.tone_hz * pitch;
                let tone = ((TAU * phase1).sin() + 0.6 * (TAU * phase2).sin())
                    * (-t / self.tone_decay_s).exp()
                    * self.tone_level;
                phase1 = (phase1 + f1 / sr).fract();
                phase2 = (phase2 + f1 * self.tone_ratio / sr).fract();
                let wires = lp.process(hp.process(rng.noise()).high).low;
                let crack = (-t / self.noise_decay_s).exp() + 0.6 * (-t / 0.012).exp();
                saturate(tone + wires * crack * self.noise_level, self.drive)
            })
            .collect();
        finish(out, sample_rate, self.era)
    }
}

/// A side-stick: two short tones and a tick of noise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rim {
    pub high_hz: f32,
    pub low_hz: f32,
    pub length_s: f32,
}

impl Rim {
    pub const CLASSIC: Rim = Rim {
        high_hz: 1700.0,
        low_hz: 480.0,
        length_s: 0.12,
    };

    pub fn render(&self, sample_rate: u32, seed: u64) -> Vec<f32> {
        let sr = sample_rate as f32;
        let mut rng = Rng::new(seed);
        let mut band = Svf::new(3000.0, 1.5, sample_rate);
        let out = (0..frames(sample_rate, self.length_s))
            .map(|i| {
                let t = i as f32 / sr;
                let tones = (TAU * self.high_hz * t).sin() * (-t / 0.010).exp() * 0.6
                    + (TAU * self.low_hz * t).sin() * (-t / 0.015).exp() * 0.5;
                let tick = band.process(rng.noise()).band * (-t / 0.006).exp();
                saturate(tones + tick, 1.5)
            })
            .collect();
        finish(out, sample_rate, None)
    }
}

/// Several hands a few milliseconds apart, then the room.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clap {
    pub band_hz: f32,
    pub tail_decay_s: f32,
    pub length_s: f32,
}

impl Clap {
    pub const CLASSIC: Clap = Clap {
        band_hz: 1200.0,
        tail_decay_s: 0.12,
        length_s: 0.45,
    };

    /// Onsets of the individual claps, in seconds.
    const BURSTS: [f32; 4] = [0.0, 0.010, 0.020, 0.031];

    pub fn render(&self, sample_rate: u32, seed: u64) -> Vec<f32> {
        let sr = sample_rate as f32;
        let mut rng = Rng::new(seed);
        let mut band = Svf::new(self.band_hz, 1.4, sample_rate);
        let mut low_cut = Svf::new(500.0, 0.7, sample_rate);
        let last = Self::BURSTS[Self::BURSTS.len() - 1];
        let out = (0..frames(sample_rate, self.length_s))
            .map(|i| {
                let t = i as f32 / sr;
                let bursts: f32 = Self::BURSTS
                    .iter()
                    .filter(|&&b| t >= b)
                    .map(|&b| (-(t - b) / 0.0035).exp())
                    .sum();
                let tail = if t >= last {
                    0.7 * (-(t - last) / self.tail_decay_s).exp()
                } else {
                    0.0
                };
                let noise = low_cut.process(band.process(rng.noise()).band).high;
                saturate(noise * (bursts + tail) * 2.0, 1.4)
            })
            .collect();
        finish(out, sample_rate, None)
    }
}

/// A tuned tom or low percussion hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tom {
    pub start_hz: f32,
    pub end_hz: f32,
    pub pitch_decay_s: f32,
    pub amp_decay_s: f32,
    pub noise: f32,
    pub length_s: f32,
}

impl Tom {
    pub const LOW: Tom = Tom {
        start_hz: 150.0,
        end_hz: 95.0,
        pitch_decay_s: 0.06,
        amp_decay_s: 0.2,
        noise: 0.15,
        length_s: 0.5,
    };

    pub fn render(&self, sample_rate: u32, seed: u64) -> Vec<f32> {
        let sr = sample_rate as f32;
        let mut rng = Rng::new(seed);
        let mut skin = Svf::new(2500.0, 0.8, sample_rate);
        let mut phase = 0.0f32;
        let out = (0..frames(sample_rate, self.length_s))
            .map(|i| {
                let t = i as f32 / sr;
                let freq = self.end_hz + (self.start_hz - self.end_hz) * (-t / self.pitch_decay_s).exp();
                let body = (TAU * phase).sin() * (-t / self.amp_decay_s).exp();
                phase = (phase + freq / sr).fract();
                let stick = skin.process(rng.noise()).band * (-t / 0.008).exp() * self.noise;
                saturate(body + stick, 1.3)
            })
            .collect();
        finish(out, sample_rate, None)
    }
}

/// Six detuned square waves through band- and high-pass filters, the way the
/// classic analogue drum machines made metal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hat {
    pub decay_s: f32,
    pub length_s: f32,
    /// Scales all six oscillators: above 1 is brighter and smaller.
    pub pitch: f32,
    /// Share of white noise in the mix, 0–1.
    pub noise: f32,
}

impl Hat {
    pub const CLOSED: Hat = Hat {
        decay_s: 0.028,
        length_s: 0.12,
        pitch: 1.0,
        noise: 0.25,
    };
    pub const OPEN: Hat = Hat {
        decay_s: 0.22,
        length_s: 0.7,
        pitch: 1.0,
        noise: 0.3,
    };

    const OSCILLATORS_HZ: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

    pub fn render(&self, sample_rate: u32, seed: u64) -> Vec<f32> {
        let sr = sample_rate as f32;
        let mut rng = Rng::new(seed);
        let mut band = Svf::new(10_000.0, 0.9, sample_rate);
        let mut low_cut = Svf::new(7000.0, 0.7, sample_rate);
        let mut phases = [0.0f32; 6];
        let out = (0..frames(sample_rate, self.length_s))
            .map(|i| {
                let t = i as f32 / sr;
                let mut metal = 0.0;
                for (phase, hz) in phases.iter_mut().zip(Self::OSCILLATORS_HZ) {
                    let dt = hz * self.pitch / sr;
                    metal += polyblep_square(*phase, dt);
                    *phase = (*phase + dt).fract();
                }
                let mixed = metal / 6.0 * (1.0 - self.noise) + rng.noise() * self.noise;
                low_cut.process(band.process(mixed).band).high * (-t / self.decay_s).exp()
            })
            .collect();
        finish(out, sample_rate, None)
    }
}

/// A ride or a crash: two clusters of inharmonic squares ring-modulated
/// together, a struck bell, and a wash of noise, all above a high-pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cymbal {
    /// Scales every partial: above 1 is smaller and brighter.
    pub pitch: f32,
    /// The bell's level: the ride's ping.
    pub bell: f32,
    pub decay_s: f32,
    /// Share of noise in the wash, 0–1.
    pub noise: f32,
    pub low_cut_hz: f32,
    pub length_s: f32,
}

impl Cymbal {
    pub const RIDE: Cymbal = Cymbal {
        pitch: 1.0,
        bell: 0.5,
        decay_s: 0.9,
        noise: 0.2,
        low_cut_hz: 3_500.0,
        length_s: 1.6,
    };
    pub const CRASH: Cymbal = Cymbal {
        pitch: 1.15,
        bell: 0.0,
        decay_s: 1.3,
        noise: 0.55,
        low_cut_hz: 2_500.0,
        length_s: 2.2,
    };

    const CLUSTERS_HZ: [[f32; 3]; 2] = [[255.0, 385.0, 495.0], [621.0, 812.0, 1023.0]];
    const BELL_HZ: [f32; 4] = [740.0, 1185.0, 1610.0, 2290.0];

    pub fn render(&self, sample_rate: u32, seed: u64) -> Vec<f32> {
        let sr = sample_rate as f32;
        let mut rng = Rng::new(seed);
        let mut low_cut = Svf::new(self.low_cut_hz, 0.7, sample_rate);
        let mut sheen = Svf::new(9_000.0, 0.8, sample_rate);
        let mut phases = [[0.0f32; 3]; 2];
        let out = (0..frames(sample_rate, self.length_s))
            .map(|i| {
                let t = i as f32 / sr;
                let mut clusters = [0.0f32; 2];
                for (sum, (phases, freqs)) in clusters.iter_mut().zip(phases.iter_mut().zip(Self::CLUSTERS_HZ)) {
                    for (phase, hz) in phases.iter_mut().zip(freqs) {
                        let dt = hz * self.pitch / sr;
                        *sum += polyblep_square(*phase, dt);
                        *phase = (*phase + dt).fract();
                    }
                }
                let metal = clusters[0] * clusters[1] / 9.0;
                let wash = metal * (1.0 - self.noise) + rng.noise() * self.noise;
                let shimmer = sheen.process(wash).band + 0.5 * wash;
                let body = low_cut.process(shimmer).high * (-t / self.decay_s).exp() * (t / 0.002).min(1.0);
                let bell: f32 = Self::BELL_HZ
                    .iter()
                    .enumerate()
                    .map(|(k, hz)| (TAU * hz * self.pitch * t).sin() / (k as f32 + 1.0))
                    .sum();
                body + self.bell * 0.4 * bell * (-t / (0.35 * self.decay_s)).exp()
            })
            .collect();
        finish(out, sample_rate, None)
    }
}

/// Band-passed noise that swells and dies: a shaker, a maraca.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shaker {
    pub attack_s: f32,
    pub decay_s: f32,
    pub band_hz: f32,
    pub length_s: f32,
}

impl Shaker {
    pub const CLASSIC: Shaker = Shaker {
        attack_s: 0.012,
        decay_s: 0.05,
        band_hz: 7_000.0,
        length_s: 0.15,
    };

    pub fn render(&self, sample_rate: u32, seed: u64) -> Vec<f32> {
        let sr = sample_rate as f32;
        let mut rng = Rng::new(seed);
        let mut band = Svf::new(self.band_hz, 1.2, sample_rate);
        let out = (0..frames(sample_rate, self.length_s))
            .map(|i| {
                let t = i as f32 / sr;
                let swell = (t / self.attack_s).min(1.0) * (-(t - self.attack_s).max(0.0) / self.decay_s).exp();
                band.process(rng.noise()).band * swell
            })
            .collect();
        finish(out, sample_rate, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn check(name: &str, hit: &[f32], expected_seconds: f32) {
        assert_eq!(hit.len(), frames(SR, expected_seconds), "{name} length");
        assert!(hit.iter().all(|x| x.is_finite()), "{name} is finite");
        let peak = hit.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!((peak - HIT_PEAK).abs() < 1e-4, "{name} peak {peak}");
        assert!(hit[hit.len() - 1].abs() < 1e-3, "{name} ends on silence");
    }

    #[test]
    fn every_drum_renders_clean_normalised_hits() {
        check("kick", &Kick::DNB.render(SR, 1), Kick::DNB.length_s);
        check("snare", &Snare::DNB.render(SR, 2), Snare::DNB.length_s);
        check("ghost", &Snare::GHOST.render(SR, 3), Snare::GHOST.length_s);
        check("jungle snare", &Snare::JUNGLE.render(SR, 4), Snare::JUNGLE.length_s);
        check("rim", &Rim::CLASSIC.render(SR, 5), Rim::CLASSIC.length_s);
        check("clap", &Clap::CLASSIC.render(SR, 6), Clap::CLASSIC.length_s);
        check("tom", &Tom::LOW.render(SR, 7), Tom::LOW.length_s);
        check("closed hat", &Hat::CLOSED.render(SR, 8), Hat::CLOSED.length_s);
        check("open hat", &Hat::OPEN.render(SR, 9), Hat::OPEN.length_s);
        check("ride", &Cymbal::RIDE.render(SR, 10), Cymbal::RIDE.length_s);
        check("crash", &Cymbal::CRASH.render(SR, 11), Cymbal::CRASH.length_s);
        check("shaker", &Shaker::CLASSIC.render(SR, 12), Shaker::CLASSIC.length_s);
    }

    #[test]
    fn baking_is_deterministic() {
        assert_eq!(Snare::JUNGLE.render(SR, 11), Snare::JUNGLE.render(SR, 11));
        assert_ne!(Hat::CLOSED.render(SR, 11), Hat::CLOSED.render(SR, 12));
    }

    /// Energy below 120 Hz as a share of the total, by brute-force DFT bins.
    fn low_share(hit: &[f32]) -> f32 {
        let n = hit.len().min(8192);
        let (mut low, mut total) = (0.0f32, 0.0f32);
        for k in 1..n / 2 {
            let freq = k as f32 * SR as f32 / n as f32;
            if freq > 2000.0 && k % 4 != 0 {
                continue;
            }
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, x) in hit[..n].iter().enumerate() {
                let a = TAU * k as f32 * i as f32 / n as f32;
                re += x * a.cos();
                im -= x * a.sin();
            }
            let power = re * re + im * im;
            total += if freq > 2000.0 { power * 4.0 } else { power };
            if freq < 120.0 {
                low += power;
            }
        }
        low / total
    }

    #[test]
    fn kick_lives_in_the_lows_and_hats_do_not() {
        assert!(low_share(&Kick::DNB.render(SR, 1)) > 0.5);
        assert!(low_share(&Hat::CLOSED.render(SR, 8)) < 0.01);
        assert!(low_share(&Cymbal::RIDE.render(SR, 10)) < 0.01);
    }
}
