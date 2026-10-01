//! Small UI helpers shared by the screens.

use bevy::prelude::*;

/// A fixed-size, absolutely positioned node centred on a point given relative
/// to the centre of the screen.
pub fn centred_on(x: f32, y: f32, width: f32, height: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: percent(50),
        top: percent(50),
        margin: UiRect {
            left: px(x - width / 2.0),
            top: px(y - height / 2.0),
            ..default()
        },
        width: px(width),
        height: px(height),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    }
}

/// A full-screen root that disappears when its screen is left.
pub fn screen_root<S: States>(screen: S) -> impl Bundle {
    (
        DespawnOnExit(screen),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
    )
}

/// A line of text in the default font.
pub fn label(text: impl Into<String>, size: f32, colour: Color) -> impl Bundle {
    (Text::new(text), TextFont::from_font_size(size), TextColor(colour))
}

/// Several lines of text, each centred.
pub fn centred_label(text: impl Into<String>, size: f32, colour: Color) -> impl Bundle {
    (label(text, size, colour), TextLayout::justify(Justify::Center))
}
