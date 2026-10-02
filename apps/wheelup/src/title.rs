//! The header: the name, small, at the top of the screen.

use bevy::prelude::*;

use crate::fonts::Fonts;
use crate::palette;
use crate::screens::Chrome;

#[derive(Debug)]
pub struct TitlePlugin;

impl Plugin for TitlePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_title);
    }
}

fn spawn_title(mut commands: Commands, fonts: Res<Fonts>) {
    commands
        .spawn((
            Chrome,
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::top(px(36)),
                row_gap: px(6),
                ..default()
            },
        ))
        .with_children(|header| {
            header.spawn((
                Text::new("WHEEL UP!"),
                TextFont {
                    font: fonts.display.clone().into(),
                    ..TextFont::from_font_size(52.0)
                },
                TextColor(palette::FLYER_YELLOW),
            ));
            header.spawn((
                Text::new("a junglist rhythm game & controller-first DAW"),
                TextFont::from_font_size(16.0),
                TextColor(palette::MUTED),
            ));
        });
}
