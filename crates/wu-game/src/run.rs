//! A run: one play of one chart. Ties the judge to the score, and keeps the
//! score deterministic, so a replay of the same presses scores the same.
//!
//! Outcomes reach the score in the order they happened in song time (a hit at
//! its press, a miss when its window closed), whatever order the frames
//! discovered them in. A press may reach the game up to a frame or two after it
//! happened, so outcomes only settle once they are `SETTLE_MS` old; the judgement
//! itself is shown at once.

use serde::{Deserialize, Serialize};

use crate::judge::{Judge, Lane, Outcome, TimedNote, Windows};
use crate::score::{Score, ScoreRules};

/// Longer than any press can take to reach the judge (the input thread's wait,
/// the queue, then up to a whole frame even at 20 fps). The score lags by this
/// much; nobody can see a tenth of a second on a score counter.
pub const SETTLE_MS: f64 = 100.0;

/// A press as the judge saw it, in song milliseconds after calibration.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Press {
    /// `Lane::index`: the pads 0–7, then the left and right rails.
    #[serde(alias = "pad")]
    pub lane: u8,
    pub ms: f64,
    /// A rail let go, rather than pressed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub up: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug)]
pub struct Run {
    judge: Judge,
    score: Score,
    /// Outcomes not yet settled, each with its song time and arrival order.
    pending: Vec<(f64, u64, Outcome)>,
    sequence: u64,
    presses: Vec<Press>,
}

impl Run {
    pub fn new(notes: Vec<TimedNote>, windows: Windows, rules: ScoreRules) -> Run {
        Run {
            judge: Judge::new(notes, windows),
            score: Score::new(rules),
            pending: Vec::new(),
            sequence: 0,
            presses: Vec::new(),
        }
    }

    pub fn judge(&self) -> &Judge {
        &self.judge
    }

    pub fn score(&self) -> &Score {
        &self.score
    }

    pub fn presses(&self) -> &[Press] {
        &self.presses
    }

    /// Judges a press; returns what it decided, for immediate feedback.
    pub fn press(&mut self, lane: Lane, ms: f64) -> Vec<Outcome> {
        self.presses.push(Press {
            lane: lane.index() as u8,
            ms,
            up: false,
        });
        let mut outcomes = Vec::new();
        self.judge.press(lane, ms, &mut outcomes);
        self.queue(&outcomes);
        outcomes
    }

    /// A rail let go: ends the hold it was holding.
    pub fn release(&mut self, lane: Lane, ms: f64) -> Vec<Outcome> {
        self.presses.push(Press {
            lane: lane.index() as u8,
            ms,
            up: true,
        });
        let mut outcomes = Vec::new();
        self.judge.release(lane, ms, &mut outcomes);
        self.queue(&outcomes);
        outcomes
    }

    /// Misses whatever was left behind, then lets every outcome older than
    /// `SETTLE_MS` reach the score, in song-time order. Returns the new misses.
    pub fn settle(&mut self, now_ms: f64) -> Vec<Outcome> {
        let horizon = now_ms - SETTLE_MS;
        let mut missed = Vec::new();
        self.judge.expire(horizon, &mut missed);
        self.queue(&missed);
        self.apply_until(horizon);
        missed
    }

    /// Ends the run: every remaining note is missed and everything settles.
    pub fn finish(&mut self) -> Vec<Outcome> {
        let mut missed = Vec::new();
        self.judge.expire(f64::INFINITY, &mut missed);
        self.queue(&missed);
        self.apply_until(f64::INFINITY);
        missed
    }

    fn queue(&mut self, outcomes: &[Outcome]) {
        for outcome in outcomes {
            let at = self.event_ms(outcome);
            self.pending.push((at, self.sequence, *outcome));
            self.sequence += 1;
        }
    }

    fn apply_until(&mut self, horizon: f64) {
        self.pending.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let ready = self.pending.partition_point(|(at, _, _)| *at < horizon);
        for (_, _, outcome) in self.pending.drain(..ready) {
            self.score.apply(&outcome);
        }
    }

    /// When an outcome happened in song time.
    fn event_ms(&self, outcome: &Outcome) -> f64 {
        match *outcome {
            Outcome::Hit { note, offset_ms, .. } => self.judge.notes()[note].ms + offset_ms,
            Outcome::Missed { note } => self.judge.notes()[note].ms + self.judge.windows().safe,
            Outcome::Overhit { ms, .. } | Outcome::HoldEnd { ms, .. } => ms,
        }
    }
}

/// Scores a recorded run again from its presses alone.
///
/// Presses are judged in the order they were recorded, which is the order the
/// live judge saw them. That is usually time order, but not always: two input
/// paths (a controller and the keyboard) can deliver a later press first, and
/// which note each press took depends on that order.
pub fn rejudge(notes: Vec<TimedNote>, windows: Windows, rules: ScoreRules, presses: &[Press]) -> Score {
    let mut run = Run::new(notes, windows, rules);
    for &press in presses {
        match (Lane::from_index(usize::from(press.lane)), press.up) {
            (Some(lane), false) => {
                run.press(lane, press.ms);
            }
            (Some(lane), true) => {
                run.release(lane, press.ms);
            }
            (None, _) => {}
        }
    }
    run.finish();
    run.score
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use wu_dsp::Rng;

    use wu_instruments::Pad;

    use super::*;
    use crate::judge::Judgement;

    const RULES: ScoreRules = ScoreRules {
        overhit_penalty: 0.02,
        no_fail: true,
    };

    fn chart(n: usize) -> Vec<TimedNote> {
        (0..n)
            .map(|i| TimedNote::tap(1000.0 + i as f64 * 178.6, Pad::from_index(i % 3 * 3).unwrap_or(Pad::P1)))
            .collect()
    }

    #[test]
    fn perfect_presses_score_all_wicked() {
        let notes = chart(64);
        let mut run = Run::new(notes.clone(), Windows::TIGHT, RULES);
        for note in &notes {
            run.press(note.lane, note.ms);
            run.settle(note.ms);
        }
        run.finish();
        let score = run.score();
        assert_eq!(score.counts[Judgement::Wicked.index()], 64);
        assert_eq!(score.accuracy(), 1.0);
        assert_eq!(score.max_combo, 64);
    }

    #[test]
    fn human_jitter_stays_wicked_or_big() {
        // σ = 15 ms, Box–Muller from a fixed seed: the brief's acceptance test.
        let notes = chart(1000);
        let mut rng = Rng::new(15);
        let mut run = Run::new(notes.clone(), Windows::TIGHT, RULES);
        for note in &notes {
            let (u1, u2) = (f64::from(rng.next_f32()).max(1e-9), f64::from(rng.next_f32()));
            let gaussian = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
            run.press(note.lane, note.ms + 15.0 * gaussian);
        }
        run.finish();
        let score = run.score();
        let good = score.counts[Judgement::Wicked.index()] + score.counts[Judgement::Big.index()];
        assert!(good >= 990, "{good} of 1000 WICKED or BIG");
    }

    proptest! {
        /// Live play (presses delivered late, frames settling at their own pace)
        /// and a replay of the same presses give the same score.
        #[test]
        fn replays_score_exactly_like_live_play(
            offsets in prop::collection::vec((-150.0f64..150.0, 0usize..8, 0.0f64..45.0), 1..120),
            frame_ms in 4.0f64..34.0,
        ) {
            let notes = chart(48);
            let presses: Vec<(f64, Pad, f64)> = offsets
                .iter()
                .enumerate()
                .map(|(i, &(offset, pad, delay))| {
                    let base = 1000.0 + (i % 48) as f64 * 178.6;
                    (base + offset, Pad::from_index(pad).unwrap_or(Pad::P1), delay)
                })
                .collect();
            let mut arrivals: Vec<(f64, Pad, f64)> = presses.iter().map(|&(ms, pad, delay)| (ms + delay, pad, ms)).collect();
            arrivals.sort_by(|a, b| a.0.total_cmp(&b.0));

            let mut live = Run::new(notes.clone(), Windows::TIGHT, RULES);
            let mut now = 0.0;
            let mut next = 0;
            let end = 1000.0 + 48.0 * 178.6 + 500.0;
            while now < end {
                now += frame_ms;
                let mut this_frame: Vec<(f64, Pad)> = Vec::new();
                while next < arrivals.len() && arrivals[next].0 <= now {
                    this_frame.push((arrivals[next].2, arrivals[next].1));
                    next += 1;
                }
                this_frame.sort_by(|a, b| a.0.total_cmp(&b.0));
                for (ms, pad) in this_frame {
                    live.press(Lane::Pad(pad), ms);
                }
                live.settle(now);
            }
            live.finish();

            let replayed = rejudge(notes, Windows::TIGHT, RULES, live.presses());
            prop_assert_eq!(&replayed.counts, &live.score().counts);
            prop_assert_eq!(replayed.points, live.score().points);
            prop_assert_eq!(replayed.max_combo, live.score().max_combo);
        }
    }
}
