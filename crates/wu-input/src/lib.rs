//! WHEEL UP! input.
//!
//! Controllers are read on their own thread ([`InputThread`]), and every event is
//! stamped on the shared monotonic clock the moment it is read, or earlier when the
//! OS supplies its own timestamp. Pad presses go two ways at once: straight to the
//! audio engine through a callback, so the sound never waits for a frame, and to
//! the main thread with their timestamps, for judging and display.

#![forbid(unsafe_code)]

pub mod backend;
mod event;
mod mapping;
mod stats;
mod thread;

pub use event::{Axis, Button, DeviceId, DeviceInfo, Family, InputEvent, InputKind, KEYBOARD};
pub use mapping::{Action, ActionEvent, Hand, Layout, Mapper, Phase, RAIL_PRESS, RAIL_RELEASE};
pub use stats::IntervalStats;
pub use thread::{InputError, InputThread, LiveAction, LiveControl, RailNote};
