//! Opening a tune: MP3, WAV, FLAC, OGG Vorbis or AAC (M4A), decoded to
//! floating-point stereo at the file's own sample rate.

use std::fs::File;
use std::path::Path;

use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as Symphonia;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{MetadataOptions, StandardTag};

/// A tune, decoded.
#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    pub sample_rate: u32,
    /// Interleaved left and right; a mono file is on both sides.
    pub stereo: Vec<f32>,
    /// From the file's tags, if it has them.
    pub title: Option<String>,
    pub artist: Option<String>,
}

impl Decoded {
    pub fn frames(&self) -> usize {
        self.stereo.len() / 2
    }

    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.sample_rate)
    }

    /// Left and right averaged.
    pub fn mono(&self) -> Vec<f32> {
        let (frames, _) = self.stereo.as_chunks::<2>();
        frames.iter().map(|[l, r]| 0.5 * (l + r)).collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("can't open it: {0}")]
    Open(#[from] std::io::Error),
    #[error("not an audio file this game can read (MP3, WAV, FLAC, OGG or M4A): {0}")]
    Format(String),
    #[error("it has no audio in it")]
    NoAudio,
}

/// Decodes the whole of `path`. Damaged packets are skipped, as a player would.
pub fn decode(path: &Path) -> Result<Decoded, DecodeError> {
    let source = MediaSourceStream::new(Box::new(File::open(path)?), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, source, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| DecodeError::Format(e.to_string()))?;
    let track = format
        .default_track(TrackType::Audio)
        .or_else(|| format.first_track_known_codec(TrackType::Audio))
        .ok_or(DecodeError::NoAudio)?;
    let track_id = track.id;
    let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
        return Err(DecodeError::NoAudio);
    };
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| DecodeError::Format(e.to_string()))?;

    let (mut title, mut artist) = (None, None);
    if let Some(revision) = format.metadata().skip_to_latest() {
        let tags = revision
            .media
            .tags
            .iter()
            .chain(revision.per_track.iter().flat_map(|t| &t.metadata.tags));
        for tag in tags {
            match &tag.std {
                Some(StandardTag::TrackTitle(name)) => title = title.or_else(|| Some(name.to_string())),
                Some(StandardTag::Artist(name)) => artist = artist.or_else(|| Some(name.to_string())),
                _ => {}
            }
        }
    }

    let mut sample_rate = params.sample_rate.unwrap_or(0);
    let mut stereo = Vec::new();
    let mut block: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(Symphonia::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(DecodeError::Format(e.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        let audio = match decoder.decode(&packet) {
            Ok(audio) => audio,
            // A damaged packet: skip it, as a player would.
            Err(Symphonia::DecodeError(_) | Symphonia::IoError(_)) => continue,
            Err(e) => return Err(DecodeError::Format(e.to_string())),
        };
        let spec = audio.spec();
        sample_rate = spec.rate();
        let channels = spec.channels().count().max(1);
        audio.copy_to_vec_interleaved(&mut block);
        for frame in block.chunks_exact(channels) {
            let left = frame[0];
            let right = if channels > 1 { frame[1] } else { left };
            stereo.extend([left, right]);
        }
    }
    if stereo.is_empty() || sample_rate == 0 {
        return Err(DecodeError::NoAudio);
    }
    Ok(Decoded {
        sample_rate,
        stereo,
        title,
        artist,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_decodes_to_the_samples_it_holds() {
        let dir = std::env::temp_dir().join(format!("wheelup-decode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("ramp.wav");
        // A quarter second of a 441 Hz sine, mono, 16-bit.
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).expect("create");
        for i in 0..11_025 {
            let x = (std::f32::consts::TAU * 441.0 * i as f32 / 44_100.0).sin();
            writer.write_sample((x * 16_000.0) as i16).expect("write");
        }
        writer.finalize().expect("finalize");

        let tune = decode(&path).expect("decodes");
        assert_eq!((tune.sample_rate, tune.frames()), (44_100, 11_025));
        assert!((tune.seconds() - 0.25).abs() < 1e-9);
        let expected = (std::f32::consts::TAU * 441.0 * 100.0 / 44_100.0).sin() * 16_000.0 / 32_768.0;
        assert!((tune.stereo[200] - expected).abs() < 1e-3, "frame 100, left");
        assert_eq!(tune.stereo[200], tune.stereo[201], "mono on both sides");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_that_is_not_audio_is_refused() {
        let dir = std::env::temp_dir().join(format!("wheelup-decode-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("notes.mp3");
        std::fs::write(&path, b"this is a shopping list").expect("write");
        assert!(matches!(decode(&path), Err(DecodeError::Format(_))));
        assert!(matches!(decode(&dir.join("missing.wav")), Err(DecodeError::Open(_))));
        let _ = std::fs::remove_dir_all(dir);
    }
}
