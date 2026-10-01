//! Song projects: patterns and an arrangement, written in RON, compiled into
//! the hits and notes the engine plays and the charts are cut from.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wu_audio::{BUS_COUNT, Hit, MixSettings, Note, Program};
use wu_instruments::{Kit, Pad, Tone};
use wu_time::{STEPS_PER_BAR, TempoMap, TempoPoint, Tick};

use crate::notes::{NoteError, parse_notes};
use crate::settings::AudioMode;
use crate::steps::{Step, StepError, parse_steps};

pub const PROJECT_VERSION: u32 = 1;
/// Kits a project may name.
pub const KITS: [&str; 1] = ["ragga-93"];

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
    pub patterns: BTreeMap<String, Pattern>,
    pub arrangement: Vec<Section>,
}

/// How the song is mixed and mastered, in dB: each bus's level, how far the
/// bass ducks under the kick and how fast it comes back, and the gain into the
/// master limiter (set so the song lands at the target loudness: see `mastering`).
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
    /// A bass line in note notation (see `notes`).
    Bass { bars: i64, notes: String },
}

/// A stretch of the song; each pattern it plays repeats to fill it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub bars: i64,
    pub play: Vec<String>,
}

/// A note of the bass line: the engine's own note type.
pub type BassNote = Note;

/// A compiled song: everything in ticks, sorted.
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub meta: Meta,
    pub kit: String,
    pub mix: MixSettings,
    pub tempo: TempoMap,
    pub drums: Vec<Hit>,
    pub bass: Vec<BassNote>,
    /// Name, first tick, end tick.
    pub sections: Vec<(String, Tick, Tick)>,
    pub length: Tick,
}

impl Song {
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
        let (theirs, backing): (Vec<Hit>, Vec<Hit>) =
            self.drums.iter().copied().partition(|h| player_plays(h.tick, h.pad));
        let (held, bass): (Vec<Note>, Vec<Note>) = self.bass.iter().copied().partition(|n| player_holds(n.tick, n.key));
        let program = Program::new(sample_rate, tempo.clone(), Kit::ragga_93(sample_rate))
            .with_mix(self.mix)
            .with_tone(Tone::sub(sample_rate))
            .with_hits(count_in.chain(backing))
            .with_notes(bass);
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
}

fn pad_named(name: &str) -> Option<Pad> {
    let index: usize = name.strip_prefix('P')?.parse().ok()?;
    Pad::from_index(index.checked_sub(1)?)
}

impl Project {
    pub fn from_ron(text: &str) -> Result<Project, ProjectError> {
        let project: Project = ron::from_str(text).map_err(|e| ProjectError::Parse(e.to_string()))?;
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
        if !KITS.contains(&self.kit.as_str()) {
            return Err(ProjectError::Kit(self.kit.clone()));
        }
        let compiled = self.compile_patterns()?;
        let mut drums = Vec::new();
        let mut bass = Vec::new();
        let mut sections = Vec::new();
        let mut bar = 0i64;
        for section in &self.arrangement {
            if section.bars < 1 {
                return Err(ProjectError::EmptySection(section.name.clone()));
            }
            let (start, end) = (Tick::from_bars(bar), Tick::from_bars(bar + section.bars));
            for name in &section.play {
                let pattern = compiled.get(name).ok_or_else(|| ProjectError::MissingPattern {
                    section: section.name.clone(),
                    pattern: name.clone(),
                })?;
                let mut offset = start;
                while offset < end {
                    match pattern {
                        Compiled::Drums { hits, .. } => drums.extend(
                            hits.iter()
                                .map(|h| Hit {
                                    tick: h.tick + offset,
                                    ..*h
                                })
                                .filter(|h| h.tick < end)
                                .map(|h| Hit {
                                    tick: h.tick.swung(self.swing),
                                    ..h
                                }),
                        ),
                        Compiled::Bass { notes, .. } => bass.extend(
                            notes
                                .iter()
                                .map(|n| BassNote {
                                    tick: n.tick + offset,
                                    ..*n
                                })
                                .filter(|n| n.tick < end)
                                .map(|n| BassNote {
                                    length: n.length.min(end - n.tick),
                                    ..n
                                }),
                        ),
                    }
                    offset += Tick::from_bars(pattern.bars());
                }
            }
            sections.push((section.name.clone(), start, end));
            bar += section.bars;
        }
        drums.sort_by_key(|h| (h.tick, h.pad));
        bass.sort_by_key(|n| n.tick);
        Ok(Song {
            meta: self.meta.clone(),
            kit: self.kit.clone(),
            mix: self.mix.settings(),
            tempo,
            drums,
            bass,
            sections,
            length: Tick::from_bars(bar),
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
                    let (parsed, steps) = parse_notes(notes).map_err(|source| ProjectError::Notes {
                        pattern: name.clone(),
                        source,
                    })?;
                    if steps != bars * STEPS_PER_BAR {
                        return Err(length_error(*bars, steps));
                    }
                    let notes = parsed
                        .into_iter()
                        .map(|n| BassNote {
                            tick: Tick::from_steps(n.step),
                            length: Tick::from_steps(n.length),
                            key: n.key,
                            velocity: 0.9,
                        })
                        .collect();
                    Compiled::Bass { bars: *bars, notes }
                }
            };
            if entry.bars() < 1 {
                return Err(length_error(entry.bars(), 0));
            }
            compiled.insert(name.clone(), entry);
        }
        Ok(compiled)
    }
}

enum Compiled {
    Drums { bars: i64, hits: Vec<Hit> },
    Bass { bars: i64, notes: Vec<BassNote> },
}

impl Compiled {
    fn bars(&self) -> i64 {
        match self {
            Compiled::Drums { bars, .. } | Compiled::Bass { bars, .. } => *bars,
        }
    }
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
        assert!(all.tone.is_some());
    }

    #[test]
    fn swing_moves_odd_steps() {
        let swung = SMALL.replace("kit: \"ragga-93\",", "kit: \"ragga-93\", swing: 0.25,");
        let song = Project::from_ron(&swung).expect("parses").compile().expect("compiles");
        // The kick on step 10 is even: unmoved. Nothing in this beat is on an odd step.
        assert!(song.drums.iter().all(|h| h.tick.0 % 240 == 0));
    }
}
