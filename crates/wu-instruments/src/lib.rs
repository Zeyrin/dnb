//! Instruments for WHEEL UP!.
//!
//! Drums are synthesised once into samples when a kit is baked (see
//! `docs/DECISIONS.md`, ADR-003), and so are the sub bass and the FX one-shots,
//! which are pitched by playback rate. The melodic instruments are synth
//! patches played in real time, a voice per note. Every sound is generated
//! from code: no third-party audio.

#![forbid(unsafe_code)]

pub mod breaks;
pub mod bus;
pub mod drums;
pub mod fx;
pub mod instrument;
pub mod kit;
pub mod kits;
pub mod steps;
pub mod synth;
pub mod tone;

pub use bus::{Bus, Sends};
pub use fx::RewindSounds;
pub use instrument::{INSTRUMENTS, Instrument};
pub use kit::{Break, Kit, PAD_COUNT, Pad, PadSound};
pub use kits::{KITS, KitDef, kit_def};
pub use synth::{Patch, SynthVoice, VoiceOut};
pub use tone::Tone;
