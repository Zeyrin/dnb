//! How often a controller reports, and how steadily.

use std::collections::{BTreeMap, VecDeque};

use crate::event::{DeviceId, InputEvent};

/// Intervals between consecutive events from the same device. Only gaps under
/// 50 ms count: longer ones are the player resting, not the controller's rate.
/// Move a stick to see the report rate.
#[derive(Clone, Debug, Default)]
pub struct IntervalStats {
    last: BTreeMap<DeviceId, u64>,
    intervals_ns: VecDeque<u64>,
}

const KEEP: usize = 512;
const MAX_GAP_NS: u64 = 50_000_000;

impl IntervalStats {
    pub fn observe(&mut self, event: &InputEvent) {
        if let Some(previous) = self.last.insert(event.device, event.at_ns) {
            let gap = event.at_ns.saturating_sub(previous);
            if gap > 0 && gap < MAX_GAP_NS {
                if self.intervals_ns.len() == KEEP {
                    self.intervals_ns.pop_front();
                }
                self.intervals_ns.push_back(gap);
            }
        }
    }

    pub fn samples(&self) -> usize {
        self.intervals_ns.len()
    }

    /// The `q` quantile (0–1) of the recent intervals, in milliseconds.
    pub fn quantile_ms(&self, q: f64) -> Option<f64> {
        if self.intervals_ns.is_empty() {
            return None;
        }
        let mut sorted: Vec<u64> = self.intervals_ns.iter().copied().collect();
        sorted.sort_unstable();
        let index = ((sorted.len() - 1) as f64 * q.clamp(0.0, 1.0)).round() as usize;
        Some(sorted[index] as f64 / 1e6)
    }

    /// Reports per second, from the median interval.
    pub fn rate_hz(&self) -> Option<f64> {
        self.quantile_ms(0.5).filter(|&ms| ms > 0.0).map(|ms| 1000.0 / ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Axis, InputKind};

    #[test]
    fn a_steady_250_hz_controller_reads_as_250_hz() {
        let mut stats = IntervalStats::default();
        for k in 0..100u64 {
            stats.observe(&InputEvent {
                at_ns: 1_000_000_000 + k * 4_000_000,
                device: DeviceId(1),
                kind: InputKind::Axis(Axis::LeftX, 0.1),
            });
        }
        assert_eq!(stats.samples(), 99);
        assert!((stats.rate_hz().expect("data") - 250.0).abs() < 1e-6);
        assert_eq!(stats.quantile_ms(0.95), Some(4.0));
    }

    #[test]
    fn long_pauses_are_not_intervals() {
        let mut stats = IntervalStats::default();
        let kind = InputKind::Axis(Axis::LeftX, 0.1);
        stats.observe(&InputEvent {
            at_ns: 0,
            device: DeviceId(1),
            kind,
        });
        stats.observe(&InputEvent {
            at_ns: 900_000_000,
            device: DeviceId(1),
            kind,
        });
        assert_eq!(stats.samples(), 0);
        assert_eq!(stats.rate_hz(), None);
    }
}
