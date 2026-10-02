//! Song projects: patterns and an arrangement, written in RON, compiled into
//! the hits and notes the engine plays and the charts are cut from.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use wu_audio::{BUS_COUNT, Hit, MixSettings, Note, Program};
use wu_instruments::{Bus, INSTRUMENTS, Instrument, Kit, Pad, RewindSounds, Sends, Tone};
use wu_time::{STEPS_PER_BAR, TempoMap, TempoPoint, Tick};

use crate::notes::{NoteError, parse_notes};
use crate::settings::{AudioMode, Language};
use crate::steps::{Step, StepError, parse_steps};

pub const PROJECT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub meta: Meta,
    pub bpm: f64,
    /// Odd 16ths pushed late by this share of a step (0–0.5).
    #[serde(default)]
    pub swing: f64,
    pub kit: String,
    #[serde(default)]
    pub mix: Mix,
    /// The sounds the bass line plays on, layered; the rails play them all.
    /// The sub alone unless the song says otherwise.
    #[serde(default = "Track::sub_only")]
    pub bass: Vec<Track>,
    /// The other parts, by name: an instrument each, played by `Notes` patterns.
    #[serde(default)]
    pub tracks: BTreeMap<String, Track>,
    pub patterns: BTreeMap<String, Pattern>,
    pub arrangement: Vec<Section>,
}

/// An instrument (see `wu_instruments::INSTRUMENTS`) as a part plays it: its
/// level in dB on top of the instrument's own, and, if set, its pan (-1 to 1)
/// and how much it sends to the reverb and the dub delay (0 to 1 each).
///
/// `break/<id>` plays a kit's break (`break/rough-rider`), sped up or slowed
/// down to the song's tempo like a sampler would: a note as long as the break
/// plays it once round.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub instrument: String,
    #[serde(default)]
    pub level: f32,
    #[serde(default)]
    pub pan: Option<f32>,
    #[serde(default)]
    pub reverb: Option<f32>,
    #[serde(default)]
    pub delay: Option<f32>,
}

impl Track {
    fn sub_only() -> Vec<Track> {
        vec![Track {
            instrument: "sub".to_owned(),
            level: 0.0,
            pan: None,
            reverb: None,
            delay: None,
        }]
    }

    /// Whether the instrument it names exists.
    fn is_known(&self) -> bool {
        match self.instrument.strip_prefix("break/") {
            Some(id) => crate::kits::has_break(id),
            None => INSTRUMENTS.contains(&self.instrument.as_str()),
        }
    }

    /// The instrument, mixed as the track says, with tempo-synced
    /// modulation set for `bpm`. `None` for an unknown instrument.
    pub fn instrument(&self, sample_rate: u32, bpm: f64) -> Option<Instrument> {
        let named = match self.instrument.strip_prefix("break/") {
            Some(id) => {
                let brk = crate::kits::break_loop(id, sample_rate)?;
                Instrument::Sampled(Tone {
                    name: brk.name,
                    sample: brk.sample,
                    root_key: BREAK_KEY,
                    sustain: None,
                    gain: 1.0,
                    pan: 0.0,
                    bus: Bus::Drums,
                    sends: Sends::DRY,
                    tune: bpm / brk.bpm,
                })
            }
            None => Instrument::named(&self.instrument, sample_rate)?,
        };
        let mut instrument = named.with_level_db(self.level).at_tempo(bpm);
        if let Some(pan) = self.pan {
            instrument = instrument.with_pan(pan);
        }
        if self.reverb.is_some() || self.delay.is_some() {
            let own = instrument.sends();
            instrument = instrument.with_sends(Sends {
                reverb: self.reverb.unwrap_or(own.reverb),
                delay: self.delay.unwrap_or(own.delay),
            });
        }
        Some(instrument)
    }
}

/// How the song is mixed and mastered, in dB: each bus's level, how far the
/// bass ducks under the kick and how fast it comes back, the gain into the
/// master limiter (set so the song lands at the target loudness: see
/// `mastering`), and the two returns: the reverb (its tail's length in
/// seconds) and the dub delay (its echoes so many beats apart, each this
/// share of the last).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mix {
    pub drums: f32,
    pub bass: f32,
    pub music: f32,
    pub fx: f32,
    pub duck: f32,
    pub duck_release_ms: f32,
    pub master: f32,
    pub reverb: f32,
    pub reverb_decay: f32,
    pub delay: f32,
    pub delay_beats: f32,
    pub delay_feedback: f32,
}

impl Default for Mix {
    fn default() -> Mix {
        Mix {
            drums: 0.0,
            bass: 0.0,
            music: 0.0,
            fx: 0.0,
            // The sub always gets out of the kick's way.
            duck: -6.0,
            duck_release_ms: 120.0,
            master: 0.0,
            reverb: 0.0,
            reverb_decay: 2.4,
            delay: 0.0,
            delay_beats: 0.75,
            delay_feedback: 0.55,
        }
    }
}

impl Mix {
    pub fn settings(&self) -> MixSettings {
        let bus_db: [f32; BUS_COUNT] = [self.drums, self.bass, self.music, self.fx];
        MixSettings {
            bus_db,
            duck_db: self.duck,
            duck_release_ms: self.duck_release_ms,
            master_db: self.master,
            reverb_db: self.reverb,
            reverb_decay_s: self.reverb_decay,
            delay_db: self.delay,
            delay_beats: self.delay_beats,
            delay_feedback: self.delay_feedback,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub title: String,
    /// Always a fictional in-house producer: no real artists in content.
    pub artist: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub subgenre: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Pattern {
    /// One step string per pad, keyed "P1"–"P8" (see `steps`).
    Drums { bars: i64, steps: BTreeMap<String, String> },
    /// A bass line in note notation (see `notes`), one note at a time.
    Bass { bars: i64, notes: String },
    /// A track's part in note notation, chords and all.
    Notes { track: String, bars: i64, notes: String },
}

/// A stretch of the song; each pattern it plays repeats to fill it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub bars: i64,
    pub play: Vec<String>,
    /// Each 8 bars of it is a hype phrase: cleared without a miss, it fills the
    /// hype meter that WHEEL UP! spends.
    #[serde(default)]
    pub hype: bool,
    /// A drum pattern played instead of the section's drums at the end of
    /// every eight bars on the song's phrase grid: the fill that turns the
    /// phrase round. As long as the pattern is.
    #[serde(default)]
    pub fill: Option<String>,
    /// Beats of silence at the section's end for a drop to land in: the drums
    /// and the breaks stop and the bass is cut, while a riser or a shout can
    /// carry on.
    #[serde(default)]
    pub gap: i64,
    /// What the section teaches, in a song that is a lesson.
    #[serde(default)]
    pub lesson: Option<Lesson>,
}

/// A section as a lesson: what to do, and which parts are the player's. A song
/// with lessons is charted from them alone, the same at every difficulty; the
/// rest of it plays itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lesson {
    /// What to do, shown while the section plays. `{P1}`–`{P8}` stand for the
    /// buttons that play those pads on the player's layout.
    pub caption: String,
    /// The section's name and the caption in French.
    #[serde(default)]
    pub title_fr: Option<String>,
    #[serde(default)]
    pub caption_fr: Option<String>,
    /// The pads that are the player's here, "P1"–"P8".
    #[serde(default)]
    pub pads: Vec<String>,
    /// Whether the bass line is the player's here, on the rails.
    #[serde(default)]
    pub rails: bool,
}

/// A lesson where it plays in the compiled song.
#[derive(Clone, Debug, PartialEq)]
pub struct LessonSpan {
    /// The section's name.
    pub name: String,
    pub start: Tick,
    pub end: Tick,
    pub caption: String,
    /// The name and the caption in French, if the lesson has them.
    pub name_fr: Option<String>,
    pub caption_fr: Option<String>,
    pub pads: Vec<Pad>,
    pub rails: bool,
}

impl LessonSpan {
    /// The section's name, in `language` (in English when it has no French).
    pub fn name_in(&self, language: Language) -> &str {
        match (language, &self.name_fr) {
            (Language::French, Some(french)) => french,
            _ => &self.name,
        }
    }

    /// What to do, in `language`.
    pub fn caption_in(&self, language: Language) -> &str {
        match (language, &self.caption_fr) {
            (Language::French, Some(french)) => french,
            _ => &self.caption,
        }
    }
}

/// The note that plays a break at its own pitch (any other repitches it).
const BREAK_KEY: u8 = 60;

/// Hype phrases are this many bars long (shorter only at a section's end).
pub const PHRASE_BARS: i64 = 8;

/// A note of the bass line: the engine's own note type.
pub type BassNote = Note;

/// A recording a song plays instead of sounding its own parts: an imported
/// tune. Its hits and bass line are what the charts are made from.
#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    /// The audio file.
    pub path: PathBuf,
    /// Where its first bar line (tick 0) falls, in seconds into it.
    pub first_bar_s: f64,
    /// How much to turn it up or down, in dB, to sit at the game's loudness.
    pub gain_db: f32,
}

/// A compiled song: everything in ticks, sorted.
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub meta: Meta,
    pub kit: String,
    pub mix: MixSettings,
    pub tempo: TempoMap,
    pub drums: Vec<Hit>,
    pub bass: Vec<BassNote>,
    /// What the bass line plays on.
    pub bass_sounds: Vec<Track>,
    /// The other parts, by name, and their notes.
    pub tracks: Vec<(String, Track, Vec<Note>)>,
    /// Name, first tick, end tick.
    pub sections: Vec<(String, Tick, Tick)>,
    /// Hype phrases: first tick, end tick.
    pub hype: Vec<(Tick, Tick)>,
    /// What each section teaches, in a song that is a lesson; empty otherwise.
    pub lessons: Vec<LessonSpan>,
    pub length: Tick,
    /// The recording it plays, if it is an imported tune.
    pub recording: Option<Recording>,
}

impl Song {
    /// A lesson: charted from its lessons, the same at every difficulty.
    pub fn is_lesson(&self) -> bool {
        !self.lessons.is_empty()
    }

    /// Section `index`'s name in `language`: a lesson's has its own French.
    pub fn section_name(&self, index: usize, language: Language) -> Option<&str> {
        let (name, start, _) = self.sections.get(index)?;
        let lesson = self.lessons.iter().find(|lesson| lesson.start == *start);
        Some(lesson.map_or(name.as_str(), |lesson| lesson.name_in(language)))
    }

    /// The whole song as the engine plays it with nobody playing along.
    pub fn whole_program(&self, sample_rate: u32, tempo: &TempoMap, count_in_bars: i64) -> Program {
        self.program(
            sample_rate,
            tempo,
            count_in_bars,
            |_, _| false,
            |_, _| false,
            AudioMode::Live,
        )
    }

    /// What the engine plays while someone plays along, at `tempo` (the practice
    /// tempo; the song's own unless slowed or sped up): a count-in on the rim,
    /// then the song. The drum hits the player plays and the bass notes they
    /// hold (by start and key) are left out in Live audio, where their presses
    /// play them, and kept but marked as theirs in Classic, so a miss can mute them.
    /// A song with a recording sounds nothing but the count-in: the recording,
    /// given to the program as its backing, is the music.
    pub fn program(
        &self,
        sample_rate: u32,
        tempo: &TempoMap,
        count_in_bars: i64,
        player_plays: impl Fn(Tick, Pad) -> bool,
        player_holds: impl Fn(Tick, u8) -> bool,
        mode: AudioMode,
    ) -> Program {
        let count_in = (0..count_in_bars.max(0) * 4).map(|beat| Hit {
            tick: Tick::from_beats(beat - count_in_bars * 4),
            pad: Pad::P4,
            velocity: if beat % 4 == 0 { 1.0 } else { 0.7 },
        });
        if self.recording.is_some() {
            let kit = crate::kits::kit(&self.kit, sample_rate).unwrap_or_else(|| Kit::ragga_93(sample_rate));
            return Program::new(sample_rate, tempo.clone(), kit)
                .with_rewind(RewindSounds::new(sample_rate))
                .with_hits(count_in);
        }
        let (theirs, backing): (Vec<Hit>, Vec<Hit>) =
            self.drums.iter().copied().partition(|h| player_plays(h.tick, h.pad));
        let (held, bass): (Vec<Note>, Vec<Note>) = self.bass.iter().copied().partition(|n| player_holds(n.tick, n.key));
        let bpm = tempo.bpm_at(Tick::ZERO);
        // Compiling checked the kit's name.
        let kit = crate::kits::kit(&self.kit, sample_rate).unwrap_or_else(|| Kit::ragga_93(sample_rate));
        let mut program = Program::new(sample_rate, tempo.clone(), kit)
            .with_mix(self.mix)
            .with_rewind(RewindSounds::new(sample_rate))
            .with_hits(count_in.chain(backing))
            .with_notes(bass);
        // Compiling checked every instrument's name.
        for sound in self.bass_sounds.iter().filter_map(|t| t.instrument(sample_rate, bpm)) {
            program = program.with_bass_sound(sound);
        }
        for (_, track, notes) in &self.tracks {
            let Some(instrument) = track.instrument(sample_rate, bpm) else {
                continue;
            };
            let Ok(index) = u8::try_from(program.instruments.len()) else {
                break;
            };
            program = program
                .with_instrument(instrument)
                .with_track_notes(index, notes.iter().copied());
        }
        match mode {
            AudioMode::Live => program,
            AudioMode::Classic => program.with_player_hits(theirs).with_player_notes(held),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("not a valid project: {0}")]
    Parse(String),
    #[error("project version {0} is newer than this game understands ({PROJECT_VERSION})")]
    Version(u32),
    #[error("tempo: {0}")]
    Tempo(#[from] wu_time::TempoError),
    #[error("unknown kit \"{0}\"")]
    Kit(String),
    #[error("{part} plays \"{instrument}\", which isn't an instrument")]
    Instrument { part: String, instrument: String },
    #[error("pattern \"{pattern}\" is for track \"{track}\", which doesn't exist")]
    MissingTrack { pattern: String, track: String },
    #[error("pattern \"{pattern}\": the bass line plays one note at a time (no chords)")]
    BassChord { pattern: String },
    #[error("pattern \"{pattern}\": {source}")]
    Steps { pattern: String, source: StepError },
    #[error("pattern \"{pattern}\": {source}")]
    Notes { pattern: String, source: NoteError },
    #[error("pattern \"{pattern}\": no pad called \"{pad}\" (P1–P8)")]
    Pad { pattern: String, pad: String },
    #[error("pattern \"{pattern}\" says {bars} bars but is {steps} steps long")]
    Length { pattern: String, bars: i64, steps: i64 },
    #[error("section \"{section}\" plays \"{pattern}\", which doesn't exist")]
    MissingPattern { section: String, pattern: String },
    #[error("section \"{0}\" needs at least one bar")]
    EmptySection(String),
    #[error("section \"{section}\" fills with \"{pattern}\", which isn't a drum pattern")]
    FillNotDrums { section: String, pattern: String },
    #[error("section \"{section}\" teaches pad \"{pad}\", which doesn't exist (P1–P8)")]
    LessonPad { section: String, pad: String },
}

fn pad_named(name: &str) -> Option<Pad> {
    let index: usize = name.strip_prefix('P')?.parse().ok()?;
    Pad::from_index(index.checked_sub(1)?)
}

impl Project {
    pub fn from_ron(text: &str) -> Result<Project, ProjectError> {
        // Optional fields take a bare value (`pan: 0.3`), no `Some(…)` around it.
        let project: Project = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(text)
            .map_err(|e| ProjectError::Parse(e.to_string()))?;
        if project.version > PROJECT_VERSION {
            return Err(ProjectError::Version(project.version));
        }
        Ok(project)
    }

    pub fn compile(&self) -> Result<Song, ProjectError> {
        let tempo = TempoMap::new(&[TempoPoint {
            tick: Tick::ZERO,
            bpm: self.bpm,
        }])?;
        if wu_instruments::kit_def(&self.kit).is_none() {
            return Err(ProjectError::Kit(self.kit.clone()));
        }
        let parts = self.bass.iter().map(|t| ("the bass".to_owned(), t));
        for (part, track) in parts.chain(self.tracks.iter().map(|(name, t)| (format!("track \"{name}\""), t))) {
            if !track.is_known() {
                return Err(ProjectError::Instrument {
                    part,
                    instrument: track.instrument.clone(),
                });
            }
        }
        let compiled = self.compile_patterns()?;
        let mut drums = Vec::new();
        let mut bass = Vec::new();
        let mut track_notes: Vec<Vec<Note>> = vec![Vec::new(); self.tracks.len()];
        let mut sections = Vec::new();
        let mut hype = Vec::new();
        let mut lessons = Vec::new();
        let mut bar = 0i64;
        for section in &self.arrangement {
            if section.bars < 1 {
                return Err(ProjectError::EmptySection(section.name.clone()));
            }
            let (start, end) = (Tick::from_bars(bar), Tick::from_bars(bar + section.bars));
            if let Some(lesson) = &section.lesson {
                let pads = lesson
                    .pads
                    .iter()
                    .map(|name| {
                        pad_named(name).ok_or_else(|| ProjectError::LessonPad {
                            section: section.name.clone(),
                            pad: name.clone(),
                        })
                    })
                    .collect::<Result<Vec<Pad>, ProjectError>>()?;
                lessons.push(LessonSpan {
                    name: section.name.clone(),
                    start,
                    end,
                    caption: lesson.caption.clone(),
                    name_fr: lesson.title_fr.clone(),
                    caption_fr: lesson.caption_fr.clone(),
                    pads,
                    rails: lesson.rails,
                });
            }
            let pattern_named = |name: &String| {
                compiled.get(name).ok_or_else(|| ProjectError::MissingPattern {
                    section: section.name.clone(),
                    pattern: name.clone(),
                })
            };
            let mut section_drums = Vec::new();
            let mut section_bass = Vec::new();
            let mut section_notes: Vec<Vec<Note>> = vec![Vec::new(); self.tracks.len()];
            for name in &section.play {
                let pattern = pattern_named(name)?;
                let mut offset = start;
                while offset < end {
                    match pattern {
                        Compiled::Drums { hits, .. } => section_drums.extend(placed(hits, offset, end)),
                        Compiled::Bass { notes, .. } => section_bass.extend(within(notes, offset, end)),
                        Compiled::Notes { track, notes, .. } => {
                            section_notes[*track].extend(within(notes, offset, end))
                        }
                    }
                    offset += Tick::from_bars(pattern.bars());
                }
            }
            // The fill, in place of the drums, at the end of each eight bars.
            if let Some(name) = &section.fill {
                let Compiled::Drums { bars: length, hits } = pattern_named(name)? else {
                    return Err(ProjectError::FillNotDrums {
                        section: section.name.clone(),
                        pattern: name.clone(),
                    });
                };
                for last in (bar..bar + section.bars).filter(|b| (b + 1) % PHRASE_BARS == 0) {
                    let from = Tick::from_bars((last + 1 - length).max(bar));
                    let to = Tick::from_bars(last + 1);
                    section_drums.retain(|h| h.tick < from || h.tick >= to);
                    section_drums
                        .extend(placed(hits, Tick::from_bars(last + 1 - length), to).filter(|h| h.tick >= from));
                }
            }
            // The gap: drums, breaks and bass out for the drop to land in.
            if section.gap > 0 {
                let silence = end - Tick::from_beats(section.gap.min(section.bars * 4));
                section_drums.retain(|h| h.tick < silence);
                cut_at(&mut section_bass, silence);
                for ((_, track), notes) in self.tracks.iter().zip(&mut section_notes) {
                    if track.instrument.starts_with("break/") {
                        cut_at(notes, silence);
                    }
                }
            }
            drums.extend(section_drums.into_iter().map(|h| Hit {
                tick: h.tick.swung(self.swing),
                ..h
            }));
            bass.extend(section_bass);
            for (all, these) in track_notes.iter_mut().zip(section_notes) {
                all.extend(these);
            }
            sections.push((section.name.clone(), start, end));
            if section.hype {
                let mut phrase = start;
                while phrase < end {
                    let phrase_end = (phrase + Tick::from_bars(PHRASE_BARS)).min(end);
                    hype.push((phrase, phrase_end));
                    phrase = phrase_end;
                }
            }
            bar += section.bars;
        }
        drums.sort_by_key(|h| (h.tick, h.pad));
        bass.sort_by_key(|n| n.tick);
        for notes in &mut track_notes {
            notes.sort_by_key(|n| (n.tick, n.key));
        }
        let tracks = self
            .tracks
            .iter()
            .zip(track_notes)
            .map(|((name, track), notes)| (name.clone(), track.clone(), notes))
            .collect();
        Ok(Song {
            meta: self.meta.clone(),
            kit: self.kit.clone(),
            mix: self.mix.settings(),
            tempo,
            drums,
            bass,
            bass_sounds: self.bass.clone(),
            tracks,
            sections,
            hype,
            lessons,
            length: Tick::from_bars(bar),
            recording: None,
        })
    }

    fn compile_patterns(&self) -> Result<BTreeMap<String, Compiled>, ProjectError> {
        let mut compiled = BTreeMap::new();
        for (name, pattern) in &self.patterns {
            let length_error = |bars: i64, steps: i64| ProjectError::Length {
                pattern: name.clone(),
                bars,
                steps,
            };
            let entry = match pattern {
                Pattern::Drums { bars, steps } => {
                    let mut hits = Vec::new();
                    for (pad_name, text) in steps {
                        let pad = pad_named(pad_name).ok_or_else(|| ProjectError::Pad {
                            pattern: name.clone(),
                            pad: pad_name.clone(),
                        })?;
                        let parsed = parse_steps(text).map_err(|source| ProjectError::Steps {
                            pattern: name.clone(),
                            source,
                        })?;
                        if parsed.len() as i64 != bars * STEPS_PER_BAR {
                            return Err(length_error(*bars, parsed.len() as i64));
                        }
                        hits.extend(parsed.iter().enumerate().filter_map(|(i, step)| match step {
                            Step::Hit(velocity) => Some(Hit {
                                tick: Tick::from_steps(i as i64),
                                pad,
                                velocity: *velocity,
                            }),
                            Step::Rest => None,
                        }));
                    }
                    Compiled::Drums { bars: *bars, hits }
                }
                Pattern::Bass { bars, notes } => {
                    let notes = self.line(name, *bars, notes)?;
                    if notes.windows(2).any(|w| w[0].tick == w[1].tick) {
                        return Err(ProjectError::BassChord { pattern: name.clone() });
                    }
                    Compiled::Bass { bars: *bars, notes }
                }
                Pattern::Notes { track, bars, notes } => {
                    let Some(index) = self.tracks.keys().position(|t| t == track) else {
                        return Err(ProjectError::MissingTrack {
                            pattern: name.clone(),
                            track: track.clone(),
                        });
                    };
                    Compiled::Notes {
                        bars: *bars,
                        track: index,
                        notes: self.line(name, *bars, notes)?,
                    }
                }
            };
            if entry.bars() < 1 {
                return Err(length_error(entry.bars(), 0));
            }
            compiled.insert(name.clone(), entry);
        }
        Ok(compiled)
    }

    /// A pattern's note line, checked to fill its bars exactly.
    fn line(&self, pattern: &str, bars: i64, text: &str) -> Result<Vec<Note>, ProjectError> {
        let (parsed, steps) = parse_notes(text).map_err(|source| ProjectError::Notes {
            pattern: pattern.to_owned(),
            source,
        })?;
        if steps != bars * STEPS_PER_BAR {
            return Err(ProjectError::Length {
                pattern: pattern.to_owned(),
                bars,
                steps,
            });
        }
        Ok(parsed
            .into_iter()
            .map(|n| Note {
                tick: Tick::from_steps(n.step),
                length: Tick::from_steps(n.length),
                key: n.key,
                velocity: 0.9,
            })
            .collect())
    }
}

enum Compiled {
    Drums { bars: i64, hits: Vec<Hit> },
    Bass { bars: i64, notes: Vec<BassNote> },
    Notes { bars: i64, track: usize, notes: Vec<Note> },
}

impl Compiled {
    fn bars(&self) -> i64 {
        match self {
            Compiled::Drums { bars, .. } | Compiled::Bass { bars, .. } | Compiled::Notes { bars, .. } => *bars,
        }
    }
}

/// A pattern's notes moved to `offset`, those starting before `end` only,
/// and cut off there.
/// A drum pattern's hits from `offset`, those before `end`.
fn placed(hits: &[Hit], offset: Tick, end: Tick) -> impl Iterator<Item = Hit> + '_ {
    hits.iter()
        .map(move |h| Hit {
            tick: h.tick + offset,
            ..*h
        })
        .filter(move |h| h.tick < end)
}

/// Notes stopped at `at`: those that start later dropped, those that ring past it cut short.
fn cut_at(notes: &mut Vec<Note>, at: Tick) {
    notes.retain(|n| n.tick < at);
    for note in notes {
        note.length = note.length.min(at - note.tick);
    }
}

fn within(notes: &[Note], offset: Tick, end: Tick) -> impl Iterator<Item = Note> + '_ {
    notes
        .iter()
        .map(move |n| Note {
            tick: n.tick + offset,
            ..*n
        })
        .filter(move |n| n.tick < end)
        .map(move |n| Note {
            length: n.length.min(end - n.tick),
            ..n
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = r#"
        Project(
            version: 1,
            meta: (title: "Test", artist: "House Band"),
            bpm: 170.0,
            kit: "ragga-93",
            patterns: {
                "beat": Drums(bars: 1, steps: {
                    "P1": "X... .... ..x. ....",
                    "P2": ".... X... .... X...",
                }),
                "sub": Bass(bars: 2, notes: "F1:16 | Ab1:8 C2:8"),
            },
            arrangement: [
                (name: "Intro", bars: 2, play: ["beat"]),
                (name: "Drop", bars: 3, play: ["beat", "sub"]),
            ],
        )
    "#;

    #[test]
    fn patterns_tile_their_sections() {
        let song = Project::from_ron(SMALL).expect("parses").compile().expect("compiles");
        assert_eq!(song.length, Tick::from_bars(5));
        assert_eq!(song.drums.len(), 5 * 4, "four hits a bar for five bars");
        assert_eq!(
            song.sections[1],
            ("Drop".to_owned(), Tick::from_bars(2), Tick::from_bars(5))
        );
        // The two-bar bass line starts again in bar 5 and is cut at the section end.
        let keys: Vec<u8> = song.bass.iter().map(|n| n.key).collect();
        assert_eq!(keys, vec![29, 32, 36, 29]);
        assert_eq!(song.bass[3].length, Tick::from_bars(1));
    }

    #[test]
    fn a_lesson_says_what_to_do_and_hands_its_parts_over() {
        let lesson = SMALL.replace(
            r#"(name: "Drop", bars: 3, play: ["beat", "sub"]),"#,
            r#"(name: "Drop", bars: 3, play: ["beat", "sub"], lesson: (caption: "Kick on {P1}", pads: ["P1"], rails: true)),"#,
        );
        let song = Project::from_ron(&lesson).expect("parses").compile().expect("compiles");
        assert!(song.is_lesson());
        let taught = &song.lessons[..];
        assert_eq!(taught.len(), 1);
        assert_eq!(
            (taught[0].name.as_str(), taught[0].start, taught[0].end),
            ("Drop", Tick::from_bars(2), Tick::from_bars(5))
        );
        assert_eq!(
            (taught[0].pads.as_slice(), taught[0].rails),
            ([Pad::P1].as_slice(), true)
        );
        assert_eq!(taught[0].caption, "Kick on {P1}");
        let plain = Project::from_ron(SMALL).expect("parses").compile().expect("compiles");
        assert!(!plain.is_lesson());
        let broken = lesson.replace(r#"pads: ["P1"]"#, r#"pads: ["P0"]"#);
        assert!(matches!(
            Project::from_ron(&broken).expect("parses").compile(),
            Err(ProjectError::LessonPad { .. })
        ));
    }

    #[test]
    fn mistakes_are_explained() {
        let broken = SMALL.replace("\"P2\"", "\"P9\"");
        assert!(matches!(
            Project::from_ron(&broken).expect("parses").compile(),
            Err(ProjectError::Pad { .. })
        ));
        let broken = SMALL.replace("play: [\"beat\", \"sub\"]", "play: [\"beat\", \"lead\"]");
        assert!(matches!(
            Project::from_ron(&broken).expect("parses").compile(),
            Err(ProjectError::MissingPattern { .. })
        ));
        let broken = SMALL.replace("bars: 2, notes", "bars: 3, notes");
        assert!(matches!(
            Project::from_ron(&broken).expect("parses").compile(),
            Err(ProjectError::Length { .. })
        ));
        assert!(matches!(Project::from_ron("nonsense"), Err(ProjectError::Parse(_))));
        let future = SMALL.replace("version: 1", "version: 99");
        assert!(matches!(Project::from_ron(&future), Err(ProjectError::Version(99))));
    }

    #[test]
    fn the_program_leaves_out_what_the_player_plays() {
        let song = Project::from_ron(SMALL).expect("parses").compile().expect("compiles");
        let all = song.whole_program(48_000, &song.tempo, 1);
        let kicks = |_: Tick, pad: Pad| pad == Pad::P1;
        let live = song.program(48_000, &song.tempo, 1, kicks, |_, _| false, AudioMode::Live);
        let kick_count = song.drums.iter().filter(|h| h.pad == Pad::P1).count();
        assert_eq!(all.events().len() - live.events().len(), kick_count);
        let without_bass = song.program(48_000, &song.tempo, 1, |_, _| false, |_, _| true, AudioMode::Live);
        assert_eq!(all.events().len() - without_bass.events().len(), song.bass.len());
        // Classic keeps everything, the player's kicks marked as theirs.
        let classic = song.program(48_000, &song.tempo, 1, kicks, |_, _| false, AudioMode::Classic);
        assert_eq!(classic.events().len(), all.events().len());
        assert_eq!(classic.events().iter().filter(|e| e.player).count(), kick_count);
        // Four count-in clicks before tick 0, then the song.
        assert_eq!(all.events().iter().filter(|e| e.tick < Tick::ZERO).count(), 4);
        assert_eq!(all.rails.len(), 1, "the sub plays the bass line");
    }

    const WITH_TRACKS: &str = r#"
        Project(
            version: 1,
            meta: (title: "Test", artist: "House Band", key: "F minor"),
            bpm: 170.0,
            kit: "ragga-93",
            bass: [(instrument: "sub"), (instrument: "reese", level: -3.0)],
            tracks: {
                "pad": (instrument: "atmos-pad", reverb: 0.8),
                "stab": (instrument: "rave-stab", level: -6.0, pan: -0.3),
            },
            patterns: {
                "beat": Drums(bars: 1, steps: { "P1": "X... .... ..x. ...." }),
                "sub": Bass(bars: 1, notes: "F1:16"),
                "chords": Notes(track: "pad", bars: 2, notes: "F3+Ab3+C4:16 | Db3+F3+Ab3:16"),
                "stabs": Notes(track: "stab", bars: 1, notes: ".:4 F3:2 .:10"),
            },
            arrangement: [
                (name: "Intro", bars: 2, play: ["beat", "chords"]),
                (name: "Drop", bars: 2, play: ["beat", "sub", "stabs"]),
            ],
        )
    "#;

    #[test]
    fn tracks_play_their_notes_on_their_instruments() {
        use wu_audio::{EventKind, Part};

        let song = Project::from_ron(WITH_TRACKS)
            .expect("parses")
            .compile()
            .expect("compiles");
        let names: Vec<&str> = song.tracks.iter().map(|(name, _, _)| name.as_str()).collect();
        assert_eq!(names, ["pad", "stab"]);
        assert_eq!(song.tracks[0].2.len(), 6, "two chords of three");
        let stabs: Vec<Tick> = song.tracks[1].2.iter().map(|n| n.tick).collect();
        let on_step_4 = |bar| Tick::from_bars(bar) + Tick::from_steps(4);
        assert_eq!(stabs, [on_step_4(2), on_step_4(3)]);

        let program = song.whole_program(48_000, &song.tempo, 0);
        assert_eq!(program.rails, [0, 1], "the sub and the Reese under the bass line");
        let names: Vec<&str> = program.instruments.iter().map(|i| i.name()).collect();
        assert_eq!(names, ["Sub", "Reese", "Atmos Pad", "Rave Stab"]);
        assert_eq!(program.instruments[2].sends().reverb, 0.8);
        let on = |part| {
            program
                .events()
                .iter()
                .filter(|e| matches!(e.kind, EventKind::Note { part: p, .. } if p == part))
                .count()
        };
        assert_eq!((on(Part::Bass), on(Part::Track(2)), on(Part::Track(3))), (2, 6, 2));
    }

    #[test]
    fn track_mistakes_are_explained() {
        let compile = |text: &str| Project::from_ron(text).expect("parses").compile();
        let unknown = WITH_TRACKS.replace("\"rave-stab\"", "\"kazoo\"");
        assert!(matches!(compile(&unknown), Err(ProjectError::Instrument { .. })));
        let missing = WITH_TRACKS.replace("Notes(track: \"stab\"", "Notes(track: \"lead\"");
        assert!(matches!(compile(&missing), Err(ProjectError::MissingTrack { .. })));
        let chord = WITH_TRACKS.replace("notes: \"F1:16\"", "notes: \"F1+C2:16\"");
        assert!(matches!(compile(&chord), Err(ProjectError::BassChord { .. })));
    }

    #[test]
    fn swing_moves_odd_steps() {
        let swung = SMALL.replace("kit: \"ragga-93\",", "kit: \"ragga-93\", swing: 0.25,");
        let song = Project::from_ron(&swung).expect("parses").compile().expect("compiles");
        // The kick on step 10 is even: unmoved. Nothing in this beat is on an odd step.
        assert!(song.drums.iter().all(|h| h.tick.0 % 240 == 0));
    }
}
