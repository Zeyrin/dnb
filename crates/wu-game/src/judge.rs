//! The judge: pairs presses with notes and grades their timing.
//!
//! All times are song time in milliseconds (tick 0 = 0 ms), after the player's
//! calibration offset has been taken off. A press goes to the earliest unjudged
//! note on its lane whose window contains it; each note is judged exactly once;
//! lanes never steal each other's notes.

use wu_instruments::{PAD_COUNT, Pad};

/// Half-widths of the timing windows, in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Windows {
    pub wicked: f64,
    pub big: f64,
    pub safe: f64,
}

impl Windows {
    /// Hard and Junglist.
    pub const TIGHT: Windows = Windows {
        wicked: 25.0,
        big: 50.0,
        safe: 90.0,
    };
    pub const MEDIUM: Windows = Windows {
        wicked: 30.0,
        big: 60.0,
        safe: 110.0,
    };
    /// Beginner and Easy.
    pub const LOOSE: Windows = Windows {
        wicked: 35.0,
        big: 70.0,
        safe: 130.0,
    };

    pub fn grade(&self, offset_ms: f64) -> Option<Judgement> {
        let off = offset_ms.abs();
        if off <= self.wicked {
            Some(Judgement::Wicked)
        } else if off <= self.big {
            Some(Judgement::Big)
        } else if off <= self.safe {
            Some(Judgement::Safe)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Judgement {
    Wicked,
    Big,
    Safe,
    Miss,
}

impl Judgement {
    pub const ALL: [Judgement; 4] = [Judgement::Wicked, Judgement::Big, Judgement::Safe, Judgement::Miss];

    pub fn label(self) -> &'static str {
        match self {
            Judgement::Wicked => "WICKED",
            Judgement::Big => "BIG",
            Judgement::Safe => "SAFE",
            Judgement::Miss => "MISS",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// A note to judge: when it sounds, and which pad plays it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimedNote {
    pub ms: f64,
    pub pad: Pad,
}

/// What one press, or the passing of time, decided.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// A note hit; `offset_ms` is negative when early.
    Hit {
        note: usize,
        judgement: Judgement,
        offset_ms: f64,
    },
    /// A note whose window closed without a press.
    Missed { note: usize },
    /// A press with no note in reach.
    Overhit { pad: Pad, ms: f64 },
}

#[derive(Clone, Debug)]
pub struct Judge {
    notes: Vec<TimedNote>,
    judged: Vec<Option<Judgement>>,
    /// Note indices per lane, in time order.
    lanes: [Vec<usize>; PAD_COUNT],
    /// Per lane: the first position in `lanes` that may still be unjudged.
    cursors: [usize; PAD_COUNT],
    windows: Windows,
}

impl Judge {
    /// `notes` in any order; they are judged by time.
    pub fn new(mut notes: Vec<TimedNote>, windows: Windows) -> Judge {
        notes.sort_by(|a, b| a.ms.total_cmp(&b.ms).then(a.pad.cmp(&b.pad)));
        let mut lanes: [Vec<usize>; PAD_COUNT] = Default::default();
        for (i, note) in notes.iter().enumerate() {
            lanes[note.pad.index()].push(i);
        }
        let judged = vec![None; notes.len()];
        Judge {
            notes,
            judged,
            lanes,
            cursors: [0; PAD_COUNT],
            windows,
        }
    }

    pub fn notes(&self) -> &[TimedNote] {
        &self.notes
    }

    pub fn judgement(&self, note: usize) -> Option<Judgement> {
        self.judged.get(note).copied().flatten()
    }

    pub fn windows(&self) -> Windows {
        self.windows
    }

    /// Every note judged.
    pub fn finished(&self) -> bool {
        self.judged.iter().all(Option::is_some)
    }

    /// A press of `pad` at `ms`. Notes on that lane whose window already closed
    /// are missed first, so outcomes always come out in time order.
    pub fn press(&mut self, pad: Pad, ms: f64, outcomes: &mut Vec<Outcome>) {
        let lane = pad.index();
        self.expire_lane(lane, ms, outcomes);
        let mut position = self.cursors[lane];
        while let Some(&note) = self.lanes[lane].get(position) {
            let offset = ms - self.notes[note].ms;
            if offset < -self.windows.safe {
                break;
            }
            if self.judged[note].is_none()
                && let Some(judgement) = self.windows.grade(offset)
            {
                self.judged[note] = Some(judgement);
                self.advance_cursor(lane);
                outcomes.push(Outcome::Hit {
                    note,
                    judgement,
                    offset_ms: offset,
                });
                return;
            }
            position += 1;
        }
        outcomes.push(Outcome::Overhit { pad, ms });
    }

    /// Misses every note, on every lane, whose window closed before `ms`.
    pub fn expire(&mut self, ms: f64, outcomes: &mut Vec<Outcome>) {
        let mut missed = Vec::new();
        for lane in 0..PAD_COUNT {
            self.expire_lane(lane, ms, &mut missed);
        }
        missed.sort_by(|a, b| match (a, b) {
            (Outcome::Missed { note: x }, Outcome::Missed { note: y }) => x.cmp(y),
            _ => std::cmp::Ordering::Equal,
        });
        outcomes.extend(missed);
    }

    fn expire_lane(&mut self, lane: usize, ms: f64, outcomes: &mut Vec<Outcome>) {
        while let Some(&note) = self.lanes[lane].get(self.cursors[lane]) {
            if self.judged[note].is_some() {
                self.cursors[lane] += 1;
                continue;
            }
            if self.notes[note].ms + self.windows.safe >= ms {
                break;
            }
            self.judged[note] = Some(Judgement::Miss);
            self.cursors[lane] += 1;
            outcomes.push(Outcome::Missed { note });
        }
    }

    fn advance_cursor(&mut self, lane: usize) {
        while let Some(&note) = self.lanes[lane].get(self.cursors[lane]) {
            if self.judged[note].is_none() {
                break;
            }
            self.cursors[lane] += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn notes(times: &[(f64, Pad)]) -> Vec<TimedNote> {
        times.iter().map(|&(ms, pad)| TimedNote { ms, pad }).collect()
    }

    #[test]
    fn windows_grade_by_distance() {
        let w = Windows::TIGHT;
        assert_eq!(w.grade(0.0), Some(Judgement::Wicked));
        assert_eq!(w.grade(-25.0), Some(Judgement::Wicked));
        assert_eq!(w.grade(40.0), Some(Judgement::Big));
        assert_eq!(w.grade(-89.0), Some(Judgement::Safe));
        assert_eq!(w.grade(91.0), None);
    }

    #[test]
    fn a_press_takes_the_earliest_note_in_reach_on_its_lane() {
        let mut judge = Judge::new(
            notes(&[(1000.0, Pad::P1), (1060.0, Pad::P1), (1000.0, Pad::P2)]),
            Windows::TIGHT,
        );
        let mut out = Vec::new();
        judge.press(Pad::P1, 1050.0, &mut out);
        assert_eq!(
            out,
            vec![Outcome::Hit {
                note: 0,
                judgement: Judgement::Big,
                offset_ms: 50.0
            }]
        );
        out.clear();
        judge.press(Pad::P1, 1062.0, &mut out);
        assert_eq!(
            out,
            vec![Outcome::Hit {
                note: 2,
                judgement: Judgement::Wicked,
                offset_ms: 2.0
            }]
        );
        assert_eq!(judge.judgement(1), None, "P2 untouched by P1 presses");
    }

    #[test]
    fn presses_with_nothing_in_reach_are_overhits() {
        let mut judge = Judge::new(notes(&[(1000.0, Pad::P1)]), Windows::TIGHT);
        let mut out = Vec::new();
        judge.press(Pad::P1, 500.0, &mut out);
        judge.press(Pad::P7, 1000.0, &mut out);
        assert_eq!(
            out,
            vec![
                Outcome::Overhit {
                    pad: Pad::P1,
                    ms: 500.0
                },
                Outcome::Overhit {
                    pad: Pad::P7,
                    ms: 1000.0
                },
            ]
        );
    }

    #[test]
    fn notes_left_behind_are_missed_once() {
        let mut judge = Judge::new(
            notes(&[(100.0, Pad::P1), (200.0, Pad::P2), (900.0, Pad::P1)]),
            Windows::TIGHT,
        );
        let mut out = Vec::new();
        judge.expire(500.0, &mut out);
        assert_eq!(out, vec![Outcome::Missed { note: 0 }, Outcome::Missed { note: 1 }]);
        out.clear();
        judge.expire(600.0, &mut out);
        assert!(out.is_empty());
        judge.press(Pad::P1, 905.0, &mut out);
        assert_eq!(
            out,
            vec![Outcome::Hit {
                note: 2,
                judgement: Judgement::Wicked,
                offset_ms: 5.0
            }]
        );
        assert!(judge.finished());
    }

    #[test]
    fn a_late_press_misses_old_notes_before_hitting_the_next() {
        let mut judge = Judge::new(notes(&[(100.0, Pad::P1), (400.0, Pad::P1)]), Windows::TIGHT);
        let mut out = Vec::new();
        judge.press(Pad::P1, 410.0, &mut out);
        assert_eq!(
            out,
            vec![
                Outcome::Missed { note: 0 },
                Outcome::Hit {
                    note: 1,
                    judgement: Judgement::Wicked,
                    offset_ms: 10.0
                },
            ]
        );
    }

    proptest! {
        #[test]
        fn every_note_is_judged_exactly_once(
            times in prop::collection::vec((0.0f64..10_000.0, 0usize..8), 1..200),
            presses in prop::collection::vec((0.0f64..10_000.0, 0usize..8), 0..400),
        ) {
            let pad = |i: usize| Pad::from_index(i).unwrap_or(Pad::P1);
            let mut judge = Judge::new(times.iter().map(|&(ms, p)| TimedNote { ms, pad: pad(p) }).collect(), Windows::LOOSE);
            let mut presses = presses;
            presses.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut out = Vec::new();
            for (ms, p) in presses {
                judge.expire(ms, &mut out);
                judge.press(pad(p), ms, &mut out);
            }
            judge.expire(f64::INFINITY, &mut out);
            let mut seen = vec![0u32; judge.notes().len()];
            for outcome in &out {
                match outcome {
                    Outcome::Hit { note, .. } | Outcome::Missed { note } => seen[*note] += 1,
                    Outcome::Overhit { .. } => {}
                }
            }
            prop_assert!(seen.iter().all(|&n| n == 1));
            prop_assert!(judge.finished());
        }
    }
}
