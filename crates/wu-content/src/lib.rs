//! WHEEL UP! content: the step notation songs are written in, the demo groove,
//! and the player's settings file.
//! The full project format (RON files with patterns and an arrangement) builds on
//! this in M3.

#![forbid(unsafe_code)]

pub mod demo;
pub mod settings;
pub mod steps;

pub use steps::{Step, StepError, parse_steps};
