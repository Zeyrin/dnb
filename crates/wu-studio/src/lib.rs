//! WHEEL UP!'s Studio, without the screen: a tune being made from a template,
//! edited one step at a time with unlimited undo and redo, saved crash-safe,
//! and compiled into a song the game can play (Chart It).

#![forbid(unsafe_code)]

pub mod grid;
pub mod store;
pub mod templates;

use std::path::PathBuf;

use wu_content::project::{Pattern, Project, ProjectError, Song};
use wu_instruments::Pad;

use crate::grid::{Cell, Grid};
use crate::templates::BEAT;

/// Tempos the Studio offers.
pub const TEMPOS: std::ops::RangeInclusive<f64> = 150.0..=185.0;

/// One change to the tune; each can be undone.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// ✕ on a step: a hit, an accent, a ghost, nothing.
    Cycle {
        pad: Pad,
        step: usize,
    },
    /// One step set as it is wanted (○ erases).
    Set {
        pad: Pad,
        step: usize,
        cell: Cell,
    },
    /// A pass recorded in the Live view: its hits, undone together.
    Hits(Vec<(Pad, usize, Cell)>),
    /// Every step of a pad emptied.
    Clear(Pad),
    /// The beat's length in bars (1–8).
    Bars(i64),
    Tempo(f64),
    Kit(String),
}

/// The tune on the Studio's desk.
#[derive(Clone, Debug)]
pub struct Studio {
    pub project: Project,
    /// Its folder, once saved (see [`store`]).
    pub folder: Option<PathBuf>,
    /// Changed since it was last saved.
    pub dirty: bool,
    undo: Vec<Project>,
    redo: Vec<Project>,
}

impl Studio {
    pub fn new(project: Project, folder: Option<PathBuf>) -> Studio {
        Studio {
            project,
            folder,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// The beat, as the Pattern view shows it.
    pub fn grid(&self) -> Grid {
        match self.project.patterns.get(BEAT) {
            Some(Pattern::Drums { bars, steps }) => {
                Grid::from_steps(*bars, steps).unwrap_or_else(|_| Grid::empty(*bars))
            }
            _ => Grid::empty(2),
        }
    }

    fn set_grid(&mut self, grid: &Grid) {
        self.project.patterns.insert(
            BEAT.to_owned(),
            Pattern::Drums {
                bars: grid.bars,
                steps: grid.to_steps(),
            },
        );
    }

    /// Makes `edit`; whether it changed anything.
    pub fn apply(&mut self, edit: Edit) -> bool {
        let before = self.project.clone();
        let mut grid = self.grid();
        match edit {
            Edit::Cycle { pad, step } => {
                let next = grid.cell(pad, step).next();
                grid.set(pad, step, next);
                self.set_grid(&grid);
            }
            Edit::Set { pad, step, cell } => {
                grid.set(pad, step, cell);
                self.set_grid(&grid);
            }
            Edit::Hits(hits) => {
                for (pad, step, cell) in hits {
                    grid.set(pad, step, cell);
                }
                self.set_grid(&grid);
            }
            Edit::Clear(pad) => {
                for step in 0..grid.steps() {
                    grid.set(pad, step, Cell::Rest);
                }
                self.set_grid(&grid);
            }
            Edit::Bars(bars) => {
                grid.set_bars(bars);
                self.set_grid(&grid);
            }
            Edit::Tempo(bpm) => self.project.bpm = bpm.clamp(*TEMPOS.start(), *TEMPOS.end()).round(),
            Edit::Kit(kit) => {
                if wu_instruments::kit_def(&kit).is_some() {
                    self.project.kit = kit;
                }
            }
        }
        if self.project == before {
            return false;
        }
        self.undo.push(before);
        self.redo.clear();
        self.dirty = true;
        true
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.project, previous));
        self.dirty = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.project, next));
        self.dirty = true;
        true
    }

    /// The tune, ready to play: the engine's program, or Chart It's song.
    pub fn compile(&self) -> Result<Song, ProjectError> {
        self.project.compile()
    }

    /// The beat alone, looped while it is edited: one pass of it with its bass.
    pub fn loop_song(&self) -> Result<Song, ProjectError> {
        let mut project = self.project.clone();
        let bars = self.grid().bars;
        let play: Vec<String> = project.patterns.keys().cloned().collect();
        project.arrangement = vec![wu_content::project::Section {
            name: "Loop".to_owned(),
            bars,
            play,
            hype: false,
            fill: None,
            gap: 0,
            lesson: None,
        }];
        project.compile()
    }

    /// Saves to its folder, choosing one under `dir` the first time.
    pub fn save(&mut self, dir: &std::path::Path) -> std::io::Result<PathBuf> {
        let folder = self
            .folder
            .clone()
            .unwrap_or_else(|| store::fresh_folder(dir, &self.project.meta.title));
        store::save(&folder, &self.project)?;
        self.folder = Some(folder.clone());
        self.dirty = false;
        Ok(folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::templates;

    fn studio() -> Studio {
        Studio::new(templates().remove(0).project, None)
    }

    #[test]
    fn edits_undo_and_redo_without_end() {
        let mut studio = studio();
        let start = studio.project.clone();
        let step = (0..16)
            .find(|&s| studio.grid().cell(Pad::P8, s) == Cell::Rest)
            .expect("a rest");
        assert!(studio.apply(Edit::Cycle { pad: Pad::P8, step }));
        assert_eq!(studio.grid().cell(Pad::P8, step), Cell::Hit);
        assert!(studio.apply(Edit::Bars(4)));
        assert!(studio.apply(Edit::Tempo(500.0)));
        assert_eq!(studio.project.bpm, *TEMPOS.end());
        assert!(!studio.apply(Edit::Kit("no-such-kit".into())), "nothing changed");
        while studio.undo() {}
        assert_eq!(studio.project, start);
        assert!(studio.redo() && studio.redo());
        assert_eq!(studio.grid().bars, 4);
        assert!(studio.apply(Edit::Clear(Pad::P1)));
        assert!(!studio.redo(), "a new edit drops what was undone");
        // A recorded pass is one edit: one undo takes it all back.
        let before = studio.project.clone();
        assert!(studio.apply(Edit::Hits(vec![(Pad::P5, 3, Cell::Hit), (Pad::P6, 9, Cell::Hit)])));
        assert!(studio.undo());
        assert_eq!(studio.project, before);
    }

    #[test]
    fn the_beat_loops_and_the_tune_charts() {
        let mut studio = studio();
        studio.apply(Edit::Bars(1));
        let looped = studio.loop_song().expect("compiles");
        assert_eq!(looped.length, wu_time::Tick::from_bars(1));
        let song = studio.compile().expect("compiles");
        assert!(!song.hype.is_empty(), "the drop is a hype phrase");
        let chart = wu_chart::auto_chart(&song.drums, &song.bass, &song.tempo, wu_chart::Difficulty::Hard);
        assert!(!chart.notes.is_empty());
        assert!(wu_chart::validate(&chart, &song.tempo).is_empty(), "a playable chart");
    }
}
