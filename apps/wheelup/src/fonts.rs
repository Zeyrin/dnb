//! Fonts, compiled into the binary so they load the same everywhere.
//! Both are under the SIL Open Font License (see `assets/fonts/`).

use bevy::asset::AssetId;
use bevy::prelude::*;

/// JetBrains Mono: every label, and the controller symbols ↑ ↓ ← → △ □ ✕ ○.
const MONO: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf");
const MONO_BOLD: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Bold.ttf");
/// Bungee: headlines only (it has no □ ○ ✕).
const DISPLAY: &[u8] = include_bytes!("../../../assets/fonts/Bungee-Regular.ttf");

#[derive(Resource, Debug)]
pub struct Fonts {
    pub bold: Handle<Font>,
    pub display: Handle<Font>,
}

#[derive(Debug)]
pub struct FontsPlugin;

impl Plugin for FontsPlugin {
    fn build(&self, app: &mut App) {
        let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
        // The default handle: text that names no font gets JetBrains Mono.
        if fonts
            .insert(AssetId::default(), Font::from_bytes(MONO.to_vec()))
            .is_err()
        {
            warn!("could not install the default font");
        }
        let bold = fonts.add(Font::from_bytes(MONO_BOLD.to_vec()));
        let display = fonts.add(Font::from_bytes(DISPLAY.to_vec()));
        app.insert_resource(Fonts { bold, display });
    }
}
