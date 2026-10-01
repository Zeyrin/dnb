//! Where a sound sits in the mix.

/// The mixer's buses. Every sound plays into one; the buses are levelled,
/// processed and summed into the master.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Bus {
    #[default]
    Drums,
    /// Ducks under the kick, so the two never fight for the low end.
    Bass,
    /// Stabs, pads, keys and leads.
    Music,
    /// Sirens, horns, risers, the crowd.
    Fx,
}

impl Bus {
    pub const ALL: [Bus; 4] = [Bus::Drums, Bus::Bass, Bus::Music, Bus::Fx];

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// How much of a sound goes to the reverb and to the dub delay, 0–1 each.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sends {
    pub reverb: f32,
    pub delay: f32,
}

impl Sends {
    pub const DRY: Sends = Sends {
        reverb: 0.0,
        delay: 0.0,
    };

    pub fn is_dry(&self) -> bool {
        self.reverb <= 0.0 && self.delay <= 0.0
    }
}
