//! The songs that ship with the game, compiled into the binary.

use crate::project::{Project, ProjectError, Song};

#[derive(Clone, Copy, Debug)]
pub struct BuiltinSong {
    pub id: &'static str,
    pub project: &'static str,
}

pub const BUILTIN: [BuiltinSong; 1] = [BuiltinSong {
    id: "rooftop-transmission",
    project: include_str!("../../../content/songs/rooftop-transmission/project.ron"),
}];

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
            for (name, track, notes) in &compiled.tracks {
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
}
