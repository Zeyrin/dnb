//! The menu Esc pulls up over any screen (OPTIONS too, in a song): carry on,
//! start the song again, leave it, or quit the game. While it is up the screen
//! behind it takes no menu keys, and a song stands paused.

use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;
use wu_input::{Button, InputKind};

use crate::input::RawInput;
use crate::palette;
use crate::screens::Screen;
use crate::session::Session;
use crate::settings::SettingsStore;
use crate::ui::{centred_label, label};
use crate::words::tr;

/// Whether the menu is up. Shared with the screens' own menu keys (which it
/// silences) and the song (which it pauses).
static OPEN: AtomicBool = AtomicBool::new(false);

pub fn is_open() -> bool {
    OPEN.load(Ordering::Relaxed)
}

pub fn set_open(open: bool) {
    if OPEN.swap(open, Ordering::Relaxed) && !open {
        crate::songs_screen::start_fresh();
    }
}

#[derive(Debug)]
pub struct EscMenuPlugin;

impl Plugin for EscMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Choice>()
            .add_systems(Startup, spawn)
            .add_systems(Update, (open_on_escape, choose, show).chain());
    }
}

/// What the menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Resume,
    Restart,
    LeaveSong,
    Quit,
}

impl Item {
    fn name(self) -> &'static str {
        match self {
            Item::Resume => "Resume",
            Item::Restart => "Restart the song",
            Item::LeaveSong => "Leave the song",
            Item::Quit => "Quit WHEEL UP!",
        }
    }
}

/// The items on `screen`: a song can be started again or left.
fn items(screen: Screen) -> &'static [Item] {
    if screen == Screen::Rhythm {
        &[Item::Resume, Item::Restart, Item::LeaveSong, Item::Quit]
    } else {
        &[Item::Resume, Item::Quit]
    }
}

/// The highlighted item.
#[derive(Resource, Default)]
struct Choice(usize);

#[derive(Component)]
struct Overlay;

#[derive(Component)]
enum Part {
    Heading,
    Item(usize),
}

fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Overlay,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(14),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.02, 0.82)),
            GlobalZIndex(100),
            Visibility::Hidden,
        ))
        .with_children(|menu| {
            menu.spawn((Part::Heading, centred_label("", 34.0, palette::FLYER_YELLOW)));
            for i in 0..4 {
                menu.spawn((Part::Item(i), label("", 22.0, palette::INK)));
            }
        });
}

/// Esc pulls the menu up, or puts it away.
fn open_on_escape(keys: Res<ButtonInput<KeyCode>>, mut choice: ResMut<Choice>) {
    if keys.just_pressed(KeyCode::Escape) {
        set_open(!is_open());
        choice.0 = 0;
    }
}

fn choose(
    mut raw: MessageReader<RawInput>,
    mut choice: ResMut<Choice>,
    screen: Res<State<Screen>>,
    session: Res<Session>,
    mut next: ResMut<NextState<Screen>>,
    mut exit: MessageWriter<AppExit>,
) {
    if !is_open() {
        raw.clear();
        return;
    }
    let offered = items(*screen.get());
    for RawInput(event) in raw.read() {
        match event.kind {
            InputKind::Pressed(Button::DPadUp) => choice.0 = (choice.0 + offered.len() - 1) % offered.len(),
            InputKind::Pressed(Button::DPadDown) => choice.0 = (choice.0 + 1) % offered.len(),
            InputKind::Pressed(Button::East) => set_open(false),
            InputKind::Pressed(Button::South) => {
                set_open(false);
                match offered[choice.0.min(offered.len() - 1)] {
                    Item::Resume => {}
                    Item::Restart => next.set(Screen::Rhythm),
                    Item::LeaveSong => next.set(if session.from_tour { Screen::Tour } else { Screen::Songs }),
                    Item::Quit => {
                        exit.write(AppExit::Success);
                    }
                }
                return;
            }
            _ => {}
        }
    }
}

fn show(
    choice: Res<Choice>,
    screen: Res<State<Screen>>,
    settings: Res<SettingsStore>,
    mut overlay: Query<&mut Visibility, With<Overlay>>,
    mut parts: Query<(&Part, &mut Text, &mut TextColor)>,
) {
    let open = is_open();
    for mut visibility in &mut overlay {
        *visibility = if open { Visibility::Visible } else { Visibility::Hidden };
    }
    if !open {
        return;
    }
    let language = settings.language();
    let offered = items(*screen.get());
    for (part, mut text, mut colour) in &mut parts {
        match part {
            Part::Heading => text.0 = tr(language, "PAUSED").to_owned(),
            Part::Item(i) => match offered.get(*i) {
                Some(item) => {
                    let selected = *i == choice.0;
                    text.0 = format!("{} {}", if selected { "›" } else { " " }, tr(language, item.name()));
                    colour.0 = if selected { palette::FLYER_YELLOW } else { palette::INK };
                }
                None => text.0.clear(),
            },
        }
    }
}
