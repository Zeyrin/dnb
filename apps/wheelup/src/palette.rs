//! Colours: a 90s rave flyer, neon on near-black.

use bevy::prelude::*;
use wu_instruments::Pad;

/// Near-black with a hint of violet: the back wall of every venue.
pub const BACKDROP: Color = Color::srgb(0.035, 0.027, 0.055);
/// Acid yellow, the colour of a flyer's headline.
pub const FLYER_YELLOW: Color = Color::srgb(0.98, 0.91, 0.18);
pub const INK: Color = Color::srgb(0.96, 0.95, 0.99);
pub const MUTED: Color = Color::srgb(0.62, 0.58, 0.72);
pub const SIGNAL: Color = Color::srgb(0.55, 0.85, 0.6);
pub const WARNING: Color = Color::srgb(1.0, 0.45, 0.35);
/// The bass rails: deep red, a speaker cone under load.
pub const BASS: Color = Color::srgb(0.92, 0.15, 0.22);

/// Each pad's colour, used wherever that pad appears.
pub fn pad(pad: Pad) -> Color {
    match pad {
        Pad::P1 => FLYER_YELLOW,
        Pad::P2 => Color::srgb(1.0, 0.25, 0.55),
        Pad::P3 => Color::srgb(0.62, 0.45, 1.0),
        Pad::P4 => Color::srgb(0.2, 0.85, 1.0),
        Pad::P5 => Color::srgb(1.0, 0.55, 0.15),
        Pad::P6 => Color::srgb(0.55, 0.95, 0.3),
        Pad::P7 => Color::srgb(0.72, 0.9, 1.0),
        Pad::P8 => Color::srgb(0.25, 0.95, 0.75),
    }
}

/// The same hue, unlit.
pub fn dim(colour: Color) -> Color {
    mix(BACKDROP, colour, 0.2)
}

pub fn mix(from: Color, to: Color, amount: f32) -> Color {
    let (a, b, t) = (from.to_srgba(), to.to_srgba(), amount.clamp(0.0, 1.0));
    Color::srgb(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
    )
}
