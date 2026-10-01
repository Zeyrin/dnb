//! Rendering without a sound card: deterministic, faster than real time.

use std::path::Path;

use wu_dsp::Rng;

use crate::engine::{BufferTiming, Command, Report, VoiceStart, engine};
use crate::program::Program;

/// What an offline render produced.
#[derive(Clone, Debug)]
pub struct OfflineRender {
    pub sample_rate: u32,
    /// Interleaved stereo.
    pub audio: Vec<f32>,
    /// Every voice that started within the audio, in order.
    pub starts: Vec<VoiceStart>,
}

/// Plays `program` from tick 0 for `frames` frames, `block` frames per callback.
/// Like a DAW's bounce, the master's look-ahead is compensated: a hit sequenced
/// on frame `f` sounds on frame `f` of the audio.
pub fn render_offline(program: Program, frames: usize, block: usize) -> OfflineRender {
    let sample_rate = program.sample_rate;
    let mut parts = engine(sample_rate);
    for command in [Command::Load(Box::new(program)), Command::Play] {
        // A fresh queue has room for two commands.
        let _ = parts.handle.send(command);
    }
    let latency = parts.engine.latency_frames();
    let mut audio = vec![0.0f32; (frames + latency) * 2];
    let mut starts = Vec::new();
    let ns_per_frame = 1e9 / f64::from(sample_rate);
    let mut done = 0usize;
    for chunk in audio.chunks_mut(block.max(1) * 2) {
        let timing = BufferTiming {
            playback_ns: (done as f64 * ns_per_frame) as u64,
            output_latency_ns: 0,
        };
        parts.engine.process(chunk, timing);
        done += chunk.len() / 2;
        parts.handle.poll(|report| {
            if let Report::VoiceStarted(start) = report {
                starts.push(start);
            }
        });
    }
    audio.drain(..latency * 2);
    // The look-ahead's extra frames may start voices nobody will hear.
    starts.retain(|start| start.device_frame < frames as u64);
    OfflineRender {
        sample_rate,
        audio,
        starts,
    }
}

/// Writes interleaved stereo as 16-bit PCM with triangular dither.
pub fn write_wav(path: &Path, audio: &[f32], sample_rate: u32) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    let mut rng = Rng::new(0x57A7);
    for &x in audio {
        let dither = (rng.next_f32() - rng.next_f32()) / 32_768.0;
        let scaled = ((x + dither).clamp(-1.0, 1.0) * 32_767.0).round() as i16;
        writer.write_sample(scaled)?;
    }
    writer.finalize()
}
