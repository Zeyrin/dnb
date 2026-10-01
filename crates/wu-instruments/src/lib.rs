//! Instruments for WHEEL UP!.
//!
//! Drums are synthesised once into samples when a kit is baked (see
//! `docs/DECISIONS.md`, ADR-003), and so is the sub bass, which is pitched by
//! playback rate. Every sound is generated from code: no third-party audio.

#![forbid(unsafe_code)]

pub mod bus;
pub mod drums;
pub mod fx;
pub mod kit;
pub mod tone;

pub use bus::Bus;
pub use fx::RewindSounds;
pub use kit::{Kit, PAD_COUNT, Pad, PadSound};
pub use tone::Tone;
