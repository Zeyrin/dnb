//! The drums: on every sixteenth of the grid, whether a kick, a snare (or a
//! ghost of one) or a hat struck, and how hard. Drum & bass is programmed on
//! the grid, so the listener asks each step rather than chasing every onset.
//!
//! Each question is a small logistic model over the step's sound: how much
//! each of nine bands jumps, starts and rings on, against the steps around
//! it. The weights were fitted on our own songs played on all eight kits,
//! with and without each break, where every hit is known; checked on kits left
//! out of the fitting, they find kicks with an F1 of 0.91, snares 0.93, hats
//! 0.88 (see `examples/listener_corpus.rs` and `training/train.py`).

use wu_audio::Hit;
use wu_instruments::Pad;
use wu_time::Tick;

use crate::spectrum::{FINE, Spectrogram};
use crate::tempo::Grid;

/// Spectral onsets run a little early: a step's hit is looked for from this
/// long before its line…
const BEFORE_S: f64 = 0.025;
/// …to this long after.
const AFTER_S: f64 = 0.02;
/// How long after its line a hit is asked whether it still rings.
const RING_FROM_S: f64 = 0.05;
const RING_TO_S: f64 = 0.06;
/// Steps either side that set what "loud" means for a step.
const CONTEXT_STEPS: usize = 32;

/// The fine bands, grouped: sub, kick, low mids… up to the air.
const GROUPS: [&[usize]; 9] = [
    &[1, 2, 3],
    &[4, 5, 6],
    &[7, 8],
    &[9, 10, 11],
    &[12, 13],
    &[14, 15, 16],
    &[17, 18, 19],
    &[20, 21],
    &[22, 23],
];
/// Per group: jump, start and ring; then how evenly the upper bands jump.
pub const FEATURES: usize = 3 * GROUPS.len() + 1;

/// Fitted weights, the bias last.
const KICK: [f32; FEATURES + 1] = [
    0.1155, 0.1939, 1.485, 1.851, 0.4108, -0.7144, 0.5827, -0.01771, -0.01231, 0.34, -0.06581, -1.325, -0.1611,
    -0.5364, -0.3705, -0.1314, -0.6275, -0.6516, -0.172, -0.3942, -0.9525, -0.2923, -0.1377, -0.4823, 0.2132, 0.2955,
    -0.4429, -0.1119, -1.293,
];
const SNARE: [f32; FEATURES + 1] = [
    -0.01993, -0.2639, -0.6608, -0.3145, -0.4683, -0.5175, 0.6783, -0.363, -0.6231, 0.5352, -0.1888, -1.027, -0.2291,
    -0.4711, -1.194, 0.3103, -0.1103, -0.4575, 0.8401, 0.3234, 0.4291, 0.9523, 0.5511, 0.5731, 0.806, 0.5027, 0.355,
    1.262, -1.584,
];
const GHOST: [f32; FEATURES + 1] = [
    -0.7403, -0.03897, -0.1034, -0.9663, 0.1325, -0.0241, -0.4023, 0.3616, -0.5208, -0.1298, 0.582, -0.4823, -0.4852,
    -0.08658, -0.1761, -0.7245, -0.3069, 0.1374, 0.294, 0.1126, -0.1247, -0.01753, 0.3991, 0.608, -0.6679, -0.1471,
    0.9221, 0.1954, 0.3654,
];
const HAT: [f32; FEATURES + 1] = [
    -0.1223, 0.2615, 0.4577, -0.06858, 0.1601, -0.3471, -0.008748, -0.003583, -0.1447, -0.0303, -0.1531, -0.1899,
    0.06689, -0.117, 0.0781, -0.1583, -0.4224, -0.139, -0.2109, -0.1046, -0.596, 0.7242, 0.8812, -0.4027, 2.019, 1.708,
    -0.5553, 0.1176, -0.1568,
];

/// How sure a model must be.
const KICK_AT: f32 = 0.5;
const SNARE_AT: f32 = 0.45;
const GHOST_AT: f32 = 0.7;
const HAT_AT: f32 = 0.6;
/// Sure enough to be an accent.
const ACCENT_AT: f32 = 0.85;

/// A hit as heard, before it is put on a pad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Heard {
    /// Sixteenths from the first bar line.
    pub step: i64,
    pub drum: Drum,
    /// How sure the listener is, 0–1.
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Drum {
    Kick,
    Snare,
    Ghost,
    Hat,
}

impl Drum {
    pub fn pad(self) -> Pad {
        match self {
            Drum::Kick => Pad::P1,
            Drum::Snare => Pad::P2,
            Drum::Ghost => Pad::P3,
            Drum::Hat => Pad::P7,
        }
    }
}

/// The frames a window of time covers.
fn frames(spec: &Spectrogram, from_s: f64, to_s: f64) -> std::ops::Range<usize> {
    let rate = spec.rate();
    let from = (from_s * rate).floor().max(0.0) as usize;
    let to = ((to_s * rate).ceil().max(0.0) as usize + 1).min(spec.frames);
    from..to.max(from)
}

fn most(curve: &[f32], frames: std::ops::Range<usize>) -> f32 {
    curve.get(frames).map_or(0.0, |w| w.iter().copied().fold(0.0, f32::max))
}

/// Each value over the loud end (90th percentile) of the values around it.
fn against_context(values: &[f32]) -> Vec<f32> {
    (0..values.len())
        .map(|i| {
            let from = i.saturating_sub(CONTEXT_STEPS);
            let to = (i + CONTEXT_STEPS + 1).min(values.len());
            let mut window = values[from..to].to_vec();
            window.sort_by(f32::total_cmp);
            let loud = window[(0.9 * (window.len() - 1) as f32) as usize];
            if loud > 1e-9 { values[i] / loud } else { 0.0 }
        })
        .collect()
}

/// The features of every step, ready for the models.
fn features(spec: &Spectrogram, grid: &Grid, steps: i64) -> Vec<[f32; FEATURES]> {
    let rate = spec.rate();
    let count = steps.max(0) as usize;
    let mut jump = vec![vec![0.0f32; count]; GROUPS.len()];
    let mut start = vec![vec![0.0f32; count]; GROUPS.len()];
    let mut ring = vec![vec![0.0f32; count]; GROUPS.len()];
    for step in 0..count {
        let line = grid.time_of_step(step as f64);
        let window = frames(spec, line - BEFORE_S, line + AFTER_S);
        let later = frames(spec, line + RING_FROM_S, line + RING_TO_S);
        // Energy jumps over three frames, from the window's own first frame on.
        let first = ((line - BEFORE_S) * rate).max(3.0) as usize;
        let last = (((line + AFTER_S) * rate).max(0.0) as usize).min(spec.frames.saturating_sub(1));
        let mut band_jump = [0.0f32; FINE];
        let mut band_start = [0.0f32; FINE];
        let mut band_ring = [0.0f32; FINE];
        for b in 0..FINE {
            let energy = &spec.fine_energy[b];
            let mut best = 0.0f32;
            let mut peak = 0.0f32;
            for i in first..=last {
                best = best.max((energy[i] - energy[i - 3]).max(0.0).sqrt());
                peak = peak.max(energy[i]);
            }
            band_jump[b] = best;
            band_start[b] = most(&spec.fine_flux[b], window.clone());
            band_ring[b] = if peak > 0.0 {
                most(energy, later.clone()) / peak
            } else {
                0.0
            };
        }
        for (g, bands) in GROUPS.iter().enumerate() {
            jump[g][step] = bands.iter().map(|&b| band_jump[b]).sum();
            start[g][step] = bands.iter().map(|&b| band_start[b]).sum();
            ring[g][step] = bands.iter().map(|&b| band_ring[b]).sum::<f32>() / bands.len() as f32;
        }
    }
    let jump: Vec<Vec<f32>> = jump.iter().map(|v| against_context(v)).collect();
    let start: Vec<Vec<f32>> = start.iter().map(|v| against_context(v)).collect();
    (0..count)
        .map(|step| {
            let mut x = [0.0f32; FEATURES];
            for g in 0..GROUPS.len() {
                x[3 * g] = jump[g][step];
                x[3 * g + 1] = start[g][step];
                x[3 * g + 2] = ring[g][step];
            }
            // A snare's noise jumps right across the upper bands at once.
            x[FEATURES - 1] = jump[5][step].min(jump[6][step]).min(jump[7][step]);
            x.map(|v| v.min(4.0))
        })
        .collect()
}

fn probability(weights: &[f32; FEATURES + 1], x: &[f32; FEATURES]) -> f32 {
    let z = weights[FEATURES] + weights.iter().zip(x).map(|(w, v)| w * v).sum::<f32>();
    1.0 / (1.0 + (-z.clamp(-30.0, 30.0)).exp())
}

/// Every drum hit in the first `steps` sixteenths.
pub fn hear_drums(spec: &Spectrogram, grid: &Grid, steps: i64) -> Vec<Heard> {
    let mut heard = Vec::new();
    for (step, x) in features(spec, grid, steps).iter().enumerate() {
        let step = step as i64;
        let mut hear = |drum: Drum, confidence: f32| heard.push(Heard { step, drum, confidence });
        let kick = probability(&KICK, x);
        if kick > KICK_AT {
            hear(Drum::Kick, kick);
        }
        let snare = probability(&SNARE, x);
        let ghost = probability(&GHOST, x);
        if snare > SNARE_AT {
            hear(Drum::Snare, snare);
        } else if ghost > GHOST_AT {
            hear(Drum::Ghost, ghost);
        }
        let hat = probability(&HAT, x);
        if hat > HAT_AT {
            hear(Drum::Hat, hat);
        }
    }
    heard
}

/// The hits on the game's pads: the surest at full velocity, ghosts soft.
pub fn to_hits(heard: &[Heard]) -> Vec<Hit> {
    heard
        .iter()
        .map(|h| Hit {
            tick: Tick::from_steps(h.step),
            pad: h.drum.pad(),
            velocity: match h.drum {
                Drum::Ghost => 0.45,
                _ if h.confidence >= ACCENT_AT => 1.0,
                _ => 0.8,
            },
        })
        .collect()
}
