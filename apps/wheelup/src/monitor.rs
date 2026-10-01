//! The Controller screen: the reel's overlay, rebuilt. Each button's pad and
//! MIDI note, live stick and trigger readouts, the controller's report rate and
//! jitter, and a log of the latest events with their timestamps.

use bevy::prelude::*;
use wu_input::{Axis, Button, Family, Hand, InputKind, KEYBOARD, Layout};
use wu_instruments::{Kit, Pad};

use crate::audio::AudioLink;
use crate::input::{InputLink, PlayerAction};
use crate::palette;
use crate::screens::Screen;
use crate::settings::SettingsStore;
use crate::ui::{centred_on, label, screen_root};

#[derive(Debug)]
pub struct MonitorPlugin;

impl Plugin for MonitorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Controller), enter).add_systems(
            Update,
            (switch_layout, show_buttons, show_sticks, show_texts)
                .chain()
                .run_if(in_state(Screen::Controller)),
        );
    }
}

/// The reel's note for each pad: the General MIDI drum map, notes 36–43.
pub fn midi_note(pad: Pad) -> u8 {
    [36, 38, 37, 39, 40, 41, 42, 43][pad.index()]
}

/// Note names the way most DAWs print them (middle C = C3), as in the video.
pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[usize::from(note % 12)], i32::from(note / 12) - 2)
}

const ROW_HEIGHT: f32 = 38.0;
const PANEL_X: f32 = 430.0;
const PANEL_Y: f32 = -60.0;
const STICK_X: f32 = 120.0;
const STICK_Y: f32 = -40.0;
const STICK_SIZE: f32 = 116.0;

/// The reel's row order: the D-pad on the left, the face buttons on the right.
const LEFT_ROWS: [Button; 4] = [Button::DPadUp, Button::DPadDown, Button::DPadLeft, Button::DPadRight];
const RIGHT_ROWS: [Button; 4] = [Button::North, Button::West, Button::East, Button::South];

#[derive(Component)]
struct Row(Button);

#[derive(Component)]
struct StickDot(Hand);

#[derive(Component)]
struct StickText(Hand);

#[derive(Component)]
struct TriggerFill(Hand);

#[derive(Component)]
struct Shoulder(Hand);

#[derive(Component)]
enum Readout {
    Device,
    Stats,
    Log,
    Layout,
}

fn enter(mut commands: Commands, audio: NonSend<AudioLink>, input: NonSend<InputLink>) {
    let kit = Kit::ragga_93(audio.sample_rate());
    let layout = input.layout();
    commands.spawn(screen_root(Screen::Controller)).with_children(|screen| {
        screen
            .spawn(centred_on(0.0, -192.0, 1100.0, 22.0))
            .with_child((Readout::Device, label("", 15.0, palette::INK)));
        for (rows, x) in [(LEFT_ROWS, -PANEL_X), (RIGHT_ROWS, PANEL_X)] {
            for (i, button) in rows.into_iter().enumerate() {
                let y = PANEL_Y + (i as f32 - 1.5) * ROW_HEIGHT;
                let pad = layout.pad_for(button).unwrap_or(Pad::P1);
                let text = format!(
                    "{:<2} {:<4} {}",
                    button.glyph(),
                    note_name(midi_note(pad)),
                    kit.pad(pad).name
                );
                screen
                    .spawn((
                        Row(button),
                        Node {
                            border: UiRect::all(px(2)),
                            border_radius: BorderRadius::all(px(6)),
                            justify_content: JustifyContent::FlexStart,
                            padding: UiRect::left(px(12)),
                            ..centred_on(x, y, 250.0, ROW_HEIGHT - 6.0)
                        },
                        BorderColor::all(palette::pad(pad)),
                        BackgroundColor(palette::dim(palette::pad(pad))),
                    ))
                    .with_child(label(text, 15.0, palette::INK));
            }
        }
        for (hand, side) in [(Hand::Left, -1.0), (Hand::Right, 1.0)] {
            let x = side * STICK_X;
            let name = if hand == Hand::Left { "L" } else { "R" };
            screen
                .spawn((
                    Shoulder(hand),
                    Node {
                        border_radius: BorderRadius::all(px(5)),
                        ..centred_on(x - 34.0, -140.0, 44.0, 22.0)
                    },
                    BackgroundColor(palette::dim(palette::INK)),
                ))
                .with_child(label(format!("{name}1"), 12.0, palette::INK));
            screen
                .spawn((
                    Node {
                        border_radius: BorderRadius::all(px(4)),
                        justify_content: JustifyContent::FlexStart,
                        ..centred_on(x + 24.0, -140.0, 64.0, 12.0)
                    },
                    BackgroundColor(palette::dim(palette::INK)),
                ))
                .with_child((
                    TriggerFill(hand),
                    Node {
                        width: percent(0),
                        height: percent(100),
                        border_radius: BorderRadius::all(px(4)),
                        ..default()
                    },
                    BackgroundColor(palette::FLYER_YELLOW),
                ));
            screen.spawn(centred_on(x + 24.0, -157.0, 64.0, 14.0)).with_child(label(
                format!("{name}2"),
                11.0,
                palette::MUTED,
            ));
            screen
                .spawn((
                    Node {
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::MAX,
                        ..centred_on(x, STICK_Y, STICK_SIZE, STICK_SIZE)
                    },
                    BorderColor::all(palette::MUTED),
                ))
                .with_child((
                    StickDot(hand),
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(14),
                        height: px(14),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(palette::FLYER_YELLOW),
                ));
            screen
                .spawn(centred_on(x, STICK_Y + STICK_SIZE / 2.0 + 18.0, 220.0, 18.0))
                .with_child((StickText(hand), label("", 13.0, palette::MUTED)));
        }
        screen
            .spawn(centred_on(0.0, 100.0, 1100.0, 20.0))
            .with_child((Readout::Stats, label("", 14.0, palette::SIGNAL)));
        screen
            .spawn(centred_on(0.0, 215.0, 900.0, 190.0))
            .with_child((Readout::Log, label("", 13.0, palette::MUTED)));
        screen
            .spawn(centred_on(0.0, 325.0, 1100.0, 18.0))
            .with_child((Readout::Layout, label("", 13.0, palette::MUTED)));
    });
}

/// L3 (or X on the keyboard) swaps between the Reel and Drummer layouts.
fn switch_layout(
    keys: Res<ButtonInput<KeyCode>>,
    mut raw: MessageReader<crate::input::RawInput>,
    mut input: NonSendMut<InputLink>,
    mut settings: ResMut<SettingsStore>,
    mut next: ResMut<NextState<Screen>>,
) {
    let pressed_l3 = raw
        .read()
        .any(|e| e.0.kind == InputKind::Pressed(Button::L3) && e.0.device != KEYBOARD);
    if pressed_l3 || keys.just_pressed(KeyCode::KeyX) {
        let layout = match input.layout() {
            Layout::Reel => Layout::Drummer,
            Layout::Drummer => Layout::Reel,
        };
        input.set_layout(layout);
        settings.settings.layout = layout.name().to_owned();
        settings.save();
        // Re-enter the screen so every row shows its new pad.
        next.set(Screen::Controller);
    }
}

fn show_buttons(
    input: NonSend<InputLink>,
    mut actions: MessageReader<PlayerAction>,
    mut rows: Query<(&Row, &mut BackgroundColor), Without<Shoulder>>,
    mut shoulders: Query<(&Shoulder, &mut BackgroundColor), Without<Row>>,
) {
    actions.clear();
    let state = input.states.get(&input.focus()).cloned().unwrap_or_default();
    for (row, mut background) in &mut rows {
        let colour = input.layout().pad_for(row.0).map_or(palette::INK, palette::pad);
        background.0 = if state.is_held(row.0) {
            colour
        } else {
            palette::dim(colour)
        };
    }
    for (shoulder, mut background) in &mut shoulders {
        let button = if shoulder.0 == Hand::Left {
            Button::L1
        } else {
            Button::R1
        };
        background.0 = if state.is_held(button) {
            palette::INK
        } else {
            palette::dim(palette::INK)
        };
    }
}

fn show_sticks(
    input: NonSend<InputLink>,
    mut dots: Query<(&StickDot, &mut Node), Without<TriggerFill>>,
    mut fills: Query<(&TriggerFill, &mut Node), Without<StickDot>>,
    mut texts: Query<(&StickText, &mut Text)>,
) {
    let state = input.states.get(&input.focus()).cloned().unwrap_or_default();
    let stick = |hand: Hand| match hand {
        Hand::Left => (state.axis(Axis::LeftX), state.axis(Axis::LeftY)),
        Hand::Right => (state.axis(Axis::RightX), state.axis(Axis::RightY)),
    };
    for (dot, mut node) in &mut dots {
        let (x, y) = stick(dot.0);
        node.left = percent(50.0 + 44.0 * x.clamp(-1.0, 1.0));
        node.top = percent(50.0 - 44.0 * y.clamp(-1.0, 1.0));
        node.margin = UiRect {
            left: px(-7),
            top: px(-7),
            ..default()
        };
    }
    for (fill, mut node) in &mut fills {
        let value = state.axis(if fill.0 == Hand::Left { Axis::L2 } else { Axis::R2 });
        node.width = percent(100.0 * value.clamp(0.0, 1.0));
    }
    for (text, mut content) in &mut texts {
        let (x, y) = stick(text.0);
        content.0 = format!("X: {x:+.3}  Y: {y:+.3}");
    }
}

fn show_texts(input: NonSend<InputLink>, mut readouts: Query<(&Readout, &mut Text)>) {
    let focus = input.focus();
    let devices = input.devices();
    let backend = match input.backend() {
        Ok(name) => name,
        Err(error) => format!("no controller backend: {error}"),
    };
    let device = devices.iter().find(|d| d.id == focus).or(devices.first());
    let badge = |family: Family| match family {
        Family::PlayStation => "PS",
        Family::Xbox => "XBOX",
        Family::Nintendo => "SWITCH",
        Family::Steam => "STEAM",
        Family::Generic => "PAD",
        Family::Keyboard => "KEYS",
    };
    let device_line = match device {
        Some(device) => format!(
            "[{}]  {}  ·  {}  ·  {} connected",
            badge(device.family),
            device,
            backend,
            devices.len()
        ),
        None => format!("No controller found ({backend}). Keyboard: ↑ ↓ ← → and I J K L, E/O = L1/R1, Z/N = L2/R2"),
    };
    let stats = &input.stats;
    let stats_line = match (stats.quantile_ms(0.5), stats.quantile_ms(0.95), stats.rate_hz()) {
        (Some(median), Some(p95), Some(rate)) => format!(
            "event interval: median {median:.2} ms · 95th percentile {p95:.2} ms · ≈ {rate:.0} reports/s ({} samples)",
            stats.samples()
        ),
        _ => "move a stick to measure the controller's report rate and jitter".to_owned(),
    };
    let first = input.log.front().map_or(0, |e| e.at_ns);
    let log: Vec<String> = input
        .log
        .iter()
        .rev()
        .map(|e| {
            let what = match e.kind {
                InputKind::Pressed(b) => format!("pressed  {}", b.glyph()),
                InputKind::Released(b) => format!("released {}", b.glyph()),
                InputKind::Connected => "connected".to_owned(),
                InputKind::Disconnected => "disconnected".to_owned(),
                InputKind::Axis(axis, value) => format!("{axis:?} {value:+.3}"),
            };
            let who = if e.device == KEYBOARD {
                "keys".to_owned()
            } else {
                format!("#{}", e.device.0)
            };
            format!("+{:>9.3} ms  {who:<5} {what}", (e.at_ns - first) as f64 / 1e6)
        })
        .collect();
    let layout_line = format!(
        "layout: {} · L3 (or X) switches · notes follow the reel: the General MIDI drum map",
        input.layout().name()
    );
    for (readout, mut text) in &mut readouts {
        text.0 = match readout {
            Readout::Device => device_line.clone(),
            Readout::Stats => stats_line.clone(),
            Readout::Log => log.join("\n"),
            Readout::Layout => layout_line.clone(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_match_the_reel() {
        let names: Vec<String> = Pad::ALL.iter().map(|&p| note_name(midi_note(p))).collect();
        assert_eq!(names, ["C1", "D1", "C#1", "D#1", "E1", "F1", "F#1", "G1"]);
        assert_eq!(note_name(60), "C3");
    }
}
