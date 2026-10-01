//! Musical time for WHEEL UP!
//!
//! Positions are integer [`Tick`]s (960 per beat). A [`TempoMap`] turns them into
//! seconds and sample frames; nothing downstream ever accumulates time step by step,
//! so a song that runs for an hour lands on the same sample it would after a bar.
//! [`mono`] is the one monotonic clock that input timestamps and audio clock
//! snapshots are both expressed in.

#![forbid(unsafe_code)]

pub mod mono;
mod tempo;
mod tick;

pub use tempo::{MAX_BPM, MIN_BPM, TempoError, TempoMap, TempoPoint};
pub use tick::{BEATS_PER_BAR, PPQ, STEPS_PER_BAR, TICKS_PER_BAR, TICKS_PER_STEP, Tick};
