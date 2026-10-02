//! Listening to a whole tune: its grid, every drum hit, its bass line, its
//! sections, how loud it is.

use wu_audio::Note;
use wu_dsp::LoudnessMeter;

use crate::bass::hear_bass;
use crate::decode::Decoded;
use crate::hits::{Drum, Heard, hear_drums};
use crate::spectrum::analyse;
use crate::structure::{Section, find_sections};
use crate::tempo::{Grid, find_grid};

/// The fewest whole bars a tune needs to be played.
pub const FEWEST_BARS: i64 = 16;

/// What the listener heard in a tune.
#[derive(Clone, Debug, PartialEq)]
pub struct Listened {
    pub grid: Grid,
    /// Whole bars from the first bar line that fit in the tune.
    pub bars: i64,
    pub hits: Vec<Heard>,
    pub bass: Vec<Note>,
    pub sections: Vec<Section>,
    /// Integrated loudness, in LUFS.
    pub loudness_lufs: f64,
}

/// What the listener is busy with, for a progress bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Spectrum,
    Grid,
    Drums,
    Bass,
    Sections,
    Loudness,
}

impl Stage {
    /// How much of the listening is done when this stage starts (0–1).
    pub fn done(self) -> f32 {
        match self {
            Stage::Spectrum => 0.0,
            Stage::Grid => 0.35,
            Stage::Drums => 0.45,
            Stage::Bass => 0.55,
            Stage::Sections => 0.85,
            Stage::Loudness => 0.9,
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Stage::Spectrum => "listening",
            Stage::Grid => "finding the beat",
            Stage::Drums => "hearing the drums",
            Stage::Bass => "hearing the bass line",
            Stage::Sections => "finding the drops",
            Stage::Loudness => "measuring the level",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ListenError {
    #[error("there is no steady beat in it to play along to")]
    NoBeat,
    #[error("it is too short to play: {bars} bars (a song needs at least {FEWEST_BARS})")]
    TooShort { bars: i64 },
}

/// Listens to all of `tune`, telling `on_stage` as each part of the listening starts.
pub fn listen(tune: &Decoded, mut on_stage: impl FnMut(Stage)) -> Result<Listened, ListenError> {
    on_stage(Stage::Spectrum);
    let mono = tune.mono();
    let spec = analyse(&mono, tune.sample_rate);
    on_stage(Stage::Grid);
    let grid = find_grid(&mono, &spec).ok_or(ListenError::NoBeat)?;
    let bars = ((tune.seconds() - grid.first_bar_s) / grid.bar_s()).floor() as i64;
    if bars < FEWEST_BARS {
        return Err(ListenError::TooShort { bars: bars.max(0) });
    }
    let steps = bars * 16;
    on_stage(Stage::Drums);
    let hits = hear_drums(&spec, &grid, steps);
    on_stage(Stage::Bass);
    let kicks: Vec<i64> = hits.iter().filter(|h| h.drum == Drum::Kick).map(|h| h.step).collect();
    let bass = hear_bass(&mono, tune.sample_rate, &grid, steps, &kicks);
    on_stage(Stage::Sections);
    let sections = find_sections(&spec, &grid, bars, &hits, &bass);
    on_stage(Stage::Loudness);
    let mut meter = LoudnessMeter::new(tune.sample_rate);
    meter.process(&tune.stereo);
    Ok(Listened {
        grid,
        bars,
        hits,
        bass,
        sections,
        loudness_lufs: meter.integrated().unwrap_or(-70.0),
    })
}
