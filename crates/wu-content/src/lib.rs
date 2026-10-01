//! WHEEL UP! content: the notations songs are written in, song projects and the
//! songs that ship with the game, the demo groove, and the player's settings file.
//! The full project format (RON files with patterns and an arrangement) builds on
//! this in M3.

#![forbid(unsafe_code)]

pub mod demo;
pub mod mastering;
pub mod notes;
pub mod project;
pub mod settings;
pub mod songs;
pub mod steps;
pub mod theory;

pub use steps::{Step, StepError, parse_steps};
