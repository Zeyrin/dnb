//! Breaks: a drummer's performance played by the drum synth, wandering in time
//! and touch like a person, recorded in a room, sped up the way a sampler tunes
//! a funk break to jungle tempo, crunched through an old sampler and tape, then
//! cut into slices for the pads. The jungle sound, every hit of it original.

use std::f32::consts::TAU;

use wu_dsp::{OnePole, Reverb, Rng, SamplerEra, Svf, soft_clip};
use wu_time::STEPS_PER_BAR;

use crate::drums::{Cymbal, HIT_PEAK, Hat, Kick, Snare, Tom};
use crate::steps::{StepError, parse_steps};

/// The drummer's kit, voice by voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voice {
    Kick,
    Snare,
    Ghost,
    ClosedHat,
    OpenHat,
    Ride,
    Crash,
    Tom,
}

impl Voice {
    /// How loud each sits in the drummer's own balance.
    fn level(self) -> f32 {
        match self {
            Voice::Kick => 1.0,
            Voice::Snare => 0.9,
            Voice::Ghost => 0.55,
            Voice::ClosedHat => 0.32,
            Voice::OpenHat => 0.3,
            Voice::Ride => 0.28,
            Voice::Crash => 0.35,
            Voice::Tom => 0.6,
        }
    }
}

/// What each voice of the drummer's kit sounds like.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drummer {
    pub kick: Kick,
    pub snare: Snare,
    pub ghost: Snare,
    pub closed_hat: Hat,
    pub open_hat: Hat,
    pub ride: Cymbal,
    pub crash: Cymbal,
    pub tom: Tom,
}

impl Drummer {
    /// A funk drummer's kit: a round, open kick, a fat snare, a bright ride.
    pub const FUNK: Drummer = Drummer {
        kick: Kick {
            start_hz: 140.0,
            end_hz: 58.0,
            pitch_decay_s: 0.025,
            amp_decay_s: 0.22,
            click: 0.25,
            drive: 1.6,
            length_s: 0.5,
        },
        snare: Snare {
            tone_hz: 205.0,
            noise_decay_s: 0.18,
            drive: 1.4,
            ..Snare::DNB
        },
        ghost: Snare::GHOST,
        closed_hat: Hat::CLOSED,
        open_hat: Hat::OPEN,
        ride: Cymbal::RIDE,
        crash: Cymbal::CRASH,
        tom: Tom::LOW,
    };

    fn hit(&self, voice: Voice, sample_rate: u32, seed: u64) -> Vec<f32> {
        match voice {
            Voice::Kick => self.kick.render(sample_rate, seed),
            Voice::Snare => self.snare.render(sample_rate, seed),
            Voice::Ghost => self.ghost.render(sample_rate, seed),
            Voice::ClosedHat => self.closed_hat.render(sample_rate, seed),
            Voice::OpenHat => self.open_hat.render(sample_rate, seed),
            Voice::Ride => self.ride.render(sample_rate, seed),
            Voice::Crash => self.crash.render(sample_rate, seed),
            Voice::Tom => self.tom.render(sample_rate, seed),
        }
    }
}

/// A stretch of the break for a pad: from `step`, `steps` long.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliceDef {
    pub name: &'static str,
    pub step: u32,
    pub steps: u32,
}

/// A drummer's performance, and how it was recorded and sampled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BreakDef {
    pub name: &'static str,
    /// The tempo the drummer played at…
    pub played_bpm: f64,
    /// …and the tempo the sampler speeds it up to (pitching it up with it).
    pub bpm: f64,
    /// Odd 16ths come this share of a step late.
    pub swing: f32,
    /// How far the drummer's timing wanders (standard deviation, in ms),
    /// and how much the strength of each hit varies (0–1).
    pub timing_ms: f32,
    pub touch: f32,
    pub drummer: Drummer,
    /// One step string per voice (see `steps`), all the same length.
    pub voices: &'static [(Voice, &'static str)],
    /// How long the room rings, and how much of it the mics hear.
    pub room_s: f32,
    pub room: f32,
    /// The sampler it went through; `None` for a clean one.
    pub era: Option<SamplerEra>,
    /// Tape saturation, 0 for none.
    pub tape: f32,
    /// The converter's gentle low-pass.
    pub low_pass_hz: f32,
    pub slices: &'static [SliceDef],
    pub seed: u64,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum BreakError {
    #[error("break \"{name}\": {source}")]
    Steps { name: &'static str, source: StepError },
    #[error("break \"{0}\": every voice needs the same number of whole bars")]
    Length(&'static str),
    #[error("break \"{name}\": slice \"{slice}\" runs past the end")]
    Slice { name: &'static str, slice: &'static str },
}

/// A performed break: the loop, exactly its bars long, and its slices.
#[derive(Clone, Debug, PartialEq)]
pub struct Performance {
    pub bars: u32,
    pub audio: Vec<f32>,
    pub slices: Vec<(&'static str, Vec<f32>)>,
    /// Where each step's first hit landed, in seconds into the loop (at `bpm`).
    pub onsets: Vec<Option<f64>>,
}

/// The takes of each hit a drummer plays from, so no two in a row sound alike.
const TAKES: u64 = 4;
/// Slices start this long before their hit, so its attack survives the cut.
const PRE_ROLL_S: f64 = 0.0015;
/// A drummer is never further off the grid than this.
const MOST_OFF_S: f64 = 0.025;

/// A standard normal deviate (Box–Muller).
fn gaussian(rng: &mut Rng) -> f32 {
    let u1 = rng.next_f32().max(1e-7);
    let u2 = rng.next_f32();
    (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos()
}

fn normalise(samples: &mut [f32]) {
    let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if peak > 0.0 {
        samples.iter_mut().for_each(|x| *x *= HIT_PEAK / peak);
    }
}

/// Plays `input` `rate` times faster (and higher), as a sampler does,
/// smoothing first so the pitch-up doesn't fold the highs back down.
fn speed_up(input: &[f32], rate: f64, sample_rate: u32) -> Vec<f32> {
    let cutoff = (0.45 * f64::from(sample_rate) / rate) as f32;
    let [mut first, mut second] = [(); 2].map(|_| Svf::new(cutoff, 0.7, sample_rate));
    let smoothed: Vec<f32> = input
        .iter()
        .map(|&x| second.process(first.process(x).low).low)
        .collect();
    let frames = (input.len() as f64 / rate) as usize;
    (0..frames)
        .map(|i| {
            let pos = i as f64 * rate;
            let j = pos as usize;
            let frac = (pos - j as f64) as f32;
            let a = smoothed[j];
            let b = smoothed.get(j + 1).copied().unwrap_or(a);
            a + (b - a) * frac
        })
        .collect()
}

impl BreakDef {
    /// How many bars it lasts (0 if its steps don't parse).
    pub fn bars(&self) -> u32 {
        self.voices
            .first()
            .and_then(|(_, text)| parse_steps(text).ok())
            .map_or(0, |steps| (steps.len() / STEPS_PER_BAR as usize) as u32)
    }

    /// Plays, records and samples the break. Deterministic: the same
    /// definition gives the same audio, sample for sample, on one platform.
    pub fn perform(&self, sample_rate: u32) -> Result<Performance, BreakError> {
        let sr = f64::from(sample_rate);
        let mut parts = Vec::with_capacity(self.voices.len());
        let mut length = None;
        for &(voice, text) in self.voices {
            let steps = parse_steps(text).map_err(|source| BreakError::Steps {
                name: self.name,
                source,
            })?;
            if *length.get_or_insert(steps.len()) != steps.len() {
                return Err(BreakError::Length(self.name));
            }
            parts.push((voice, steps));
        }
        let steps = length.unwrap_or(0);
        if steps == 0 || steps % STEPS_PER_BAR as usize != 0 {
            return Err(BreakError::Length(self.name));
        }

        // The take, at the drummer's own tempo.
        let step_s = 60.0 / self.played_bpm / 4.0;
        let loop_frames = (steps as f64 * step_s * sr).round() as usize;
        let tail = (2.5 * sr) as usize;
        let mut dry = vec![0.0f32; loop_frames + tail];
        let mut onsets: Vec<Option<f64>> = vec![None; steps];
        let mut rng = Rng::new(self.seed);
        for (index, (voice, part)) in parts.iter().enumerate() {
            let takes: Vec<Vec<f32>> = (0..TAKES)
                .map(|take| {
                    self.drummer
                        .hit(*voice, sample_rate, self.seed ^ ((index as u64 + 1) << 16) ^ take)
                })
                .collect();
            for (step, hit) in part.iter().enumerate() {
                let Some(velocity) = hit.velocity() else { continue };
                let swing = if step % 2 == 1 {
                    f64::from(self.swing) * step_s
                } else {
                    0.0
                };
                let late =
                    (f64::from(gaussian(&mut rng)) * f64::from(self.timing_ms) / 1000.0).clamp(-MOST_OFF_S, MOST_OFF_S);
                let at = (step as f64 * step_s + swing + late).max(0.0);
                let strength = (velocity * (1.0 + 0.3 * self.touch * gaussian(&mut rng))).clamp(0.05, 1.0);
                onsets[step] = Some(onsets[step].map_or(at, |first: f64| first.min(at)));
                let take = &takes[(rng.next_u64() % TAKES) as usize];
                // A softer hit is darker as well as quieter.
                let mut tone = OnePole::new(1_500.0 + 16_000.0 * strength * strength, sample_rate);
                let start = (at * sr).round() as usize;
                let gain = voice.level() * strength;
                for (slot, &x) in dry.iter_mut().skip(start).zip(take) {
                    *slot += gain * tone.low(x);
                }
            }
        }

        // The room around the kit.
        let mut room = Reverb::new(sample_rate, 6.0, self.room_s, 6_000.0);
        let mut take: Vec<f32> = dry
            .iter()
            .map(|&x| {
                let (l, r) = room.process(x, x);
                x + self.room * 0.5 * (l + r)
            })
            .collect();
        normalise(&mut take);

        // Into the sampler: sped up to tempo, through its converters, onto tape.
        let rate = self.bpm / self.played_bpm;
        let mut audio = speed_up(&take, rate, sample_rate);
        if let Some(era) = self.era {
            audio = era.apply(&audio, sample_rate);
        }
        if self.tape > 0.0 {
            let ceiling = soft_clip(self.tape);
            audio.iter_mut().for_each(|x| *x = soft_clip(*x * self.tape) / ceiling);
        }
        let mut converter = Svf::new(self.low_pass_hz, 0.6, sample_rate);
        audio.iter_mut().for_each(|x| *x = converter.process(*x).low);

        // A loop: the room's tail after the last bar rings on into the first.
        let loop_frames = ((loop_frames as f64) / rate).round() as usize;
        let mut looped = audio[..loop_frames.min(audio.len())].to_vec();
        looped.resize(loop_frames, 0.0);
        for (i, &x) in audio.iter().enumerate().skip(loop_frames) {
            looped[i % loop_frames] += x;
        }
        normalise(&mut looped);
        let onsets: Vec<Option<f64>> = onsets.iter().map(|at| at.map(|s| s / rate)).collect();

        let bpm_step_s = 60.0 / self.bpm / 4.0;
        let at = |step: usize| onsets.get(step).copied().flatten().unwrap_or(step as f64 * bpm_step_s);
        let mut slices = Vec::with_capacity(self.slices.len());
        for slice in self.slices {
            let (from, to) = (slice.step as usize, (slice.step + slice.steps) as usize);
            if slice.steps == 0 || to > steps {
                return Err(BreakError::Slice {
                    name: self.name,
                    slice: slice.name,
                });
            }
            let end_s = if to == steps { steps as f64 * bpm_step_s } else { at(to) };
            let frame = |s: f64| (((s - PRE_ROLL_S).max(0.0) * sr).round() as usize).min(loop_frames);
            let (start, end) = (frame(at(from)), frame(end_s));
            let mut cut = looped[start..end.max(start + 1).min(loop_frames)].to_vec();
            let fade_in = (0.0005 * sr) as usize;
            let fade_out = ((0.006 * sr) as usize).min(cut.len());
            for (i, x) in cut.iter_mut().take(fade_in).enumerate() {
                *x *= i as f32 / fade_in as f32;
            }
            let len = cut.len();
            for (i, x) in cut[len - fade_out..].iter_mut().enumerate() {
                *x *= 1.0 - (i as f32 + 1.0) / fade_out as f32;
            }
            normalise(&mut cut);
            slices.push((slice.name, cut));
        }
        Ok(Performance {
            bars: (steps / STEPS_PER_BAR as usize) as u32,
            audio: looped,
            slices,
            onsets,
        })
    }
}

/// The breaks the kits are built from: original performances in the style of
/// the funk and soul records jungle was cut from.
pub mod library {
    use super::*;

    const CUTS: &[SliceDef] = &[
        SliceDef {
            name: "Kick",
            step: 0,
            steps: 2,
        },
        SliceDef {
            name: "Snare",
            step: 4,
            steps: 2,
        },
        SliceDef {
            name: "Ghost",
            step: 7,
            steps: 1,
        },
        SliceDef {
            name: "Hat",
            step: 2,
            steps: 1,
        },
    ];

    /// Syncopated funk: the kick dances around a steady backbeat, ghosts in between.
    pub const ROUGH_RIDER: BreakDef = BreakDef {
        name: "Rough Rider",
        played_bpm: 108.0,
        bpm: 165.0,
        swing: 0.08,
        timing_ms: 6.0,
        touch: 0.5,
        drummer: Drummer::FUNK,
        voices: &[
            (Voice::Kick, "X... .... ..X. .... | X... .... ..X. .x.."),
            (Voice::Snare, ".... X... .... X... | .... X... .... X..."),
            (Voice::Ghost, ".... ...o .o.. ...o | .o.. ...o .... ..o."),
            (Voice::ClosedHat, "x.x. x.x. x.x. x.x. | x.x. x.x. x.x. x..."),
            (Voice::OpenHat, ".... .... .... .... | .... .... .... ..x."),
        ],
        room_s: 0.5,
        room: 0.25,
        era: Some(SamplerEra::DRUM_MACHINE),
        tape: 1.6,
        low_pass_hz: 11_000.0,
        slices: CUTS,
        seed: 0xB4EA_0001,
    };

    /// A gospel shuffle on the ride, the snare lazy behind the beat.
    pub const SUNDAY_SERVICE: BreakDef = BreakDef {
        name: "Sunday Service",
        played_bpm: 96.0,
        bpm: 160.0,
        swing: 0.18,
        timing_ms: 8.0,
        touch: 0.6,
        drummer: Drummer::FUNK,
        voices: &[
            (Voice::Kick, "X... .... .X.. .... | X... ..X. .... ...."),
            (Voice::Snare, ".... X... .... X... | .... X... .... X..."),
            (Voice::Ghost, "..o. ...o ..o. .o.o | ..o. ...o ..o. .o.."),
            (Voice::Ride, "x.x. x.x. x.x. x.x. | x.x. x.x. x.x. x.x."),
            (Voice::Crash, "x... .... .... .... | .... .... .... ...."),
        ],
        room_s: 0.8,
        room: 0.4,
        era: Some(SamplerEra::RACK_SAMPLER),
        tape: 1.3,
        low_pass_hz: 12_000.0,
        slices: CUTS,
        seed: 0xB4EA_0002,
    };

    /// Sparse and heavy: a kick that lands like a door, the hats held back.
    pub const BUNKER_FUNK: BreakDef = BreakDef {
        name: "Bunker Funk",
        played_bpm: 112.0,
        bpm: 170.0,
        swing: 0.04,
        timing_ms: 5.0,
        touch: 0.4,
        drummer: Drummer {
            kick: Kick {
                start_hz: 120.0,
                end_hz: 48.0,
                amp_decay_s: 0.3,
                drive: 2.6,
                ..Drummer::FUNK.kick
            },
            ..Drummer::FUNK
        },
        voices: &[
            (Voice::Kick, "X... .... ..X. ..X. | .... ..X. .... .X.."),
            (Voice::Snare, ".... X... .... X... | .... X... .... X..."),
            (Voice::Ghost, ".... .... .... .... | .o.. .... ..o. ...."),
            (Voice::ClosedHat, "..x. ..x. ..x. ..x. | ..x. ..x. ..x. ..x."),
            (Voice::Tom, ".... .... .... .... | .... .... .... ..x."),
        ],
        room_s: 0.6,
        room: 0.3,
        era: Some(SamplerEra::TRACKER),
        tape: 2.2,
        low_pass_hz: 9_000.0,
        slices: CUTS,
        seed: 0xB4EA_0003,
    };

    /// Smooth and rolling: ride bell, soft ghosts, a drummer who never pushes.
    pub const VELVET_RIDE: BreakDef = BreakDef {
        name: "Velvet Ride",
        played_bpm: 100.0,
        bpm: 172.0,
        swing: 0.1,
        timing_ms: 4.0,
        touch: 0.35,
        drummer: Drummer {
            snare: Snare {
                tone_hz: 230.0,
                noise_decay_s: 0.14,
                drive: 1.2,
                ..Snare::DNB
            },
            ..Drummer::FUNK
        },
        voices: &[
            (Voice::Kick, "X... .... ..X. .... | X... .... .... ...."),
            (Voice::Snare, ".... X... .... X... | .... X... .... X..."),
            (Voice::Ghost, ".... ..o. .o.. ..o. | .... ..o. .oo. ...."),
            (Voice::Ride, "x.xx x.xx x.xx x.xx | x.xx x.xx x.xx x.x."),
        ],
        room_s: 1.0,
        room: 0.35,
        era: None,
        tape: 1.2,
        low_pass_hz: 15_000.0,
        slices: CUTS,
        seed: 0xB4EA_0004,
    };

    /// Tight and busy, sixteenth hats, and a tom fill to end the four bars.
    pub const TIN_CAN: BreakDef = BreakDef {
        name: "Tin Can",
        played_bpm: 118.0,
        bpm: 175.0,
        swing: 0.02,
        timing_ms: 3.0,
        touch: 0.3,
        drummer: Drummer {
            snare: Snare {
                tone_hz: 280.0,
                tone_decay_s: 0.04,
                noise_decay_s: 0.11,
                ..Snare::DNB
            },
            ..Drummer::FUNK
        },
        voices: &[
            (
                Voice::Kick,
                "X... .... ..X. .... | X... ..X. .... .... | X... .... ..X. .... | X... .... .... ....",
            ),
            (
                Voice::Snare,
                ".... X... .... X... | .... X... .... X... | .... X... .... X... | .... X... X.X. XXXX",
            ),
            (
                Voice::Ghost,
                ".o.. ...o .... ...o | .o.. .... .o.. ...o | .o.. ...o .... ...o | .... .... .... ....",
            ),
            (
                Voice::ClosedHat,
                "xxxx xxxx xxxx xxxx | xxxx xxxx xxxx xxxx | xxxx xxxx xxxx xxxx | xxxx xxxx .... ....",
            ),
            (
                Voice::Tom,
                ".... .... .... .... | .... .... .... .... | .... .... .... .... | .... .... .x.x ....",
            ),
        ],
        room_s: 0.4,
        room: 0.2,
        era: Some(SamplerEra::DRUM_MACHINE),
        tape: 1.8,
        low_pass_hz: 12_000.0,
        slices: CUTS,
        seed: 0xB4EA_0005,
    };

    /// Half time: the snare on three, the space around it as heavy as the hits.
    pub const HALF_STEP: BreakDef = BreakDef {
        name: "Half Step",
        played_bpm: 85.0,
        bpm: 170.0,
        swing: 0.06,
        timing_ms: 5.0,
        touch: 0.4,
        drummer: Drummer {
            snare: Snare {
                noise_decay_s: 0.3,
                length_s: 0.6,
                ..Drummer::FUNK.snare
            },
            ..Drummer::FUNK
        },
        voices: &[
            (Voice::Kick, "X... .... .... ..X. | X... ..X. .... ...."),
            (Voice::Snare, ".... .... X... .... | .... .... X... ...."),
            (Voice::Ghost, ".... ...o .... .... | .... .... .... .o.o"),
            (Voice::ClosedHat, "x.x. x.x. x.x. x.x. | x.x. x.x. x.x. x.x."),
        ],
        room_s: 0.9,
        room: 0.35,
        era: Some(SamplerEra::RACK_SAMPLER),
        tape: 1.5,
        low_pass_hz: 12_000.0,
        slices: &[
            SliceDef {
                name: "Kick",
                step: 0,
                steps: 2,
            },
            SliceDef {
                name: "Snare",
                step: 8,
                steps: 4,
            },
            SliceDef {
                name: "Ghost",
                step: 7,
                steps: 1,
            },
            SliceDef {
                name: "Hat",
                step: 2,
                steps: 1,
            },
        ],
        seed: 0xB4EA_0006,
    };

    pub const ALL: [&BreakDef; 6] = [
        &ROUGH_RIDER,
        &SUNDAY_SERVICE,
        &BUNKER_FUNK,
        &VELVET_RIDE,
        &TIN_CAN,
        &HALF_STEP,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    #[test]
    fn every_break_performs_whole_bars_at_its_tempo_with_its_slices() {
        for def in library::ALL {
            let take = def.perform(SR).unwrap_or_else(|e| panic!("{e}"));
            let bar_s = 4.0 * 60.0 / def.bpm;
            let expected = (f64::from(take.bars) * bar_s * f64::from(SR)).round() as usize;
            assert!(
                take.audio.len().abs_diff(expected) <= 2,
                "{}: {} frames",
                def.name,
                take.audio.len()
            );
            let peak = take.audio.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!((peak - HIT_PEAK).abs() < 1e-3, "{}", def.name);
            assert_eq!(take.slices.len(), def.slices.len());
            for (name, cut) in &take.slices {
                assert!(cut.len() > SR as usize / 50, "{}: slice {name} too short", def.name);
                assert_eq!(
                    cut.last().copied(),
                    Some(0.0),
                    "{}: slice {name} ends on silence",
                    def.name
                );
            }
        }
    }

    #[test]
    fn the_drummer_is_human_but_never_lost() {
        let take = library::ROUGH_RIDER.perform(SR).expect("valid");
        let step_s = 60.0 / library::ROUGH_RIDER.bpm / 4.0;
        let offsets: Vec<f64> = take
            .onsets
            .iter()
            .enumerate()
            .filter_map(|(i, at)| at.map(|at| at - i as f64 * step_s))
            .collect();
        assert!(
            offsets.iter().any(|&o| o.abs() > 0.0005),
            "someone played it, not a grid"
        );
        let rate = library::ROUGH_RIDER.bpm / library::ROUGH_RIDER.played_bpm;
        let swing_s = f64::from(library::ROUGH_RIDER.swing) * step_s;
        assert!(offsets.iter().all(|&o| o.abs() <= MOST_OFF_S / rate + swing_s + 1e-9));
    }

    #[test]
    fn the_same_break_is_the_same_bytes() {
        let a = library::TIN_CAN.perform(SR).expect("valid");
        let b = library::TIN_CAN.perform(SR).expect("valid");
        assert_eq!(a, b);
        let other = BreakDef {
            seed: 99,
            ..library::TIN_CAN
        };
        assert_ne!(other.perform(SR).expect("valid").audio, a.audio, "another take");
    }

    #[test]
    fn the_loop_has_no_seam() {
        // The room's tail wraps round: the loop's last frame flows into its first.
        let take = library::SUNDAY_SERVICE.perform(SR).expect("valid");
        let audio = &take.audio;
        let seam = (audio[0] - audio[audio.len() - 1]).abs();
        let typical = audio.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(seam < typical, "seam {seam}, biggest step {typical}");
    }

    #[test]
    fn mistakes_are_explained() {
        let ragged = BreakDef {
            voices: &[
                (Voice::Kick, "X... .... .... ...."),
                (Voice::Snare, "X... .... .... .... | ...."),
            ],
            ..library::ROUGH_RIDER
        };
        assert!(ragged.perform(SR).is_err());
        let past = BreakDef {
            slices: &[SliceDef {
                name: "Too far",
                step: 30,
                steps: 4,
            }],
            ..library::ROUGH_RIDER
        };
        assert_eq!(
            past.perform(SR),
            Err(BreakError::Slice {
                name: "Rough Rider",
                slice: "Too far"
            })
        );
    }
}
