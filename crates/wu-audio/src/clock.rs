//! The audio clock: what the engine was playing, and when it reaches the speaker.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering, fence};

/// The engine's position at the first frame of one callback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClockSnapshot {
    /// Frames rendered since the output started. Never jumps.
    pub device_frame: u64,
    /// Song position at that frame (transport frames, 0 = tick 0).
    pub transport_frame: i64,
    /// When that frame reaches the speaker, on the shared monotonic clock.
    pub playback_ns: u64,
    /// The device's own estimate of callback-to-speaker latency.
    pub output_latency_ns: u64,
    pub sample_rate: u32,
    pub playing: bool,
    /// Bumps on every jump of the transport: load, play, stop, seek, loop wrap.
    pub epoch: u64,
    /// How many programs have been loaded: tells the game when its own load
    /// has taken effect.
    pub generation: u64,
    /// The active loop, as transport frames; `end <= start` means none.
    pub loop_start: i64,
    pub loop_end: i64,
}

impl ClockSnapshot {
    fn wrap_into_loop(&self, frame: f64) -> f64 {
        let (start, end) = (self.loop_start as f64, self.loop_end as f64);
        if self.loop_end > self.loop_start && self.transport_frame < self.loop_end && frame >= end {
            start + (frame - start).rem_euclid(end - start)
        } else {
            frame
        }
    }
}

/// A seqlock: one writer (the audio thread) publishes snapshots without ever
/// waiting; readers retry if they caught a write half-way.
#[derive(Debug, Default)]
pub struct SharedClock {
    seq: AtomicU64,
    device_frame: AtomicU64,
    transport_frame: AtomicI64,
    playback_ns: AtomicU64,
    output_latency_ns: AtomicU64,
    sample_rate: AtomicU32,
    playing: AtomicBool,
    epoch: AtomicU64,
    generation: AtomicU64,
    loop_start: AtomicI64,
    loop_end: AtomicI64,
}

impl SharedClock {
    /// Only ever called by the single writer.
    pub(crate) fn publish(&self, s: &ClockSnapshot) {
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        self.device_frame.store(s.device_frame, Ordering::Relaxed);
        self.transport_frame.store(s.transport_frame, Ordering::Relaxed);
        self.playback_ns.store(s.playback_ns, Ordering::Relaxed);
        self.output_latency_ns.store(s.output_latency_ns, Ordering::Relaxed);
        self.sample_rate.store(s.sample_rate, Ordering::Relaxed);
        self.playing.store(s.playing, Ordering::Relaxed);
        self.epoch.store(s.epoch, Ordering::Relaxed);
        self.generation.store(s.generation, Ordering::Relaxed);
        self.loop_start.store(s.loop_start, Ordering::Relaxed);
        self.loop_end.store(s.loop_end, Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(2), Ordering::Release);
    }

    pub fn read(&self) -> ClockSnapshot {
        loop {
            let before = self.seq.load(Ordering::Acquire);
            if before % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let snapshot = ClockSnapshot {
                device_frame: self.device_frame.load(Ordering::Relaxed),
                transport_frame: self.transport_frame.load(Ordering::Relaxed),
                playback_ns: self.playback_ns.load(Ordering::Relaxed),
                output_latency_ns: self.output_latency_ns.load(Ordering::Relaxed),
                sample_rate: self.sample_rate.load(Ordering::Relaxed),
                playing: self.playing.load(Ordering::Relaxed),
                epoch: self.epoch.load(Ordering::Relaxed),
                generation: self.generation.load(Ordering::Relaxed),
                loop_start: self.loop_start.load(Ordering::Relaxed),
                loop_end: self.loop_end.load(Ordering::Relaxed),
            };
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == before {
                return snapshot;
            }
        }
    }
}

/// Turns snapshots into a smooth map from instants to song time.
///
/// Callbacks arrive with scheduling jitter, so a single snapshot can be a
/// millisecond or two off. The estimator fits a line through recent
/// `(playback time, device frame)` pairs, which averages the jitter away and
/// follows any drift between the sound card's crystal and the system clock.
#[derive(Debug)]
pub struct ClockEstimator {
    points: VecDeque<(u64, u64)>,
    capacity: usize,
    last: Option<ClockSnapshot>,
    fit: Option<Fit>,
}

#[derive(Clone, Copy, Debug)]
struct Fit {
    ns0: f64,
    frame0: f64,
    frames_per_ns: f64,
}

impl ClockEstimator {
    /// `capacity` snapshots are kept: at one per video frame, 120 is two seconds.
    pub fn new(capacity: usize) -> ClockEstimator {
        ClockEstimator {
            points: VecDeque::with_capacity(capacity),
            capacity: capacity.max(2),
            last: None,
            fit: None,
        }
    }

    pub fn last(&self) -> Option<ClockSnapshot> {
        self.last
    }

    pub fn observe(&mut self, snapshot: ClockSnapshot) {
        if snapshot.sample_rate == 0 {
            return;
        }
        if let Some(last) = self.last {
            if snapshot.device_frame == last.device_frame && snapshot.epoch == last.epoch {
                return;
            }
            let restarted = snapshot.device_frame < last.device_frame || snapshot.sample_rate != last.sample_rate;
            if restarted {
                self.points.clear();
            }
        }
        self.last = Some(snapshot);
        if self
            .points
            .back()
            .is_none_or(|&(_, frame)| frame != snapshot.device_frame)
        {
            if self.points.len() == self.capacity {
                self.points.pop_front();
            }
            self.points.push_back((snapshot.playback_ns, snapshot.device_frame));
        }
        self.fit = self.compute_fit(snapshot.sample_rate);
    }

    fn compute_fit(&self, sample_rate: u32) -> Option<Fit> {
        let nominal = f64::from(sample_rate) / 1e9;
        let n = self.points.len() as f64;
        let &(first_ns, first_frame) = self.points.front()?;
        // Centre on the first point so the sums stay small and precise.
        let (mut mean_t, mut mean_f) = (0.0, 0.0);
        for &(ns, frame) in &self.points {
            mean_t += (ns as f64 - first_ns as f64) / n;
            mean_f += (frame as f64 - first_frame as f64) / n;
        }
        let (mut cov, mut var) = (0.0, 0.0);
        for &(ns, frame) in &self.points {
            let dt = ns as f64 - first_ns as f64 - mean_t;
            let df = frame as f64 - first_frame as f64 - mean_f;
            cov += dt * df;
            var += dt * dt;
        }
        // Until there is a long enough baseline, trust the nominal rate; then
        // accept the fitted slope only within 1 % of it.
        let span_ns = self.points.back().map_or(0.0, |&(ns, _)| ns as f64 - first_ns as f64);
        let fitted = if var > 0.0 && span_ns > 0.2e9 {
            cov / var
        } else {
            nominal
        };
        let frames_per_ns = if (fitted / nominal - 1.0).abs() < 0.01 {
            fitted
        } else {
            nominal
        };
        Some(Fit {
            ns0: first_ns as f64 + mean_t,
            frame0: first_frame as f64 + mean_f,
            frames_per_ns,
        })
    }

    /// The device frame sounding at `ns`, extrapolated from the fit.
    pub fn device_frame_at(&self, ns: u64) -> Option<f64> {
        let fit = self.fit?;
        Some(fit.frame0 + (ns as f64 - fit.ns0) * fit.frames_per_ns)
    }

    /// When `device_frame` reaches (or reached) the speaker, on the shared clock.
    pub fn ns_at_device_frame(&self, device_frame: f64) -> Option<f64> {
        let fit = self.fit?;
        Some(fit.ns0 + (device_frame - fit.frame0) / fit.frames_per_ns)
    }

    /// The song position (transport frame, fractional) sounding at `ns`.
    /// While stopped, the position the transport is parked at.
    pub fn transport_frame_at(&self, ns: u64) -> Option<f64> {
        let last = self.last?;
        if !last.playing {
            return Some(last.transport_frame as f64);
        }
        let device = self.device_frame_at(ns)?;
        let frame = last.transport_frame as f64 + (device - last.device_frame as f64);
        Some(last.wrap_into_loop(frame))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use wu_dsp::Rng;

    use super::*;

    fn snapshot(device_frame: u64, playback_ns: u64) -> ClockSnapshot {
        ClockSnapshot {
            device_frame,
            transport_frame: device_frame as i64,
            playback_ns,
            sample_rate: 48_000,
            playing: true,
            ..ClockSnapshot::default()
        }
    }

    #[test]
    fn readers_never_see_a_torn_snapshot() {
        let clock = Arc::new(SharedClock::default());
        let writer = {
            let clock = Arc::clone(&clock);
            thread::spawn(move || {
                for i in 0..200_000u64 {
                    clock.publish(&ClockSnapshot {
                        device_frame: i,
                        transport_frame: i as i64 * 2,
                        playback_ns: i * 3,
                        sample_rate: 48_000,
                        ..ClockSnapshot::default()
                    });
                }
            })
        };
        for _ in 0..200_000 {
            let s = clock.read();
            assert_eq!(s.transport_frame, s.device_frame as i64 * 2);
            assert_eq!(s.playback_ns, s.device_frame * 3);
        }
        writer.join().expect("writer finished");
    }

    #[test]
    fn the_fit_averages_out_callback_jitter() {
        let mut estimator = ClockEstimator::new(120);
        let mut rng = Rng::new(3);
        let frames_per_ns = 48_000.0 / 1e9;
        let start_ns = 5_000_000_000u64;
        // One snapshot per ~16.7 ms video frame, each up to ±1.5 ms late or early.
        for k in 0..120u64 {
            let device_frame = k * 800;
            let true_ns = start_ns as f64 + device_frame as f64 / frames_per_ns;
            let jitter = f64::from(rng.range(-1.5e6, 1.5e6));
            estimator.observe(snapshot(device_frame, (true_ns + jitter) as u64));
        }
        let probe_frame = 119.0 * 800.0 + 300.0;
        let probe_ns = (start_ns as f64 + probe_frame / frames_per_ns) as u64;
        let error_frames = estimator.transport_frame_at(probe_ns).expect("fitted") - probe_frame;
        // Half a millisecond at 48 kHz, from raw snapshots up to 1.5 ms off.
        assert!(error_frames.abs() < 24.0, "error {error_frames} frames");
    }

    #[test]
    fn the_fit_follows_a_drifting_sound_card() {
        let mut estimator = ClockEstimator::new(240);
        // The card runs 100 ppm fast relative to the system clock.
        let frames_per_ns = 48_000.0 * 1.0001 / 1e9;
        for k in 0..240u64 {
            let device_frame = k * 800;
            estimator.observe(snapshot(device_frame, (device_frame as f64 / frames_per_ns) as u64));
        }
        let ten_seconds_on = 239.0 * 800.0 + 480_048.0;
        let ns = (ten_seconds_on / frames_per_ns) as u64;
        let error = estimator.transport_frame_at(ns).expect("fitted") - ten_seconds_on;
        assert!(error.abs() < 2.0, "error {error} frames");
        let back = estimator.ns_at_device_frame(ten_seconds_on).expect("fitted");
        assert!(
            (back - ns as f64).abs() < 50_000.0,
            "inverse off by {} ns",
            back - ns as f64
        );
    }

    #[test]
    fn positions_inside_a_loop_wrap_back_to_its_start() {
        let mut estimator = ClockEstimator::new(8);
        let mut s = snapshot(0, 0);
        s.transport_frame = 900;
        s.loop_start = 0;
        s.loop_end = 1000;
        estimator.observe(s);
        let at = estimator.transport_frame_at(200_000_000 / 48).expect("fitted");
        assert!((at - (900.0 + 200.0 - 1000.0)).abs() < 1e-3, "{at}");
    }

    #[test]
    fn a_stopped_transport_stays_put() {
        let mut estimator = ClockEstimator::new(8);
        let mut s = snapshot(4800, 100_000_000);
        s.playing = false;
        s.transport_frame = 1234;
        estimator.observe(s);
        assert_eq!(estimator.transport_frame_at(900_000_000), Some(1234.0));
    }
}
