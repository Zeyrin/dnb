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

/// Each pad's colour, used wherever that pad appears. The face buttons' pads
/// wear their buttons' own colours: △ green, □ pink, ✕ blue, ○ red.
pub fn pad(pad: Pad) -> Color {
    match pad {
        Pad::P1 => FLYER_YELLOW,
        Pad::P2 => Color::srgb(1.0, 0.55, 0.15),
        Pad::P3 => Color::srgb(0.62, 0.45, 1.0),
        Pad::P4 => Color::srgb(0.2, 0.85, 1.0),
        Pad::P5 => Color::srgb(0.25, 0.88, 0.55),
        Pad::P6 => Color::srgb(1.0, 0.48, 0.78),
        Pad::P7 => Color::srgb(0.42, 0.6, 1.0),
        Pad::P8 => Color::srgb(1.0, 0.27, 0.27),
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

/// A tune's colour, by its style: its record's label, its glow.
pub fn subgenre(name: &str) -> Color {
    match name.to_lowercase().as_str() {
        "jungle" => FLYER_YELLOW,
        "darkside" => Color::srgb(0.62, 0.45, 1.0),
        "liquid" => Color::srgb(0.25, 0.95, 0.75),
        "jump-up" => Color::srgb(1.0, 0.55, 0.15),
        "atmospheric" => Color::srgb(0.5, 0.75, 1.0),
        "halftime" => Color::srgb(0.95, 0.2, 0.25),
        "lesson" => SIGNAL,
        "roller" => Color::srgb(0.85, 0.85, 0.3),
        "neurofunk" => Color::srgb(0.3, 1.0, 0.45),
        "ragga" => Color::srgb(1.0, 0.38, 0.45),
        _ => Color::srgb(0.9, 0.9, 0.96),
    }
}
