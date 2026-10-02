//! What the engine plays: a kit, instruments, a tempo map and a sorted list of
//! events, prepared on the main thread so the audio thread only compares frames.

use wu_instruments::{Instrument, Kit, Pad, RewindSounds, Tone};
use wu_time::{TempoMap, Tick};

use crate::backing::Backing;
use crate::mixer::MixSettings;

/// A drum hit to sequence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub tick: Tick,
    pub pad: Pad,
    /// 0–1.
    pub velocity: f32,
}

/// A held note: of the bass line, or of a track (see `Part`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub tick: Tick,
    pub length: Tick,
    /// MIDI key.
    pub key: u8,
    pub velocity: f32,
}

/// What a note plays on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Part {
    /// The bass line: on every one of the bass sounds, the ones the rails play.
    Bass,
    /// One instrument, by its index in the program.
    Track(u8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventKind {
    Pad {
        pad: Pad,
        velocity: f32,
    },
    /// Held for `frames`, then released.
    Note {
        part: Part,
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
    /// Part of what the player is playing (Classic audio): a miss mutes it.
    pub player: bool,
}

impl SeqEvent {
    pub fn pad(&self) -> Option<Pad> {
        match self.kind {
            EventKind::Pad { pad, .. } => Some(pad),
            EventKind::Note { .. } => None,
        }
    }

    /// Pads before notes at the same frame, each in a fixed order.
    fn order(&self) -> (i64, u8, Part, u8) {
        match self.kind {
            EventKind::Pad { pad, .. } => (self.frame, 0, Part::Bass, pad.index() as u8),
            EventKind::Note { part, key, .. } => (self.frame, 1, part, key),
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
    /// What notes play on. Track notes name one by its index.
    pub instruments: Vec<Instrument>,
    /// The instruments the bass line plays on, and the rails with it.
    pub rails: Vec<u8>,
    /// What a WHEEL UP! rewind sounds like, if the program allows one.
    pub rewind: Option<RewindSounds>,
    pub mix: MixSettings,
    /// A recording playing under everything: an imported tune.
    pub backing: Option<Backing>,
    events: Vec<SeqEvent>,
    loop_range: Option<LoopRange>,
}

impl Program {
    pub fn new(sample_rate: u32, tempo: TempoMap, kit: Kit) -> Program {
        Program {
            sample_rate,
            tempo,
            kit,
            instruments: Vec::new(),
            rails: Vec::new(),
            rewind: None,
            mix: MixSettings::default(),
            backing: None,
            events: Vec::new(),
            loop_range: None,
        }
    }

    /// Adds hits, placing each on its frame through the tempo map.
    pub fn with_hits(self, hits: impl IntoIterator<Item = Hit>) -> Program {
        self.add_hits(hits, false)
    }

    /// Adds hits the player is playing along to (Classic audio): they sound
    /// unless the player's part is muted (`Command::MutePlayer`).
    pub fn with_player_hits(self, hits: impl IntoIterator<Item = Hit>) -> Program {
        self.add_hits(hits, true)
    }

    fn add_hits(mut self, hits: impl IntoIterator<Item = Hit>, player: bool) -> Program {
        let (tempo, sample_rate) = (&self.tempo, self.sample_rate);
        self.events.extend(hits.into_iter().map(|hit| SeqEvent {
            tick: hit.tick,
            frame: tempo.frame_at(hit.tick, sample_rate),
            kind: EventKind::Pad {
                pad: hit.pad,
                velocity: clamp_velocity(hit.velocity),
            },
            player,
        }));
        self.events.sort_by_key(SeqEvent::order);
        self
    }

    /// A sampled bass sound: see `with_bass_sound`.
    pub fn with_tone(self, tone: Tone) -> Program {
        self.with_bass_sound(Instrument::Sampled(tone))
    }

    /// Adds a sound the bass line plays on, which the rails play live too.
    /// Several layer (a sub under a Reese).
    pub fn with_bass_sound(mut self, instrument: Instrument) -> Program {
        if let Ok(index) = u8::try_from(self.instruments.len()) {
            self.instruments.push(instrument);
            self.rails.push(index);
        }
        self
    }

    /// Adds an instrument for track notes; they name it by its index, which
    /// counts every instrument added so far, bass sounds included.
    pub fn with_instrument(mut self, instrument: Instrument) -> Program {
        if self.instruments.len() < usize::from(u8::MAX) {
            self.instruments.push(instrument);
        }
        self
    }

    pub fn with_mix(mut self, mix: MixSettings) -> Program {
        self.mix = mix;
        self
    }

    pub fn with_rewind(mut self, sounds: RewindSounds) -> Program {
        self.rewind = Some(sounds);
        self
    }

    /// Plays `backing` under everything, in step with the transport.
    pub fn with_backing(mut self, backing: Backing) -> Program {
        self.backing = Some(backing);
        self
    }

    /// Adds notes of the bass line, each held for its length.
    pub fn with_notes(self, notes: impl IntoIterator<Item = Note>) -> Program {
        self.add_notes(notes, Part::Bass, false)
    }

    /// Adds bass notes the player is playing along to (Classic audio).
    pub fn with_player_notes(self, notes: impl IntoIterator<Item = Note>) -> Program {
        self.add_notes(notes, Part::Bass, true)
    }

    /// Adds notes for the instrument at `track` (see `with_instrument`).
    pub fn with_track_notes(self, track: u8, notes: impl IntoIterator<Item = Note>) -> Program {
        self.add_notes(notes, Part::Track(track), false)
    }

    fn add_notes(mut self, notes: impl IntoIterator<Item = Note>, part: Part, player: bool) -> Program {
        let (tempo, sample_rate) = (&self.tempo, self.sample_rate);
        self.events.extend(notes.into_iter().map(|note| {
            let frame = tempo.frame_at(note.tick, sample_rate);
            let end = tempo.frame_at(note.tick + note.length, sample_rate);
            SeqEvent {
                tick: note.tick,
                frame,
                kind: EventKind::Note {
                    part,
                    key: note.key,
                    velocity: clamp_velocity(note.velocity),
                    frames: u32::try_from((end - frame).max(1)).unwrap_or(u32::MAX),
                },
                player,
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
                part: Part::Bass,
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
