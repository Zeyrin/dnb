//! The pad screen: the eight pads in the reel's layout, lit by what the engine
//! actually plays, at the moment it reaches the speaker. Keyboard stands in for
//! the controller until `wu-input` lands (M2).

use std::collections::VecDeque;

use bevy::prelude::*;
use wu_audio::{Command, LiveHit, Report};
use wu_instruments::{PAD_COUNT, Pad};
use wu_time::{BEATS_PER_BAR, PPQ, Tick};

use crate::audio::{AudioLink, EngineReport};
use crate::fonts::Fonts;
use crate::palette;

#[derive(Debug)]
pub struct PadsPlugin;

impl Plugin for PadsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PadLights>()
            .add_systems(Startup, spawn_pads)
            .add_systems(Update, (keyboard, collect_hits, light_pads, show_position).chain());
    }
}

/// Where a pad sits, offset from the centre of the screen, and how it is drawn.
struct Spot {
    pad: Pad,
    glyph: &'static str,
    key: &'static str,
    x: f32,
    y: f32,
}

const PAD_SIZE: f32 = 92.0;
const SPREAD: f32 = 106.0;
const HAND_X: f32 = 250.0;
const CENTRE_Y: f32 = 20.0;

/// D-pad on the left thumb, face buttons on the right, as on the controller.
const SPOTS: [Spot; PAD_COUNT] = [
    Spot {
        pad: Pad::P1,
        glyph: "↑",
        key: "↑",
        x: -HAND_X,
        y: CENTRE_Y - SPREAD,
    },
    Spot {
        pad: Pad::P2,
        glyph: "↓",
        key: "↓",
        x: -HAND_X,
        y: CENTRE_Y + SPREAD,
    },
    Spot {
        pad: Pad::P3,
        glyph: "←",
        key: "←",
        x: -HAND_X - SPREAD,
        y: CENTRE_Y,
    },
    Spot {
        pad: Pad::P4,
        glyph: "→",
        key: "→",
        x: -HAND_X + SPREAD,
        y: CENTRE_Y,
    },
    Spot {
        pad: Pad::P5,
        glyph: "△",
        key: "I",
        x: HAND_X,
        y: CENTRE_Y - SPREAD,
    },
    Spot {
        pad: Pad::P6,
        glyph: "□",
        key: "J",
        x: HAND_X - SPREAD,
        y: CENTRE_Y,
    },
    Spot {
        pad: Pad::P7,
        glyph: "✕",
        key: "K",
        x: HAND_X,
        y: CENTRE_Y + SPREAD,
    },
    Spot {
        pad: Pad::P8,
        glyph: "○",
        key: "L",
        x: HAND_X + SPREAD,
        y: CENTRE_Y,
    },
];

const KEYS: [(KeyCode, Pad); PAD_COUNT] = [
    (KeyCode::ArrowUp, Pad::P1),
    (KeyCode::ArrowDown, Pad::P2),
    (KeyCode::ArrowLeft, Pad::P3),
    (KeyCode::ArrowRight, Pad::P4),
    (KeyCode::KeyI, Pad::P5),
    (KeyCode::KeyJ, Pad::P6),
    (KeyCode::KeyK, Pad::P7),
    (KeyCode::KeyL, Pad::P8),
];

/// How long a pad glows after a hit, as a decay time constant.
const GLOW_SECONDS: f64 = 0.12;

#[derive(Component)]
struct PadFace(Pad);

#[derive(Component)]
struct BeatDot(i64);

#[derive(Component)]
struct PositionText;

/// Hits waiting to be heard, and the last one heard, per pad, as device frames.
#[derive(Resource, Default)]
struct PadLights {
    pending: [VecDeque<u64>; PAD_COUNT],
    heard: [Option<u64>; PAD_COUNT],
}

/// Centres a fixed-size, absolutely positioned node on a point relative to the screen centre.
fn centred_on(x: f32, y: f32, width: f32, height: f32) -> Node {
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

fn spawn_pads(mut commands: Commands, link: NonSend<AudioLink>, fonts: Res<Fonts>) {
    // The kit's sound names, as the engine will play them.
    let names: Vec<String> = wu_instruments::Kit::ragga_93(link.info().sample_rate)
        .pads
        .iter()
        .map(|p| p.name.clone())
        .collect();
    commands
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            ..default()
        })
        .with_children(|screen| {
            for spot in &SPOTS {
                let colour = palette::pad(spot.pad);
                screen
                    .spawn((
                        PadFace(spot.pad),
                        Node {
                            border: UiRect::all(px(3)),
                            border_radius: BorderRadius::all(px(18)),
                            ..centred_on(spot.x, spot.y, PAD_SIZE, PAD_SIZE)
                        },
                        BackgroundColor(palette::dim(colour)),
                        BorderColor::all(colour),
                    ))
                    .with_child((
                        Text::new(spot.glyph),
                        TextFont {
                            font: fonts.bold.clone().into(),
                            ..TextFont::from_font_size(40.0)
                        },
                        TextColor(palette::INK),
                    ));
                let label = format!(
                    "{} · {}",
                    names.get(spot.pad.index()).map_or("", String::as_str),
                    spot.key
                );
                screen
                    .spawn(centred_on(spot.x, spot.y + PAD_SIZE / 2.0 + 14.0, 180.0, 20.0))
                    .with_child((
                        Text::new(label),
                        TextFont::from_font_size(13.0),
                        TextColor(palette::MUTED),
                    ));
            }
            for beat in 0..BEATS_PER_BAR {
                let x = (beat as f32 - 1.5) * 34.0;
                screen.spawn((
                    BeatDot(beat),
                    Node {
                        border_radius: BorderRadius::MAX,
                        ..centred_on(x, 250.0, 16.0, 16.0)
                    },
                    BackgroundColor(palette::dim(palette::FLYER_YELLOW)),
                ));
            }
            screen.spawn(centred_on(0.0, 285.0, 600.0, 24.0)).with_child((
                PositionText,
                Text::new(""),
                TextFont::from_font_size(16.0),
                TextColor(palette::MUTED),
            ));
            screen.spawn(centred_on(0.0, 330.0, 900.0, 20.0)).with_child((
                Text::new("Space play/stop · R restart · ↑ ↓ ← → and I J K L play pads · Esc quit"),
                TextFont::from_font_size(13.0),
                TextColor(palette::MUTED),
            ));
        });
}

fn keyboard(keys: Res<ButtonInput<KeyCode>>, mut link: NonSendMut<AudioLink>, mut exit: MessageWriter<AppExit>) {
    let now = wu_time::mono::now_ns();
    for (key, pad) in KEYS {
        if keys.just_pressed(key)
            && !link.live.hit(LiveHit {
                pad,
                velocity: 1.0,
                at_ns: now,
            })
        {
            warn!("live hit dropped: queue full");
        }
    }
    if keys.just_pressed(KeyCode::Space) {
        let command = if link.playing() { Command::Stop } else { Command::Play };
        link.send(command);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        link.send(Command::Seek(Tick::ZERO));
    }
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}

fn collect_hits(mut reports: MessageReader<EngineReport>, mut lights: ResMut<PadLights>) {
    for EngineReport(report) in reports.read() {
        if let Report::VoiceStarted(start) = report {
            let queue = &mut lights.pending[start.pad.index()];
            if queue.len() >= 64 {
                queue.pop_front();
            }
            queue.push_back(start.device_frame);
        }
    }
}

fn light_pads(
    link: NonSend<AudioLink>,
    mut lights: ResMut<PadLights>,
    mut faces: Query<(&PadFace, &mut BackgroundColor)>,
) {
    let Some(now) = link.device_frame_now() else { return };
    let lights = &mut *lights;
    for (pending, heard) in lights.pending.iter_mut().zip(lights.heard.iter_mut()) {
        while let Some(&frame) = pending.front() {
            if frame as f64 > now {
                break;
            }
            *heard = Some(frame);
            pending.pop_front();
        }
    }
    let glow_frames = GLOW_SECONDS * f64::from(link.info().sample_rate);
    for (face, mut background) in &mut faces {
        let colour = palette::pad(face.0);
        let glow = lights.heard[face.0.index()].map_or(0.0, |frame| (-(now - frame as f64) / glow_frames).exp());
        background.0 = palette::mix(palette::dim(colour), colour, glow as f32);
    }
}

fn show_position(
    link: NonSend<AudioLink>,
    mut text: Single<&mut Text, With<PositionText>>,
    mut dots: Query<(&BeatDot, &mut BackgroundColor)>,
) {
    let position = link.tick_now().filter(|_| link.playing());
    let (beat_in_bar, glow) = match position {
        Some(tick) => {
            let beats = tick / PPQ as f64;
            let beat = beats.floor() as i64;
            let bar = beat.div_euclid(BEATS_PER_BAR);
            let beat_in_bar = beat.rem_euclid(BEATS_PER_BAR);
            text.0 = format!(
                "bar {}   beat {}   {:.0} BPM",
                bar + 1,
                beat_in_bar + 1,
                link.tempo.bpm_at(Tick(tick as i64))
            );
            (Some(beat_in_bar), (-(beats - beat as f64) * 5.0).exp() as f32)
        }
        None => {
            text.0 = "stopped: press Space".to_owned();
            (None, 0.0)
        }
    };
    for (dot, mut background) in &mut dots {
        let level = if Some(dot.0) == beat_in_bar {
            0.35 + 0.65 * glow
        } else {
            0.0
        };
        background.0 = palette::mix(palette::dim(palette::FLYER_YELLOW), palette::FLYER_YELLOW, level);
    }
}
