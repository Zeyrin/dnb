//! Screens, and the tab bar that switches between them (Tab, or the
//! controller's Create button). Each screen decides whether pads sound live.

use bevy::prelude::*;
use wu_input::{Action, Phase};

use wu_content::settings::AudioMode;

use crate::input::{InputLink, PlayerAction};
use crate::palette;
use crate::session::Session;
use crate::settings::SettingsStore;
use crate::songs_screen::SongLibrary;
use crate::words::tr;

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Screen {
    /// The photosensitivity notice, every launch.
    Notice,
    #[default]
    Songs,
    /// The Pirate Radio Tour.
    Tour,
    Settings,
    Jam,
    Controller,
    Calibrate,
    /// Playing a chart. Reached from Songs, not from the tab bar.
    Rhythm,
    Results,
}

impl Screen {
    /// The screens on the tab bar, in order.
    const TABS: [Screen; 6] = [
        Screen::Songs,
        Screen::Tour,
        Screen::Jam,
        Screen::Controller,
        Screen::Calibrate,
        Screen::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Screen::Notice | Screen::Songs | Screen::Rhythm | Screen::Results => "SONGS",
            Screen::Tour => "TOUR",
            Screen::Settings => "SETTINGS",
            Screen::Jam => "JAM",
            Screen::Controller => "CONTROLLER",
            Screen::Calibrate => "CALIBRATE",
        }
    }

    /// The tab a screen belongs to.
    fn tab(self) -> Screen {
        match self {
            Screen::Notice | Screen::Rhythm | Screen::Results => Screen::Songs,
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
            Screen::Notice | Screen::Songs | Screen::Tour | Screen::Calibrate | Screen::Settings | Screen::Results => {
                false
            }
        }
    }

    const ALL: [Screen; 9] = [
        Screen::Notice,
        Screen::Songs,
        Screen::Tour,
        Screen::Settings,
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
            .add_systems(Update, (switch_screens, highlight_tabs, quit_on_escape, show_chrome));
        for screen in Screen::ALL {
            app.add_systems(
                OnEnter(screen),
                move |mut input: NonSendMut<InputLink>,
                      session: Res<Session>,
                      settings: Res<SettingsStore>,
                      library: Res<SongLibrary>| {
                    // An imported tune is its own recording: presses never sound over it.
                    let recorded = library.get(session.song).is_some_and(|song| song.recording.is_some());
                    let live = screen.live(session.autoplay, settings.audio_mode());
                    input.set_live(live && !(recorded && screen == Screen::Rhythm));
                },
            );
        }
    }
}

#[derive(Component)]
struct Tab(Screen);

/// The header and the tab bar: hidden while a song plays, so the highway has
/// the whole screen.
#[derive(Component)]
pub struct Chrome;

fn spawn_tabs(mut commands: Commands) {
    commands
        .spawn((
            Chrome,
            Node {
                position_type: PositionType::Absolute,
                top: px(132),
                width: percent(100),
                justify_content: JustifyContent::Center,
                column_gap: px(28),
                ..default()
            },
        ))
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
        // While playing, CREATE belongs to the rhythm screen (it quits the run);
        // the notice is read before anything else.
        let own = matches!(current.get(), Screen::Rhythm | Screen::Notice);
        if action.action == Action::Select && action.phase == Phase::Pressed && !own {
            next.set(current.get().next());
        }
    }
}

fn highlight_tabs(
    current: Res<State<Screen>>,
    settings: Res<SettingsStore>,
    mut tabs: Query<(&Tab, &mut Text, &mut TextColor)>,
) {
    let language = settings.language();
    for (tab, mut text, mut colour) in &mut tabs {
        let name = tr(language, tab.0.label());
        if text.0 != name {
            text.0 = name.to_owned();
        }
        colour.0 = if tab.0 == current.get().tab() {
            palette::FLYER_YELLOW
        } else {
            palette::MUTED
        };
    }
}

fn show_chrome(current: Res<State<Screen>>, mut chrome: Query<&mut Visibility, With<Chrome>>) {
    let shown = if matches!(current.get(), Screen::Rhythm | Screen::Notice) {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut visibility in &mut chrome {
        visibility.set_if_neq(shown);
    }
}

fn quit_on_escape(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
