//! DSP building blocks for WHEEL UP!.
//!
//! Everything that runs per sample here is allocation-free and safe to call on
//! the audio thread. Constructors may allocate; `process` methods never do,
//! except the loudness meter's, which is an offline tool.

#![forbid(unsafe_code)]

pub mod era;
pub mod filter;
pub mod limiter;
pub mod loudness;
pub mod osc;
pub mod rng;
pub mod sample;
pub mod shape;
pub mod smooth;
pub mod truepeak;

pub use era::SamplerEra;
pub use filter::{OnePole, Svf, SvfOut};
pub use limiter::Limiter;
pub use loudness::{Biquad, KWeighting, LoudnessMeter};
pub use osc::{Phase, polyblep_saw, polyblep_square};
pub use rng::Rng;
pub use sample::Sample;
pub use shape::{db_to_gain, gain_to_db, midi_to_hz, soft_clip};
pub use smooth::Smoothed;
pub use truepeak::TruePeak;
