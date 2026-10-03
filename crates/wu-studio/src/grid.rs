//! The Pattern view's step grid: 16 steps a bar for each of the eight pads,
//! read from a drum pattern's step strings and written back to them.

use std::collections::BTreeMap;

use wu_content::{Step, StepError, parse_steps};
use wu_instruments::steps::{ACCENT, GHOST, HIT};
use wu_instruments::{PAD_COUNT, Pad};
use wu_time::STEPS_PER_BAR;

/// The longest pattern the grid edits.
pub const MOST_BARS: i64 = 8;

/// One step of one pad.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cell {
    #[default]
    Rest,
    Ghost,
    Hit,
    Accent,
    /// Two to four hits inside the step: a ratchet, a snare roll's stutter.
    Ratchet(u8),
}

impl Cell {
    /// As step notation writes it.
    pub fn symbol(self) -> char {
        match self {
            Cell::Rest => '.',
            Cell::Ghost => 'o',
            Cell::Hit => 'x',
            Cell::Accent => 'X',
            Cell::Ratchet(strokes) => char::from(b'0' + strokes.clamp(2, 4)),
        }
    }

    /// The nearest of the notation's three strengths.
    fn of(step: Step) -> Cell {
        match step {
            Step::Rest => Cell::Rest,
            Step::Hit(v) if v >= (HIT + ACCENT) / 2.0 => Cell::Accent,
            Step::Hit(v) if v >= (GHOST + HIT) / 2.0 => Cell::Hit,
            Step::Hit(_) => Cell::Ghost,
            Step::Ratchet { strokes, .. } => Cell::Ratchet(strokes),
        }
    }

    /// What ✕ turns it into: a hit, then an accent, then a ghost, then nothing.
    pub fn next(self) -> Cell {
        match self {
            Cell::Rest => Cell::Hit,
            Cell::Hit => Cell::Accent,
            Cell::Accent => Cell::Ghost,
            Cell::Ghost | Cell::Ratchet(_) => Cell::Rest,
        }
    }

    /// What R3 turns it into: a ratchet of two, three, four, then a plain hit.
    pub fn next_ratchet(self) -> Cell {
        match self {
            Cell::Ratchet(strokes) if strokes >= 4 => Cell::Hit,
            Cell::Ratchet(strokes) => Cell::Ratchet(strokes + 1),
            _ => Cell::Ratchet(2),
        }
    }
}

/// A drum pattern, pad by pad, step by step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub bars: i64,
    /// Per pad, `bars × 16` cells.
    pub cells: [Vec<Cell>; PAD_COUNT],
}

impl Grid {
    pub fn empty(bars: i64) -> Grid {
        let bars = bars.clamp(1, MOST_BARS);
        Grid {
            bars,
            cells: std::array::from_fn(|_| vec![Cell::Rest; (bars * STEPS_PER_BAR) as usize]),
        }
    }

    /// From a `Drums` pattern's step strings, keyed "P1"–"P8"; a shorter string
    /// repeats to fill the pattern, as it plays.
    pub fn from_steps(bars: i64, steps: &BTreeMap<String, String>) -> Result<Grid, StepError> {
        let mut grid = Grid::empty(bars);
        for pad in Pad::ALL {
            let Some(text) = steps.get(&pad_name(pad)) else {
                continue;
            };
            let parsed = parse_steps(text)?;
            if parsed.is_empty() {
                continue;
            }
            for (i, cell) in grid.cells[pad.index()].iter_mut().enumerate() {
                *cell = Cell::of(parsed[i % parsed.len()]);
            }
        }
        Ok(grid)
    }

    /// Back to step strings: a pad with nothing to play is left out.
    pub fn to_steps(&self) -> BTreeMap<String, String> {
        let mut steps = BTreeMap::new();
        for pad in Pad::ALL {
            let cells = &self.cells[pad.index()];
            if cells.iter().all(|&c| c == Cell::Rest) {
                continue;
            }
            let bars: Vec<String> = cells
                .chunks(STEPS_PER_BAR as usize)
                .map(|bar| {
                    let beats: Vec<String> = bar
                        .chunks(4)
                        .map(|beat| beat.iter().map(|c| c.symbol()).collect())
                        .collect();
                    beats.join(" ")
                })
                .collect();
            steps.insert(pad_name(pad), bars.join(" | "));
        }
        steps
    }

    pub fn steps(&self) -> usize {
        (self.bars * STEPS_PER_BAR) as usize
    }

    pub fn cell(&self, pad: Pad, step: usize) -> Cell {
        self.cells[pad.index()].get(step).copied().unwrap_or_default()
    }

    pub fn set(&mut self, pad: Pad, step: usize, cell: Cell) {
        if let Some(slot) = self.cells[pad.index()].get_mut(step) {
            *slot = cell;
        }
    }

    /// Longer, it repeats what is there; shorter, it keeps the first bars.
    pub fn set_bars(&mut self, bars: i64) {
        let bars = bars.clamp(1, MOST_BARS);
        let steps = (bars * STEPS_PER_BAR) as usize;
        for cells in &mut self.cells {
            let old = cells.clone();
            *cells = (0..steps).map(|i| old[i % old.len()]).collect();
        }
        self.bars = bars;
    }
}

pub fn pad_name(pad: Pad) -> String {
    format!("P{}", pad.index() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(rows: &[(&str, &str)]) -> BTreeMap<String, String> {
        rows.iter()
            .map(|&(pad, text)| (pad.to_owned(), text.to_owned()))
            .collect()
    }

    #[test]
    fn step_strings_round_trip_through_the_grid() {
        let steps = pattern(&[
            ("P1", "X... .... ..X. .... | X.x. .... ..xx ...."),
            ("P3", ".... ...o .o.. ...o | .... .... .... ...."),
        ]);
        let grid = Grid::from_steps(2, &steps).expect("parses");
        assert_eq!(grid.cell(Pad::P1, 0), Cell::Accent);
        assert_eq!(grid.cell(Pad::P1, 18), Cell::Hit);
        assert_eq!(grid.cell(Pad::P3, 7), Cell::Ghost);
        assert_eq!(grid.to_steps(), steps);
    }

    #[test]
    fn a_short_pattern_repeats_and_a_long_one_is_cut() {
        let steps = pattern(&[("P2", ".... X... .... X...")]);
        let mut grid = Grid::from_steps(2, &steps).expect("parses");
        assert_eq!(grid.cell(Pad::P2, 20), Cell::Accent, "the bar repeats");
        grid.set(Pad::P2, 20, Cell::Rest);
        grid.set_bars(4);
        assert_eq!(grid.steps(), 64);
        assert_eq!(grid.cell(Pad::P2, 52), Cell::Rest, "bar 2 comes round again in bar 4");
        grid.set_bars(1);
        assert_eq!(grid.to_steps()["P2"], ".... X... .... X...");
        grid.set_bars(99);
        assert_eq!(grid.bars, MOST_BARS);
    }

    #[test]
    fn ratchets_write_as_digits_and_cycle_from_two_to_four() {
        let steps = pattern(&[("P2", ".... X... .... X.34")]);
        let grid = Grid::from_steps(1, &steps).expect("parses");
        assert_eq!(grid.cell(Pad::P2, 14), Cell::Ratchet(3));
        assert_eq!(grid.to_steps(), steps);
        let mut cell = Cell::Hit;
        let seen: Vec<char> = (0..4)
            .map(|_| {
                cell = cell.next_ratchet();
                cell.symbol()
            })
            .collect();
        assert_eq!(seen, ['2', '3', '4', 'x']);
    }

    #[test]
    fn a_cross_cycles_a_step_through_every_strength() {
        let mut cell = Cell::Rest;
        let seen: Vec<char> = (0..4)
            .map(|_| {
                cell = cell.next();
                cell.symbol()
            })
            .collect();
        assert_eq!(seen, ['x', 'X', 'o', '.']);
    }
}
