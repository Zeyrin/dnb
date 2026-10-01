//! Loudness targets, and measuring a song against them. Every song ships at
//! the same loudness, so nothing jumps out of the song list and the limiter
//! only shaves peaks.

use wu_audio::render_offline;
use wu_dsp::LoudnessMeter;
use wu_time::Tick;

use crate::project::Song;

/// Integrated loudness every song is mastered to, in LUFS…
pub const TARGET_LUFS: f64 = -16.0;
/// …give or take this much.
pub const TOLERANCE_LU: f64 = 1.0;
/// The highest true peak allowed, in dBTP.
pub const MAX_TRUE_PEAK_DB: f32 = -1.0;

/// What a loudness meter made of a render.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loudness {
    pub integrated: f64,
    pub max_short_term: f64,
    pub max_momentary: f64,
    pub true_peak_db: f32,
    pub sample_peak_db: f32,
    pub seconds: f64,
}

impl Loudness {
    /// Measures interleaved stereo; `None` if it is silent.
    pub fn of(audio: &[f32], sample_rate: u32) -> Option<Loudness> {
        let mut meter = LoudnessMeter::new(sample_rate);
        meter.process(audio);
        Some(Loudness {
            integrated: meter.integrated()?,
            max_short_term: meter.max_short_term().unwrap_or(f64::NEG_INFINITY),
            max_momentary: meter.max_momentary().unwrap_or(f64::NEG_INFINITY),
            true_peak_db: meter.true_peak_db(),
            sample_peak_db: meter.sample_peak_db(),
            seconds: meter.seconds(),
        })
    }

    /// How far off the target loudness this is, in LU (positive: too loud).
    pub fn excess(&self) -> f64 {
        self.integrated - TARGET_LUFS
    }

    pub fn on_target(&self) -> bool {
        self.excess().abs() <= TOLERANCE_LU && self.true_peak_db <= MAX_TRUE_PEAK_DB
    }
}

/// Renders the whole song as it ships (every part playing, one bar of tail)
/// and measures it.
pub fn measure(song: &Song, sample_rate: u32) -> Option<Loudness> {
    let program = song.program(sample_rate, &song.tempo, 0, |_, _| false);
    let frames = program.tempo.frame_at(song.length + Tick::from_bars(1), sample_rate);
    let render = render_offline(program, usize::try_from(frames).ok()?, 512);
    Loudness::of(&render.audio, sample_rate)
}
