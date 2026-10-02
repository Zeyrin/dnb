//! The bass line: what the sub plays, sixteenth by sixteenth, as notes the
//! triggers can hold.
//!
//! The kick lives in the same octaves as the sub and hides it wherever they
//! meet, and in drum & bass they meet all the time: the bass moves with the
//! kick. So the kick is learnt from the tune and taken out first
//! (`without_kicks`), then every step's pitch is read (`readings`), then the
//! likeliest bass line behind those readings is found, the drums helping to
//! say where its notes change (`decode`).

use wu_audio::Note;
use wu_dsp::Svf;
use wu_time::Tick;

use crate::tempo::Grid;

/// The pitches a bass line is looked for in: A0 to C3…
const LOWEST_HZ: f64 = 27.5;
const HIGHEST_HZ: f64 = 131.0;
/// …and the keys its notes may take.
const FIRST_KEY: u8 = 21;
const LAST_KEY: u8 = 48;
/// Everything over this is filtered out before the pitch is measured.
const CUTOFF_HZ: f32 = 140.0;
/// Pitch is measured at a sixth of the sample rate: plenty for the sub.
const DECIMATE: usize = 6;

/// How long a kick may ring in the lows, at most.
const KICK_SPAN_S: f64 = 0.8;
/// How far from its step's line a kick may land (a played break drags and rushes).
const KICK_SLACK_S: f64 = 0.015;
/// The kick's opening, which it is lined up by: the sweep down to its note.
const KICK_OPENING_S: f64 = 0.1;
/// Fewer kicks than this are too few to learn the kick from.
const FEWEST_KICKS: usize = 8;
/// The kick is learnt again from the kicks its shape explains this well (0–1):
/// the ones the bass leaves alone, if the tune has enough of them.
const CLEAN_KICK: f32 = 0.9;
/// Kicks either side whose placing a kick's own is the median of.
const NEIGHBOURS: usize = 16;
/// A step the drum listener heard no kick on holds one all the same when the
/// kick's opening is found there this clearly (0–1)…
const KICK_LIKENESS: f32 = 0.5;
/// …at least this loud, against a typical kick.
const KICK_LEVEL: f32 = 0.35;

/// A step's pitch is measured from this share of the way into it, so a kick's
/// attack has passed…
const INTO_STEP: f64 = 0.35;
/// …over this long: two periods of the lowest note and some.
const WINDOW_S: f64 = 0.075;
/// YIN's threshold: how clean a period must be to be believed…
const CLEAR: f32 = 0.3;
/// …and, failing any that clean, how clean the cleanest must be.
const PLAUSIBLE: f32 = 0.3;

/// Steps either side that set how loud the bass is around a step…
const LOCAL_STEPS: usize = 128;
/// …never taken for less than this share of how loud it is in the whole tune.
const FLOOR: f32 = 0.5;
/// A step sounds from about this share of the bass's loudness around it…
const SOUNDING_AT: f32 = 0.55;
/// …the doubt spread over this much either side.
const SOUNDING_SPREAD: f32 = 0.08;
/// A step off the kicks with no pitch to it is this much less likely to sound
/// (the bass is a clean tone; noise is a drum's leftovers).
const UNPITCHED: f32 = 0.5;
/// How far, in semitones, a reading strays from the key it is of.
const SPREAD_KEYS: f64 = 0.6;
/// The most one step's reading can count against a key.
const WORST: f64 = 6.0;
/// What a reading an octave low (YIN taking two periods for one) costs on top.
const OCTAVE_LOW: f64 = 2.0;
/// A reading this unclean (YIN's clarity) counts for nothing.
const UNCLEAR: f32 = 0.6;
/// How much a reading on a kick counts, what the kick leaves blurring it.
const ON_KICK_TRUST: f64 = 0.25;
/// What it costs the line to start or change a note…
const CHANGE: f64 = 5.0;
/// …less on a kick, a beat or an eighth (a bass line moves with the drums)…
const ON_KICK: f64 = 0.5;
const ON_BEAT: f64 = 0.6;
const ON_EIGHTH: f64 = 0.8;
/// …and what stopping costs, wherever it falls.
const STOP: f64 = 0.5;
/// A level under this share of both its neighbours', within a note and off the
/// kicks, is the note let go and played again.
const DIP: f32 = 0.6;
/// Notes shorter than this many steps are dropped: too short to hold.
const SHORTEST_STEPS: i64 = 2;

/// The bass line in the first `steps` sixteenths, given where the kicks are
/// (sorted steps), as notes the triggers can hold.
pub fn hear_bass(mono: &[f32], sample_rate: u32, grid: &Grid, steps: i64, kicks: &[i64]) -> Vec<Note> {
    let rate = f64::from(sample_rate) / DECIMATE as f64;
    let low = without_kicks(&lows(mono, sample_rate), rate, grid, kicks);
    decode(&readings(&low, rate, grid, steps), kicks)
}

/// The lows, filtered forwards and backwards (no delay), at a sixth of the rate.
fn lows(mono: &[f32], sample_rate: u32) -> Vec<f32> {
    let filter = |signal: &mut Vec<f32>| {
        let [mut a, mut b] = [(); 2].map(|_| Svf::new(CUTOFF_HZ, 0.7, sample_rate));
        for x in signal.iter_mut() {
            *x = b.process(a.process(*x).low).low;
        }
    };
    let mut signal = mono.to_vec();
    filter(&mut signal);
    signal.reverse();
    filter(&mut signal);
    signal.reverse();
    signal.iter().step_by(DECIMATE).copied().collect()
}

/// The lows (at `rate`) with the kicks taken out: the kick learnt, found and
/// subtracted.
fn without_kicks(low: &[f32], rate: f64, grid: &Grid, kicks: &[i64]) -> Vec<f32> {
    match learn_kick(low, rate, grid, kicks) {
        Some(kick) => subtract(low, &kick.template, &find_kicks(low, rate, grid, &kick)),
        None => low.to_vec(),
    }
}

/// The kick, as the tune plays it.
#[derive(Clone, Debug, PartialEq)]
struct Kick {
    /// Its lows, from a little before it opens.
    template: Vec<f32>,
    /// The steps it was heard on…
    heard: Vec<i64>,
    /// …and where, from each one's line, in samples, its template starts.
    offset: Vec<f64>,
    /// How loud a typical one is, against the template.
    typical_gain: f32,
}

/// Where a kick's window fits in `length` samples of lows, and where on its
/// step it starts looking.
struct Windows {
    span: usize,
    slack: usize,
    opening: usize,
}

impl Windows {
    fn new(rate: f64) -> Windows {
        let span = (KICK_SPAN_S * rate) as usize;
        Windows {
            span,
            slack: (KICK_SLACK_S * rate).round() as usize,
            opening: ((KICK_OPENING_S * rate) as usize).min(span),
        }
    }

    /// Whether a kick on a line `line` samples in has room to be looked at.
    fn fit(&self, line: f64, length: usize) -> bool {
        line >= (2 * self.slack + 1) as f64 && line + ((self.span + 2 * self.slack + 2) as f64) <= length as f64
    }
}

/// The kick learnt from the tune itself: every kick's lows, lined up to a
/// fraction of a sample, and averaged sample by sample over the middle of the
/// pile, which the bass over some of them does not sway (it plays different
/// notes over different kicks); then again from the kicks heard clean. A
/// programmed kick sits the same distance from the grid every time, so each
/// kick is placed where its neighbours on the same sixteenth of the beat lie,
/// not where the bass over it pulls its own window.
fn learn_kick(low: &[f32], rate: f64, grid: &Grid, kicks: &[i64]) -> Option<Kick> {
    let Windows { span, slack, opening } = Windows::new(rate);
    let line = |step: i64| grid.time_of_step(step as f64) * rate;
    let heard: Vec<i64> = kicks
        .iter()
        .copied()
        .filter(|&k| Windows::new(rate).fit(line(k), low.len()))
        .collect();
    if heard.len() < FEWEST_KICKS {
        return None;
    }
    // How much of each kick's window is its own: up to the next kick's line.
    let own: Vec<usize> = heard
        .iter()
        .enumerate()
        .map(|(i, &k)| {
            heard
                .get(i + 1)
                .map_or(span, |&next| ((line(next) - line(k)) as usize).min(span))
        })
        .collect();
    let mut offset = vec![-(slack as f64); heard.len()];
    let mut gain = vec![1.0f32; heard.len()];
    let mut chosen = vec![true; heard.len()];
    let mut template = vec![0.0f32; span];
    let mut column = Vec::with_capacity(heard.len());
    let mut window = Vec::with_capacity(span + 2 * slack + 1);
    for _ in 0..4 {
        // Sample by sample, the chosen kicks still sounding alone there, averaged.
        let occurrences: Vec<Vec<f32>> = heard
            .iter()
            .zip(&offset)
            .zip(&own)
            .map(|((&k, &o), &mine)| {
                let mut x = Vec::with_capacity(mine);
                excerpt(low, line(k) + o, mine, &mut x);
                x
            })
            .collect();
        for (j, value) in template.iter_mut().enumerate() {
            column.clear();
            column.extend(
                occurrences
                    .iter()
                    .zip(&gain)
                    .zip(&chosen)
                    .filter(|&((x, &g), &c)| c && j < x.len() && g > 1e-6)
                    .map(|((x, &g), _)| x[j] / g),
            );
            *value = if column.len() >= FEWEST_KICKS / 2 {
                middle_mean(&mut column)
            } else {
                0.0
            };
        }
        // Each kick lined up by its opening, to a fraction of a sample…
        let head = &template[..opening];
        let lined: Vec<f64> = heard
            .iter()
            .map(|&k| {
                excerpt(low, line(k) - (2 * slack) as f64, opening + 2 * slack + 1, &mut window);
                let matching: Vec<f32> = (0..=2 * slack).map(|lag| dot(head, &window[lag..])).collect();
                let best = (0..matching.len())
                    .max_by(|&a, &b| matching[a].total_cmp(&matching[b]))
                    .unwrap_or(slack);
                best as f64 + vertex(&matching, best) - (2 * slack) as f64
            })
            .collect();
        // …then placed where its neighbours lie, and its level fitted.
        offset = heard.iter().map(|&k| neighbours_median(&heard, &lined, k)).collect();
        let mut likeness = vec![0.0f32; heard.len()];
        for (k, (&step, &o)) in heard.iter().zip(&offset).enumerate() {
            excerpt(low, line(step) + o, own[k], &mut window);
            let mine = &template[..own[k]];
            let fit = dot(&window, mine);
            gain[k] = (fit / dot(mine, mine).max(1e-12)).max(0.0);
            likeness[k] = fit / (dot(&window, &window) * dot(mine, mine)).sqrt().max(1e-12);
        }
        // Learnt again from the kicks heard clean, when there are enough of them.
        let clean = likeness.iter().filter(|&&l| l >= CLEAN_KICK).count();
        chosen = likeness
            .iter()
            .map(|&l| clean < FEWEST_KICKS || l >= CLEAN_KICK)
            .collect();
    }
    gain.sort_by(f32::total_cmp);
    Some(Kick {
        template,
        heard,
        offset,
        typical_gain: gain[gain.len() / 2],
    })
}

/// Where every kick starts its template, in samples: every kick heard, and
/// every step that clearly opens with one (the drum listener misses a soft
/// kick under a loud bass).
fn find_kicks(low: &[f32], rate: f64, grid: &Grid, kick: &Kick) -> Vec<f64> {
    let windows = Windows::new(rate);
    let head = &kick.template[..windows.opening];
    let head_energy = dot(head, head).max(1e-12);
    let mut window = Vec::with_capacity(windows.opening);
    (0..grid.step_at(low.len() as f64 / rate).0)
        .filter_map(|step| {
            let line = grid.time_of_step(step as f64) * rate;
            if !windows.fit(line, low.len()) {
                return None;
            }
            let at = line + neighbours_median(&kick.heard, &kick.offset, step);
            let found = kick.heard.binary_search(&step).is_ok() || {
                excerpt(low, at, windows.opening, &mut window);
                let fit = dot(&window, head);
                let likeness = fit / (dot(&window, &window) * head_energy).sqrt().max(1e-12);
                likeness >= KICK_LIKENESS && fit / head_energy >= KICK_LEVEL * kick.typical_gain
            };
            found.then_some(at)
        })
        .collect()
}

/// `low` with `template` subtracted wherever a kick starts, each fitted
/// together with the rest: where kicks overlap, each is fitted to what the
/// others leave.
fn subtract(low: &[f32], template: &[f32], starts: &[f64]) -> Vec<f32> {
    let mut residual = low.to_vec();
    let placed: Vec<(usize, Vec<f32>)> = starts.iter().map(|&at| place(template, at)).collect();
    let mut fitted = vec![0.0f32; placed.len()];
    for _ in 0..4 {
        for ((start, shape), fitted) in placed.iter().zip(&mut fitted) {
            let end = (start + shape.len()).min(residual.len());
            let part = &mut residual[*start..end];
            let shape = &shape[..part.len()];
            for (x, &t) in part.iter_mut().zip(shape) {
                *x += *fitted * t;
            }
            *fitted = (dot(part, shape) / dot(shape, shape).max(1e-12)).max(0.0);
            for (x, &t) in part.iter_mut().zip(shape) {
                *x -= *fitted * t;
            }
        }
    }
    residual
}

/// The median of `values` (one per heard kick) over the kicks around `step` on
/// the same sixteenth of the beat (a swung kick sits later than a straight
/// one), or over all of those around it when too few share it.
fn neighbours_median(heard: &[i64], values: &[f64], step: i64) -> f64 {
    let at = heard.partition_point(|&k| k < step);
    let range = at.saturating_sub(NEIGHBOURS)..(at + NEIGHBOURS + 1).min(heard.len());
    let place = step.rem_euclid(4);
    let mut around: Vec<f64> = range
        .clone()
        .filter(|&j| heard[j].rem_euclid(4) == place)
        .map(|j| values[j])
        .collect();
    if around.len() < 3 {
        around = values[range].to_vec();
    }
    around.sort_by(f64::total_cmp);
    around.get(around.len() / 2).copied().unwrap_or(0.0)
}

/// The mean of the middle half of `values` (sorting them): steadier than the
/// median where a bass note's sine piles values at both ends, deaf like it to
/// the odd drum that is not the kick.
fn middle_mean(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    let quarter = values.len() / 4;
    let middle = &values[quarter..values.len() - quarter];
    middle.iter().sum::<f32>() / middle.len().max(1) as f32
}

/// Where between samples the peak of `values` at `i` really is: the vertex of
/// the parabola through it and its neighbours, from −0.5 to 0.5.
fn vertex(values: &[f32], i: usize) -> f64 {
    if i == 0 || i + 1 >= values.len() {
        return 0.0;
    }
    let (a, b, c) = (f64::from(values[i - 1]), f64::from(values[i]), f64::from(values[i + 1]));
    let bend = a - 2.0 * b + c;
    if bend < -1e-12 {
        (0.5 * (a - c) / bend).clamp(-0.5, 0.5)
    } else {
        0.0
    }
}

/// `length` samples of `signal` from a position between samples, read along
/// straight lines between them (the lows are smooth at this rate).
fn excerpt(signal: &[f32], from: f64, length: usize, out: &mut Vec<f32>) {
    let base = from.floor();
    let fraction = (from - base) as f32;
    let base = base as i64;
    let get = |i: i64| {
        usize::try_from(i)
            .ok()
            .and_then(|i| signal.get(i))
            .copied()
            .unwrap_or(0.0)
    };
    out.clear();
    out.extend((0..length as i64).map(|j| get(base + j) * (1.0 - fraction) + get(base + j + 1) * fraction));
}

/// `template` set down from a position between samples: the first whole
/// sample it covers, and its values from there.
fn place(template: &[f32], at: f64) -> (usize, Vec<f32>) {
    let start = at.ceil().max(0.0);
    // Sample `start + j` lies `j + behind` into the template.
    let behind = (start - at) as f32;
    let get = |i: usize| template.get(i).copied().unwrap_or(0.0);
    let values = (0..template.len())
        .map(|j| get(j) * (1.0 - behind) + get(j + 1) * behind)
        .collect();
    (start as usize, values)
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// YIN: the period of `window`, in samples, and how clean it is (0 is perfect).
fn period(window: &[f32], shortest: usize, longest: usize) -> Option<(f64, f32)> {
    if window.len() < 2 * longest + 2 {
        return None;
    }
    let length = window.len() - longest - 1;
    let difference: Vec<f32> = (0..=longest + 1)
        .map(|lag| {
            (0..length)
                .map(|i| {
                    let d = window[i] - window[i + lag];
                    d * d
                })
                .sum()
        })
        .collect();
    // Each lag against the mean of those before it.
    let mut running = 0.0f32;
    let normalised: Vec<f32> = difference
        .iter()
        .enumerate()
        .map(|(lag, &d)| {
            if lag == 0 {
                return 1.0;
            }
            running += d;
            if running > 0.0 { d * lag as f32 / running } else { 1.0 }
        })
        .collect();
    let mut chosen = None;
    let mut lag = shortest;
    while lag <= longest {
        if normalised[lag] < CLEAR {
            // Down to the bottom of this dip.
            while lag < longest && normalised[lag + 1] < normalised[lag] {
                lag += 1;
            }
            chosen = Some(lag);
            break;
        }
        lag += 1;
    }
    let lag = chosen.or_else(|| {
        let best = (shortest..=longest).map(|lag| normalised[lag]).fold(f32::MAX, f32::min);
        // The shortest period nearly as clean as the cleanest: longer ones are its multiples.
        (best < PLAUSIBLE)
            .then(|| (shortest..=longest).find(|&lag| normalised[lag] < best + 0.05))
            .flatten()
    })?;
    // A dip still falling at either end of the range belongs to a pitch outside
    // it, and so does one whose half is as clean below the range: two periods of
    // a pitch above it (a pad's low note through the filter).
    let falls_to = |other: usize| normalised[other] < normalised[lag];
    if (lag == shortest && falls_to(lag - 1)) || (lag == longest && falls_to(lag + 1)) {
        return None;
    }
    let half = lag.div_ceil(2);
    if half < shortest
        && normalised[half - 1..=half + 1]
            .iter()
            .any(|&v| v < CLEAR.max(normalised[lag] + 0.1))
    {
        return None;
    }
    // Between samples: the vertex of the parabola through the dip.
    let (a, b, c) = (
        f64::from(normalised[lag - 1]),
        f64::from(normalised[lag]),
        f64::from(normalised[lag + 1]),
    );
    let bend = a - 2.0 * b + c;
    let shift = if bend > 1e-12 {
        (0.5 * (a - c) / bend).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    Some((lag as f64 + shift, normalised[lag]))
}

/// The MIDI key of a frequency, between keys where it falls between them.
fn fractional_key(hz: f64) -> f64 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

/// One step of the lows, as measured.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Reading {
    /// The pitch as a fractional key, and YIN's clarity (0 is perfectly periodic).
    pitch: Option<(f64, f32)>,
    /// How loud the lows are.
    level: f32,
}

/// Every step's reading of the lows (at `rate`).
fn readings(low: &[f32], rate: f64, grid: &Grid, steps: i64) -> Vec<Reading> {
    let shortest = (rate / HIGHEST_HZ).floor() as usize;
    let longest = (rate / LOWEST_HZ).ceil() as usize;
    let length = ((WINDOW_S * rate) as usize).max(2 * longest + 2);
    (0..steps)
        .map(|step| {
            let start = (grid.time_of_step(step as f64 + INTO_STEP) * rate).max(0.0) as usize;
            let window = low.get(start..(start + length).min(low.len())).unwrap_or(&[]);
            Reading {
                pitch: period(window, shortest, longest).map(|(lag, clarity)| (fractional_key(rate / lag), clarity)),
                level: (window.iter().map(|x| x * x).sum::<f32>() / window.len().max(1) as f32).sqrt(),
            }
        })
        .collect()
}

/// The likeliest bass line behind the readings: each step silent or on a key,
/// the line changing key or stopping as little as it can and, when it does,
/// by preference on a kick or a beat (a bass line moves with the drums). Where
/// a kick blurs a reading, the steps around it decide (Viterbi).
fn decode(readings: &[Reading], kicks: &[i64]) -> Vec<Note> {
    let count = readings.len();
    let levels: Vec<f32> = readings.iter().map(|r| r.level).collect();
    // How loud the bass is around each step; never taken for less than a share
    // of how loud it is in the whole tune, so an intro's pad or a kick's
    // leftovers are not heard as a bass line just because nothing else plays.
    let loud_end = |values: &[f32]| {
        let mut sorted = values.to_vec();
        sorted.sort_by(f32::total_cmp);
        sorted.get((sorted.len().max(1) - 1) * 9 / 10).copied().unwrap_or(0.0)
    };
    let whole = loud_end(&levels);
    let typical: Vec<f32> = (0..count)
        .map(|s| loud_end(&levels[s.saturating_sub(LOCAL_STEPS)..(s + LOCAL_STEPS + 1).min(count)]).max(FLOOR * whole))
        .collect();
    // State 0 is silence; state j is key FIRST_KEY + j − 1.
    let states = usize::from(LAST_KEY - FIRST_KEY) + 2;
    let mut cost = vec![0.0f64; states];
    let mut next = vec![0.0f64; states];
    let mut back: Vec<Vec<u8>> = Vec::with_capacity(count);
    for (s, reading) in readings.iter().enumerate() {
        let kick = kicks.binary_search(&(s as i64)).is_ok();
        let relative = reading.level / typical[s].max(1e-9);
        let mut sounding = 1.0 / (1.0 + (-(relative - SOUNDING_AT) / SOUNDING_SPREAD).exp());
        if reading.pitch.is_none() && !kick {
            sounding *= UNPITCHED;
        }
        let sounding = f64::from(sounding.clamp(1e-4, 1.0 - 1e-4));
        let trust = reading
            .pitch
            .map_or(0.0, |(_, clarity)| f64::from((1.0 - clarity / UNCLEAR).clamp(0.0, 1.0)))
            * if kick { ON_KICK_TRUST } else { 1.0 };
        let change = CHANGE
            * if kick {
                ON_KICK
            } else if s % 4 == 0 {
                ON_BEAT
            } else if s % 2 == 0 {
                ON_EIGHTH
            } else {
                1.0
            };
        // The cheapest key to come from, and the next cheapest (for coming from another key).
        let mut best = (0, f64::INFINITY);
        let mut second = (0, f64::INFINITY);
        for (j, &c) in cost.iter().enumerate().skip(1) {
            if c < best.1 {
                second = best;
                best = (j, c);
            } else if c < second.1 {
                second = (j, c);
            }
        }
        let cheapest = |options: &[(f64, usize)]| {
            options
                .iter()
                .copied()
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap_or((0.0, 0))
        };
        let mut from = vec![0u8; states];
        for j in 0..states {
            let (came, via) = if s == 0 {
                (0.0, j)
            } else if j == 0 {
                cheapest(&[(cost[0], 0), (best.1 + CHANGE * STOP, best.0)])
            } else {
                let other = if best.0 == j { second } else { best };
                cheapest(&[(cost[j], j), (cost[0] + change, 0), (other.1 + change, other.0)])
            };
            let emit = if j == 0 {
                -(1.0 - sounding).ln()
            } else {
                let key = f64::from(FIRST_KEY) + (j - 1) as f64;
                let off_key = reading.pitch.map_or(0.0, |(pitch, _)| {
                    let near = (pitch - key) / SPREAD_KEYS;
                    let octave_low = (pitch + 12.0 - key) / SPREAD_KEYS;
                    (near * near).min(octave_low * octave_low + OCTAVE_LOW).min(WORST)
                });
                -sounding.ln() + trust * off_key
            };
            next[j] = came + emit;
            from[j] = via as u8;
        }
        std::mem::swap(&mut cost, &mut next);
        back.push(from);
    }
    let mut state = cost
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0, |(j, _)| j);
    let mut path = vec![0usize; count];
    for s in (0..count).rev() {
        path[s] = state;
        state = usize::from(back[s][state]);
    }
    // A dip in the level within a note, off the kicks, is the note let go and played again.
    for s in 1..count.saturating_sub(1) {
        if path[s] != 0
            && path[s - 1] == path[s]
            && path[s + 1] == path[s]
            && levels[s] < DIP * levels[s - 1].min(levels[s + 1])
            && kicks.binary_search(&(s as i64)).is_err()
        {
            path[s] = 0;
        }
    }
    let mut notes = Vec::new();
    let mut s = 0;
    while s < count {
        let (state, start) = (path[s], s);
        while s < count && path[s] == state {
            s += 1;
        }
        let length = (s - start) as i64;
        if state != 0 && length >= SHORTEST_STEPS {
            notes.push(Note {
                tick: Tick::from_steps(start as i64),
                length: Tick::from_steps(length),
                key: FIRST_KEY + (state - 1) as u8,
                velocity: 0.9,
            });
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use wu_time::TICKS_PER_STEP;

    use super::*;

    const RATE: f64 = 8_000.0;

    fn sine(hz: f64, samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|i| (std::f64::consts::TAU * hz * i as f64 / RATE).sin() as f32)
            .collect()
    }

    #[test]
    fn a_sine_gives_its_pitch() {
        for hz in [43.65, 55.0, 87.31, 110.0] {
            let (lag, clean) = period(&sine(hz, 800), 60, 300).expect("a clear pitch");
            assert!((RATE / lag - hz).abs() < 0.3, "{hz} Hz heard as {}", RATE / lag);
            assert!(clean < 0.05);
        }
        assert!((fractional_key(43.65) - 29.0).abs() < 0.01, "F1");
    }

    #[test]
    fn noise_and_pitches_out_of_range_have_none() {
        let mut state = 0x9E37_79B9u32;
        let noise: Vec<f32> = (0..800)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as f32 / u32::MAX as f32 - 0.5
            })
            .collect();
        assert!(period(&noise, 60, 300).is_none());
        // 200 Hz is above a range that tops out at 8000 / 60 ≈ 133 Hz.
        assert!(period(&sine(200.0, 800), 60, 300).is_none());
    }

    /// A kick of the kind the kits make: a sweep down to its note, dying away.
    fn kick() -> Vec<f32> {
        let mut phase = 0.0f64;
        (0..(0.5 * RATE) as usize)
            .map(|i| {
                let t = i as f64 / RATE;
                phase += (52.0 + 140.0 * (-t / 0.03).exp()) / RATE;
                ((std::f64::consts::TAU * phase).sin() * (-t / 0.15).exp()) as f32
            })
            .collect()
    }

    /// The kick is taken out of a tune that opens on 8 bars of drums, then
    /// plays the bass as loud as the kick on every beat for 24, and out of one
    /// with the bass over every kick from the start: how far down it goes in each.
    fn kicks_taken_out(intro_bars: usize) -> f32 {
        let grid = Grid {
            bpm: 170.0,
            first_bar_s: 0.1,
            fit: 1.0,
        };
        let bars = 32;
        let length = (grid.time_of_step((bars * 16 + 8) as f64) * RATE) as usize;
        let mut bass = vec![0.0f32; length];
        for bar in intro_bars..bars {
            // F1, A♭1, B♭1, C2: a synth starting each note on the same phase.
            let hz = [43.65, 51.91, 58.27, 65.41][bar % 4];
            let from = (grid.time_of_step((bar * 16) as f64) * RATE) as usize;
            let to = (grid.time_of_step(((bar + 1) * 16) as f64) * RATE) as usize;
            for (i, x) in bass[from..to].iter_mut().enumerate() {
                *x = 0.3 * (std::f64::consts::TAU * hz * i as f64 / RATE).sin() as f32;
            }
        }
        let shape = kick();
        let kicks: Vec<i64> = (0..(bars * 16) as i64).step_by(4).collect();
        let mut drums = vec![0.0f32; length];
        for &k in &kicks {
            let at = (grid.time_of_step(k as f64) * RATE).round() as usize;
            for (x, &v) in drums[at..].iter_mut().zip(&shape) {
                *x += 0.8 * v;
            }
        }
        let mix: Vec<f32> = bass.iter().zip(&drums).map(|(b, d)| b + d).collect();
        let cleaned = without_kicks(&mix, RATE, &grid, &kicks);
        let energy = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
        let left: Vec<f32> = cleaned.iter().zip(&bass).map(|(c, b)| c - b).collect();
        10.0 * (energy(&left) / energy(&drums)).log10()
    }

    #[test]
    fn the_kick_is_learnt_and_taken_out_and_the_bass_left() {
        let with_intro = kicks_taken_out(8);
        assert!(
            with_intro < -18.0,
            "with an intro, the kicks are only {with_intro:.1} dB down"
        );
        let bass_throughout = kicks_taken_out(0);
        assert!(
            bass_throughout < -10.0,
            "under the bass throughout, only {bass_throughout:.1} dB down"
        );
    }

    fn reading(key: f64, level: f32) -> Reading {
        Reading {
            pitch: Some((key, 0.02)),
            level,
        }
    }

    fn heard(notes: &[Note]) -> Vec<(i64, i64, u8)> {
        notes
            .iter()
            .map(|n| (n.tick.0 / TICKS_PER_STEP, n.length.0 / TICKS_PER_STEP, n.key))
            .collect()
    }

    #[test]
    fn a_note_a_kick_blurs_starts_on_the_kick() {
        // F1 for a bar, then C2; the kick on 16 reads as a muddy F♯1.
        let mut steps: Vec<Reading> = (0..32)
            .map(|s| reading(if s < 16 { 29.0 } else { 36.0 }, 1.0))
            .collect();
        steps[16] = Reading {
            pitch: Some((30.2, 0.3)),
            level: 0.7,
        };
        assert_eq!(heard(&decode(&steps, &[0, 16])), [(0, 16, 29), (16, 16, 36)]);
    }

    #[test]
    fn a_note_played_again_after_a_breath_is_two_notes() {
        // F1, let go on step 3, F1 again: the dip in level parts them.
        let mut steps: Vec<Reading> = (0..8).map(|_| reading(29.0, 1.0)).collect();
        steps[3] = reading(29.0, 0.4);
        assert_eq!(heard(&decode(&steps, &[0])), [(0, 3, 29), (4, 4, 29)]);
    }
}
