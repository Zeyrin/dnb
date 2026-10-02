//! Your tune into a WHEEL UP! song. Decode the file, listen to it (the tempo
//! and the bar grid, every kick, snare and hat and where it really sounds, the
//! sub-bass line, the drops), and hand that to the charter like any built-in
//! song.

#![forbid(unsafe_code)]

pub mod bass;
pub mod decode;
pub mod feel;
pub mod hits;
pub mod library;
pub mod listen;
pub mod spectrum;
pub mod structure;
pub mod tempo;

pub use decode::{DecodeError, Decoded, decode};
pub use library::{ImportError, Imported, ImportedSong, import, relisten};
pub use listen::{Listened, Stage, listen};
pub use tempo::{Grid, find_grid};
