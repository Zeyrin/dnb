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
    /// Not a press but WHEEL UP!: at `ms` the song went back this many
    /// milliseconds (see `Run::wheel_up`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewind_ms: Option<f64>,
}

/// How much one hype phrase cleared without a miss fills the meter…
pub const HYPE_PER_PHRASE: f32 = 0.25;
/// …and how full it must be for WHEEL UP!.
pub const HYPE_TO_WHEEL_UP: f32 = 0.5;

/// A hype phrase on the run's timeline.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Phrase {
    start_ms: f64,
    end_ms: f64,
    /// A note in it was missed.
    broken: bool,
    /// Over, and paid out if unbroken.
    done: bool,
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
    phrases: Vec<Phrase>,
    hype: f32,
    /// The stretch of timeline a WHEEL UP! doubles the multiplier for.
    boost: Option<(f64, f64)>,
    /// Until then, misses cost no vibe.
    warm_up_ms: f64,
}

impl Run {
    pub fn new(notes: Vec<TimedNote>, windows: Windows, rules: ScoreRules) -> Run {
        Run {
            judge: Judge::new(notes, windows),
            score: Score::new(rules),
            pending: Vec::new(),
            sequence: 0,
            presses: Vec::new(),
            phrases: Vec::new(),
            hype: 0.0,
            boost: None,
            warm_up_ms: f64::NEG_INFINITY,
        }
    }

    /// A warm-up until `until_ms`: misses there break the combo but cost no
    /// vibe, so the plug can't be pulled before the player has found the beat.
    pub fn with_warm_up(mut self, until_ms: f64) -> Run {
        self.warm_up_ms = until_ms;
        self
    }

    /// The run's hype phrases, as (start, end) in milliseconds.
    pub fn with_hype(mut self, phrases: impl IntoIterator<Item = (f64, f64)>) -> Run {
        self.phrases = phrases
            .into_iter()
            .map(|(start_ms, end_ms)| Phrase {
                start_ms,
                end_ms,
                broken: false,
                done: false,
            })
            .collect();
        self
    }

    /// The hype meter, 0–1.
    pub fn hype(&self) -> f32 {
        self.hype
    }

    pub fn can_wheel_up(&self) -> bool {
        self.hype >= HYPE_TO_WHEEL_UP - 1e-6
    }

    /// The hype phrases, and whether each is still clean: (start, end, unbroken).
    pub fn phrases(&self) -> impl Iterator<Item = (f64, f64, bool)> + '_ {
        self.phrases.iter().map(|p| (p.start_ms, p.end_ms, !p.broken))
    }

    /// The WHEEL UP! replay going on, if any: (from, to) on the run's timeline.
    pub fn boost(&self) -> Option<(f64, f64)> {
        self.boost
    }

    /// WHEEL UP!: at `at_ms` (on the run's timeline) the song goes back
    /// `back_ms` and plays that stretch again. Holds are let go, everything so far
    /// is scored as it stands, the stretch's notes and hype phrases come round
    /// again, and the multiplier doubles while they do. Spends the hype meter;
    /// `None`, and nothing happens, if it isn't full enough.
    pub fn wheel_up(&mut self, at_ms: f64, back_ms: f64) -> Option<(Vec<Outcome>, std::ops::Range<usize>)> {
        // Settle to the cut first: a replay, which never settles between presses,
        // then sees the same hype the live run did.
        let mut outcomes = self.settle(at_ms);
        if !self.can_wheel_up() || back_ms <= 0.0 {
            return None;
        }
        self.presses.push(Press {
            lane: 0,
            ms: at_ms,
            up: false,
            rewind_ms: Some(back_ms),
        });
        let copies = self.go_back(at_ms, back_ms, &mut outcomes);
        self.hype = 0.0;
        self.boost = Some((at_ms, at_ms + back_ms));
        Some((outcomes, copies))
    }

    /// Practice: at `at_ms` (on the run's timeline) the loop comes round and the
    /// song plays the `back_ms` before it again. Its notes come round as they do
    /// for WHEEL UP!, but for nothing: no hype spent, no multiplier, and nothing
    /// a replay could replay.
    pub fn again(&mut self, at_ms: f64, back_ms: f64) -> (Vec<Outcome>, std::ops::Range<usize>) {
        let mut outcomes = self.settle(at_ms);
        let copies = self.go_back(at_ms, back_ms.max(0.0), &mut outcomes);
        (outcomes, copies)
    }

    /// The song goes back `back_ms` at `at_ms`: holds are let go, what was due
    /// before the cut is scored, and the stretch's notes and hype phrases come
    /// round again. Returns the indices of the notes copied.
    fn go_back(&mut self, at_ms: f64, back_ms: f64, outcomes: &mut Vec<Outcome>) -> std::ops::Range<usize> {
        let from = at_ms - back_ms;
        // `settle` queued its own misses; queue only what the cut adds.
        let mut cut = Vec::new();
        self.judge.cut_holds(at_ms, &mut cut);
        // Notes whose window closed before the cut were missed; the rest come round again.
        self.judge.expire(at_ms, &mut cut);
        self.queue(&cut);
        self.apply_until(f64::INFINITY);
        outcomes.extend(cut);
        let copies = self.judge.splice(at_ms, back_ms);
        let mut again = Vec::new();
        for phrase in &mut self.phrases {
            if phrase.start_ms < from {
                continue;
            }
            let shifted = Phrase {
                start_ms: phrase.start_ms + back_ms,
                end_ms: phrase.end_ms + back_ms,
                broken: false,
                done: false,
            };
            if phrase.done {
                again.push(shifted);
            } else {
                *phrase = shifted;
            }
        }
        self.phrases.extend(again);
        copies
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
            rewind_ms: None,
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
            rewind_ms: None,
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
        for (at, _, outcome) in self.pending.drain(..ready) {
            self.score.boosted = self.boost.is_some_and(|(from, to)| from <= at && at < to);
            if let Outcome::Missed { note } = outcome {
                let ms = self.judge.notes()[note].ms;
                for phrase in &mut self.phrases {
                    phrase.broken |= phrase.start_ms <= ms && ms < phrase.end_ms;
                }
            }
            if at < self.warm_up_ms {
                self.score.apply_warming_up(&outcome);
            } else {
                self.score.apply(&outcome);
            }
        }
        // A phrase is over once its last note's window has closed.
        let safe = self.judge.windows().safe;
        for phrase in &mut self.phrases {
            if !phrase.done && phrase.end_ms + safe < horizon {
                phrase.done = true;
                if !phrase.broken {
                    self.hype = (self.hype + HYPE_PER_PHRASE).min(1.0);
                }
            }
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
/// which note each press took depends on that order. `run` is a fresh run set
/// up as the live one was.
pub fn rejudge(mut run: Run, presses: &[Press]) -> Score {
    for &press in presses {
        if let Some(back_ms) = press.rewind_ms {
            run.wheel_up(press.ms, back_ms);
            continue;
        }
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

    /// Plays every note of `run` from `from_ms` to `to_ms` dead on time, settling as it goes.
    fn play_perfectly(run: &mut Run, from_ms: f64, to_ms: f64) {
        let mut due: Vec<(f64, Lane)> = run
            .judge()
            .notes()
            .iter()
            .enumerate()
            .filter(|(i, n)| run.judge().judgement(*i).is_none() && n.ms >= from_ms && n.ms < to_ms)
            .map(|(_, n)| (n.ms, n.lane))
            .collect();
        due.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (ms, lane) in due {
            run.press(lane, ms);
            run.settle(ms);
        }
        run.settle(to_ms);
    }

    #[test]
    fn wheel_up_replays_the_phrase_with_the_multiplier_doubled() {
        // A note every 500 ms for 8 s; two hype phrases in the first 4 s.
        let notes: Vec<TimedNote> = (0..16).map(|i| TimedNote::tap(f64::from(i) * 500.0, Pad::P1)).collect();
        let mut run = Run::new(notes, Windows::TIGHT, RULES).with_hype([(0.0, 2000.0), (2000.0, 4000.0)]);
        play_perfectly(&mut run, 0.0, 4999.0);
        assert!((run.hype() - 0.5).abs() < 1e-6, "two clean phrases");
        assert!(run.can_wheel_up());

        // At 5 s the song goes back 3 s: the notes from 2 s come round again.
        let (_, copies) = run.wheel_up(5000.0, 3000.0).expect("hype to spend");
        assert_eq!(copies.len(), 6, "2000 to 4500 were hit and come back");
        assert_eq!(run.hype(), 0.0);
        let before = run.score().points;
        play_perfectly(&mut run, 4999.0, 11_000.0);
        run.finish();
        let score = run.score();
        assert_eq!(score.counts[Judgement::Wicked.index()], 22, "16 notes and 6 again");
        assert_eq!(score.counts[Judgement::Miss.index()], 0);
        assert!(score.points > before);
        assert!((run.hype() - 0.25).abs() < 1e-6, "the replayed phrase pays out again");

        // A replay of the same presses, WHEEL UP! included, scores the same.
        let notes: Vec<TimedNote> = (0..16).map(|i| TimedNote::tap(f64::from(i) * 500.0, Pad::P1)).collect();
        let fresh = Run::new(notes, Windows::TIGHT, RULES).with_hype([(0.0, 2000.0), (2000.0, 4000.0)]);
        let replayed = rejudge(fresh, run.presses());
        assert_eq!(replayed.points, run.score().points);
        assert_eq!(replayed.counts, run.score().counts);
    }

    #[test]
    fn a_miss_before_wheel_up_counts_once_live_and_in_replay() {
        let notes = || {
            (0..16)
                .map(|i| TimedNote::tap(f64::from(i) * 500.0, Pad::P1))
                .collect::<Vec<_>>()
        };
        let hype = [(0.0, 2000.0), (2000.0, 4000.0)];
        let mut live = Run::new(notes(), Windows::TIGHT, RULES).with_hype(hype);
        // Both phrases clean, then the notes at 4 and 4.5 s left alone.
        play_perfectly(&mut live, 0.0, 4000.0);
        live.settle(4999.0);
        assert!(live.wheel_up(5000.0, 3000.0).is_some());
        play_perfectly(&mut live, 5000.0, 11_000.0);
        live.finish();
        assert_eq!(live.score().counts[Judgement::Miss.index()], 2);

        // The replay never settled before the cut: it finds those misses there.
        let replayed = rejudge(Run::new(notes(), Windows::TIGHT, RULES).with_hype(hype), live.presses());
        assert_eq!(replayed.counts, live.score().counts);
        assert_eq!(replayed.points, live.score().points);
    }

    #[test]
    fn a_practice_loop_brings_the_notes_round_for_nothing() {
        // A two-second loop with a note every 500 ms, played three times.
        let notes: Vec<TimedNote> = (0..4).map(|i| TimedNote::tap(f64::from(i) * 500.0, Pad::P1)).collect();
        let mut run = Run::new(notes, Windows::TIGHT, RULES).with_hype([(0.0, 2000.0)]);
        play_perfectly(&mut run, 0.0, 2000.0);
        let (_, copies) = run.again(2000.0, 2000.0);
        assert_eq!(copies.len(), 4, "every note of the loop comes round");
        // The second time round, two of them are left alone.
        run.press(Lane::Pad(Pad::P1), 2000.0);
        run.press(Lane::Pad(Pad::P1), 2500.0);
        run.settle(4000.0);
        let (_, copies) = run.again(4000.0, 2000.0);
        assert_eq!(copies.len(), 4, "missed or hit, they all come round");
        play_perfectly(&mut run, 4000.0, 6000.0);
        run.finish();
        let score = run.score();
        assert_eq!(score.counts[Judgement::Wicked.index()], 10);
        assert_eq!(score.counts[Judgement::Miss.index()], 2);
        // Nothing to replay, and the multiplier never doubled.
        assert!(run.presses().iter().all(|p| p.rewind_ms.is_none()));
        assert!(run.boost().is_none());
    }

    #[test]
    fn no_wheel_up_without_the_hype() {
        let notes: Vec<TimedNote> = (0..8).map(|i| TimedNote::tap(f64::from(i) * 500.0, Pad::P1)).collect();
        let mut run = Run::new(notes, Windows::TIGHT, RULES).with_hype([(0.0, 2000.0)]);
        // The phrase is broken: nothing pressed in it.
        run.settle(3000.0);
        assert_eq!(run.hype(), 0.0);
        assert!(run.wheel_up(3000.0, 1000.0).is_none());
        assert!(run.presses().is_empty(), "a refused WHEEL UP! isn't recorded");
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

            let replayed = rejudge(Run::new(notes, Windows::TIGHT, RULES), live.presses());
            prop_assert_eq!(&replayed.counts, &live.score().counts);
            prop_assert_eq!(replayed.points, live.score().points);
            prop_assert_eq!(replayed.max_combo, live.score().max_combo);
        }
    }
}
