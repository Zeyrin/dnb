//! WHEEL UP! rules, independent of rendering and of the sound card: calibration,
//! the judge, scoring, runs, replays, records, and the tour's progress.

#![forbid(unsafe_code)]

pub mod calibration;
pub mod judge;
pub mod play;
pub mod records;
pub mod replay;
pub mod run;
pub mod score;
pub mod tour;
