//! The JAM screen: the demo groove: the eight pads in the reel's
//! layout, lit by what the engine actually plays, at the moment it reaches the
//! speaker. Pads come from the controller (through the input thread) or the keyboard.

use std::collections::VecDeque;

use bevy::prelude::*;
use wu_audio::{Command, Report};
use wu_content::demo::{DEMO_BARS, DEMO_BPM, demo_program};
use wu_input::{Action, Button, Phase};
use wu_instruments::kits::RAGGA_93;
use wu_instruments::{PAD_COUNT, Pad};
use wu_time::{BEATS_PER_BAR, PPQ, Tick};

use crate::audio::{AudioLink, EngineReport};
use crate::fonts::Fonts;
use crate::input::{InputLink, PlayerAction};
use crate::palette;
use crate::screens::Screen;
use crate::settings::SettingsStore;
use crate::ui::{centred_on, label, screen_root};
use crate::words::{fill, tr};

#[derive(Debug)]
pub struct PadsPlugin {
    pub autoplay: bool,
}

impl Plugin for PadsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PadLights {
            autoplay: self.autoplay,
            ..default()
        })
        .add_systems(OnEnter(Screen::Jam), enter)
        .add_systems(
            Update,
            (transport, collect_hits, light_pads, show_position)
                .chain()
                .run_if(in_state(Screen::Jam)),
        );
    }
}

const PAD_SIZE: f32 = 92.0;
const SPREAD: f32 = 106.0;
const HAND_X: f32 = 250.0;
const CENTRE_Y: f32 = 40.0;

/// Where each button sits, relative to the centre of the screen: D-pad on the
/// left thumb, face buttons on the right, as on the controller.
const SPOTS: [(Button, f32, f32); PAD_COUNT] = [
    (Button::DPadUp, -HAND_X, CENTRE_Y - SPREAD),
    (Button::DPadDown, -HAND_X, CENTRE_Y + SPREAD),
    (Button::DPadLeft, -HAND_X - SPREAD, CENTRE_Y),
    (Button::DPadRight, -HAND_X + SPREAD, CENTRE_Y),
    (Button::North, HAND_X, CENTRE_Y - SPREAD),
    (Button::West, HAND_X - SPREAD, CENTRE_Y),
    (Button::South, HAND_X, CENTRE_Y + SPREAD),
    (Button::East, HAND_X + SPREAD, CENTRE_Y),
];

/// The keyboard key standing in for each button.
fn key_hint(button: Button) -> &'static str {
    match button {
        Button::North => "I",
        Button::West => "J",
        Button::South => "K",
        Button::East => "L",
        other => other.glyph(),
    }
}

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
    autoplay: bool,
}

fn enter(
    mut commands: Commands,
    mut audio: NonSendMut<AudioLink>,
    input: NonSend<InputLink>,
    fonts: Res<Fonts>,
    mut lights: ResMut<PadLights>,
    settings: Res<SettingsStore>,
) {
    let language = settings.language();
    let sample_rate = audio.sample_rate();
    audio.load(demo_program(sample_rate, DEMO_BPM, DEMO_BARS, true));
    if std::mem::take(&mut lights.autoplay) {
        audio.send(Command::Play);
    }
    lights.pending.iter_mut().for_each(VecDeque::clear);
    lights.heard = [None; PAD_COUNT];

    let layout = input.layout();
    commands.spawn(screen_root(Screen::Jam)).with_children(|screen| {
        for (button, x, y) in SPOTS {
            let Some(pad) = layout.pad_for(button) else { continue };
            let colour = palette::pad(pad);
            screen
                .spawn((
                    PadFace(pad),
                    Node {
                        border: UiRect::all(px(3)),
                        border_radius: BorderRadius::all(px(18)),
                        ..centred_on(x, y, PAD_SIZE, PAD_SIZE)
                    },
                    BackgroundColor(palette::dim(colour)),
                    BorderColor::all(colour),
                ))
                .with_child((
                    Text::new(button.glyph()),
                    TextFont {
                        font: fonts.bold.clone().into(),
                        ..TextFont::from_font_size(40.0)
                    },
                    TextColor(palette::INK),
                ));
            let caption = format!("{} · {}", RAGGA_93.pads[pad.index()].name, key_hint(button));
            screen
                .spawn(centred_on(x, y + PAD_SIZE / 2.0 + 14.0, 180.0, 20.0))
                .with_child(label(caption, 13.0, palette::MUTED));
        }
        for beat in 0..BEATS_PER_BAR {
            let x = (beat as f32 - 1.5) * 34.0;
            screen.spawn((
                BeatDot(beat),
                Node {
                    border_radius: BorderRadius::MAX,
                    ..centred_on(x, 245.0, 16.0, 16.0)
                },
                BackgroundColor(palette::dim(palette::FLYER_YELLOW)),
            ));
        }
        screen
            .spawn(centred_on(0.0, 280.0, 600.0, 24.0))
            .with_child((PositionText, label("", 16.0, palette::MUTED)));
        screen.spawn(centred_on(0.0, 320.0, 1000.0, 20.0)).with_child(label(
            tr(
                language,
                "Space / OPTIONS play·stop · R restart · pads: a controller, or ↑ ↓ ← → and I J K L · Esc menu",
            ),
            13.0,
            palette::MUTED,
        ));
    });
}

fn transport(
    keys: Res<ButtonInput<KeyCode>>,
    mut actions: MessageReader<PlayerAction>,
    mut audio: NonSendMut<AudioLink>,
) {
    for PlayerAction(action) in actions.read() {
        if action.action == Action::Pause && action.phase == Phase::Pressed {
            let command = if audio.playing() { Command::Stop } else { Command::Play };
            audio.send(command);
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        audio.send(Command::Seek(Tick::ZERO));
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
    audio: NonSend<AudioLink>,
    mut lights: ResMut<PadLights>,
    mut faces: Query<(&PadFace, &mut BackgroundColor)>,
) {
    let Some(now) = audio.device_frame_now() else { return };
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
    let glow_frames = GLOW_SECONDS * f64::from(audio.sample_rate());
    for (face, mut background) in &mut faces {
        let colour = palette::pad(face.0);
        let glow = lights.heard[face.0.index()].map_or(0.0, |frame| (-(now - frame as f64) / glow_frames).exp());
        background.0 = palette::mix(palette::dim(colour), colour, glow as f32);
    }
}

fn show_position(
    audio: NonSend<AudioLink>,
    settings: Res<SettingsStore>,
    mut text: Single<&mut Text, With<PositionText>>,
    mut dots: Query<(&BeatDot, &mut BackgroundColor)>,
) {
    let position = audio.tick_now().filter(|_| audio.playing());
    let (beat_in_bar, glow) = match position {
        Some(tick) => {
            let beats = tick / PPQ as f64;
            let beat = beats.floor() as i64;
            let bar = beat.div_euclid(BEATS_PER_BAR);
            let beat_in_bar = beat.rem_euclid(BEATS_PER_BAR);
            let bpm = audio.tempo.bpm_at(Tick(tick as i64));
            text.0 = fill(
                tr(settings.language(), "bar {}   beat {}   {} BPM"),
                &[&(bar + 1), &(beat_in_bar + 1), &format!("{bpm:.0}")],
            );
            (Some(beat_in_bar), (-(beats - beat as f64) * 5.0).exp() as f32)
        }
        None => {
            text.0 = tr(settings.language(), "stopped: press Space or OPTIONS").to_owned();
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
