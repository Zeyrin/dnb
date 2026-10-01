//! Calibration: how late a player's taps land after the beats they hear or see.
//!
//! The audio clock already accounts for the sound card's own latency, so what
//! is left is everything between the speaker and the timestamp: the ears, the
//! thumb, the controller's report rate, the USB stack. Tapping along to clicks
//! measures it for sound; tapping along to flashes measures the display path.

/// The fewest taps a calibration needs after outliers are dropped.
pub const MIN_TAPS: usize = 8;
/// Taps scattered more widely than this (robust standard deviation) say more
/// about the tapping than about the latency: such a result should be redone.
pub const MAX_SPREAD_MS: f64 = 35.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estimate {
    /// Positive: taps land after the beat. Subtract this from future taps.
    pub offset_ms: f64,
    /// How consistent the taps were (robust standard deviation).
    pub spread_ms: f64,
    /// Taps that counted.
    pub used: usize,
}

impl Estimate {
    /// Steady enough to save.
    pub fn is_steady(&self) -> bool {
        self.spread_ms <= MAX_SPREAD_MS
    }
}

/// Matches each tap with its nearest beat and returns the typical lag, ignoring
/// taps more than half a beat away and outliers (beyond 3 robust deviations).
pub fn estimate(beats_ns: &[u64], taps_ns: &[u64]) -> Option<Estimate> {
    if beats_ns.len() < 2 {
        return None;
    }
    let mut beats = beats_ns.to_vec();
    beats.sort_unstable();
    let spacing = median(beats.windows(2).map(|w| (w[1] - w[0]) as f64).collect())?;
    let lags: Vec<f64> = taps_ns
        .iter()
        .filter_map(|&tap| {
            let i = beats.partition_point(|&b| b < tap);
            let candidates = [i.checked_sub(1).map(|j| beats[j]), beats.get(i).copied()];
            candidates
                .into_iter()
                .flatten()
                .map(|beat| tap as f64 - beat as f64)
                .min_by(|a, b| a.abs().total_cmp(&b.abs()))
        })
        .filter(|lag| lag.abs() < spacing / 2.0)
        .collect();
    let centre = median(lags.clone())?;
    let mad = median(lags.iter().map(|lag| (lag - centre).abs()).collect())?;
    let limit = (3.0 * 1.4826 * mad).max(2e6);
    let kept: Vec<f64> = lags.into_iter().filter(|lag| (lag - centre).abs() <= limit).collect();
    if kept.len() < MIN_TAPS {
        return None;
    }
    let offset = median(kept.clone())?;
    let spread = 1.4826 * median(kept.iter().map(|lag| (lag - offset).abs()).collect())?;
    Some(Estimate {
        offset_ms: offset / 1e6,
        spread_ms: spread / 1e6,
        used: kept.len(),
    })
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const BEAT: u64 = 500_000_000;

    fn beats(n: u64) -> Vec<u64> {
        (1..=n).map(|k| k * BEAT).collect()
    }

    #[test]
    fn a_steady_late_player_reads_as_late() {
        let taps: Vec<u64> = beats(16).iter().map(|b| b + 23_000_000).collect();
        let estimate = estimate(&beats(16), &taps).expect("enough taps");
        assert!((estimate.offset_ms - 23.0).abs() < 1e-9);
        assert_eq!(estimate.used, 16);
        assert!(estimate.spread_ms < 1e-9);
        assert!(estimate.is_steady());
    }

    #[test]
    fn scattered_taps_are_not_steady() {
        let jitter = [0, 120, -90, 60, -140, 100, -40, 150, -110, 80, -60, 130];
        let taps: Vec<u64> = beats(12)
            .iter()
            .zip(jitter)
            .map(|(b, j)| (*b as i64 + j * 1_000_000) as u64)
            .collect();
        let estimate = estimate(&beats(12), &taps).expect("enough taps");
        assert!(!estimate.is_steady(), "spread {}", estimate.spread_ms);
    }

    #[test]
    fn early_taps_give_a_negative_offset() {
        let taps: Vec<u64> = beats(12).iter().map(|b| b - 15_000_000).collect();
        assert!((estimate(&beats(12), &taps).expect("enough").offset_ms + 15.0).abs() < 1e-9);
    }

    #[test]
    fn stray_taps_are_ignored() {
        let mut taps: Vec<u64> = beats(12).iter().map(|b| b + 30_000_000).collect();
        taps.push(3 * BEAT + 180_000_000);
        taps.push(7 * BEAT + 240_000_000);
        let estimate = estimate(&beats(12), &taps).expect("enough");
        assert!((estimate.offset_ms - 30.0).abs() < 1e-9);
        assert_eq!(estimate.used, 12);
    }

    #[test]
    fn too_few_taps_give_nothing() {
        let taps: Vec<u64> = beats(5).iter().map(|b| b + 10_000_000).collect();
        assert_eq!(estimate(&beats(16), &taps), None);
        assert_eq!(estimate(&[BEAT], &taps), None);
    }

    proptest! {
        #[test]
        fn jittery_taps_still_find_the_lag(lag_ms in -60i64..90, jitter in prop::collection::vec(-12i64..12, 16)) {
            let taps: Vec<u64> = beats(16)
                .iter()
                .zip(&jitter)
                .map(|(b, j)| (*b as i64 + (lag_ms + j) * 1_000_000) as u64)
                .collect();
            let estimate = estimate(&beats(16), &taps).expect("enough");
            prop_assert!((estimate.offset_ms - lag_ms as f64).abs() <= 12.0);
        }
    }
}
