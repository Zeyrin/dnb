//! Kits: what the eight pads play.

use std::fmt;
use std::sync::Arc;

use wu_dsp::Sample;

use crate::bus::{Bus, Sends};
use crate::drums::{Hat, Kick, Rim, Snare, Tom};

pub const PAD_COUNT: usize = 8;

/// The eight pads, in the order of the default "Reel" layout:
/// D-pad ↑ ↓ ← →, then △ □ ✕ ○.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Pad {
    P1,
    P2,
    P3,
    P4,
    P5,
    P6,
    P7,
    P8,
}

impl Pad {
    pub const ALL: [Pad; PAD_COUNT] = [Pad::P1, Pad::P2, Pad::P3, Pad::P4, Pad::P5, Pad::P6, Pad::P7, Pad::P8];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(index: usize) -> Option<Pad> {
        Pad::ALL.get(index).copied()
    }
}

impl fmt::Display for Pad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{}", self.index() + 1)
    }
}

/// One pad's sound and how it sits in the mix.
#[derive(Clone, Debug)]
pub struct PadSound {
    pub name: String,
    pub sample: Arc<Sample>,
    /// Linear gain at full velocity.
    pub gain: f32,
    /// -1 (left) to 1 (right).
    pub pan: f32,
    /// Pads sharing a choke group cut each other off, like a hi-hat pedal.
    pub choke: Option<u8>,
    pub bus: Bus,
    /// Its hits duck the bass bus (the kick's job).
    pub sidechain: bool,
    pub sends: Sends,
}

#[derive(Clone, Debug)]
pub struct Kit {
    pub name: String,
    pub pads: [PadSound; PAD_COUNT],
}

impl Kit {
    pub fn pad(&self, pad: Pad) -> &PadSound {
        &self.pads[pad.index()]
    }

    /// The default kit: '93 ragga jungle. Kick and snare on the left thumb,
    /// hats and percussion on the right.
    pub fn ragga_93(sample_rate: u32) -> Kit {
        let sound = |name: &str, samples: Vec<f32>, gain: f32, pan: f32, choke: Option<u8>| PadSound {
            name: name.to_owned(),
            sample: Arc::new(Sample::mono(samples, sample_rate)),
            gain,
            pan,
            choke,
            bus: Bus::Drums,
            sidechain: false,
            sends: Sends::DRY,
        };
        const HATS: Option<u8> = Some(1);
        let mut kit = Kit {
            name: "Ragga '93".to_owned(),
            pads: [
                sound("Kick", Kick::DNB.render(sample_rate, 0x93_01), 1.0, 0.0, None),
                sound("Snare", Snare::DNB.render(sample_rate, 0x93_02), 0.85, 0.0, None),
                sound("Ghost", Snare::GHOST.render(sample_rate, 0x93_03), 0.4, -0.1, None),
                sound("Rim", Rim::CLASSIC.render(sample_rate, 0x93_04), 0.45, 0.15, None),
                sound(
                    "Jungle Snare",
                    Snare::JUNGLE.render(sample_rate, 0x93_05),
                    0.8,
                    0.05,
                    None,
                ),
                sound("Low Tom", Tom::LOW.render(sample_rate, 0x93_06), 0.55, -0.25, None),
                sound("Closed Hat", Hat::CLOSED.render(sample_rate, 0x93_07), 0.3, 0.2, HATS),
                sound("Open Hat", Hat::OPEN.render(sample_rate, 0x93_08), 0.28, 0.25, HATS),
            ],
        };
        kit.pads[Pad::P1.index()].sidechain = true;
        kit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_round_trip_through_indices() {
        for (i, pad) in Pad::ALL.into_iter().enumerate() {
            assert_eq!(pad.index(), i);
            assert_eq!(Pad::from_index(i), Some(pad));
        }
        assert_eq!(Pad::from_index(8), None);
        assert_eq!(Pad::P7.to_string(), "P7");
    }

    #[test]
    fn the_default_kit_has_eight_baked_sounds_and_choked_hats() {
        let kit = Kit::ragga_93(48_000);
        assert!(
            kit.pads
                .iter()
                .all(|p| p.sample.frames() > 0 && p.sample.sample_rate() == 48_000)
        );
        assert_eq!(kit.pad(Pad::P1).name, "Kick");
        assert!(kit.pad(Pad::P1).sidechain, "the kick ducks the bass");
        assert_eq!(kit.pads.iter().filter(|p| p.sidechain).count(), 1);
        assert_eq!(kit.pad(Pad::P7).choke, kit.pad(Pad::P8).choke);
        assert!(kit.pad(Pad::P7).choke.is_some());
    }
}
