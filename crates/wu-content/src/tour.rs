//! The Pirate Radio Tour: the stops, what each plays, and what it asks.

use serde::{Deserialize, Serialize};

/// The tour that ships with the game.
pub const TOUR: &str = include_str!("../../../content/tour.ron");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tour {
    pub venues: Vec<Venue>,
}

/// One stop of the tour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Venue {
    pub id: String,
    pub name: String,
    /// The place, in a line.
    pub line: String,
    /// The tunes it plays, by song id, in order. Empty while they are being cut.
    pub set: Vec<String>,
    /// Played once the set has earned `encore_stars`.
    #[serde(default)]
    pub encore: Option<String>,
    #[serde(default)]
    pub encore_stars: u32,
    /// Stars earned on the whole tour needed to get in.
    pub opens_at: u32,
    pub challenge: Challenge,
}

/// What a stop asks of the player, beyond playing it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Challenge {
    /// A full combo on any tune of the set.
    FullCombo,
    /// Every tune of the set at this grade or better ("A", "S"…).
    AllAtLeast(String),
    /// So many stars from the set and its encore.
    Stars(u32),
}

impl Challenge {
    pub fn describe(&self) -> String {
        match self {
            Challenge::FullCombo => "a full combo on any tune of the set".to_owned(),
            Challenge::AllAtLeast(grade) => format!("every tune of the set at {grade} or better"),
            Challenge::Stars(n) => format!("{n} stars from this stop"),
        }
    }
}

impl Tour {
    pub fn load() -> Result<Tour, ron::error::SpannedError> {
        ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(TOUR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::songs::BUILTIN;

    #[test]
    fn the_tour_plays_built_in_tunes_each_once_and_opens_stop_by_stop() {
        let tour = Tour::load().expect("the tour parses");
        assert_eq!(tour.venues.len(), 6, "the brief's six stops");
        let mut played = Vec::new();
        for venue in &tour.venues {
            for id in venue.set.iter().chain(&venue.encore) {
                let song = BUILTIN
                    .iter()
                    .find(|s| s.id == id)
                    .unwrap_or_else(|| panic!("{}: no {id}", venue.id));
                let compiled = song.load().expect("compiles");
                assert!(
                    !compiled.is_lesson(),
                    "{}: {id} is a lesson, which sets no record",
                    venue.id
                );
                assert!(!played.contains(id), "{id} plays twice on the tour");
                played.push(id.clone());
            }
            // A set's tunes must be able to earn its encore: five stars each.
            assert!(venue.encore_stars <= 5 * venue.set.len() as u32, "{}", venue.id);
        }
        // The first stop is open from the start, and each opens later than the last.
        assert_eq!(tour.venues[0].opens_at, 0);
        assert!(tour.venues.windows(2).all(|w| w[0].opens_at < w[1].opens_at));
    }
}
