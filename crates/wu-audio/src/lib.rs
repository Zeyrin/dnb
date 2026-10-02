//! The WHEEL UP! audio engine.
//!
//! [`Engine`] lives on the audio thread and owns everything that makes sound:
//! the transport, the sequencer, the sample and synth voices, a recording to
//! play under them, and the mix. The main thread talks
//! to it through an [`EngineHandle`] (commands in, reports and garbage out) and
//! the input thread through a [`LiveSender`] (pad hits straight to the sound,
//! without waiting for a frame). Every queue is a lock-free single-producer,
//! single-consumer ring buffer; the engine never allocates, locks or blocks
//! while processing.
//!
//! Song time is the transport's sample counter. Each callback publishes a
//! [`ClockSnapshot`] through a seqlock; a [`ClockEstimator`] on the main thread
//! turns a run of snapshots into a jitter-free map from any instant on the
//! shared monotonic clock to song time.

#![forbid(unsafe_code)]

mod backing;
mod clock;
mod engine;
mod mixer;
pub mod output;
mod program;
mod render;
mod synths;
mod voice;

pub use backing::Backing;
pub use clock::{ClockEstimator, ClockSnapshot, SharedClock};
pub use engine::{
    BufferTiming, Command, Engine, EngineHandle, EngineParts, Garbage, LiveHit, LiveMode, LiveNote, LiveSender, Report,
    VoiceSource, VoiceStart, engine,
};
pub use mixer::{BUS_COUNT, CEILING_DB, MixSettings};
pub use program::{EventKind, Hit, LoopRange, Note, Part, Program, SeqEvent};
pub use render::{OfflineRender, render_offline, write_wav};

/// The most frames the engine renders in one internal block. Longer device
/// buffers are processed in several blocks.
pub const MAX_BLOCK: usize = 512;

/// Sample voices that can sound at once before the oldest is stolen.
pub const VOICES: usize = 128;

/// Synth voices, likewise: a chord takes one a note.
pub const SYNTH_VOICES: usize = 48;
