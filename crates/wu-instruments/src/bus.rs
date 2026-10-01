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
