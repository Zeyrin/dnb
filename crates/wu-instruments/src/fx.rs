//! The sounds of a rewind, synthesised like the drums: the record dragged back
//! to a stop (the spinback), the dancehall air horn, and the crowd roaring as
//! the tune drops again.

use std::f32::consts::TAU;
use std::sync::Arc;

use wu_dsp::{Rng, Sample, Svf, polyblep_saw, soft_clip};

use crate::bus::Bus;
use crate::kit::PadSound;

fn frames(sample_rate: u32, seconds: f32) -> usize {
    (seconds * sample_rate as f32).round() as usize
}

/// Peak at full scale, the last 20 ms faded out.
fn finish(mut samples: Vec<f32>, sample_rate: u32) -> Vec<f32> {
    let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if peak > 0.0 {
        samples.iter_mut().for_each(|x| *x /= peak);
    }
    let fade = frames(sample_rate, 0.02).min(samples.len());
    let start = samples.len() - fade;
    for (i, x) in samples[start..].iter_mut().enumerate() {
        *x *= 1.0 - (i as f32 + 1.0) / fade as f32;
    }
    samples
}

/// A record dragged backwards to a stop: a smeared tone falling with the
/// platter's speed, over a whoosh of noise falling with it.
pub fn spinback(sample_rate: u32, seed: u64) -> Vec<f32> {
    const LENGTH_S: f32 = 0.75;
    let sr = sample_rate as f32;
    let mut rng = Rng::new(seed);
    let mut whoosh = Svf::new(4000.0, 1.2, sample_rate);
    let mut phase = 0.0f32;
    let out = (0..frames(sample_rate, LENGTH_S))
        .map(|i| {
            let t = i as f32 / sr;
            let left = 1.0 - t / LENGTH_S;
            // The platter slows from six times speed to a standstill, wobbling.
            let speed = 6.0 * left.powf(1.6);
            let freq = 110.0 * speed * (1.0 + 0.08 * (TAU * 9.0 * t).sin());
            phase = (phase + freq / sr).fract();
            let tone = 0.6 * polyblep_saw(phase, (freq / sr).max(1e-6)) + 0.3 * (TAU * 2.0 * phase).sin();
            whoosh.set(80.0 + 6000.0 * left * left, 1.2, sample_rate);
            let noise = 1.4 * whoosh.process(rng.noise()).band;
            let envelope = (t / 0.02).min(1.0) * left.powf(0.7);
            soft_clip(1.5 * (tone + noise) * envelope)
        })
        .collect();
    finish(out, sample_rate)
}

/// The dancehall air horn: two short blasts and a long one, a bright detuned
/// chord whose last blast sags as the air runs out.
pub fn air_horn(sample_rate: u32) -> Vec<f32> {
    const BLASTS: [(f32, f32); 3] = [(0.0, 0.12), (0.17, 0.12), (0.34, 0.65)];
    const PARTIALS: [(f32, f32); 5] = [(1.0, 1.0), (1.007, 0.8), (1.5, 0.6), (1.503, 0.5), (2.0, 0.3)];
    const BASE_HZ: f32 = 415.0;
    let sr = sample_rate as f32;
    let mut phases = [0.0f32; PARTIALS.len()];
    let mut tone = Svf::new(4500.0, 0.8, sample_rate);
    let out = (0..frames(sample_rate, 1.05))
        .map(|i| {
            let t = i as f32 / sr;
            let gate = BLASTS
                .iter()
                .map(|&(start, length)| {
                    let local = t - start;
                    if local < 0.0 || local > length + 0.03 {
                        0.0
                    } else {
                        (local / 0.008).min(1.0) * ((length + 0.03 - local) / 0.03).min(1.0)
                    }
                })
                .fold(0.0f32, f32::max);
            let sag = 1.0 - 0.06 * ((t - 0.8) / 0.2).clamp(0.0, 1.0);
            let vibrato = 1.0 + 0.004 * (TAU * 6.0 * t).sin();
            let mut x = 0.0;
            for (phase, (ratio, gain)) in phases.iter_mut().zip(PARTIALS) {
                let freq = BASE_HZ * ratio * sag * vibrato;
                *phase = (*phase + freq / sr).fract();
                x += gain * polyblep_saw(*phase, freq / sr);
            }
            tone.process(soft_clip(0.5 * x * gate)).low
        })
        .collect();
    finish(out, sample_rate)
}

/// A crowd roaring as the tune drops again: noise through a few vocal bands,
/// swelling in a quarter second and dying away, its level fluttering as
/// voices come and go.
pub fn crowd(sample_rate: u32, seed: u64) -> Vec<f32> {
    const BANDS_HZ: [f32; 5] = [450.0, 700.0, 1100.0, 1600.0, 2400.0];
    let sr = sample_rate as f32;
    let mut rng = Rng::new(seed);
    let mut bands = BANDS_HZ.map(|hz| Svf::new(hz, 2.5, sample_rate));
    let mut flutter = 0.0f32;
    let out = (0..frames(sample_rate, 2.4))
        .map(|i| {
            let t = i as f32 / sr;
            let envelope = (t / 0.25).min(1.0) * (-(t - 0.25).max(0.0) / 0.9).exp();
            flutter += (rng.noise() - flutter) * 0.002;
            let noise = rng.noise();
            let voices: f32 = bands
                .iter_mut()
                .enumerate()
                .map(|(k, band)| band.process(noise).band * (1.0 - 0.12 * k as f32))
                .sum();
            voices * envelope * (0.8 + 4.0 * flutter.abs())
        })
        .collect();
    finish(out, sample_rate)
}

/// A rewind's sounds, ready to play: what plays as the record is pulled
/// back, and as the tune drops again.
#[derive(Clone, Debug)]
pub struct RewindSounds {
    pub pull: [PadSound; 2],
    pub drop: PadSound,
}

impl RewindSounds {
    pub fn new(sample_rate: u32) -> RewindSounds {
        let sound = |name: &str, samples: Vec<f32>, gain: f32, pan: f32| PadSound {
            name: name.to_owned(),
            sample: Arc::new(Sample::mono(samples, sample_rate)),
            gain,
            pan,
            choke: None,
            bus: Bus::Fx,
            sidechain: false,
        };
        RewindSounds {
            pull: [
                sound("Spinback", spinback(sample_rate, 0x5917), 0.7, 0.0),
                sound("Air Horn", air_horn(sample_rate), 0.45, 0.2),
            ],
            drop: sound("Crowd", crowd(sample_rate, 0xC40D), 0.35, -0.1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rewind_sounds_are_full_scale_and_end_in_silence() {
        let sounds = RewindSounds::new(48_000);
        for sound in sounds.pull.iter().chain([&sounds.drop]) {
            let data = sound.sample.data();
            assert!((sound.sample.peak() - 1.0).abs() < 1e-4, "{}", sound.name);
            assert_eq!(data.last().copied(), Some(0.0), "{}", sound.name);
            assert!(data.iter().all(|x| x.is_finite()));
            assert_eq!(sound.bus, Bus::Fx);
        }
    }

    #[test]
    fn the_spinback_slows_down() {
        // Count zero crossings in the first and last fifths: the pitch falls.
        let data = spinback(48_000, 1);
        let crossings = |part: &[f32]| part.windows(2).filter(|w| w[0].signum() != w[1].signum()).count();
        let fifth = data.len() / 5;
        assert!(crossings(&data[..fifth]) > 2 * crossings(&data[4 * fifth..]));
    }

    #[test]
    fn the_horn_blasts_three_times() {
        let data = air_horn(48_000);
        // Loud in each blast, silent in the gaps between them.
        let level = |from_s: f32, to_s: f32| {
            let part = &data[frames(48_000, from_s)..frames(48_000, to_s)];
            part.iter().map(|x| x * x).sum::<f32>() / part.len() as f32
        };
        let gap = level(0.152, 0.168);
        for (from, to) in [(0.02, 0.1), (0.19, 0.27), (0.4, 0.9)] {
            assert!(level(from, to) > 100.0 * gap.max(1e-9), "blast at {from} s");
        }
    }
}
