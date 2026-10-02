//! Imported tunes, kept: each in a folder of its own named after its audio's
//! fingerprint, the audio copied in beside what was heard in it (`song.ron`),
//! and loaded back as songs the game plays like its own.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use wu_audio::{Hit, MixSettings, Note};
use wu_content::project::{Meta, PHRASE_BARS, Recording, Song};
use wu_time::{TempoMap, Tick};

use crate::decode::{DecodeError, decode};
use crate::hits::{Drum, to_hits};
use crate::listen::{ListenError, Stage, listen};

/// What the listener of this version heard is kept; a tune heard by an older
/// one is listened to again.
pub const LISTENER_VERSION: u32 = 1;
/// The kit an imported song's count-in clicks on.
const COUNT_IN_KIT: &str = "ragga-93";
/// What the folder keeps of what was heard.
const SONG_FILE: &str = "song.ron";

/// What was heard in an imported tune, as kept in its folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Imported {
    pub version: u32,
    pub title: String,
    pub artist: String,
    /// The audio, in the song's folder.
    pub audio: String,
    pub bpm: f64,
    /// Where the first bar line falls in the audio, in seconds.
    pub first_bar_s: f64,
    pub bars: i64,
    /// How much to turn it up or down to sit at the game's loudness, in dB.
    pub gain_db: f32,
    /// Every drum hit: its step, the drum, how hard.
    pub drums: Vec<(i64, Drum, f32)>,
    /// The bass line: first step, length in steps, key.
    pub bass: Vec<(i64, i64, u8)>,
    /// The sections: name, first bar, end bar, whether it is a drop.
    pub sections: Vec<(String, i64, i64, bool)>,
}

/// An imported tune, in its folder.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportedSong {
    /// Its audio's fingerprint, which names its folder.
    pub id: String,
    pub folder: PathBuf,
    pub imported: Imported,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error(transparent)]
    Listen(#[from] ListenError),
    #[error("can't keep it: {0}")]
    Keep(#[from] io::Error),
    #[error("its song file is damaged: {0}")]
    Damaged(String),
    #[error("it was heard by an older listener")]
    Outdated,
}

/// Where imported tunes are kept: `<data dir>/wheelup/imports`.
pub fn default_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "wheelup").map(|dirs| dirs.data_dir().join("imports"))
}

/// Decodes and listens to `file`, then keeps it in a folder of its own in
/// `library`. Importing the same audio again listens again and replaces it.
pub fn import(file: &Path, library: &Path, on_stage: impl FnMut(Stage)) -> Result<ImportedSong, ImportError> {
    let bytes = fs::read(file)?;
    let id = fingerprint(&bytes);
    let tune = decode(file)?;
    let heard = listen(&tune, on_stage)?;
    let extension = file
        .extension()
        .and_then(|e| e.to_str())
        .map_or_else(String::new, |e| format!(".{}", e.to_lowercase()));
    let audio = format!("tune{extension}");
    let title = tune
        .title
        .clone()
        .or_else(|| file.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "Untitled".to_owned());
    let imported = Imported {
        version: LISTENER_VERSION,
        title,
        artist: tune.artist.clone().unwrap_or_default(),
        audio: audio.clone(),
        bpm: heard.grid.bpm,
        first_bar_s: heard.grid.first_bar_s,
        bars: heard.bars,
        gain_db: (wu_content::mastering::TARGET_LUFS - heard.loudness_lufs).clamp(-24.0, 12.0) as f32,
        drums: to_hits(&heard.hits)
            .iter()
            .zip(&heard.hits)
            .map(|(hit, heard)| (heard.step, heard.drum, hit.velocity))
            .collect(),
        bass: heard
            .bass
            .iter()
            .map(|n| {
                (
                    n.tick.0 / wu_time::TICKS_PER_STEP,
                    n.length.0 / wu_time::TICKS_PER_STEP,
                    n.key,
                )
            })
            .collect(),
        sections: heard
            .sections
            .iter()
            .map(|s| (s.name.clone(), s.bars.start, s.bars.end, s.drop))
            .collect(),
    };
    let folder = library.join(&id);
    fs::create_dir_all(&folder)?;
    fs::write(folder.join(&audio), &bytes)?;
    // The song file last: a folder without one is an import that never finished.
    let text = ron::ser::to_string_pretty(&imported, ron::ser::PrettyConfig::default())
        .map_err(|e| ImportError::Damaged(e.to_string()))?;
    fs::write(folder.join(SONG_FILE), text)?;
    Ok(ImportedSong { id, folder, imported })
}

/// The imported tune kept in `folder`.
pub fn load(folder: &Path) -> Result<ImportedSong, ImportError> {
    let text = fs::read_to_string(folder.join(SONG_FILE))?;
    let imported: Imported = ron::from_str(&text).map_err(|e| ImportError::Damaged(e.to_string()))?;
    if imported.version != LISTENER_VERSION {
        return Err(ImportError::Outdated);
    }
    let id = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(ImportedSong {
        id,
        folder: folder.to_path_buf(),
        imported,
    })
}

/// Every imported tune kept in `library` that finished importing, oldest
/// first; what could not be loaded, with why.
pub fn load_all(library: &Path) -> Vec<Result<ImportedSong, (PathBuf, ImportError)>> {
    let Ok(entries) = fs::read_dir(library) else {
        return Vec::new();
    };
    let mut folders: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join(SONG_FILE).is_file())
        .map(|path| {
            let made = fs::metadata(path.join(SONG_FILE))
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (made, path)
        })
        .collect();
    folders.sort();
    folders
        .into_iter()
        .map(|(_, folder)| load(&folder).map_err(|e| (folder, e)))
        .collect()
}

impl ImportedSong {
    /// The tune as a song: its own recording for the music, what was heard in
    /// it for the charts, its drops for the hype phrases.
    pub fn song(&self) -> Song {
        let imported = &self.imported;
        let step = Tick::from_steps;
        let heard: Vec<crate::hits::Heard> = imported
            .drums
            .iter()
            .map(|&(s, drum, _)| crate::hits::Heard {
                step: s,
                drum,
                confidence: 1.0,
            })
            .collect();
        let drums: Vec<Hit> = to_hits(&heard)
            .into_iter()
            .zip(&imported.drums)
            .map(|(hit, &(_, _, velocity))| Hit { velocity, ..hit })
            .collect();
        let bass = imported
            .bass
            .iter()
            .map(|&(start, length, key)| Note {
                tick: step(start),
                length: step(length),
                key,
                velocity: 0.9,
            })
            .collect();
        let sections = imported
            .sections
            .iter()
            .map(|(name, start, end, _)| (name.clone(), Tick::from_bars(*start), Tick::from_bars(*end)))
            .collect();
        let mut hype = Vec::new();
        for &(_, start, end, drop) in &imported.sections {
            let mut bar = start;
            while drop && bar < end {
                let phrase_end = (bar + PHRASE_BARS).min(end);
                hype.push((Tick::from_bars(bar), Tick::from_bars(phrase_end)));
                bar = phrase_end;
            }
        }
        Song {
            meta: Meta {
                title: imported.title.clone(),
                artist: imported.artist.clone(),
                key: String::new(),
                subgenre: "Your tune".to_owned(),
            },
            kit: COUNT_IN_KIT.to_owned(),
            mix: MixSettings::default(),
            tempo: TempoMap::constant(imported.bpm),
            drums,
            bass,
            bass_sounds: Vec::new(),
            tracks: Vec::new(),
            sections,
            hype,
            length: Tick::from_bars(imported.bars),
            recording: Some(Recording {
                path: self.folder.join(&imported.audio),
                first_bar_s: imported.first_bar_s,
                gain_db: imported.gain_db,
            }),
        }
    }
}

/// FNV-1a over the audio: the same file always lands in the same folder.
fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{hash:016x}")
}
