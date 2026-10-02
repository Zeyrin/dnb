//! The songs that ship with the game, compiled into the binary.

use crate::project::{Project, ProjectError, Song};

#[derive(Clone, Copy, Debug)]
pub struct BuiltinSong {
    pub id: &'static str,
    pub project: &'static str,
}

pub const BUILTIN: [BuiltinSong; 9] = [
    BuiltinSong {
        id: "rooftop-transmission",
        project: include_str!("../../../content/songs/rooftop-transmission/project.ron"),
    },
    BuiltinSong {
        id: "underpass-spirits",
        project: include_str!("../../../content/songs/underpass-spirits/project.ron"),
    },
    BuiltinSong {
        id: "tidewater-lights",
        project: include_str!("../../../content/songs/tidewater-lights/project.ron"),
    },
    BuiltinSong {
        id: "bounce-patrol",
        project: include_str!("../../../content/songs/bounce-patrol/project.ron"),
    },
    BuiltinSong {
        id: "satellite-drift",
        project: include_str!("../../../content/songs/satellite-drift/project.ron"),
    },
    BuiltinSong {
        id: "undertow",
        project: include_str!("../../../content/songs/undertow/project.ron"),
    },
    BuiltinSong {
        id: "night-bus",
        project: include_str!("../../../content/songs/night-bus/project.ron"),
    },
    BuiltinSong {
        id: "circuit-breaker",
        project: include_str!("../../../content/songs/circuit-breaker/project.ron"),
    },
    // The lesson: the game lists it first.
    BuiltinSong {
        id: "first-steps",
        project: include_str!("../../../content/songs/first-steps/project.ron"),
    },
];

impl BuiltinSong {
    pub fn load(&self) -> Result<Song, ProjectError> {
        Project::from_ron(self.project)?.compile()
    }
}

#[cfg(test)]
mod tests {
    use wu_time::Tick;

    use super::*;

    #[test]
    fn every_builtin_song_compiles() {
        for song in BUILTIN {
            let compiled = song.load().unwrap_or_else(|e| panic!("{}: {e}", song.id));
            assert!(!compiled.drums.is_empty() && !compiled.bass.is_empty(), "{}", song.id);
            assert!(compiled.drums.iter().all(|h| h.tick < compiled.length), "{}", song.id);
        }
    }

    #[test]
    fn the_slice_song_is_the_length_the_brief_asks_for() {
        let song = BUILTIN[0].load().expect("compiles");
        let seconds = song.tempo.seconds_at(song.length.0 as f64);
        assert!((90.0..=210.0).contains(&seconds), "{seconds} s");
        assert_eq!(song.sections.first().map(|s| s.0.as_str()), Some("Intro"));
    }

    #[test]
    fn every_bass_line_stays_in_its_key() {
        for song in BUILTIN {
            let compiled = song.load().expect("compiles");
            let scale = crate::theory::scale(&compiled.meta.key)
                .unwrap_or_else(|| panic!("{}: unknown key {}", song.id, compiled.meta.key));
            for note in &compiled.bass {
                assert!(
                    scale.contains(&(note.key % 12)),
                    "{}: key {} at {}",
                    song.id,
                    note.key,
                    note.tick
                );
            }
        }
    }

    #[test]
    fn every_part_stays_in_its_key_chord_memory_and_all() {
        for song in BUILTIN {
            let compiled = song.load().expect("compiles");
            let scale = crate::theory::scale(&compiled.meta.key).expect("a known key");
            // A break has no key.
            for (name, track, notes) in compiled
                .tracks
                .iter()
                .filter(|(_, t, _)| !t.instrument.starts_with("break/"))
            {
                let instrument =
                    wu_instruments::Instrument::named(&track.instrument, 48_000).expect("checked on compile");
                for note in notes {
                    for &interval in instrument.chord() {
                        let key = i16::from(note.key) + i16::from(interval);
                        assert!(
                            scale.contains(&(key.rem_euclid(12) as u8)),
                            "{}: track {name} sounds key {key} at {}",
                            song.id,
                            note.tick
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_break_runs_under_the_drops_at_the_song_tempo() {
        use wu_audio::{EventKind, Part};
        use wu_instruments::Instrument;

        let song = BUILTIN[0].load().expect("compiles");
        let program = song.whole_program(48_000, &song.tempo, 0);
        let index = program
            .instruments
            .iter()
            .position(|i| i.name() == "Rough Rider")
            .expect("the break is in the program");
        let Instrument::Sampled(tone) = &program.instruments[index] else {
            panic!("a break is a sample")
        };
        assert!(
            (tone.tune - 168.0 / 165.0).abs() < 1e-9,
            "sped up to the song: {}",
            tone.tune
        );
        let rounds = program
            .events()
            .iter()
            .filter(|e| matches!(e.kind, EventKind::Note { part: Part::Track(t), .. } if usize::from(t) == index))
            .count();
        // Drop 16 bars, Jungle 6, Turn 2, Drop 2 8, Roller 8: once round every two bars.
        assert_eq!(rounds, (16 + 6 + 2 + 8 + 8) / 2);
    }

    #[test]
    fn every_song_has_a_hype_phrase_of_eight_bars_on_the_phrase_grid() {
        for song in BUILTIN {
            let compiled = song.load().expect("compiles");
            assert!(
                !compiled.hype.is_empty(),
                "{}: the brief asks for one at least",
                song.id
            );
            for &(start, end) in &compiled.hype {
                assert_eq!(start.0 % Tick::from_bars(8).0, 0, "{}: phrase at {start}", song.id);
                assert!(end > start && end - start <= Tick::from_bars(8), "{}", song.id);
            }
        }
    }

    /// Snare, ghost, rim, jungle snare and tom hits in the second half of a bar.
    fn turning(song: &crate::project::Song, bar: i64) -> usize {
        use wu_instruments::Pad::*;
        let (from, to) = (Tick::from_bars(bar) + Tick::from_beats(2), Tick::from_bars(bar + 1));
        song.drums
            .iter()
            .filter(|h| from <= h.tick && h.tick < to && matches!(h.pad, P2 | P3 | P4 | P5 | P6))
            .count()
    }

    #[test]
    fn every_phrase_with_drums_turns_round_on_a_fill() {
        for song in BUILTIN {
            let compiled = song.load().expect("compiles");
            let bars = compiled.length.bar();
            let drums_in = |bar: i64| {
                compiled
                    .drums
                    .iter()
                    .any(|h| Tick::from_bars(bar) <= h.tick && h.tick < Tick::from_bars(bar + 1))
            };
            // The last phrase rides out for the next DJ.
            for phrase in 0..bars / 8 - 1 {
                let last = phrase * 8 + 7;
                if !(phrase * 8..last).all(drums_in) {
                    continue;
                }
                assert!(
                    turning(&compiled, last) > turning(&compiled, last - 1),
                    "{}: bar {} ends a phrase without a fill",
                    song.id,
                    last + 1
                );
            }
        }
    }

    #[test]
    fn every_drop_lands_after_a_riser_and_a_beat_of_silence() {
        for song in BUILTIN {
            let compiled = song.load().expect("compiles");
            let hype_at = |tick: Tick| compiled.hype.iter().any(|&(start, _)| start == tick);
            let mut drops = 0;
            for pair in compiled.sections.windows(2) {
                let ((_, before, _), (name, start, _)) = (&pair[0], &pair[1]);
                if !hype_at(*start) || hype_at(*before) {
                    continue;
                }
                drops += 1;
                let gap = *start - Tick::from_beats(1);
                let sounding = |n: &wu_audio::Note| n.tick < *start && n.tick + n.length > gap;
                assert!(
                    !compiled.drums.iter().any(|h| gap <= h.tick && h.tick < *start),
                    "{}: drums in the beat before {name}",
                    song.id
                );
                assert!(
                    !compiled.bass.iter().any(sounding),
                    "{}: bass in the beat before {name}",
                    song.id
                );
                let mut breaks = compiled
                    .tracks
                    .iter()
                    .filter(|(_, track, _)| track.instrument.starts_with("break/"));
                assert!(
                    !breaks.any(|(_, _, notes)| notes.iter().any(sounding)),
                    "{}: a break in the beat before {name}",
                    song.id
                );
                let rising = compiled
                    .tracks
                    .iter()
                    .filter(|(_, track, _)| track.instrument == "riser")
                    .flat_map(|(_, _, notes)| notes)
                    .any(|n| n.tick + Tick::from_bars(1) < gap && n.tick + n.length >= gap);
                assert!(rising, "{}: no riser climbing to {name}", song.id);
            }
            assert!(drops > 0, "{}: no drop", song.id);
        }
    }
}
