//! WHEEL UP! content: the notations songs are written in, song projects and the
//! songs that ship with the game, the kits they play (baked once and cached),
//! the demo groove, the Pirate Radio Tour, and the player's settings file.

#![forbid(unsafe_code)]

pub mod demo;
pub mod kits;
pub mod mastering;
pub mod notes;
pub mod project;
pub mod settings;
pub mod songs;
pub mod theory;
pub mod tour;

/// Step notation lives with the drums it writes for.
pub use wu_instruments::steps;

pub use steps::{Step, StepError, parse_steps};
