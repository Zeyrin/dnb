//! Screens, and the tab bar that switches between them (Tab, or the
//! controller's Create button).

use bevy::prelude::*;
use wu_input::{Action, Phase};

use crate::input::{InputLink, PlayerAction};
use crate::palette;

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Screen {
    #[default]
    Play,
    Controller,
    Calibrate,
}

impl Screen {
    const ALL: [Screen; 3] = [Screen::Play, Screen::Controller, Screen::Calibrate];

    fn label(self) -> &'static str {
        match self {
            Screen::Play => "PLAY",
            Screen::Controller => "CONTROLLER",
            Screen::Calibrate => "CALIBRATE",
        }
    }

    fn next(self) -> Screen {
        let i = Screen::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Screen::ALL[(i + 1) % Screen::ALL.len()]
    }
}

#[derive(Debug)]
pub struct ScreensPlugin {
    pub start: Screen,
}

impl Plugin for ScreensPlugin {
    fn build(&self, app: &mut App) {
        app.insert_state(self.start)
            .add_systems(Startup, spawn_tabs)
            .add_systems(Update, (switch_screens, highlight_tabs, quit_on_escape))
            .add_systems(OnEnter(Screen::Calibrate), |mut input: NonSendMut<InputLink>| {
                input.set_live(false)
            })
            .add_systems(OnExit(Screen::Calibrate), |mut input: NonSendMut<InputLink>| {
                input.set_live(true)
            });
    }
}

#[derive(Component)]
struct Tab(Screen);

fn spawn_tabs(mut commands: Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: px(132),
            width: percent(100),
            justify_content: JustifyContent::Center,
            column_gap: px(28),
            ..default()
        })
        .with_children(|bar| {
            for screen in Screen::ALL {
                bar.spawn((Tab(screen), Text::new(screen.label()), TextFont::from_font_size(15.0)));
            }
            bar.spawn((
                Text::new("Tab / CREATE"),
                TextFont::from_font_size(12.0),
                TextColor(palette::MUTED),
                Node {
                    margin: UiRect::top(px(2)),
                    ..default()
                },
            ));
        });
}

fn switch_screens(
    mut actions: MessageReader<PlayerAction>,
    current: Res<State<Screen>>,
    mut next: ResMut<NextState<Screen>>,
) {
    for PlayerAction(action) in actions.read() {
        if action.action == Action::Select && action.phase == Phase::Pressed {
            next.set(current.get().next());
        }
    }
}

fn highlight_tabs(current: Res<State<Screen>>, mut tabs: Query<(&Tab, &mut TextColor)>) {
    for (tab, mut colour) in &mut tabs {
        colour.0 = if tab.0 == *current.get() {
            palette::FLYER_YELLOW
        } else {
            palette::MUTED
        };
    }
}

fn quit_on_escape(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
