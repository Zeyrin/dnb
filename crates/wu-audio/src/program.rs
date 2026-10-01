//! What the engine plays: a kit, a tempo map and a sorted list of events,
//! prepared on the main thread so the audio thread only compares frames.

use wu_instruments::{Kit, Pad, Tone};
use wu_time::{TempoMap, Tick};

/// A drum hit to sequence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub tick: Tick,
    pub pad: Pad,
    /// 0–1.
    pub velocity: f32,
}

/// A held note on the program's tone (the bass line).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub tick: Tick,
    pub length: Tick,
    /// MIDI key.
    pub key: u8,
    pub velocity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventKind {
    Pad {
        pad: Pad,
        velocity: f32,
    },
    /// Held for `frames`, then released.
    Note {
        key: u8,
        velocity: f32,
        frames: u32,
    },
}

/// Something to play, placed on the sample frame it starts at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeqEvent {
    pub tick: Tick,
    pub frame: i64,
    pub kind: EventKind,
}

impl SeqEvent {
    pub fn pad(&self) -> Option<Pad> {
        match self.kind {
            EventKind::Pad { pad, .. } => Some(pad),
            EventKind::Note { .. } => None,
        }
    }

    /// Pads before notes at the same frame, each in a fixed order.
    fn order(&self) -> (i64, u8, u8) {
        match self.kind {
            EventKind::Pad { pad, .. } => (self.frame, 0, pad.index() as u8),
            EventKind::Note { key, .. } => (self.frame, 1, key),
        }
    }
}

fn clamp_velocity(velocity: f32) -> f32 {
    if velocity.is_finite() {
        velocity.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// A section the transport repeats: `end` jumps back to `start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopRange {
    pub start: Tick,
    pub end: Tick,
    pub start_frame: i64,
    pub end_frame: i64,
}

impl LoopRange {
    /// `None` unless `end` is after `start` by at least one frame.
    pub fn new(start: Tick, end: Tick, tempo: &TempoMap, sample_rate: u32) -> Option<LoopRange> {
        let start_frame = tempo.frame_at(start, sample_rate);
        let end_frame = tempo.frame_at(end, sample_rate);
        (end_frame > start_frame).then_some(LoopRange {
            start,
            end,
            start_frame,
            end_frame,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Program {
    pub sample_rate: u32,
    pub tempo: TempoMap,
    pub kit: Kit,
    /// What notes play on, if the program has any.
    pub tone: Option<Tone>,
    events: Vec<SeqEvent>,
    loop_range: Option<LoopRange>,
}

impl Program {
    pub fn new(sample_rate: u32, tempo: TempoMap, kit: Kit) -> Program {
        Program {
            sample_rate,
            tempo,
            kit,
            tone: None,
            events: Vec::new(),
            loop_range: None,
        }
    }

    /// Adds hits, placing each on its frame through the tempo map.
    pub fn with_hits(mut self, hits: impl IntoIterator<Item = Hit>) -> Program {
        let (tempo, sample_rate) = (&self.tempo, self.sample_rate);
        self.events.extend(hits.into_iter().map(|hit| SeqEvent {
            tick: hit.tick,
            frame: tempo.frame_at(hit.tick, sample_rate),
            kind: EventKind::Pad {
                pad: hit.pad,
                velocity: clamp_velocity(hit.velocity),
            },
        }));
        self.events.sort_by_key(SeqEvent::order);
        self
    }

    pub fn with_tone(mut self, tone: Tone) -> Program {
        self.tone = Some(tone);
        self
    }

    /// Adds notes for the tone, each held for its length.
    pub fn with_notes(mut self, notes: impl IntoIterator<Item = Note>) -> Program {
        let (tempo, sample_rate) = (&self.tempo, self.sample_rate);
        self.events.extend(notes.into_iter().map(|note| {
            let frame = tempo.frame_at(note.tick, sample_rate);
            let end = tempo.frame_at(note.tick + note.length, sample_rate);
            SeqEvent {
                tick: note.tick,
                frame,
                kind: EventKind::Note {
                    key: note.key,
                    velocity: clamp_velocity(note.velocity),
                    frames: u32::try_from((end - frame).max(1)).unwrap_or(u32::MAX),
                },
            }
        }));
        self.events.sort_by_key(SeqEvent::order);
        self
    }

    pub fn with_loop(mut self, start: Tick, end: Tick) -> Program {
        self.loop_range = LoopRange::new(start, end, &self.tempo, self.sample_rate);
        self
    }

    pub fn events(&self) -> &[SeqEvent] {
        &self.events
    }

    pub fn loop_range(&self) -> Option<LoopRange> {
        self.loop_range
    }

    pub(crate) fn set_loop_range(&mut self, range: Option<LoopRange>) {
        self.loop_range = range;
    }

    /// Index of the first event at or after `frame`.
    pub(crate) fn first_event_at(&self, frame: i64) -> usize {
        self.events.partition_point(|e| e.frame < frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_sorted_and_placed_on_frames() {
        let tempo = TempoMap::constant(174.0);
        let program = Program::new(48_000, tempo.clone(), Kit::ragga_93(48_000)).with_hits([
            Hit {
                tick: Tick::from_beats(1),
                pad: Pad::P2,
                velocity: 1.0,
            },
            Hit {
                tick: Tick::ZERO,
                pad: Pad::P1,
                velocity: 2.0,
            },
        ]);
        let events = program.events();
        assert_eq!(
            events[0].kind,
            EventKind::Pad {
                pad: Pad::P1,
                velocity: 1.0
            }
        );
        assert_eq!(events[1].frame, tempo.frame_at(Tick::from_beats(1), 48_000));
        assert_eq!(program.first_event_at(1), 1);
    }

    #[test]
    fn notes_last_their_length_in_frames() {
        let tempo = TempoMap::constant(120.0);
        let program = Program::new(48_000, tempo, Kit::ragga_93(48_000))
            .with_tone(Tone::sub(48_000))
            .with_notes([Note {
                tick: Tick::from_beats(1),
                length: Tick::from_beats(2),
                key: 29,
                velocity: 0.9,
            }])
            .with_hits([Hit {
                tick: Tick::from_beats(1),
                pad: Pad::P1,
                velocity: 1.0,
            }]);
        let events = program.events();
        assert_eq!(events[0].pad(), Some(Pad::P1), "pads before notes on the same frame");
        assert_eq!(
            events[1].kind,
            EventKind::Note {
                key: 29,
                velocity: 0.9,
                frames: 48_000
            }
        );
    }

    #[test]
    fn empty_loops_are_refused() {
        let tempo = TempoMap::constant(174.0);
        assert!(LoopRange::new(Tick::from_bars(1), Tick::from_bars(1), &tempo, 48_000).is_none());
        assert!(LoopRange::new(Tick::ZERO, Tick::from_bars(1), &tempo, 48_000).is_some());
    }
}
