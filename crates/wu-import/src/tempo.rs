//! The beat: the tempo and the bar grid, exact enough that a hit five minutes
//! into a tune still lands on its sixteenth.

use wu_dsp::{OnePole, Svf};

use crate::spectrum::{Band, Spectrogram};

/// Tempos the listener considers, in BPM.
const SLOWEST: f64 = 60.0;
const FASTEST: f64 = 220.0;
/// The tempo it expects: drum & bass and jungle sit around here.
const EXPECTED_BPM: f64 = 172.0;
/// How far from it a tune may be before the expectation counts against it, in octaves.
const PRIOR_OCTAVES: f64 = 0.3;

/// A bar line this close before the audio starts still counts as its first.
const EARLY_BAR_S: f64 = 0.05;
/// Fewer attacks than this in a band and it can't place the grid.
const MIN_ATTACKS: usize = 8;

/// A tune's grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub bpm: f64,
    /// When the first bar line falls, in seconds into the audio: from a hair
    /// before it starts up to a bar in.
    pub first_bar_s: f64,
    /// The share of strong onsets that sit on the sixteenth grid (0–1).
    pub fit: f32,
}

impl Grid {
    pub fn beat_s(&self) -> f64 {
        60.0 / self.bpm
    }

    pub fn step_s(&self) -> f64 {
        self.beat_s() / 4.0
    }

    pub fn bar_s(&self) -> f64 {
        4.0 * self.beat_s()
    }

    /// When sixteenth `step` (counted from the first bar line) falls, in seconds.
    pub fn time_of_step(&self, step: f64) -> f64 {
        self.first_bar_s + step * self.step_s()
    }

    /// The sixteenth nearest `seconds`, and how far from it, in steps (−0.5 to 0.5).
    pub fn step_at(&self, seconds: f64) -> (i64, f64) {
        let exact = (seconds - self.first_bar_s) / self.step_s();
        let step = exact.round();
        (step as i64, exact - step)
    }
}

/// One onset strength curve from all four bands, each weighted by how much it
/// says about the beat and scaled so a quiet band still counts.
pub fn onset_envelope(spec: &Spectrogram) -> Vec<f32> {
    let weights = [
        (Band::Low, 1.0f32),
        (Band::Body, 0.6),
        (Band::Crack, 0.8),
        (Band::Air, 0.5),
    ];
    let mut envelope = vec![0.0f32; spec.frames];
    for (band, weight) in weights {
        let flux = spec.flux(band);
        let mean = flux.iter().sum::<f32>() / flux.len().max(1) as f32;
        if mean <= 0.0 {
            continue;
        }
        for (e, &x) in envelope.iter_mut().zip(flux) {
            *e += weight * x / mean;
        }
    }
    // Only what rises above its surroundings (a quarter of a second each way).
    let half = (0.25 * spec.rate()) as usize;
    let mut prefix = vec![0.0f64; envelope.len() + 1];
    for (i, &x) in envelope.iter().enumerate() {
        prefix[i + 1] = prefix[i] + f64::from(x);
    }
    envelope
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            let (from, to) = (i.saturating_sub(half), (i + half + 1).min(envelope.len()));
            let local = (prefix[to] - prefix[from]) / (to - from) as f64;
            (x - local as f32).max(0.0)
        })
        .collect()
}

/// The envelope between frames, linearly.
fn at(envelope: &[f32], frame: f64) -> f32 {
    if frame < 0.0 {
        return 0.0;
    }
    let i = frame as usize;
    let frac = (frame - i as f64) as f32;
    match (envelope.get(i), envelope.get(i + 1)) {
        (Some(&a), Some(&b)) => a + (b - a) * frac,
        (Some(&a), None) => a,
        _ => 0.0,
    }
}

/// How strongly the envelope pulses on a grid of `period` frames from `phase`:
/// the mean of the envelope on its beats, the eighths between them counting half.
fn comb(envelope: &[f32], period: f64, phase: f64) -> f64 {
    let mut sum = 0.0;
    let mut count = 0usize;
    let mut beat = phase;
    while beat < envelope.len() as f64 {
        sum += f64::from(at(envelope, beat)) + 0.5 * f64::from(at(envelope, beat + period / 2.0));
        count += 1;
        beat += period;
    }
    sum / count.max(1) as f64
}

/// The best phase for `period`, and its comb score.
fn best_phase(envelope: &[f32], period: f64) -> (f64, f64) {
    let mut best = (0.0, f64::MIN);
    let mut phase = 0.0;
    while phase < period {
        let score = comb(envelope, period, phase);
        if score > best.1 {
            best = (phase, score);
        }
        phase += 0.25;
    }
    best
}

fn prior(bpm: f64) -> f64 {
    let octaves = (bpm / EXPECTED_BPM).log2() / PRIOR_OCTAVES;
    (-0.5 * octaves * octaves).exp()
}

/// The tempo, roughly: autocorrelation picks candidates, the comb and the
/// expected tempo choose between them (and their doubles and halves).
fn rough_tempo(envelope: &[f32], rate: f64) -> Option<(f64, f64)> {
    let shortest = (60.0 * rate / FASTEST).floor() as usize;
    let longest = (60.0 * rate / SLOWEST).ceil() as usize;
    if envelope.len() < 2 * longest {
        return None;
    }
    let correlation: Vec<f64> = (0..=longest + 1)
        .map(|lag| {
            if lag < shortest {
                return 0.0;
            }
            envelope
                .iter()
                .zip(&envelope[lag..])
                .map(|(&a, &b)| f64::from(a) * f64::from(b))
                .sum::<f64>()
        })
        .collect();
    let mut candidates: Vec<f64> = (shortest + 1..longest)
        .filter(|&lag| correlation[lag] > correlation[lag - 1] && correlation[lag] >= correlation[lag + 1])
        .map(|lag| {
            // Between frames: the vertex of the parabola through the peak.
            let (a, b, c) = (correlation[lag - 1], correlation[lag], correlation[lag + 1]);
            let shift = 0.5 * (a - c) / (a - 2.0 * b + c).min(-1e-12);
            60.0 * rate / (lag as f64 + shift)
        })
        .collect();
    candidates.sort_by(|a, b| b.total_cmp(a));
    let mut best: Option<(f64, f64, f64)> = None;
    for bpm in candidates.into_iter().flat_map(|bpm| [bpm, bpm * 2.0, bpm / 2.0]) {
        if !(SLOWEST..=FASTEST).contains(&bpm) {
            continue;
        }
        let period = 60.0 * rate / bpm;
        let (phase, score) = best_phase(envelope, period);
        let weighted = score * prior(bpm);
        if best.is_none_or(|(_, _, s)| weighted > s) {
            best = Some((bpm, phase, weighted));
        }
    }
    best.map(|(bpm, phase, _)| (bpm, phase / rate))
}

/// Strong onsets, in seconds, each with its strength: peaks of the envelope
/// well above its typical level.
fn strong_onsets(envelope: &[f32], rate: f64) -> Vec<(f64, f64)> {
    let mut sorted: Vec<f32> = envelope.iter().copied().filter(|&x| x > 0.0).collect();
    sorted.sort_by(f32::total_cmp);
    let Some(&threshold) = sorted.get(sorted.len() * 3 / 4) else {
        return Vec::new();
    };
    (1..envelope.len().saturating_sub(1))
        .filter(|&i| envelope[i] > threshold && envelope[i] > envelope[i - 1] && envelope[i] >= envelope[i + 1])
        .map(|i| {
            let (a, b, c) = (
                f64::from(envelope[i - 1]),
                f64::from(envelope[i]),
                f64::from(envelope[i + 1]),
            );
            let shift = 0.5 * (a - c) / (a - 2.0 * b + c).min(-1e-12);
            ((i as f64 + shift.clamp(-0.5, 0.5)) / rate, b)
        })
        .collect()
}

/// Fits the grid to the onsets that sit near it: least squares of onset time
/// against sixteenth number, weighted by strength, outliers dropped each round.
/// It starts from the first onsets and takes in twice as many each time, so
/// a rough tempo a hair off never puts a hit late in the tune on the wrong
/// sixteenth.
fn fit(onsets: &[(f64, f64)], bpm: f64, origin_s: f64) -> Option<(f64, f64, f32)> {
    let mut step_s = 60.0 / bpm / 4.0;
    let mut count = FIRST_ONSETS.min(onsets.len());
    // Where the sixteenths fall, from where the first onsets do: the mean of
    // their phases round the step (a circular mean, so 0.95 and 0.05 agree),
    // weighted by strength. The rough phase only says which line is first.
    let (sine, cosine) = onsets[..count].iter().fold((0.0, 0.0), |(s, c), &(t, weight)| {
        let angle = std::f64::consts::TAU * t / step_s;
        (s + weight * angle.sin(), c + weight * angle.cos())
    });
    let phase = sine.atan2(cosine).rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU * step_s;
    let mut origin = phase + ((origin_s - phase) / step_s).round() * step_s;
    let mut share = 0.0f32;
    loop {
        let some = &onsets[..count];
        for tolerance in [1.0 / 3.0, 0.2, 0.12] {
            let (mut sw, mut sn, mut st, mut snn, mut snt) = (0.0, 0.0, 0.0, 0.0, 0.0);
            let mut kept = 0usize;
            for &(t, weight) in some {
                let exact = (t - origin) / step_s;
                let n = exact.round();
                if (exact - n).abs() > tolerance {
                    continue;
                }
                kept += 1;
                sw += weight;
                sn += weight * n;
                st += weight * t;
                snn += weight * n * n;
                snt += weight * n * t;
            }
            let denominator = sw * snn - sn * sn;
            if kept < 8 || denominator.abs() < 1e-12 {
                return None;
            }
            step_s = (sw * snt - sn * st) / denominator;
            origin = (st - step_s * sn) / sw;
            share = kept as f32 / some.len().max(1) as f32;
        }
        if count == onsets.len() {
            break;
        }
        count = (2 * count).min(onsets.len());
    }
    Some((60.0 / (4.0 * step_s), origin, share))
}

/// How far from the fitted tempo, in BPM, [`refine`] looks, and in what steps.
const REFINE_BPM: f64 = 0.5;
const REFINE_STEP_BPM: f64 = 0.002;

/// The tempo, near `bpm`, on whose sixteenths the whole tune's onsets line up
/// best: the strength-weighted circular mean of every frame's phase round the
/// step, as long as it can be. The fit follows strong onsets one by one, and a
/// tune with many off the grid (a voice, a pad's swells) pulls it a fifth of a
/// BPM off, enough to drift three sixteenths over four minutes; every frame of
/// the onset curve at once is not pulled.
fn refine(envelope: &[f32], rate: f64, bpm: f64, origin_s: f64) -> (f64, f64) {
    let phase_sums = |bpm: f64| {
        let step_s = 60.0 / bpm / 4.0;
        envelope.iter().enumerate().fold((0.0, 0.0), |(s, c), (i, &e)| {
            let angle = std::f64::consts::TAU * (i as f64 / rate) / step_s;
            (s + f64::from(e) * angle.sin(), c + f64::from(e) * angle.cos())
        })
    };
    let candidates = (2.0 * REFINE_BPM / REFINE_STEP_BPM).round() as i64;
    let best = (0..=candidates)
        .map(|k| bpm - REFINE_BPM + k as f64 * REFINE_STEP_BPM)
        .map(|candidate| {
            let (s, c) = phase_sums(candidate);
            (s.hypot(c), candidate)
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(bpm, |(_, candidate)| candidate);
    let step_s = 60.0 / best / 4.0;
    let (s, c) = phase_sums(best);
    let phase = s.atan2(c).rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU * step_s;
    (best, phase + ((origin_s - phase) / step_s).round() * step_s)
}

/// How many onsets the grid is first fitted to.
const FIRST_ONSETS: usize = 64;

/// Which sixteenth starts the bar: the one the kick lands on, with the snare
/// on two and four, or on three in half time. Kicks are told from the rest by
/// how far they lift the energy in the lows, which a snare's or a hat's noise
/// never matches, and which a long kick still ringing a sixteenth on no longer
/// does.
fn downbeat(spec: &Spectrogram, bpm: f64, origin_s: f64) -> f64 {
    let step_s = 60.0 / bpm / 4.0;
    let rate = spec.rate();
    let (lows, crack) = (spec.energy(Band::Low), spec.energy(Band::Crack));
    // How much more energy a band holds just after a grid line than just before it.
    let most = |energy: &[f32], from_s: f64, to_s: f64| -> f64 {
        let from = (from_s * rate).max(0.0) as usize;
        let to = ((to_s * rate) as usize + 1).min(energy.len());
        energy
            .get(from..to)
            .map_or(0.0, |w| w.iter().copied().fold(0.0f32, f32::max).into())
    };
    let after =
        |energy: &[f32], at_s: f64| (most(energy, at_s, at_s + 0.03) - most(energy, at_s - 0.04, at_s - 0.01)).max(0.0);
    // Mean strength on each sixteenth of the bar, over the whole tune.
    let mut kick_on = [0.0f64; 16];
    let mut snare_on = [0.0f64; 16];
    let mut step = 0usize;
    loop {
        let at_s = origin_s + step as f64 * step_s;
        if at_s * rate >= spec.frames as f64 - 1.0 {
            break;
        }
        if at_s >= 0.0 {
            kick_on[step % 16] += after(lows, at_s);
            snare_on[step % 16] += after(crack, at_s);
        }
        step += 1;
    }
    let normalise = |values: &mut [f64; 16]| {
        let total: f64 = values.iter().sum();
        if total > 0.0 {
            values.iter_mut().for_each(|v| *v /= total);
        }
    };
    normalise(&mut kick_on);
    normalise(&mut snare_on);
    // The kick on one, and the snare on two and four, or in half time on three.
    let score = |beat: usize| {
        let s = |offset: usize| snare_on[(beat + offset) % 16];
        let two_step = s(4) + s(12) - s(0) - 0.5 * s(8);
        let half_time = s(8) - s(0) - 0.5 * (s(4) + s(12));
        2.0 * kick_on[beat] + two_step.max(half_time)
    };
    // Every sixteenth is a candidate: the fit's origin is on the grid, not
    // necessarily on a beat.
    let first = (0..16).max_by(|&a, &b| score(a).total_cmp(&score(b))).unwrap_or(0);
    origin_s + first as f64 * step_s
}

/// A band's loudness, sample by sample: band-passed and smoothed over a
/// millisecond, each filter run forwards then backwards so nothing is delayed.
pub(crate) fn band_envelope(mono: &[f32], sample_rate: u32, band: Band) -> Vec<f32> {
    let (low, high) = band.range();
    let band_pass = |signal: &mut Vec<f32>| {
        let mut high_pass = Svf::new(low, 0.7, sample_rate);
        let mut low_pass = Svf::new(high, 0.7, sample_rate);
        for x in signal.iter_mut() {
            *x = low_pass.process(high_pass.process(*x).high).low;
        }
    };
    let mut signal = mono.to_vec();
    band_pass(&mut signal);
    signal.reverse();
    band_pass(&mut signal);
    signal.reverse();
    let smooth = |signal: &mut Vec<f32>| {
        let mut one_pole = OnePole::new(160.0, sample_rate);
        for x in signal.iter_mut() {
            *x = one_pole.low(*x);
        }
    };
    signal.iter_mut().for_each(|x| *x = x.abs());
    smooth(&mut signal);
    signal.reverse();
    smooth(&mut signal);
    signal.reverse();
    signal
}

/// Where a hit near `around_s` really starts: halfway up from the quietest
/// point before its peak to the peak, in seconds.
fn attack(envelope: &[f32], sample_rate: u32, around_s: f64) -> Option<f64> {
    let sr = f64::from(sample_rate);
    let from = ((around_s - 0.03) * sr).max(0.0) as usize;
    let to = (((around_s + 0.03) * sr) as usize).min(envelope.len());
    let window = envelope.get(from..to)?;
    let (peak_at, &peak) = window.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
    let floor = window[..=peak_at].iter().copied().fold(f32::MAX, f32::min);
    if peak <= 0.0 || peak < 2.0 * floor {
        return None;
    }
    let half = floor + 0.5 * (peak - floor);
    let rise = window[..=peak_at].iter().rposition(|&x| x < half)? + 1;
    Some((from + rise) as f64 / sr)
}

/// Moves the grid so it sits on the hits as they sound: onset curves from
/// spectra run a little early, so the median gap between each strong hit's
/// real attack and its grid line is taken off.
///
/// The attacks are read in the crack band (snares, hats, a kick's click): the
/// low band's filters smear a long kick's attack several milliseconds early,
/// which on a halftime beat, kick-heavy, pulled the grid 12 ms off. A tune with
/// too few cracks falls back on its lows.
fn align_to_attacks(mono: &[f32], sample_rate: u32, onsets: &[(f64, f64)], bpm: f64, origin_s: f64) -> f64 {
    let step_s = 60.0 / bpm / 4.0;
    let lines: Vec<f64> = onsets
        .iter()
        .filter_map(|&(t, _)| {
            let line = origin_s + ((t - origin_s) / step_s).round() * step_s;
            (((t - line) / step_s).abs() <= 0.25).then_some(line)
        })
        .collect();
    let gaps_in = |band: Band| -> Vec<f64> {
        let envelope = band_envelope(mono, sample_rate, band);
        lines
            .iter()
            .filter_map(|&line| attack(&envelope, sample_rate, line).map(|start| start - line))
            .collect()
    };
    let mut gaps = gaps_in(Band::Crack);
    if gaps.len() < MIN_ATTACKS {
        gaps = gaps_in(Band::Low);
    }
    if gaps.len() < MIN_ATTACKS {
        return origin_s;
    }
    gaps.sort_by(f64::total_cmp);
    origin_s + gaps[gaps.len() / 2]
}

/// Finds the grid, or `None` if the tune has no steady beat to find.
pub fn find_grid(mono: &[f32], spec: &Spectrogram) -> Option<Grid> {
    let envelope = onset_envelope(spec);
    let rate = spec.rate();
    let (bpm, phase_s) = rough_tempo(&envelope, rate)?;
    let onsets = strong_onsets(&envelope, rate);
    let (bpm, origin_s, share) = fit(&onsets, bpm, phase_s)?;
    let (bpm, origin_s) = refine(&envelope, rate, bpm, origin_s);
    let origin_s = align_to_attacks(mono, spec.sample_rate, &onsets, bpm, origin_s);
    let bar_line = downbeat(spec, bpm, origin_s);
    let bar_s = 4.0 * 60.0 / bpm;
    // The earliest bar line in the audio, or one a hair before it starts.
    let mut first_bar_s = bar_line.rem_euclid(bar_s);
    if first_bar_s > bar_s - EARLY_BAR_S {
        first_bar_s -= bar_s;
    }
    Some(Grid {
        bpm,
        first_bar_s,
        fit: share,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spectrum::analyse;

    const SR: u32 = 48_000;

    /// A drum & bass beat as clicks and thumps: kick on one and three-and, snare on
    /// two and four, eighth hats, from `start_s`, for `bars` bars.
    fn beat(bpm: f64, start_s: f64, bars: usize) -> Vec<f32> {
        let step_s = 60.0 / bpm / 4.0;
        let length = start_s + bars as f64 * 16.0 * step_s + 1.0;
        let mut audio = vec![0.0f32; (length * f64::from(SR)) as usize];
        let mut put = |at_s: f64, kind: u8| {
            let start = (at_s * f64::from(SR)).round() as usize;
            let mut noise = 0x1234_5678u32 ^ start as u32;
            for i in 0..9_600 {
                let t = i as f32 / SR as f32;
                noise ^= noise << 13;
                noise ^= noise >> 17;
                noise ^= noise << 5;
                let white = noise as f32 / u32::MAX as f32 * 2.0 - 1.0;
                let x = match kind {
                    0 => (std::f32::consts::TAU * 55.0 * t).sin() * (-t / 0.08).exp(),
                    1 => white * (-t / 0.05).exp() * 0.6,
                    _ => white * (-t / 0.01).exp() * 0.2,
                };
                if let Some(slot) = audio.get_mut(start + i) {
                    *slot += x;
                }
            }
        };
        for bar in 0..bars {
            let bar_s = start_s + bar as f64 * 16.0 * step_s;
            put(bar_s, 0);
            put(bar_s + 10.0 * step_s, 0);
            put(bar_s + 4.0 * step_s, 1);
            put(bar_s + 12.0 * step_s, 1);
            for eighth in 0..8 {
                put(bar_s + 2.0 * eighth as f64 * step_s, 2);
            }
        }
        audio
    }

    #[test]
    fn a_drum_and_bass_beat_gives_its_tempo_and_bar_line() {
        for (bpm, start_s) in [(174.0, 0.3), (165.5, 1.1), (160.0, 0.0)] {
            let audio = beat(bpm, start_s, 48);
            let spec = analyse(&audio, SR);
            let grid = find_grid(&audio, &spec).expect("a steady beat");
            assert!((grid.bpm - bpm).abs() < 0.01, "{bpm} BPM heard as {}", grid.bpm);
            let bar_s = 4.0 * 60.0 / bpm;
            let expected = start_s.rem_euclid(bar_s);
            let off = (grid.first_bar_s - expected).abs();
            assert!(
                off < 0.004,
                "{bpm} BPM: bar line at {} s, wanted {expected}",
                grid.first_bar_s
            );
            assert!(grid.fit > 0.8, "{bpm} BPM: fit {}", grid.fit);
        }
    }

    #[test]
    fn silence_has_no_grid() {
        let silence = vec![0.0; SR as usize * 10];
        assert!(find_grid(&silence, &analyse(&silence, SR)).is_none());
    }
}
