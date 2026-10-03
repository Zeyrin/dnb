//! Score, combo, vibe and grades.

use crate::judge::{Judgement, Outcome};

/// Points per beat of a hold held, before the combo multiplier.
pub const HOLD_POINTS_PER_BEAT: f64 = 50.0;

/// Points per judgement, before the combo multiplier.
pub fn base_points(judgement: Judgement) -> u64 {
    match judgement {
        Judgement::Wicked => 300,
        Judgement::Big => 200,
        Judgement::Safe => 100,
        Judgement::Miss => 0,
    }
}

/// How a run is scored beyond the judgements themselves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoreRules {
    /// Vibe lost per press with no note in reach (0 below Hard).
    pub overhit_penalty: f32,
    /// The run never fails, however low the vibe goes.
    pub no_fail: bool,
    /// Sudden Death: the first miss pulls the plug (No-Fail aside).
    pub sudden_death: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Grade {
    SPlus,
    S,
    A,
    B,
    C,
    D,
}

impl Grade {
    pub fn label(self) -> &'static str {
        match self {
            Grade::SPlus => "S+",
            Grade::S => "S",
            Grade::A => "A",
            Grade::B => "B",
            Grade::C => "C",
            Grade::D => "D",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Score {
    pub rules: ScoreRules,
    pub points: u64,
    pub combo: u32,
    pub max_combo: u32,
    /// Per judgement, in `Judgement::ALL` order.
    pub counts: [u32; 4],
    pub overhits: u32,
    /// Holds kept down to the end, and holds let go early.
    pub holds_completed: u32,
    pub holds_dropped: u32,
    /// The crowd: 0–1, starting at half. At zero the plug gets pulled.
    pub vibe: f32,
    /// Sticky: once failed, a run stays failed.
    pub failed: bool,
    /// Inside a WHEEL UP! replay: the multiplier doubles.
    pub boosted: bool,
    /// Every hit's timing, negative when early: the results histogram.
    pub offsets_ms: Vec<f64>,
}

impl Score {
    pub fn new(rules: ScoreRules) -> Score {
        Score {
            rules,
            points: 0,
            combo: 0,
            max_combo: 0,
            counts: [0; 4],
            overhits: 0,
            holds_completed: 0,
            holds_dropped: 0,
            vibe: 0.5,
            failed: false,
            boosted: false,
            offsets_ms: Vec::new(),
        }
    }

    /// ×1 for the first ten hits of a combo, up to ×4 from the thirty-first;
    /// doubled during a WHEEL UP! replay (×8 at most).
    pub fn multiplier(&self) -> u64 {
        let combo = 1 + u64::from(self.combo / 10).min(3);
        if self.boosted { 2 * combo } else { combo }
    }

    pub fn apply(&mut self, outcome: &Outcome) {
        self.take(outcome, true);
    }

    /// As [`apply`](Self::apply), in the warm-up: a miss or a stray press
    /// costs the combo but no vibe.
    pub fn apply_warming_up(&mut self, outcome: &Outcome) {
        self.take(outcome, false);
    }

    fn take(&mut self, outcome: &Outcome, costs_vibe: bool) {
        match *outcome {
            Outcome::Hit {
                judgement, offset_ms, ..
            } => {
                self.points += base_points(judgement) * self.multiplier();
                self.counts[judgement.index()] += 1;
                self.combo += 1;
                self.max_combo = self.max_combo.max(self.combo);
                self.offsets_ms.push(offset_ms);
                self.vibe += match judgement {
                    Judgement::Wicked => 0.03,
                    Judgement::Big => 0.02,
                    _ => 0.01,
                };
            }
            Outcome::Missed { .. } => {
                self.counts[Judgement::Miss.index()] += 1;
                self.combo = 0;
                if costs_vibe {
                    self.vibe -= if self.rules.sudden_death { 1.0 } else { 0.08 };
                }
            }
            Outcome::Overhit { .. } => {
                self.overhits += 1;
                if costs_vibe {
                    self.vibe -= self.rules.overhit_penalty;
                }
            }
            Outcome::HoldEnd { held, beats, .. } => {
                self.points += (HOLD_POINTS_PER_BEAT * beats * held).round() as u64 * self.multiplier();
                if held >= 1.0 {
                    self.holds_completed += 1;
                    self.vibe += 0.01;
                } else {
                    self.holds_dropped += 1;
                }
            }
        }
        self.vibe = self.vibe.clamp(0.0, 1.0);
        if self.vibe <= 0.0 && !self.rules.no_fail {
            self.failed = true;
        }
    }

    pub fn judged(&self) -> u32 {
        self.counts.iter().sum()
    }

    /// Weighted accuracy, 0–1: WICKED counts fully, BIG two thirds, SAFE one third.
    pub fn accuracy(&self) -> f64 {
        Score::accuracy_of(self.counts)
    }

    /// The accuracy of so many of each judgement (`counts` by `Judgement::index`):
    /// of a stretch of a run, say, from the counts before and after it.
    pub fn accuracy_of(counts: [u32; 4]) -> f64 {
        let judged: u32 = counts.iter().sum();
        if judged == 0 {
            return 1.0;
        }
        let [wicked, big, safe, _] = counts.map(f64::from);
        (wicked + big * 2.0 / 3.0 + safe / 3.0) / f64::from(judged)
    }

    pub fn full_combo(&self) -> bool {
        self.counts[Judgement::Miss.index()] == 0
    }

    pub fn grade(&self) -> Grade {
        let accuracy = self.accuracy();
        match () {
            _ if accuracy >= 0.99 && self.full_combo() => Grade::SPlus,
            _ if accuracy >= 0.95 => Grade::S,
            _ if accuracy >= 0.90 => Grade::A,
            _ if accuracy >= 0.80 => Grade::B,
            _ if accuracy >= 0.70 => Grade::C,
            _ => Grade::D,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: ScoreRules = ScoreRules {
        overhit_penalty: 0.02,
        no_fail: false,
        sudden_death: false,
    };

    fn hit(judgement: Judgement) -> Outcome {
        Outcome::Hit {
            note: 0,
            judgement,
            offset_ms: 0.0,
        }
    }

    #[test]
    fn the_multiplier_climbs_every_ten_hits_and_resets_on_a_miss() {
        let mut score = Score::new(RULES);
        for _ in 0..10 {
            score.apply(&hit(Judgement::Wicked));
        }
        assert_eq!((score.points, score.multiplier()), (3000, 2));
        for _ in 0..30 {
            score.apply(&hit(Judgement::Wicked));
        }
        assert_eq!(score.multiplier(), 4);
        score.apply(&Outcome::Missed { note: 0 });
        assert_eq!((score.combo, score.max_combo, score.multiplier()), (0, 40, 1));
    }

    #[test]
    fn misses_drain_the_vibe_until_the_plug_is_pulled() {
        let mut score = Score::new(RULES);
        for _ in 0..7 {
            score.apply(&Outcome::Missed { note: 0 });
        }
        assert!(score.failed && score.vibe == 0.0);
        score.apply(&hit(Judgement::Wicked));
        assert!(score.failed, "failing is final");

        let mut forgiving = Score::new(ScoreRules { no_fail: true, ..RULES });
        for _ in 0..20 {
            forgiving.apply(&Outcome::Missed { note: 0 });
        }
        assert!(!forgiving.failed);

        // Sudden Death: one miss is enough, warm-ups and No-Fail aside.
        let sudden = ScoreRules {
            sudden_death: true,
            ..RULES
        };
        let mut score = Score::new(sudden);
        score.apply_warming_up(&Outcome::Missed { note: 0 });
        assert!(!score.failed, "the warm-up forgives");
        score.apply(&Outcome::Missed { note: 1 });
        assert!(score.failed);
        let mut forgiving = Score::new(ScoreRules {
            no_fail: true,
            ..sudden
        });
        forgiving.apply(&Outcome::Missed { note: 0 });
        assert!(!forgiving.failed);
    }

    #[test]
    fn accuracy_and_grades() {
        let mut score = Score::new(RULES);
        assert_eq!(score.accuracy(), 1.0);
        for _ in 0..99 {
            score.apply(&hit(Judgement::Wicked));
        }
        score.apply(&hit(Judgement::Big));
        assert_eq!(score.grade(), Grade::SPlus);
        score.apply(&Outcome::Missed { note: 0 });
        assert_eq!(score.grade(), Grade::S, "a miss costs the S+");
        let mut sloppy = Score::new(RULES);
        for j in [Judgement::Safe, Judgement::Safe, Judgement::Big] {
            sloppy.apply(&hit(j));
        }
        assert!((sloppy.accuracy() - 4.0 / 9.0).abs() < 1e-12);
        assert_eq!(sloppy.grade(), Grade::D);
    }
}
