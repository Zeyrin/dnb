//! An output with no device: a thread that calls the engine at real-time pace
//! and throws the sound away.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{OutputInfo, rt_checked};
use crate::engine::{BufferTiming, Engine};

#[derive(Debug)]
pub struct NullOutput {
    pub info: OutputInfo,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl NullOutput {
    pub fn start(mut engine: Engine, buffer_frames: u32) -> NullOutput {
        let sample_rate = engine.sample_rate();
        let frames = buffer_frames.clamp(16, 8192) as usize;
        let period = Duration::from_secs_f64(frames as f64 / f64::from(sample_rate));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("wheelup-null-audio".into())
                .spawn(move || {
                    let mut buffer = vec![0.0f32; frames * 2];
                    let mut next = Instant::now();
                    while !stop.load(Ordering::Relaxed) {
                        let latency = period.as_nanos() as u64;
                        let timing = BufferTiming {
                            playback_ns: wu_time::mono::now_ns() + latency,
                            output_latency_ns: latency,
                        };
                        rt_checked(|| engine.process(&mut buffer, timing));
                        next += period;
                        if let Some(wait) = next.checked_duration_since(Instant::now()) {
                            thread::sleep(wait);
                        } else {
                            next = Instant::now();
                        }
                    }
                })
                .ok()
        };
        NullOutput {
            info: OutputInfo {
                host: "none".into(),
                device: "null output (no sound)".into(),
                sample_rate,
                channels: 2,
                buffer_frames: Some(frames as u32),
                sample_format: "f32".into(),
                bluetooth: false,
            },
            stop,
            thread,
        }
    }
}

impl Drop for NullOutput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
