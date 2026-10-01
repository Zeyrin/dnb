//! The SONGS screen: pick a tune, a difficulty, a practice tempo, then play.

use bevy::prelude::*;
use wu_chart::{Difficulty, auto_chart};
use wu_content::project::Song;
use wu_content::songs::BUILTIN;
use wu_input::{Button, InputKind};

use crate::fonts::Fonts;
use crate::input::RawInput;
use crate::palette;
use crate::screens::Screen;
use crate::session::{PLAYABLE, Session};
use crate::ui::{centred_label, centred_on, label, screen_root};

#[derive(Debug)]
pub struct SongsPlugin;

impl Plugin for SongsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SongLibrary::load())
            .init_resource::<Session>()
            .init_resource::<MenuRow>()
            .add_systems(OnEnter(Screen::Songs), enter)
            .add_systems(Update, (navigate, show).chain().run_if(in_state(Screen::Songs)));
    }
}

/// Every built-in song, compiled once at startup.
#[derive(Resource, Debug)]
pub struct SongLibrary {
    pub songs: Vec<Result<Song, String>>,
}

impl SongLibrary {
    fn load() -> SongLibrary {
        SongLibrary {
            songs: BUILTIN
                .iter()
                .map(|song| song.load().map_err(|e| format!("{}: {e}", song.id)))
                .collect(),
        }
    }

    pub fn get(&self, index: usize) -> Option<&Song> {
        self.songs.get(index).and_then(|s| s.as_ref().ok())
    }
}

/// Menu presses, from the D-pad and face buttons (or their keyboard keys).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKey {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
}

pub fn menu_keys(raw: &mut MessageReader<RawInput>) -> Vec<MenuKey> {
    raw.read()
        .filter_map(|RawInput(event)| match event.kind {
            InputKind::Pressed(Button::DPadUp) => Some(MenuKey::Up),
            InputKind::Pressed(Button::DPadDown) => Some(MenuKey::Down),
            InputKind::Pressed(Button::DPadLeft) => Some(MenuKey::Left),
            InputKind::Pressed(Button::DPadRight) => Some(MenuKey::Right),
            InputKind::Pressed(Button::South | Button::Start) => Some(MenuKey::Confirm),
            InputKind::Pressed(Button::East) => Some(MenuKey::Back),
            _ => None,
        })
        .collect()
}

const ROWS: usize = 4;

#[derive(Resource, Default)]
struct MenuRow(usize);

#[derive(Component)]
struct Row(usize);

#[derive(Component)]
enum Info {
    Title,
    Details,
    Chart,
}

fn enter(mut commands: Commands, fonts: Res<Fonts>, mut row: ResMut<MenuRow>) {
    // The texts are only rewritten on change: make sure the fresh ones get filled.
    row.set_changed();
    commands.spawn(screen_root(Screen::Songs)).with_children(|screen| {
        screen
            .spawn(centred_on(0.0, -175.0, 600.0, 20.0))
            .with_child(label("SELECT A TUNE", 13.0, palette::MUTED));
        screen.spawn(centred_on(0.0, -135.0, 1000.0, 50.0)).with_child((
            Info::Title,
            Text::new(""),
            TextFont {
                font: fonts.display.clone().into(),
                ..TextFont::from_font_size(34.0)
            },
            TextColor(palette::FLYER_YELLOW),
        ));
        screen
            .spawn(centred_on(0.0, -95.0, 1000.0, 22.0))
            .with_child((Info::Details, label("", 15.0, palette::MUTED)));
        for row in 0..ROWS {
            let y = -35.0 + row as f32 * 40.0;
            screen
                .spawn(centred_on(0.0, y, 700.0, 30.0))
                .with_child((Row(row), label("", 19.0, palette::INK)));
        }
        screen
            .spawn(centred_on(0.0, 150.0, 1000.0, 22.0))
            .with_child((Info::Chart, centred_label("", 14.0, palette::SIGNAL)));
        screen.spawn(centred_on(0.0, 230.0, 1000.0, 20.0)).with_child(label(
            "↑ ↓ choose · ← → change · ✕ / Space play",
            14.0,
            palette::MUTED,
        ));
    });
}

fn step<T: Copy + PartialEq>(options: &[T], current: T, by: i32) -> T {
    let i = options.iter().position(|&o| o == current).unwrap_or(0) as i32;
    options[(i + by).rem_euclid(options.len() as i32) as usize]
}

fn navigate(
    mut raw: MessageReader<RawInput>,
    mut row: ResMut<MenuRow>,
    mut session: ResMut<Session>,
    library: Res<SongLibrary>,
    mut next: ResMut<NextState<Screen>>,
) {
    for key in menu_keys(&mut raw) {
        let change = match key {
            MenuKey::Up => {
                row.0 = (row.0 + ROWS - 1) % ROWS;
                0
            }
            MenuKey::Down => {
                row.0 = (row.0 + 1) % ROWS;
                0
            }
            MenuKey::Left => -1,
            MenuKey::Right => 1,
            MenuKey::Confirm => {
                if library.get(session.song).is_some() {
                    next.set(Screen::Rhythm);
                }
                0
            }
            MenuKey::Back => 0,
        };
        if change != 0 {
            match row.0 {
                0 => session.difficulty = step(&PLAYABLE, session.difficulty, change),
                1 => {
                    let tempo = session.tempo_percent as i32 + 10 * change;
                    session.tempo_percent = tempo.clamp(50, 150) as u32;
                }
                2 => session.autoplay = !session.autoplay,
                _ => session.no_fail = !session.no_fail,
            }
        }
    }
}

fn show(
    session: Res<Session>,
    row: Res<MenuRow>,
    library: Res<SongLibrary>,
    mut rows: Query<(&Row, &mut Text, &mut TextColor), Without<Info>>,
    mut infos: Query<(&Info, &mut Text), Without<Row>>,
) {
    if !session.is_changed() && !row.is_changed() && !library.is_changed() {
        return;
    }
    let on_off = |on: bool| if on { "on" } else { "off" };
    let values = [
        ("Difficulty", session.difficulty.name().to_owned()),
        ("Tempo", format!("{} %", session.tempo_percent)),
        ("Autoplay (selecta bot)", on_off(session.autoplay).to_owned()),
        ("No-Fail", on_off(session.no_fail).to_owned()),
    ];
    for (r, mut text, mut colour) in &mut rows {
        let (name, value) = &values[r.0];
        let selected = r.0 == row.0;
        text.0 = format!("{} {name:<24} ◀ {value:^10} ▶", if selected { "›" } else { " " });
        colour.0 = if selected { palette::FLYER_YELLOW } else { palette::INK };
    }
    let song = library.get(session.song);
    for (info, mut text) in &mut infos {
        text.0 = match (info, song) {
            (Info::Title, Some(song)) => song.meta.title.clone(),
            (Info::Details, Some(song)) => {
                let seconds = song.tempo.seconds_at(song.length.0 as f64) * 100.0 / f64::from(session.tempo_percent);
                format!(
                    "{} · {:.0} BPM · {} · {} · {}:{:02}",
                    song.meta.artist,
                    song.tempo.bpm_at(wu_time::Tick::ZERO) * f64::from(session.tempo_percent) / 100.0,
                    song.meta.key,
                    song.meta.subgenre,
                    (seconds / 60.0) as u32,
                    (seconds % 60.0) as u32
                )
            }
            (Info::Chart, Some(song)) => {
                let chart = auto_chart(&song.drums, &song.tempo, session.difficulty);
                let lanes = Difficulty::rules(session.difficulty).pads.len();
                let rolls = match chart.rolls.len() {
                    0 => String::new(),
                    1 => " · 1 roll (L1 / R1 join in)".to_owned(),
                    n => format!(" · {n} rolls (L1 / R1 join in)"),
                };
                format!(
                    "{} notes on {lanes} pads{rolls} · the rest of the kit plays itself",
                    chart.notes.len()
                )
            }
            (_, None) => library
                .songs
                .get(session.song)
                .and_then(|s| s.as_ref().err())
                .cloned()
                .unwrap_or_default(),
        };
    }
}
