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
}
