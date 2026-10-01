//! Charts and the auto-charter.

use std::collections::BTreeMap;

use wu_audio::{Hit, Note};
use wu_instruments::Pad;
use wu_time::{PPQ, TICKS_PER_BAR, TICKS_PER_STEP, TempoMap, Tick};

use crate::rules::{
    Difficulty, MIN_ROLL_NOTES, RAIL_GAP_MS, ROLL_GAP_MS, Rail, Rules, Thumb, opposite, priority, thumb,
};

/// A note to play: a pad at a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChartNote {
    pub tick: Tick,
    pub pad: Pad,
}

/// A run of fast notes on one lane, from its first note to its last. Inside it,
/// that hand's shoulder button (L1 or R1) plays the lane too, so the thumb and a
/// finger can take turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Roll {
    pub start: Tick,
    pub end: Tick,
    pub pad: Pad,
}

impl Roll {
    pub fn covers(&self, tick: Tick, pad: Pad) -> bool {
        pad == self.pad && self.start <= tick && tick <= self.end
    }
}

/// A bass note on a rail: press the trigger as it starts, hold it until it ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hold {
    pub start: Tick,
    pub end: Tick,
    pub rail: Rail,
    /// MIDI key of the bass note.
    pub key: u8,
    pub velocity: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chart {
    pub difficulty: Difficulty,
    /// Sorted by tick, then pad; no duplicates.
    pub notes: Vec<ChartNote>,
    /// Sorted; never two at once on one thumb.
    pub rolls: Vec<Roll>,
    /// Sorted by start; on each rail, a gap of at least `RAIL_GAP_MS` between holds.
    pub holds: Vec<Hold>,
}

impl Chart {
    pub fn contains(&self, tick: Tick, pad: Pad) -> bool {
        self.notes.binary_search(&ChartNote { tick, pad }).is_ok()
    }

    /// The roll a note is part of, if any.
    pub fn roll_of(&self, tick: Tick, pad: Pad) -> Option<&Roll> {
        self.rolls.iter().find(|roll| roll.covers(tick, pad))
    }

    /// Whether the bass note starting at `tick` with `key` is the player's to hold.
    pub fn holds_note(&self, tick: Tick, key: u8) -> bool {
        self.holds.iter().any(|h| h.start == tick && h.key == key)
    }
}

/// How strong a position is in the bar: 0 for the downbeat, up to 5 off the grid.
/// Thinning keeps strong positions first, so an easier chart still sounds like
/// the beat rather than a random subset of it.
pub(crate) fn metric_level(tick: Tick) -> u8 {
    let t = tick.0.rem_euclid(TICKS_PER_BAR);
    if t == 0 {
        0
    } else if t % (2 * PPQ) == 0 {
        1
    } else if t % PPQ == 0 {
        2
    } else if t % (2 * TICKS_PER_STEP) == 0 {
        3
    } else if t % TICKS_PER_STEP == 0 {
        4
    } else {
        5
    }
}

/// Thins a drum part down to what `difficulty` allows, and puts the bass line
/// on the rails it has.
pub fn auto_chart(hits: &[Hit], bass: &[Note], tempo: &TempoMap, difficulty: Difficulty) -> Chart {
    let rules = difficulty.rules();
    let mut candidates: Vec<ChartNote> = hits
        .iter()
        .filter(|h| rules.pads.contains(&h.pad) && h.velocity >= rules.min_velocity)
        .map(|h| ChartNote {
            tick: h.tick,
            pad: h.pad,
        })
        .collect();
    candidates.sort();
    candidates.dedup();
    // Strongest positions and most important pads are placed first.
    candidates.sort_by_key(|n| (metric_level(n.tick), priority(n.pad), n.tick));

    let ms_at = |tick: Tick| tempo.seconds_at(tick.0 as f64) * 1000.0;
    let mut by_tick: BTreeMap<Tick, Vec<Pad>> = BTreeMap::new();
    let mut thumbs: [BTreeMap<Tick, f64>; 2] = [BTreeMap::new(), BTreeMap::new()];
    for note in candidates {
        let side = thumb_index(thumb(note.pad));
        let at = ms_at(note.tick);
        let accepted_thumb = &thumbs[side];
        let too_close = |neighbour: Option<(&Tick, &f64)>| {
            neighbour.is_some_and(|(&tick, &ms)| tick != note.tick && (at - ms).abs() < rules.min_same_thumb_ms)
        };
        if too_close(accepted_thumb.range(..note.tick).next_back())
            || too_close(accepted_thumb.range(note.tick..).find(|(t, _)| **t != note.tick))
        {
            continue;
        }
        let chord = by_tick.entry(note.tick).or_default();
        let same_thumb: Vec<Pad> = chord.iter().copied().filter(|&p| thumb(p) == thumb(note.pad)).collect();
        if chord.len() >= rules.max_chord || same_thumb.len() >= 2 || same_thumb.iter().any(|&p| opposite(p, note.pad))
        {
            continue;
        }
        chord.push(note.pad);
        thumbs[side].insert(note.tick, at);
    }

    let mut notes: Vec<ChartNote> = by_tick
        .into_iter()
        .flat_map(|(tick, pads)| pads.into_iter().map(move |pad| ChartNote { tick, pad }))
        .collect();
    notes.sort();
    thin_density(&mut notes, tempo, &rules);
    let rolls = if rules.rolls {
        make_rolls(&mut notes, tempo)
    } else {
        Vec::new()
    };
    let holds = auto_holds(bass, tempo, &rules, &rolls);
    Chart {
        difficulty,
        notes,
        rolls,
        holds,
    }
}

/// The shortest hold worth keeping after it was shortened to make room: half a step.
const MIN_HOLD: Tick = Tick(TICKS_PER_STEP / 2);

/// Puts each bass note on a rail. With two rails, low notes go left and high
/// notes right, and back-to-back notes take turns; with one, each hold is cut
/// short so the trigger can come up before the next. A rail is never used while
/// its hand is rolling.
fn auto_holds(bass: &[Note], tempo: &TempoMap, rules: &Rules, rolls: &[Roll]) -> Vec<Hold> {
    if rules.rails.is_empty() {
        return Vec::new();
    }
    let ms_at = |tick: Tick| tempo.seconds_at(tick.0 as f64) * 1000.0;
    let mut notes: Vec<&Note> = bass.iter().filter(|n| n.length > Tick::ZERO).collect();
    notes.sort_by_key(|n| (n.tick, n.key));
    let mut keys: Vec<u8> = notes.iter().map(|n| n.key).collect();
    keys.sort_unstable();
    let middle = keys.get(keys.len() / 2).copied().unwrap_or(0);

    let mut holds: Vec<Hold> = Vec::new();
    for note in notes {
        let (start, end) = (note.tick, note.tick + note.length);
        let rolling = |rail: Rail| {
            rolls
                .iter()
                .any(|r| thumb(r.pad) == rail.thumb() && r.start < end && start <= r.end)
        };
        let mut order: Vec<Rail> = rules.rails.iter().copied().filter(|&r| !rolling(r)).collect();
        if note.key < middle {
            order.sort();
        } else {
            order.sort_by(|a, b| b.cmp(a));
        }
        // When the rail's last hold ends; it may be pressed again a gap later.
        let free_at = |rail: Rail| {
            holds
                .iter()
                .rev()
                .find(|h| h.rail == rail)
                .map_or(f64::NEG_INFINITY, |h| ms_at(h.end))
        };
        let start_ms = ms_at(start);
        let Some(rail) = order
            .iter()
            .copied()
            .find(|&r| free_at(r) + RAIL_GAP_MS <= start_ms + 1e-6)
            .or_else(|| order.iter().copied().min_by(|a, b| free_at(*a).total_cmp(&free_at(*b))))
        else {
            // Every rail's hand is rolling: the backing keeps this note.
            continue;
        };
        if let Some(previous) = holds.iter_mut().rev().find(|h| h.rail == rail) {
            let latest_end = Tick(tempo.tick_at_seconds((start_ms - RAIL_GAP_MS) / 1000.0).floor() as i64);
            if previous.end > latest_end {
                if latest_end - previous.start < MIN_HOLD {
                    continue;
                }
                previous.end = latest_end;
            }
        }
        holds.push(Hold {
            start,
            end,
            rail,
            key: note.key,
            velocity: note.velocity,
        });
    }
    holds
}

/// The roll a run of fast notes on one thumb makes, if it is on a single lane,
/// give or take one other note just before and one just after: the shoulder
/// can take the roll's first or last stroke, freeing the thumb for that note.
fn roll_in(run: &[(Tick, Vec<usize>)], notes: &[ChartNote]) -> Option<Roll> {
    let single = |(_, chord): &(Tick, Vec<usize>)| (chord.len() == 1).then(|| notes[chord[0]].pad);
    let n = run.len();
    for (before, after) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        if n < before + after + MIN_ROLL_NOTES {
            continue;
        }
        let core = &run[before..n - after];
        let Some(pad) = single(&core[0]) else { continue };
        let neighbours_ok = run[..before]
            .iter()
            .chain(&run[n - after..])
            .all(|t| single(t).is_some_and(|p| p != pad));
        if neighbours_ok && core.iter().all(|t| single(t) == Some(pad)) {
            return Some(Roll {
                start: core[0].0,
                end: core[core.len() - 1].0,
                pad,
            });
        }
    }
    None
}

/// Keeps each fast run on a single lane as a roll, and thins every other run
/// of same-thumb notes closer than `ROLL_GAP_MS` until it is no longer one.
fn make_rolls(notes: &mut Vec<ChartNote>, tempo: &TempoMap) -> Vec<Roll> {
    let ms_at = |tick: Tick| tempo.seconds_at(tick.0 as f64) * 1000.0;
    'again: loop {
        let mut rolls = Vec::new();
        for side in [Thumb::Left, Thumb::Right] {
            // This thumb's notes, grouped by tick, as indices into `notes`.
            let mut ticks: Vec<(Tick, Vec<usize>)> = Vec::new();
            for (i, note) in notes.iter().enumerate().filter(|(_, n)| thumb(n.pad) == side) {
                match ticks.last_mut() {
                    Some((tick, chord)) if *tick == note.tick => chord.push(i),
                    _ => ticks.push((note.tick, vec![i])),
                }
            }
            let mut first = 0;
            while first < ticks.len() {
                let mut last = first;
                while last + 1 < ticks.len() && ms_at(ticks[last + 1].0) - ms_at(ticks[last].0) + 1e-6 < ROLL_GAP_MS {
                    last += 1;
                }
                let run = &ticks[first..=last];
                if run.len() > 1 {
                    if let Some(roll) = roll_in(run, notes) {
                        rolls.push(roll);
                    } else {
                        // Drop the run's weakest layer at once (its least important pad
                        // on its weakest positions), so what's left stays regular.
                        let rank = |i: usize| (metric_level(notes[i].tick), priority(notes[i].pad));
                        let members = run.iter().flat_map(|(_, chord)| chord.iter().copied());
                        if let Some(weakest) = members.clone().map(rank).max() {
                            let doomed: Vec<usize> = members.filter(|&i| rank(i) == weakest).collect();
                            let mut index = 0;
                            notes.retain(|_| {
                                index += 1;
                                !doomed.contains(&(index - 1))
                            });
                            continue 'again;
                        }
                    }
                }
                first = last + 1;
            }
        }
        rolls.sort();
        return rolls;
    }
}

pub(crate) fn thumb_index(thumb: Thumb) -> usize {
    match thumb {
        Thumb::Left => 0,
        Thumb::Right => 1,
    }
}

/// Two-bar windows, one bar apart, each with its start and end tick.
pub(crate) fn density_windows(notes: &[ChartNote]) -> impl Iterator<Item = (Tick, Tick)> {
    let last_bar = notes.last().map_or(0, |n| n.tick.bar());
    let first_bar = notes.first().map_or(0, |n| n.tick.bar());
    (first_bar..=last_bar).map(|bar| (Tick::from_bars(bar), Tick::from_bars(bar + 2)))
}

pub(crate) fn notes_per_second(notes: &[ChartNote], tempo: &TempoMap, (start, end): (Tick, Tick)) -> f64 {
    let count = notes.iter().filter(|n| n.tick >= start && n.tick < end).count();
    let seconds = tempo.seconds_at(end.0 as f64) - tempo.seconds_at(start.0 as f64);
    count as f64 / seconds
}

/// Removes the weakest notes from any two-bar window that is too dense.
fn thin_density(notes: &mut Vec<ChartNote>, tempo: &TempoMap, rules: &Rules) {
    loop {
        let crowded = density_windows(notes).find(|&w| notes_per_second(notes, tempo, w) > rules.max_notes_per_second);
        let Some((start, end)) = crowded else { return };
        let weakest = notes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.tick >= start && n.tick < end)
            .max_by_key(|(_, n)| (metric_level(n.tick), priority(n.pad), n.tick))
            .map(|(i, _)| i);
        match weakest {
            Some(i) => {
                notes.remove(i);
            }
            None => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(step: i64, pad: Pad, velocity: f32) -> Hit {
        Hit {
            tick: Tick::from_steps(step),
            pad,
            velocity,
        }
    }

    /// Two bars of 16th hats, kick and snare on the backbeat, ghosts in between.
    fn busy_bar() -> Vec<Hit> {
        let mut hits = Vec::new();
        for bar in 0..2 {
            let s = bar * 16;
            hits.extend((0..16).map(|i| hit(s + i, Pad::P7, 0.8)));
            hits.extend([hit(s, Pad::P1, 1.0), hit(s + 10, Pad::P1, 0.8)]);
            hits.extend([hit(s + 4, Pad::P2, 1.0), hit(s + 12, Pad::P2, 1.0)]);
            hits.extend([hit(s + 7, Pad::P3, 0.45), hit(s + 9, Pad::P3, 0.45)]);
        }
        hits
    }

    #[test]
    fn metric_levels_rank_the_grid() {
        let levels: Vec<u8> = [0, 1920, 960, 480, 240, 120].map(|t| metric_level(Tick(t))).to_vec();
        assert_eq!(levels, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn easy_keeps_the_backbone_on_strong_beats() {
        let tempo = TempoMap::constant(168.0);
        let chart = auto_chart(&busy_bar(), &[], &tempo, Difficulty::Easy);
        assert!(chart.contains(Tick::ZERO, Pad::P1), "kick on the one");
        assert!(chart.contains(Tick::from_steps(4), Pad::P2), "snare on two");
        assert!(chart.notes.iter().all(|n| n.pad != Pad::P3), "no ghosts");
        // Hats are thinned to strong positions only: never 16ths.
        assert!(
            chart
                .notes
                .iter()
                .filter(|n| n.pad == Pad::P7)
                .all(|n| metric_level(n.tick) <= 3)
        );
    }

    #[test]
    fn junglist_keeps_a_run_on_one_lane_as_a_roll() {
        // A bar of 16th snares (89 ms apart at 168 BPM), then a kick.
        let tempo = TempoMap::constant(168.0);
        let mut hits: Vec<Hit> = (0..16).map(|step| hit(step, Pad::P5, 0.9)).collect();
        hits.push(hit(24, Pad::P1, 1.0));
        let chart = auto_chart(&hits, &[], &tempo, Difficulty::Junglist);
        assert_eq!(
            chart.rolls,
            vec![Roll {
                start: Tick::ZERO,
                end: Tick::from_steps(15),
                pad: Pad::P5
            }]
        );
        assert_eq!(chart.notes.len(), 17, "every stroke kept");
        assert!(chart.roll_of(Tick::from_steps(7), Pad::P5).is_some());
        // Hard has no rolls: the run is thinned to 8ths.
        let hard = auto_chart(&hits, &[], &tempo, Difficulty::Hard);
        assert!(hard.rolls.is_empty());
        assert!(!hard.contains(Tick::from_steps(1), Pad::P5));
    }

    #[test]
    fn a_roll_may_run_straight_into_the_next_downbeat() {
        // The shoulder takes the roll's last stroke; the thumb moves to the kick.
        let tempo = TempoMap::constant(168.0);
        let mut hits: Vec<Hit> = (8..16).map(|step| hit(step, Pad::P2, 0.9)).collect();
        hits.push(hit(16, Pad::P1, 1.0));
        let chart = auto_chart(&hits, &[], &tempo, Difficulty::Junglist);
        assert_eq!(
            chart.rolls,
            vec![Roll {
                start: Tick::from_steps(8),
                end: Tick::from_steps(15),
                pad: Pad::P2
            }]
        );
        assert!(chart.contains(Tick::from_steps(16), Pad::P1));
        assert_eq!(crate::validate(&chart, &tempo), Vec::new());
    }

    #[test]
    fn a_fast_run_that_switches_lanes_is_thinned_not_rolled() {
        // 16th hats with the snare on two and four: the thumb would have to jump
        // lanes every 89 ms, which the shoulder button can't help with.
        let tempo = TempoMap::constant(168.0);
        let mut hits: Vec<Hit> = (0..16).map(|step| hit(step, Pad::P7, 0.8)).collect();
        hits.extend([hit(4, Pad::P5, 1.0), hit(12, Pad::P5, 1.0)]);
        let chart = auto_chart(&hits, &[], &tempo, Difficulty::Junglist);
        assert!(chart.rolls.is_empty());
        assert!(chart.contains(Tick::from_steps(4), Pad::P5) && chart.contains(Tick::from_steps(12), Pad::P5));
        assert!(chart.contains(Tick::from_steps(2), Pad::P7), "8th hats survive");
        assert!(!chart.contains(Tick::from_steps(3), Pad::P7), "16th hats don't");
    }

    /// Notes of a bass line: (first step, length in steps, key).
    fn bass(notes: &[(i64, i64, u8)]) -> Vec<Note> {
        notes
            .iter()
            .map(|&(step, length, key)| Note {
                tick: Tick::from_steps(step),
                length: Tick::from_steps(length),
                key,
                velocity: 0.9,
            })
            .collect()
    }

    /// Four legato notes, a beat each.
    fn legato_line() -> Vec<Note> {
        bass(&[(0, 4, 29), (4, 4, 32), (8, 4, 36), (12, 4, 29)])
    }

    #[test]
    fn medium_holds_the_bass_on_r2_with_time_to_let_go() {
        let tempo = TempoMap::constant(168.0);
        let chart = auto_chart(&[], &legato_line(), &tempo, Difficulty::Medium);
        assert_eq!(chart.holds.len(), 4);
        assert!(chart.holds.iter().all(|h| h.rail == Rail::Right));
        assert!(
            chart.holds[0].end < Tick::from_steps(4),
            "cut short before the next note"
        );
        assert_eq!(chart.holds[3].end, Tick::from_steps(16), "the last keeps its length");
        assert!(chart.holds_note(Tick::from_steps(8), 36));
        assert_eq!(crate::validate(&chart, &tempo), Vec::new());
    }

    #[test]
    fn hard_shares_the_line_between_both_rails() {
        let tempo = TempoMap::constant(168.0);
        let chart = auto_chart(&[], &legato_line(), &tempo, Difficulty::Hard);
        let rails: Vec<Rail> = chart.holds.iter().map(|h| h.rail).collect();
        assert_eq!(rails, vec![Rail::Left, Rail::Right, Rail::Left, Rail::Right]);
        assert!(
            chart.holds.iter().all(|h| h.end - h.start == Tick::from_steps(4)),
            "taking turns, nothing needs cutting"
        );
        assert_eq!(crate::validate(&chart, &tempo), Vec::new());
    }

    #[test]
    fn the_easiest_charts_leave_the_bass_to_the_backing() {
        let tempo = TempoMap::constant(168.0);
        for difficulty in [Difficulty::Beginner, Difficulty::Easy] {
            assert!(auto_chart(&[], &legato_line(), &tempo, difficulty).holds.is_empty());
        }
    }

    #[test]
    fn a_rail_is_never_held_while_its_hand_rolls() {
        let tempo = TempoMap::constant(168.0);
        // A left-hand snare roll, under a low bass note that would rather be on L2.
        let hits: Vec<Hit> = (8..16).map(|step| hit(step, Pad::P2, 0.9)).collect();
        let chart = auto_chart(&hits, &bass(&[(4, 12, 29), (24, 4, 40)]), &tempo, Difficulty::Junglist);
        assert_eq!(chart.rolls.len(), 1);
        assert_eq!(chart.holds[0].rail, Rail::Right);
        assert_eq!(crate::validate(&chart, &tempo), Vec::new());
    }

    #[test]
    fn hard_keeps_more_than_easy() {
        let tempo = TempoMap::constant(168.0);
        let easy = auto_chart(&busy_bar(), &[], &tempo, Difficulty::Easy);
        let hard = auto_chart(&busy_bar(), &[], &tempo, Difficulty::Hard);
        assert!(hard.notes.len() > easy.notes.len());
        assert!(hard.notes.windows(2).all(|w| w[0] < w[1]), "sorted, no duplicates");
    }
}
