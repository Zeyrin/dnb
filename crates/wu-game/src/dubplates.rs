//! Dubplates: what the Pirate Radio Tour presses for the player, spent on kits
//! to play any tune on and stages to play it in front of. Read off the records
//! like the tour's progress, so every real run pays out, on the tour or not.

use wu_chart::Difficulty;
use wu_content::tour::Tour;
use wu_instruments::kits::KITS;

use crate::records::Records;
use crate::tour::progress;

/// A kit, by its id: any tune can play on it.
pub const KIT_COST: u32 = 8;
/// A tour stop's stage, by its id: any tune can play in front of it.
pub const STAGE_COST: u32 = 10;
/// A challenge met, on top of the stars it took.
pub const PER_CHALLENGE: u32 = 3;

/// One thing dubplates press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item<'a> {
    Kit(&'a str),
    Stage(&'a str),
}

impl Item<'_> {
    /// As the records file keeps it: "kit/ragga-93", "stage/basement-club".
    pub fn key(self) -> String {
        match self {
            Item::Kit(id) => format!("kit/{id}"),
            Item::Stage(id) => format!("stage/{id}"),
        }
    }

    /// What it costs, or `None` for no such kit or stop.
    pub fn cost(self, tour: &Tour) -> Option<u32> {
        match self {
            Item::Kit(id) => KITS.iter().any(|kit| kit.id == id).then_some(KIT_COST),
            Item::Stage(id) => tour.venues.iter().any(|venue| venue.id == id).then_some(STAGE_COST),
        }
    }

    fn of_key(key: &str) -> Option<Item<'_>> {
        match key.split_once('/')? {
            ("kit", id) => Some(Item::Kit(id)),
            ("stage", id) => Some(Item::Stage(id)),
            _ => None,
        }
    }
}

/// Every dubplate the tour has pressed: one a star, at every difficulty, and
/// three more for each challenge met.
pub fn earned(tour: &Tour, records: &Records) -> u32 {
    Difficulty::ALL
        .into_iter()
        .map(|difficulty| {
            let progress = progress(tour, records, difficulty);
            let challenges = progress.stops.iter().filter(|stop| stop.challenge_done).count() as u32;
            progress.stars + PER_CHALLENGE * challenges
        })
        .sum()
}

/// The dubplates not spent yet.
pub fn left(tour: &Tour, records: &Records) -> u32 {
    let spent: u32 = records
        .pressed
        .iter()
        .filter_map(|key| Item::of_key(key)?.cost(tour))
        .sum();
    earned(tour, records).saturating_sub(spent)
}

pub fn owns(records: &Records, item: Item) -> bool {
    records.pressed.contains(&item.key())
}

/// Presses `item` when the dubplates left pay for it; whether it is the player's now.
pub fn press(tour: &Tour, records: &mut Records, item: Item) -> bool {
    if owns(records, item) {
        return true;
    }
    match item.cost(tour) {
        Some(cost) if cost <= left(tour, records) => records.pressed.insert(item.key()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::Conditions;
    use crate::score::{Score, ScoreRules};

    /// An S+ (five stars) on every tune of the first stop, at `difficulty`.
    fn clear_first_stop(tour: &Tour, records: &mut Records, difficulty: Difficulty) {
        let real = Conditions {
            tempo_percent: 100,
            autoplay: false,
            no_fail: false,
            failed: false,
        };
        for id in tour.venues[0].set.iter().chain(&tour.venues[0].encore) {
            let mut score = Score::new(ScoreRules {
                overhit_penalty: 0.0,
                no_fail: false,
                sudden_death: false,
            });
            score.counts = [100, 0, 0, 0];
            score.points = 100;
            records.submit(id, difficulty, &score, real, 1);
        }
    }

    #[test]
    fn stars_and_challenges_press_dubplates_at_every_difficulty() {
        let tour = Tour::load().expect("parses");
        let mut records = Records::default();
        assert_eq!(earned(&tour, &records), 0);
        clear_first_stop(&tour, &mut records, Difficulty::Hard);
        let tunes = tour.venues[0].set.len() as u32 + 1;
        let one = 5 * tunes + PER_CHALLENGE;
        assert_eq!(earned(&tour, &records), one);
        clear_first_stop(&tour, &mut records, Difficulty::Easy);
        assert_eq!(earned(&tour, &records), 2 * one);
    }

    #[test]
    fn pressing_spends_them_once_and_never_more_than_are_left() {
        let tour = Tour::load().expect("parses");
        let mut records = Records::default();
        let kit = Item::Kit("darkside-92");
        assert!(!press(&tour, &mut records, kit), "nothing earned yet");
        clear_first_stop(&tour, &mut records, Difficulty::Hard);
        let before = left(&tour, &records);
        assert!(press(&tour, &mut records, kit));
        assert!(owns(&records, kit));
        assert_eq!(left(&tour, &records), before - KIT_COST);
        assert!(press(&tour, &mut records, kit), "already the player's");
        assert_eq!(left(&tour, &records), before - KIT_COST, "and not paid twice");
        // Stages pressed until the dubplates run out, and not one more.
        let left_over = before - KIT_COST;
        let stages = tour.venues.iter().map(|venue| Item::Stage(&venue.id));
        let pressed = stages.take_while(|&stage| press(&tour, &mut records, stage)).count() as u32;
        assert_eq!(pressed, left_over / STAGE_COST);
        assert_eq!(left(&tour, &records), left_over % STAGE_COST);
        assert!(!press(&tour, &mut records, Item::Kit("no-such-kit")));
        assert!(!press(&tour, &mut records, Item::Stage("no-such-stop")));
    }
}
