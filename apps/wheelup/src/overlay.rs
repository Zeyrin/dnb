//! A corner readout for development: frame rate and what the audio is doing.
//! F3 shows it; a warning the player must see (no sound, Bluetooth latency)
//! shows anyway.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use crate::audio::AudioLink;
use crate::palette;

#[derive(Debug)]
pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverlayShown>()
            .add_systems(Startup, spawn_overlay)
            .add_systems(Update, (toggle_overlay, update_overlay).chain());
    }
}

#[derive(Component)]
struct OverlayText;

/// Whether the whole readout is up (F3), not just the warnings.
#[derive(Resource, Default)]
struct OverlayShown(bool);

fn toggle_overlay(keys: Res<ButtonInput<KeyCode>>, mut shown: ResMut<OverlayShown>) {
    if keys.just_pressed(KeyCode::F3) {
        shown.0 = !shown.0;
    }
}

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        OverlayText,
        Text::new(""),
        TextFont::from_font_size(13.0),
        TextColor(palette::SIGNAL),
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            top: px(10),
            ..default()
        },
    ));
}

fn update_overlay(
    diagnostics: Res<DiagnosticsStore>,
    shown: Res<OverlayShown>,
    link: NonSend<AudioLink>,
    mut overlay: Single<(&mut Text, &mut TextColor), With<OverlayText>>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let info = link.info();
    let latency_ms = link.estimator.last().map_or(0.0, |s| s.output_latency_ns as f64 / 1e6);
    let buffer = info
        .buffer_frames
        .map_or("driver default".to_owned(), |b| format!("{b} frames"));
    let mut lines = Vec::new();
    if shown.0 {
        lines.push(format!("{fps:.0} fps"));
        lines.push(format!(
            "{} · {} Hz · {} · output latency {latency_ms:.1} ms",
            info.device, info.sample_rate, buffer
        ));
    }
    let mut colour = palette::SIGNAL;
    if let Some(reason) = &link.fallback {
        lines.push(format!("no sound: {reason}"));
        colour = palette::WARNING;
    }
    if info.bluetooth {
        lines.push("Bluetooth output: 100 ms+ of latency, use a wired output to play".to_owned());
        colour = palette::WARNING;
    }
    let (text, text_colour) = &mut *overlay;
    text.0 = lines.join("\n");
    text_colour.0 = colour;
}
