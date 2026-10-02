//! The header: the name, small, at the top of the screen.

use bevy::prelude::*;

use crate::fonts::Fonts;
use crate::palette;
use crate::screens::Chrome;
use crate::settings::SettingsStore;
use crate::words::tr;

const STRAPLINE: &str = "a junglist rhythm game & controller-first DAW";

/// The line under the name.
#[derive(Component)]
struct Strapline;

/// The line under the name, in the player's language.
fn speak(settings: Res<SettingsStore>, mut lines: Query<&mut Text, With<Strapline>>) {
    if !settings.is_changed() {
        return;
    }
    for mut text in &mut lines {
        text.0 = tr(settings.language(), STRAPLINE).to_owned();
    }
}

#[derive(Debug)]
pub struct TitlePlugin;

impl Plugin for TitlePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_title).add_systems(Update, speak);
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
                Strapline,
                Text::new(STRAPLINE),
                TextFont::from_font_size(16.0),
                TextColor(palette::MUTED),
            ));
        });
}
