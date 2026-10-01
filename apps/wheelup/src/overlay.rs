//! A corner readout for development: frame rate and what the audio is doing.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use crate::audio::AudioLink;
use crate::palette;

#[derive(Debug)]
pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_overlay)
            .add_systems(Update, update_overlay);
    }
}

#[derive(Component)]
struct OverlayText;

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
    let mut lines = vec![
        format!("{fps:.0} fps"),
        format!(
            "{} · {} Hz · {} · output latency {latency_ms:.1} ms",
            info.device, info.sample_rate, buffer
        ),
    ];
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
