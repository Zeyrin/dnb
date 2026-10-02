//! Changing a recording's sample rate: band-limited interpolation through a
//! Kaiser-windowed sinc, cut off just under the lower of the two Nyquists so
//! nothing folds back. Offline (it allocates): imported tunes are resampled
//! to the output's rate once, before they play.

/// Zero crossings of the sinc on each side of a sample: how sharp the cut-off is.
const ZEROS: usize = 24;
/// The cut-off, as a share of the lower Nyquist: the band from here up is the
/// filter's transition.
const PASS: f64 = 0.94;
/// The window's shape: about 90 dB down outside the pass band.
const KAISER_BETA: f64 = 8.6;
/// Ratios with at most this many distinct phases get a table per phase.
const MAX_PHASES: u64 = 4096;
/// Otherwise the kernel is tabled this finely per zero crossing, and read
/// between entries along straight lines.
const RESOLUTION: usize = 512;

/// `input` (`channels` channels, interleaved) at `from` Hz, at `to` Hz.
pub fn resample(input: &[f32], channels: usize, from: u32, to: u32) -> Vec<f32> {
    if from == to || channels == 0 || input.is_empty() {
        return input.to_vec();
    }
    let frames = input.len() / channels;
    let ratio = f64::from(to) / f64::from(from);
    let out_frames = (frames as u64 * u64::from(to)).div_ceil(u64::from(from)) as usize;
    // Taps reach this far into the input either side: further when shrinking,
    // since the filter then cuts below the input's own Nyquist.
    let cutoff = PASS * ratio.min(1.0);
    let reach = (ZEROS as f64 / cutoff).ceil() as i64;
    let taps = (2 * reach) as usize;
    let kernel = |distance: f64| cutoff * windowed_sinc(distance * cutoff);
    let mut out = vec![0.0f32; out_frames * channels];
    let divisor = gcd(u64::from(from), u64::from(to));
    let phases = u64::from(to) / divisor;
    // Output frame n sits at input position n · from / to: phase n mod `phases`
    // has the same fraction, and so the same weights.
    let weights: Option<Vec<f32>> = (phases <= MAX_PHASES).then(|| {
        (0..phases)
            .flat_map(|phase| {
                let position = phase as f64 / ratio;
                let first = position.floor() as i64 - reach + 1;
                (0..taps).map(move |k| kernel(position - (first + k as i64) as f64) as f32)
            })
            .collect()
    });
    let table: Vec<f64> = (0..=ZEROS * RESOLUTION + 1)
        .map(|i| windowed_sinc(i as f64 / RESOLUTION as f64))
        .collect();
    let mut tap_weights = vec![0.0f32; taps];
    for (n, frame) in out.chunks_exact_mut(channels).enumerate() {
        let position = n as f64 / ratio;
        let first = position.floor() as i64 - reach + 1;
        let row = match &weights {
            Some(weights) => {
                let phase = (n as u64 % phases) as usize;
                &weights[phase * taps..(phase + 1) * taps]
            }
            None => {
                for (k, w) in tap_weights.iter_mut().enumerate() {
                    let at = (position - (first + k as i64) as f64).abs() * cutoff * RESOLUTION as f64;
                    let i = at as usize;
                    *w = if i < ZEROS * RESOLUTION {
                        let fraction = at - i as f64;
                        (cutoff * (table[i] + (table[i + 1] - table[i]) * fraction)) as f32
                    } else {
                        0.0
                    };
                }
                &tap_weights
            }
        };
        match usize::try_from(first).ok().filter(|&f| f + taps <= frames) {
            // Clear of both ends: straight through the input, no checks.
            Some(first) if channels == 2 => {
                let (span, _) = input[first * 2..(first + taps) * 2].as_chunks::<2>();
                let (mut left, mut right) = (0.0f32, 0.0f32);
                for (&weight, [l, r]) in row.iter().zip(span) {
                    left += weight * l;
                    right += weight * r;
                }
                frame[0] = left;
                frame[1] = right;
            }
            Some(first) => {
                let span = &input[first * channels..(first + taps) * channels];
                for (&weight, samples) in row.iter().zip(span.chunks_exact(channels)) {
                    for (out, &x) in frame.iter_mut().zip(samples) {
                        *out += weight * x;
                    }
                }
            }
            None => {
                for (k, &weight) in row.iter().enumerate() {
                    let Some(source) = usize::try_from(first + k as i64).ok().filter(|&s| s < frames) else {
                        continue;
                    };
                    let samples = &input[source * channels..(source + 1) * channels];
                    for (out, &x) in frame.iter_mut().zip(samples) {
                        *out += weight * x;
                    }
                }
            }
        }
    }
    out
}

/// sinc(x) under a Kaiser window `ZEROS` crossings wide.
fn windowed_sinc(x: f64) -> f64 {
    let x = x.abs();
    if x >= ZEROS as f64 {
        return 0.0;
    }
    let sinc = if x < 1e-9 {
        1.0
    } else {
        (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
    };
    let along = x / ZEROS as f64;
    sinc * bessel_i0(KAISER_BETA * (1.0 - along * along).sqrt()) / bessel_i0(KAISER_BETA)
}

/// The modified Bessel function of the first kind, order 0, by its series.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let half = x / 2.0;
    for k in 1..50 {
        term *= half / k as f64;
        sum += term * term;
        if term * term < 1e-12 * sum {
            break;
        }
    }
    sum
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f64, rate: u32, seconds: f64) -> Vec<f32> {
        (0..(seconds * f64::from(rate)) as usize)
            .map(|i| (std::f64::consts::TAU * hz * i as f64 / f64::from(rate)).sin() as f32)
            .collect()
    }

    /// The level of `hz` in `signal` (away from its ends): the amplitude of the
    /// sine and cosine at `hz` that fit it best.
    fn level_of(signal: &[f32], hz: f64, rate: u32) -> f64 {
        let middle = &signal[signal.len() / 4..3 * signal.len() / 4];
        let (mut ss, mut sc, mut cc, mut xs, mut xc) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for (i, &x) in middle.iter().enumerate() {
            let phase = std::f64::consts::TAU * hz * i as f64 / f64::from(rate);
            let (s, c, x) = (phase.sin(), phase.cos(), f64::from(x));
            ss += s * s;
            sc += s * c;
            cc += c * c;
            xs += x * s;
            xc += x * c;
        }
        let determinant = ss * cc - sc * sc;
        let a = (xs * cc - xc * sc) / determinant;
        let b = (xc * ss - xs * sc) / determinant;
        (a * a + b * b).sqrt()
    }

    #[test]
    fn a_tone_keeps_its_pitch_and_level_through_a_change_of_rate() {
        for (from, to) in [(44_100, 48_000), (48_000, 44_100), (22_050, 48_000), (44_100, 47_999)] {
            for hz in [55.0, 1_000.0, 9_000.0] {
                let out = resample(&sine(hz, from, 0.5), 1, from, to);
                assert_eq!(out.len(), (0.5 * f64::from(to)).ceil() as usize, "{from}→{to}");
                let level = level_of(&out, hz, to);
                assert!((level - 1.0).abs() < 0.01, "{hz} Hz {from}→{to}: level {level:.4}");
            }
        }
    }

    #[test]
    fn what_the_new_rate_cannot_hold_is_filtered_out_not_folded_back() {
        // 23 kHz fits under 48 kHz's Nyquist, not under 44.1 kHz's.
        let out = resample(&sine(23_000.0, 48_000, 0.5), 1, 48_000, 44_100);
        let folded = 44_100.0 - 23_000.0;
        assert!(level_of(&out, folded, 44_100) < 0.001, "folds back to {folded} Hz");
        assert!(level_of(&out, 23_000.0, 44_100) < 0.001);
    }

    #[test]
    fn stereo_stays_apart_and_the_same_rate_is_a_copy() {
        let left = sine(440.0, 44_100, 0.2);
        let stereo: Vec<f32> = left.iter().flat_map(|&l| [l, 0.0]).collect();
        let out = resample(&stereo, 2, 44_100, 48_000);
        assert!(
            out.iter().skip(1).step_by(2).all(|&r| r == 0.0),
            "the right stays silent"
        );
        assert_eq!(resample(&stereo, 2, 44_100, 44_100), stereo);
    }
}
