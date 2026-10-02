//! How far the player has got on the Pirate Radio Tour, at one difficulty: read
//! off their records, so every real run counts, on the tour or not.

use wu_chart::Difficulty;
use wu_content::tour::{Challenge, Tour};

use crate::records::{Best, Records};

/// Grades from best to worst.
const GRADES: [&str; 6] = ["S+", "S", "A", "B", "C", "D"];

/// The stars a record earns: cleared 1, B 2, A 3, S 4, S+ 5.
pub fn stars(best: &Best) -> u32 {
    match best.grade.as_str() {
        "S+" => 5,
        "S" => 4,
        "A" => 3,
        "B" => 2,
        _ => 1,
    }
}

/// Whether `grade` is `wanted` or better.
fn at_least(grade: &str, wanted: &str) -> bool {
    let rank = |g: &str| GRADES.iter().position(|&x| x == g).unwrap_or(GRADES.len());
    rank(grade) <= rank(wanted)
}

/// One stop, as the player stands on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stop {
    /// Its set exists and the tour's stars reach it.
    pub open: bool,
    /// Stars from its set and its encore…
    pub stars: u32,
    /// …out of this many: five a tune.
    pub most: u32,
    /// The set has earned the encore.
    pub encore_open: bool,
    pub challenge_done: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    /// In the tour's order.
    pub stops: Vec<Stop>,
    /// Stars earned on the whole tour.
    pub stars: u32,
}

pub fn progress(tour: &Tour, records: &Records, difficulty: Difficulty) -> Progress {
    let best = |id: &str| records.best(id, difficulty);
    let tune_stars = |id: &String| best(id).map_or(0, stars);
    let earned: Vec<u32> = tour
        .venues
        .iter()
        .map(|venue| venue.set.iter().chain(&venue.encore).map(tune_stars).sum())
        .collect();
    let total = earned.iter().sum();
    let stops = tour
        .venues
        .iter()
        .zip(earned)
        .map(|(venue, stars)| {
            let set_stars: u32 = venue.set.iter().map(tune_stars).sum();
            let challenge_done = !venue.set.is_empty()
                && match &venue.challenge {
                    Challenge::FullCombo => venue.set.iter().any(|id| best(id).is_some_and(|b| b.full_combo)),
                    Challenge::AllAtLeast(grade) => venue
                        .set
                        .iter()
                        .all(|id| best(id).is_some_and(|b| at_least(&b.grade, grade))),
                    Challenge::Stars(n) => stars >= *n,
                };
            Stop {
                open: !venue.set.is_empty() && total >= venue.opens_at,
                stars,
                most: 5 * (venue.set.len() + usize::from(venue.encore.is_some())) as u32,
                encore_open: venue.encore.is_some() && set_stars >= venue.encore_stars,
                challenge_done,
            }
        })
        .collect();
    Progress { stops, stars: total }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{Conditions, Outcome};
    use crate::score::{Score, ScoreRules};

    const REAL: Conditions = Conditions {
        tempo_percent: 100,
        autoplay: false,
        no_fail: false,
        failed: false,
    };

    /// A record of `grade` on `song` at Hard: a score with that many WICKEDs in a
    /// hundred notes, the rest misses (or BIGs, for a full combo).
    fn record(records: &mut Records, song: &str, wicked: u32, full_combo: bool) {
        let mut score = Score::new(ScoreRules {
            overhit_penalty: 0.0,
            no_fail: false,
        });
        score.counts = if full_combo {
            [wicked, 100 - wicked, 0, 0]
        } else {
            [wicked, 0, 0, 100 - wicked]
        };
        score.points = u64::from(wicked);
        assert_ne!(
            records.submit(song, Difficulty::Hard, &score, REAL, 1),
            Outcome::NotCounted
        );
    }

    #[test]
    fn a_new_player_stands_at_the_first_stop() {
        let tour = Tour::load().expect("parses");
        let progress = progress(&tour, &Records::default(), Difficulty::Hard);
        assert_eq!(progress.stars, 0);
        assert!(progress.stops[0].open);
        assert!(progress.stops[1..].iter().all(|s| !s.open));
        assert!(progress.stops.iter().all(|s| !s.encore_open && !s.challenge_done));
    }

    #[test]
    fn stars_open_the_encore_the_next_stop_and_the_challenges() {
        let tour = Tour::load().expect("parses");
        let bedroom = &tour.venues[0];
        let mut records = Records::default();
        // An A on the first tune (91 %), an S+ full combo on the second.
        record(&mut records, &bedroom.set[0], 91, false);
        record(&mut records, &bedroom.set[1], 99, true);
        let grade = |id: &str| records.best(id, Difficulty::Hard).map(|b| b.grade.clone());
        assert_eq!(grade(&bedroom.set[0]).as_deref(), Some("A"));
        assert_eq!(grade(&bedroom.set[1]).as_deref(), Some("S+"));
        let progress = progress(&tour, &records, Difficulty::Hard);
        let first = progress.stops[0];
        assert_eq!(first.stars, 3 + 5);
        assert!(first.encore_open, "{first:?}");
        assert!(first.challenge_done, "a full combo on one tune");
        assert!(progress.stops[1].open, "{} stars open the rooftop", progress.stars);
        // Another difficulty is another tour.
        let easy = super::progress(&tour, &records, Difficulty::Easy);
        assert_eq!(easy.stars, 0);
    }

    #[test]
    fn grades_rank_best_first() {
        assert!(at_least("S+", "A") && at_least("A", "A") && !at_least("B", "A"));
        assert!(!at_least("D", "C"));
    }
}
