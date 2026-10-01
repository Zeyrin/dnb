//! Instruments for WHEEL UP!.
//!
//! Drums are synthesised once into samples when a kit is baked (see
//! `docs/DECISIONS.md`, ADR-003); every sound is generated from code, so the
//! kits carry no third-party audio at all.

#![forbid(unsafe_code)]

pub mod drums;
pub mod kit;

pub use kit::{Kit, PAD_COUNT, Pad, PadSound};
