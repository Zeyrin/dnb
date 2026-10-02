//! The built-in kits, as data: what each pad plays and the breaks behind
//! them. Baking turns a definition into samples, deterministically, so a baked
//! kit can be kept and recognised by its fingerprint.
//!
//! Every kit gives its pads the same roles, so a pattern written for one plays
//! on any: P1 kick, P2 snare, P3 ghost, P4 rim (or clap), P5 the jungle snare
//! (usually a slice of the kit's break), P6 a low tom (or a shaker), P7 the
//! closed hat and P8 the open hat, choking each other.

use std::sync::Arc;

use wu_dsp::{Sample, SamplerEra};

use crate::breaks::{BreakDef, BreakError, library};
use crate::bus::{Bus, Sends};
use crate::drums::{Clap, Cymbal, Hat, Kick, Rim, Shaker, Snare, Tom};
use crate::kit::{Break, Kit, PAD_COUNT, PadSound};

/// What a pad plays, before it is baked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sound {
    Kick(Kick),
    Snare(Snare),
    Rim(Rim),
    Clap(Clap),
    Tom(Tom),
    Hat(Hat),
    Cymbal(Cymbal),
    Shaker(Shaker),
    /// A slice of one of the kit's breaks: the break's index, the slice's name.
    Slice(usize, &'static str),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadDef {
    pub name: &'static str,
    pub sound: Sound,
    pub gain: f32,
    pub pan: f32,
    pub choke: Option<u8>,
    pub sends: Sends,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KitDef {
    /// How songs name it: "ragga-93".
    pub id: &'static str,
    pub name: &'static str,
    /// Each pad's seed counts up from this.
    pub seed: u64,
    pub pads: [PadDef; PAD_COUNT],
    pub breaks: &'static [&'static BreakDef],
}

/// A kit's sounds, baked: every pad's hit, then every break's loop, by name.
pub type Baked = Vec<(String, Vec<f32>)>;

const HATS: Option<u8> = Some(1);
const DRY: Sends = Sends::DRY;

const fn pad(name: &'static str, sound: Sound, gain: f32, pan: f32) -> PadDef {
    PadDef {
        name,
        sound,
        gain,
        pan,
        choke: None,
        sends: DRY,
    }
}

const fn hat(name: &'static str, hat: Hat, gain: f32, pan: f32) -> PadDef {
    PadDef {
        choke: HATS,
        ..pad(name, Sound::Hat(hat), gain, pan)
    }
}

const fn wet(def: PadDef, reverb: f32) -> PadDef {
    PadDef {
        sends: Sends { reverb, delay: 0.0 },
        ..def
    }
}

fn pad_name(index: usize) -> String {
    format!("pad {}", index + 1)
}

fn break_name(index: usize) -> String {
    format!("break {}", index + 1)
}

impl KitDef {
    /// Bakes every pad and every break. The same definition gives the same
    /// samples, to the bit, on one platform.
    pub fn bake(&self, sample_rate: u32) -> Result<Baked, BreakError> {
        let performances = self
            .breaks
            .iter()
            .map(|def| def.perform(sample_rate))
            .collect::<Result<Vec<_>, _>>()?;
        let mut baked = Vec::with_capacity(PAD_COUNT + performances.len());
        for (i, pad) in self.pads.iter().enumerate() {
            let seed = self.seed + i as u64 + 1;
            let hit = match pad.sound {
                Sound::Kick(kick) => kick.render(sample_rate, seed),
                Sound::Snare(snare) => snare.render(sample_rate, seed),
                Sound::Rim(rim) => rim.render(sample_rate, seed),
                Sound::Clap(clap) => clap.render(sample_rate, seed),
                Sound::Tom(tom) => tom.render(sample_rate, seed),
                Sound::Hat(hat) => hat.render(sample_rate, seed),
                Sound::Cymbal(cymbal) => cymbal.render(sample_rate, seed),
                Sound::Shaker(shaker) => shaker.render(sample_rate, seed),
                Sound::Slice(index, slice) => performances
                    .get(index)
                    .and_then(|take| take.slices.iter().find(|(name, _)| *name == slice))
                    .map(|(_, cut)| cut.clone())
                    .unwrap_or_default(),
            };
            baked.push((pad_name(i), hit));
        }
        for (i, take) in performances.into_iter().enumerate() {
            baked.push((break_name(i), take.audio));
        }
        Ok(baked)
    }

    /// Builds the kit from its baked samples; `None` if any is missing or empty.
    pub fn assemble(&self, sample_rate: u32, baked: &Baked) -> Option<Kit> {
        let find = |name: &str| {
            baked
                .iter()
                .find(|(n, samples)| n == name && !samples.is_empty())
                .map(|(_, samples)| Arc::new(Sample::mono(samples.clone(), sample_rate)))
        };
        let mut pads = Vec::with_capacity(PAD_COUNT);
        for (i, def) in self.pads.iter().enumerate() {
            pads.push(PadSound {
                name: def.name.to_owned(),
                sample: find(&pad_name(i))?,
                gain: def.gain,
                pan: def.pan,
                choke: def.choke,
                bus: Bus::Drums,
                // The kick ducks the bass, in every kit.
                sidechain: i == 0,
                sends: def.sends,
            });
        }
        let breaks = self
            .breaks
            .iter()
            .enumerate()
            .map(|(i, def)| {
                Some(Break {
                    name: def.name.to_owned(),
                    bpm: def.bpm,
                    bars: def.bars(),
                    sample: find(&break_name(i))?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Kit {
            name: self.name.to_owned(),
            pads: pads.try_into().ok()?,
            breaks,
        })
    }

    /// Bakes and assembles. The built-in kits always bake (a test sees to it).
    pub fn kit(&self, sample_rate: u32) -> Kit {
        self.bake(sample_rate)
            .ok()
            .and_then(|baked| self.assemble(sample_rate, &baked))
            .unwrap_or_else(|| panic!("kit {} doesn't bake", self.id))
    }

    /// Changes whenever anything that shapes the baked sound does: the
    /// definition, the sample rate, or the code that synthesises it.
    pub fn fingerprint(&self, sample_rate: u32) -> u64 {
        let mut hash = Fnv::new();
        hash.write(b"wheelup kit 1");
        hash.write(&sample_rate.to_le_bytes());
        hash.write(format!("{self:?}").as_bytes());
        for source in SYNTHESIS {
            hash.write(source.as_bytes());
        }
        hash.finish()
    }
}

/// The code a baked kit's sound depends on: its text is part of the fingerprint.
const SYNTHESIS: [&str; 10] = [
    include_str!("drums.rs"),
    include_str!("breaks.rs"),
    include_str!("kits.rs"),
    include_str!("steps.rs"),
    include_str!("../../wu-dsp/src/era.rs"),
    include_str!("../../wu-dsp/src/filter.rs"),
    include_str!("../../wu-dsp/src/reverb.rs"),
    include_str!("../../wu-dsp/src/rng.rs"),
    include_str!("../../wu-dsp/src/osc.rs"),
    include_str!("../../wu-dsp/src/shape.rs"),
];

/// FNV-1a, 64 bits: stable across platforms and releases, unlike std's hasher.
struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// The kit by its id.
pub fn kit_def(id: &str) -> Option<&'static KitDef> {
    KITS.iter().find(|kit| kit.id == id)
}

pub const KITS: [KitDef; 8] = [
    RAGGA_93,
    DARKSIDE_92,
    ATMOS_95,
    LIQUID_VELVET,
    JUMP_UP_TIN,
    NEURO_LAB,
    HALFTIME_HEAVY,
    MINIMAL_ROLLER,
];

/// '93 ragga jungle: a punchy kick, a crisp snare, a crunched jungle snare on top.
pub const RAGGA_93: KitDef = KitDef {
    id: "ragga-93",
    name: "Ragga '93",
    seed: 0x93_00,
    pads: [
        pad("Kick", Sound::Kick(Kick::DNB), 1.0, 0.0),
        pad("Snare", Sound::Snare(Snare::DNB), 0.85, 0.0),
        pad("Ghost", Sound::Snare(Snare::GHOST), 0.4, -0.1),
        pad("Rim", Sound::Rim(Rim::CLASSIC), 0.45, 0.15),
        pad("Jungle Snare", Sound::Snare(Snare::JUNGLE), 0.8, 0.05),
        pad("Low Tom", Sound::Tom(Tom::LOW), 0.55, -0.25),
        hat("Closed Hat", Hat::CLOSED, 0.3, 0.2),
        hat("Open Hat", Hat::OPEN, 0.28, 0.25),
    ],
    breaks: &[&library::ROUGH_RIDER, &library::SUNDAY_SERVICE],
};

/// '92 darkside: a long, distorted kick, everything through an 8-bit tracker.
pub const DARKSIDE_92: KitDef = KitDef {
    id: "darkside-92",
    name: "Darkside '92",
    seed: 0x92_00,
    pads: [
        pad(
            "Kick",
            Sound::Kick(Kick {
                start_hz: 150.0,
                end_hz: 45.0,
                pitch_decay_s: 0.04,
                amp_decay_s: 0.18,
                click: 0.25,
                drive: 3.0,
                length_s: 0.5,
            }),
            1.0,
            0.0,
        ),
        wet(
            pad(
                "Snare",
                Sound::Snare(Snare {
                    tone_hz: 175.0,
                    tone_ratio: 1.6,
                    tone_decay_s: 0.07,
                    snap: 0.3,
                    noise_hp_hz: 900.0,
                    noise_lp_hz: 7_000.0,
                    noise_decay_s: 0.2,
                    tone_level: 0.7,
                    noise_level: 0.8,
                    drive: 2.5,
                    length_s: 0.5,
                    era: Some(SamplerEra::TRACKER),
                }),
                0.85,
                0.0,
            ),
            0.15,
        ),
        pad(
            "Ghost",
            Sound::Snare(Snare {
                era: Some(SamplerEra::TRACKER),
                ..Snare::GHOST
            }),
            0.4,
            -0.1,
        ),
        pad(
            "Rim",
            Sound::Rim(Rim {
                high_hz: 1_400.0,
                low_hz: 420.0,
                length_s: 0.12,
            }),
            0.4,
            0.15,
        ),
        pad("Jungle Snare", Sound::Slice(0, "Snare"), 0.85, 0.05),
        pad(
            "Low Tom",
            Sound::Tom(Tom {
                start_hz: 120.0,
                end_hz: 70.0,
                pitch_decay_s: 0.08,
                amp_decay_s: 0.3,
                noise: 0.1,
                length_s: 0.6,
            }),
            0.6,
            -0.25,
        ),
        hat(
            "Closed Hat",
            Hat {
                decay_s: 0.025,
                length_s: 0.1,
                pitch: 0.85,
                noise: 0.35,
            },
            0.28,
            0.2,
        ),
        hat(
            "Open Hat",
            Hat {
                decay_s: 0.25,
                length_s: 0.7,
                pitch: 0.85,
                noise: 0.4,
            },
            0.25,
            0.25,
        ),
    ],
    breaks: &[&library::BUNKER_FUNK],
};

/// '95 atmospheric: soft kick, roomy snare, a ride-led break behind.
pub const ATMOS_95: KitDef = KitDef {
    id: "atmos-95",
    name: "Atmos '95",
    seed: 0x95_00,
    pads: [
        pad(
            "Kick",
            Sound::Kick(Kick {
                start_hz: 160.0,
                end_hz: 55.0,
                pitch_decay_s: 0.03,
                amp_decay_s: 0.18,
                click: 0.2,
                drive: 1.4,
                length_s: 0.45,
            }),
            1.0,
            0.0,
        ),
        wet(
            pad(
                "Snare",
                Sound::Snare(Snare {
                    tone_hz: 220.0,
                    tone_ratio: 1.6,
                    tone_decay_s: 0.05,
                    snap: 0.3,
                    noise_hp_hz: 1_300.0,
                    noise_lp_hz: 10_000.0,
                    noise_decay_s: 0.17,
                    tone_level: 0.5,
                    noise_level: 0.9,
                    drive: 1.3,
                    length_s: 0.45,
                    era: Some(SamplerEra::RACK_SAMPLER),
                }),
                0.75,
                0.0,
            ),
            0.35,
        ),
        wet(pad("Ghost", Sound::Snare(Snare::GHOST), 0.35, -0.1), 0.2),
        wet(pad("Rim", Sound::Rim(Rim::CLASSIC), 0.35, 0.15), 0.3),
        wet(pad("Jungle Snare", Sound::Slice(0, "Snare"), 0.8, 0.05), 0.2),
        wet(pad("Low Tom", Sound::Tom(Tom::LOW), 0.5, -0.25), 0.2),
        hat(
            "Closed Hat",
            Hat {
                pitch: 1.1,
                noise: 0.2,
                ..Hat::CLOSED
            },
            0.25,
            0.2,
        ),
        hat(
            "Open Hat",
            Hat {
                pitch: 1.1,
                ..Hat::OPEN
            },
            0.22,
            0.25,
        ),
    ],
    breaks: &[&library::VELVET_RIDE, &library::SUNDAY_SERVICE],
};

/// Liquid: clean and crisp, a clap for the rim, a shaker for the tom.
pub const LIQUID_VELVET: KitDef = KitDef {
    id: "liquid-velvet",
    name: "Liquid Velvet",
    seed: 0x1C_00,
    pads: [
        pad(
            "Kick",
            Sound::Kick(Kick {
                start_hz: 170.0,
                end_hz: 50.0,
                pitch_decay_s: 0.028,
                amp_decay_s: 0.16,
                click: 0.4,
                drive: 1.8,
                length_s: 0.4,
            }),
            1.0,
            0.0,
        ),
        wet(
            pad(
                "Snare",
                Sound::Snare(Snare {
                    tone_hz: 200.0,
                    tone_ratio: 1.7,
                    tone_decay_s: 0.05,
                    snap: 0.4,
                    noise_hp_hz: 1_200.0,
                    noise_lp_hz: 12_000.0,
                    noise_decay_s: 0.13,
                    tone_level: 0.55,
                    noise_level: 0.85,
                    drive: 1.5,
                    length_s: 0.38,
                    era: None,
                }),
                0.8,
                0.0,
            ),
            0.2,
        ),
        pad("Ghost", Sound::Snare(Snare::GHOST), 0.38, -0.1),
        wet(pad("Clap", Sound::Clap(Clap::CLASSIC), 0.45, 0.1), 0.25),
        pad("Jungle Snare", Sound::Slice(0, "Snare"), 0.75, 0.05),
        pad("Shaker", Sound::Shaker(Shaker::CLASSIC), 0.35, -0.3),
        hat(
            "Closed Hat",
            Hat {
                decay_s: 0.03,
                length_s: 0.12,
                pitch: 1.2,
                noise: 0.15,
            },
            0.28,
            0.2,
        ),
        hat(
            "Open Hat",
            Hat {
                decay_s: 0.2,
                length_s: 0.6,
                pitch: 1.2,
                noise: 0.2,
            },
            0.24,
            0.25,
        ),
    ],
    breaks: &[&library::VELVET_RIDE],
};

/// Jump-up: a tight kick, a tinny snare cranked through a drum machine.
pub const JUMP_UP_TIN: KitDef = KitDef {
    id: "jump-up-tin",
    name: "Jump-Up Tin",
    seed: 0x7100,
    pads: [
        pad(
            "Kick",
            Sound::Kick(Kick {
                start_hz: 210.0,
                end_hz: 55.0,
                pitch_decay_s: 0.022,
                amp_decay_s: 0.13,
                click: 0.5,
                drive: 2.5,
                length_s: 0.35,
            }),
            1.0,
            0.0,
        ),
        pad(
            "Snare",
            Sound::Snare(Snare {
                tone_hz: 290.0,
                tone_ratio: 1.5,
                tone_decay_s: 0.035,
                snap: 0.6,
                noise_hp_hz: 2_000.0,
                noise_lp_hz: 12_000.0,
                noise_decay_s: 0.1,
                tone_level: 0.6,
                noise_level: 0.9,
                drive: 2.2,
                length_s: 0.3,
                era: Some(SamplerEra::DRUM_MACHINE),
            }),
            0.85,
            0.0,
        ),
        pad(
            "Ghost",
            Sound::Snare(Snare {
                tone_hz: 280.0,
                ..Snare::GHOST
            }),
            0.38,
            -0.1,
        ),
        pad("Clap", Sound::Clap(Clap::CLASSIC), 0.5, 0.1),
        pad("Jungle Snare", Sound::Slice(0, "Snare"), 0.85, 0.05),
        pad(
            "High Tom",
            Sound::Tom(Tom {
                start_hz: 200.0,
                end_hz: 130.0,
                pitch_decay_s: 0.05,
                amp_decay_s: 0.15,
                noise: 0.2,
                length_s: 0.35,
            }),
            0.5,
            -0.25,
        ),
        hat(
            "Closed Hat",
            Hat {
                decay_s: 0.02,
                length_s: 0.08,
                pitch: 1.3,
                noise: 0.2,
            },
            0.3,
            0.2,
        ),
        hat(
            "Open Hat",
            Hat {
                decay_s: 0.15,
                length_s: 0.45,
                pitch: 1.3,
                noise: 0.25,
            },
            0.25,
            0.25,
        ),
    ],
    breaks: &[&library::TIN_CAN],
};

/// Neurofunk: a hard, clicky kick, a driven snare, small bright metal.
pub const NEURO_LAB: KitDef = KitDef {
    id: "neuro-lab",
    name: "Neuro Lab",
    seed: 0x4E00,
    pads: [
        pad(
            "Kick",
            Sound::Kick(Kick {
                start_hz: 230.0,
                end_hz: 48.0,
                pitch_decay_s: 0.02,
                amp_decay_s: 0.16,
                click: 0.6,
                drive: 3.2,
                length_s: 0.4,
            }),
            1.0,
            0.0,
        ),
        wet(
            pad(
                "Snare",
                Sound::Snare(Snare {
                    tone_hz: 190.0,
                    tone_ratio: 1.8,
                    tone_decay_s: 0.05,
                    snap: 0.5,
                    noise_hp_hz: 1_500.0,
                    noise_lp_hz: 14_000.0,
                    noise_decay_s: 0.14,
                    tone_level: 0.7,
                    noise_level: 0.8,
                    drive: 3.0,
                    length_s: 0.4,
                    era: None,
                }),
                0.9,
                0.0,
            ),
            0.12,
        ),
        pad("Ghost", Sound::Snare(Snare::GHOST), 0.35, -0.1),
        pad(
            "Rim",
            Sound::Rim(Rim {
                high_hz: 2_200.0,
                low_hz: 600.0,
                length_s: 0.1,
            }),
            0.4,
            0.15,
        ),
        pad("Jungle Snare", Sound::Slice(0, "Snare"), 0.8, 0.05),
        pad(
            "Low Tom",
            Sound::Tom(Tom {
                start_hz: 110.0,
                end_hz: 60.0,
                pitch_decay_s: 0.04,
                amp_decay_s: 0.25,
                noise: 0.3,
                length_s: 0.5,
            }),
            0.6,
            -0.25,
        ),
        hat(
            "Closed Hat",
            Hat {
                decay_s: 0.022,
                length_s: 0.09,
                pitch: 1.5,
                noise: 0.1,
            },
            0.27,
            0.2,
        ),
        hat(
            "Open Hat",
            Hat {
                decay_s: 0.18,
                length_s: 0.5,
                pitch: 1.5,
                noise: 0.15,
            },
            0.22,
            0.25,
        ),
    ],
    breaks: &[&library::TIN_CAN, &library::BUNKER_FUNK],
};

/// Half time: a kick like a landslide, a snare with a long tail, and space.
pub const HALFTIME_HEAVY: KitDef = KitDef {
    id: "halftime-heavy",
    name: "Halftime Heavy",
    seed: 0x4800,
    pads: [
        pad(
            "Kick",
            Sound::Kick(Kick {
                start_hz: 140.0,
                end_hz: 42.0,
                pitch_decay_s: 0.045,
                amp_decay_s: 0.35,
                click: 0.3,
                drive: 2.8,
                length_s: 0.7,
            }),
            1.0,
            0.0,
        ),
        wet(
            pad(
                "Snare",
                Sound::Snare(Snare {
                    tone_hz: 170.0,
                    tone_ratio: 1.6,
                    tone_decay_s: 0.09,
                    snap: 0.4,
                    noise_hp_hz: 900.0,
                    noise_lp_hz: 9_000.0,
                    noise_decay_s: 0.32,
                    tone_level: 0.7,
                    noise_level: 0.9,
                    drive: 2.4,
                    length_s: 0.7,
                    era: None,
                }),
                0.9,
                0.0,
            ),
            0.3,
        ),
        pad("Ghost", Sound::Snare(Snare::GHOST), 0.35, -0.1),
        wet(
            pad(
                "Clap",
                Sound::Clap(Clap {
                    band_hz: 1_000.0,
                    tail_decay_s: 0.25,
                    length_s: 0.6,
                }),
                0.45,
                0.1,
            ),
            0.3,
        ),
        pad("Jungle Snare", Sound::Slice(0, "Snare"), 0.85, 0.05),
        pad(
            "Low Tom",
            Sound::Tom(Tom {
                start_hz: 100.0,
                end_hz: 55.0,
                pitch_decay_s: 0.07,
                amp_decay_s: 0.35,
                noise: 0.1,
                length_s: 0.7,
            }),
            0.6,
            -0.25,
        ),
        hat(
            "Closed Hat",
            Hat {
                pitch: 0.95,
                ..Hat::CLOSED
            },
            0.26,
            0.2,
        ),
        hat(
            "Open Hat",
            Hat {
                pitch: 0.95,
                ..Hat::OPEN
            },
            0.22,
            0.25,
        ),
    ],
    breaks: &[&library::HALF_STEP],
};

/// Rollers: short, clean, a shaker keeping the sixteenths moving.
pub const MINIMAL_ROLLER: KitDef = KitDef {
    id: "minimal-roller",
    name: "Minimal Roller",
    seed: 0x5200,
    pads: [
        pad(
            "Kick",
            Sound::Kick(Kick {
                start_hz: 180.0,
                end_hz: 52.0,
                pitch_decay_s: 0.025,
                amp_decay_s: 0.12,
                click: 0.35,
                drive: 1.6,
                length_s: 0.32,
            }),
            1.0,
            0.0,
        ),
        pad(
            "Snare",
            Sound::Snare(Snare {
                tone_hz: 210.0,
                noise_decay_s: 0.1,
                drive: 1.4,
                length_s: 0.3,
                ..Snare::DNB
            }),
            0.75,
            0.0,
        ),
        pad("Ghost", Sound::Snare(Snare::GHOST), 0.35, -0.1),
        pad("Rim", Sound::Rim(Rim::CLASSIC), 0.5, 0.15),
        pad("Jungle Snare", Sound::Slice(0, "Snare"), 0.75, 0.05),
        pad(
            "Shaker",
            Sound::Shaker(Shaker {
                attack_s: 0.008,
                decay_s: 0.04,
                band_hz: 8_000.0,
                length_s: 0.12,
            }),
            0.3,
            -0.3,
        ),
        hat(
            "Closed Hat",
            Hat {
                decay_s: 0.02,
                length_s: 0.08,
                pitch: 1.1,
                noise: 0.2,
            },
            0.28,
            0.2,
        ),
        hat(
            "Open Hat",
            Hat {
                decay_s: 0.12,
                length_s: 0.35,
                pitch: 1.1,
                noise: 0.25,
            },
            0.22,
            0.25,
        ),
    ],
    breaks: &[&library::ROUGH_RIDER],
};

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    #[test]
    fn every_kit_bakes_eight_pads_and_its_breaks() {
        for def in &KITS {
            let kit = def.kit(SR);
            assert_eq!(kit.breaks.len(), def.breaks.len(), "{}", def.id);
            assert!((1..=3).contains(&kit.breaks.len()), "{}: one to three breaks", def.id);
            for pad in &kit.pads {
                assert!(pad.sample.frames() > 0, "{}: {} is silent", def.id, pad.name);
                assert!(pad.sample.peak() > 0.5, "{}: {} is too quiet", def.id, pad.name);
            }
            assert!(kit.pads[0].sidechain, "{}: the kick ducks the bass", def.id);
            assert_eq!(kit.pads[6].choke, kit.pads[7].choke, "{}: the hats choke", def.id);
        }
    }

    #[test]
    fn every_kit_has_its_own_id_and_seed() {
        for (i, a) in KITS.iter().enumerate() {
            for b in &KITS[i + 1..] {
                assert_ne!(a.id, b.id);
                assert_ne!(a.seed, b.seed, "{} and {}", a.id, b.id);
            }
        }
        assert_eq!(kit_def("darkside-92").map(|k| k.name), Some("Darkside '92"));
        assert!(kit_def("kazoo").is_none());
    }

    #[test]
    fn baking_is_deterministic_and_fingerprinted() {
        assert_eq!(DARKSIDE_92.bake(SR), DARKSIDE_92.bake(SR));
        assert_eq!(DARKSIDE_92.fingerprint(SR), DARKSIDE_92.fingerprint(SR));
        assert_ne!(DARKSIDE_92.fingerprint(SR), DARKSIDE_92.fingerprint(44_100));
        let louder = KitDef { seed: 1, ..DARKSIDE_92 };
        assert_ne!(DARKSIDE_92.fingerprint(SR), louder.fingerprint(SR));
    }

    #[test]
    fn a_kit_missing_a_sample_does_not_assemble() {
        let mut baked = RAGGA_93.bake(SR).expect("bakes");
        assert!(RAGGA_93.assemble(SR, &baked).is_some());
        baked.retain(|(name, _)| name != "pad 3");
        assert!(RAGGA_93.assemble(SR, &baked).is_none());
    }
}
