//! Screens, and the tab bar that switches between them (Tab, or the
//! controller's Create button). Each screen decides whether pads sound live.

use bevy::prelude::*;
use wu_input::{Action, Phase};

use wu_content::settings::AudioMode;

use crate::input::{InputLink, PlayerAction};
use crate::palette;
use crate::session::Session;
use crate::settings::SettingsStore;

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Screen {
    #[default]
    Songs,
    Jam,
    Controller,
    Calibrate,
    /// Playing a chart. Reached from Songs, not from the tab bar.
    Rhythm,
    Results,
}

impl Screen {
    /// The screens on the tab bar, in order.
    const TABS: [Screen; 4] = [Screen::Songs, Screen::Jam, Screen::Controller, Screen::Calibrate];

    fn label(self) -> &'static str {
        match self {
            Screen::Songs | Screen::Rhythm | Screen::Results => "SONGS",
            Screen::Jam => "JAM",
            Screen::Controller => "CONTROLLER",
            Screen::Calibrate => "CALIBRATE",
        }
    }

    /// The tab a screen belongs to.
    fn tab(self) -> Screen {
        match self {
            Screen::Rhythm | Screen::Results => Screen::Songs,
            other => other,
        }
    }

    fn next(self) -> Screen {
        let i = Screen::TABS.iter().position(|&s| s == self.tab()).unwrap_or(0);
        Screen::TABS[(i + 1) % Screen::TABS.len()]
    }

    /// Whether pad presses sound straight away here. Menus stay silent, and so
    /// does calibration (a click under the thumb would bias the taps), and so
    /// does a song in Classic audio, where the song itself plays the part.
    fn live(self, autoplay: bool, mode: AudioMode) -> bool {
        match self {
            Screen::Jam | Screen::Controller => true,
            Screen::Rhythm => !autoplay && mode == AudioMode::Live,
            Screen::Songs | Screen::Calibrate | Screen::Results => false,
        }
    }

    const ALL: [Screen; 6] = [
        Screen::Songs,
        Screen::Jam,
        Screen::Controller,
        Screen::Calibrate,
        Screen::Rhythm,
        Screen::Results,
    ];
}

#[derive(Debug)]
pub struct ScreensPlugin {
    pub start: Screen,
}

impl Plugin for ScreensPlugin {
    fn build(&self, app: &mut App) {
        app.insert_state(self.start)
            .add_systems(Startup, spawn_tabs)
            .add_systems(Update, (switch_screens, highlight_tabs, quit_on_escape));
        for screen in Screen::ALL {
            app.add_systems(
                OnEnter(screen),
                move |mut input: NonSendMut<InputLink>, session: Res<Session>, settings: Res<SettingsStore>| {
                    input.set_live(screen.live(session.autoplay, settings.audio_mode()));
                },
            );
        }
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
            for screen in Screen::TABS {
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
        // While playing, CREATE belongs to the rhythm screen (it quits the run).
        if action.action == Action::Select && action.phase == Phase::Pressed && *current.get() != Screen::Rhythm {
            next.set(current.get().next());
        }
    }
}

fn highlight_tabs(current: Res<State<Screen>>, mut tabs: Query<(&Tab, &mut TextColor)>) {
    for (tab, mut colour) in &mut tabs {
        colour.0 = if tab.0 == current.get().tab() {
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
