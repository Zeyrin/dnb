//! Imported tunes, kept: each in a folder of its own named after its audio's
//! fingerprint, the audio copied in beside what was heard in it (`song.ron`),
//! and loaded back as songs the game plays like its own.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use wu_audio::{Hit, MixSettings, Note};
use wu_content::project::{Groove, Meta, PHRASE_BARS, Recording, Song};
use wu_instruments::Pad;
use wu_time::{TempoMap, Tick};

use crate::decode::{DecodeError, decode};
use crate::feel::Feel;
use crate::hits::{Drum, to_hits};
use crate::listen::{ListenError, Listened, Stage, listen};

/// What the listener of this version heard is kept; a tune heard by an older
/// one plays as it was heard while it is listened to again.
/// 2: the feel, where the drums really sound.
/// 3: the tempo refined over the whole tune; the two-step's kicks and snares
/// heard through a ringing bass.
pub const LISTENER_VERSION: u32 = 3;
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
    /// Where the drums really sound against the grid (older imports: on it).
    #[serde(default)]
    pub feel: Feel,
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
    #[error("it was heard by a newer listener than this game's")]
    Newer,
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
    let imported = what_was_heard(&heard, title, tune.artist.clone().unwrap_or_default(), audio.clone());
    let folder = library.join(&id);
    fs::create_dir_all(&folder)?;
    fs::write(folder.join(&audio), &bytes)?;
    keep(&folder, &imported)?;
    Ok(ImportedSong { id, folder, imported })
}

/// Listens again, with this version's listener, to a tune kept in its folder;
/// its title and artist stay as they were.
pub fn relisten(song: &ImportedSong, on_stage: impl FnMut(Stage)) -> Result<ImportedSong, ImportError> {
    let kept = &song.imported;
    let tune = decode(&song.folder.join(&kept.audio))?;
    let heard = listen(&tune, on_stage)?;
    let imported = what_was_heard(&heard, kept.title.clone(), kept.artist.clone(), kept.audio.clone());
    keep(&song.folder, &imported)?;
    Ok(ImportedSong {
        id: song.id.clone(),
        folder: song.folder.clone(),
        imported,
    })
}

/// What the listener heard, as kept.
fn what_was_heard(heard: &Listened, title: String, artist: String, audio: String) -> Imported {
    Imported {
        version: LISTENER_VERSION,
        title,
        artist,
        audio,
        bpm: heard.grid.bpm,
        first_bar_s: heard.grid.first_bar_s,
        bars: heard.bars,
        gain_db: (wu_content::mastering::TARGET_LUFS - heard.loudness_lufs).clamp(-24.0, 12.0) as f32,
        drums: to_hits(&heard.hits)
            .iter()
            .zip(&heard.hits)
            .map(|(hit, heard)| (heard.step, heard.drum, hit.velocity))
            .collect(),
        feel: heard.feel,
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
    }
}

/// Writes what was heard into the tune's folder. The song file goes last: a
/// folder without one is an import that never finished.
fn keep(folder: &Path, imported: &Imported) -> Result<(), ImportError> {
    let text = ron::ser::to_string_pretty(imported, ron::ser::PrettyConfig::default())
        .map_err(|e| ImportError::Damaged(e.to_string()))?;
    fs::write(folder.join(SONG_FILE), text)?;
    Ok(())
}

/// The imported tune kept in `folder`.
pub fn load(folder: &Path) -> Result<ImportedSong, ImportError> {
    let text = fs::read_to_string(folder.join(SONG_FILE))?;
    let imported: Imported = ron::from_str(&text).map_err(|e| ImportError::Damaged(e.to_string()))?;
    if imported.version > LISTENER_VERSION {
        return Err(ImportError::Newer);
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
    /// Whether an older listener heard it: it plays as heard, and wants
    /// [`relisten`]ing.
    pub fn heard_by_an_older_listener(&self) -> bool {
        self.imported.version < LISTENER_VERSION
    }

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
            lessons: Vec::new(),
            length: Tick::from_bars(imported.bars),
            recording: Some(Recording {
                path: self.folder.join(&imported.audio),
                first_bar_s: imported.first_bar_s,
                gain_db: imported.gain_db,
                groove: groove(&imported.feel, imported.bpm),
            }),
        }
    }
}

/// The feel on the pads the hits were put on: the kick's on the kick, the
/// snare's on every snare and ghost, the hats' on the hats.
fn groove(feel: &Feel, bpm: f64) -> Groove {
    let step_ms = 60_000.0 / bpm / 4.0;
    let ticks = |ms: f32| (f64::from(ms) / step_ms * wu_time::TICKS_PER_STEP as f64).round() as i64;
    let mut groove = Groove::default();
    let pads = [
        (Pad::P1, Drum::Kick),
        (Pad::P2, Drum::Snare),
        (Pad::P3, Drum::Ghost),
        (Pad::P5, Drum::Snare),
        (Pad::P7, Drum::Hat),
        (Pad::P8, Drum::Hat),
    ];
    for (pad, drum) in pads {
        for (step, offset) in groove.ticks[pad.index()].iter_mut().enumerate() {
            *offset = ticks(feel.offset_ms(drum, step as i64));
        }
    }
    groove
}

/// FNV-1a over the audio: the same file always lands in the same folder.
fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{hash:016x}")
}
