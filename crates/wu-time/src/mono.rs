//! The process-wide monotonic clock, in nanoseconds.
//!
//! Input events, audio clock snapshots and frame times are all stamped on this
//! clock, so they can be compared without converting between time bases. Call
//! [`epoch`] once at startup so the epoch predates every timestamp; instants
//! taken before it read as 0.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

static EPOCH: OnceLock<Instant> = OnceLock::new();

/// The instant every timestamp counts from. The first call fixes it.
pub fn epoch() -> Instant {
    *EPOCH.get_or_init(Instant::now)
}

/// Now, in nanoseconds since [`epoch`].
pub fn now_ns() -> u64 {
    instant_to_ns(Instant::now())
}

pub fn instant_to_ns(instant: Instant) -> u64 {
    let since = instant.saturating_duration_since(epoch());
    u64::try_from(since.as_nanos()).unwrap_or(u64::MAX)
}

pub fn ns_to_instant(ns: u64) -> Instant {
    epoch() + Duration::from_nanos(ns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_instants() {
        let now = Instant::now();
        let ns = instant_to_ns(now);
        assert_eq!(instant_to_ns(ns_to_instant(ns)), ns);
    }

    #[test]
    fn never_goes_backwards() {
        let a = now_ns();
        let b = now_ns();
        assert!(b >= a);
    }
}
