//! The SONGS screen: pick a tune, a difficulty, a practice tempo, then play.

use bevy::prelude::*;
use wu_chart::{Difficulty, auto_chart};
use wu_content::project::Song;
use wu_content::songs::BUILTIN;
use wu_input::{Button, InputKind};

use wu_content::settings::{AudioMode, NOTE_SPEEDS};

use crate::audio::AudioLink;
use crate::fonts::Fonts;
use crate::highway::{glowing, see_through, spawn_vinyl};
use crate::imports::{Importing, Recordings};
use crate::input::RawInput;
use crate::palette;
use crate::preview::Preview;
use crate::records::{RecordsStore, describe};
use crate::screens::Screen;
use crate::session::{PLAYABLE, Session};
use crate::settings::SettingsStore;
use crate::stage::StageMood;
use crate::ui::{centred_label, centred_on, label, screen_root};

#[derive(Debug)]
pub struct SongsPlugin;

impl Plugin for SongsPlugin {
    fn build(&self, app: &mut App) {
        let library = SongLibrary::load();
        // The song asked for on the command line, if the library has it; else
        // the lesson for a new player (no record yet), the first tune for anyone else.
        let wanted = app
            .world()
            .get_resource::<WantedSong>()
            .and_then(|w| w.0.as_deref())
            .and_then(|id| library.songs.iter().position(|s| s.id == id));
        let new_player = app
            .world()
            .get_resource::<RecordsStore>()
            .is_none_or(RecordsStore::is_empty);
        let first = library
            .songs
            .iter()
            .position(|s| s.song.as_ref().is_ok_and(|song| song.is_lesson() == new_player))
            .unwrap_or(0);
        let song = wanted.unwrap_or(first);
        // The section asked for, by name, to practise.
        let practice = app
            .world()
            .get_resource::<WantedPractice>()
            .and_then(|w| w.0.as_deref())
            .and_then(|name| {
                library
                    .get(song)?
                    .sections
                    .iter()
                    .position(|(section, _, _)| section.eq_ignore_ascii_case(name))
            });
        if let Some(mut session) = app.world_mut().get_resource_mut::<Session>() {
            session.song = song;
            session.practice = practice;
        }
        app.insert_resource(library)
            .init_resource::<Session>()
            .init_resource::<MenuRow>()
            .add_systems(OnEnter(Screen::Songs), (enter, spawn_record))
            .add_systems(
                Update,
                (navigate, show, spin_record).chain().run_if(in_state(Screen::Songs)),
            );
    }
}

/// The song the game was asked to start on, by id.
#[derive(Resource, Debug, Default)]
pub struct WantedSong(pub Option<String>);

/// The section of it the game was asked to practise, by name.
#[derive(Resource, Debug, Default)]
pub struct WantedPractice(pub Option<String>);

/// Every song the player can pick: the built-in ones, compiled once at
/// startup (the lessons first), then the tunes they imported.
#[derive(Resource, Debug)]
pub struct SongLibrary {
    pub songs: Vec<LibrarySong>,
}

/// A song in the library: its id (for replays), and the song or why it won't load.
#[derive(Debug)]
pub struct LibrarySong {
    pub id: String,
    pub song: Result<Song, String>,
}

impl SongLibrary {
    fn load() -> SongLibrary {
        let mut builtin: Vec<LibrarySong> = BUILTIN
            .iter()
            .map(|song| LibrarySong {
                id: song.id.to_owned(),
                song: song.load().map_err(|e| format!("{}: {e}", song.id)),
            })
            .collect();
        builtin.sort_by_key(|s| !s.song.as_ref().is_ok_and(Song::is_lesson));
        let imported = wu_import::library::default_dir()
            .map(|dir| wu_import::library::load_all(&dir))
            .unwrap_or_default()
            .into_iter()
            .map(|imported| match imported {
                Ok(imported) => LibrarySong {
                    id: imported.id.clone(),
                    song: Ok(imported.song()),
                },
                Err((folder, error)) => LibrarySong {
                    id: folder
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                    song: Err(format!("{}: {error}", folder.display())),
                },
            });
        SongLibrary {
            songs: builtin.into_iter().chain(imported).collect(),
        }
    }

    pub fn get(&self, index: usize) -> Option<&Song> {
        self.songs.get(index).and_then(|s| s.song.as_ref().ok())
    }

    pub fn id(&self, index: usize) -> Option<&str> {
        self.songs.get(index).map(|s| s.id.as_str())
    }

    /// Adds a tune just imported (in place of the one it replaces, if it was
    /// imported before); returns where it is.
    pub fn add_import(&mut self, imported: &wu_import::ImportedSong) -> usize {
        let entry = LibrarySong {
            id: imported.id.clone(),
            song: Ok(imported.song()),
        };
        match self.songs.iter().position(|s| s.id == imported.id) {
            Some(index) => {
                self.songs[index] = entry;
                index
            }
            None => {
                self.songs.push(entry);
                self.songs.len() - 1
            }
        }
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

const ROWS: usize = 8;
/// The menu sits right of the record.
const MENU_X: f32 = 170.0;
/// The record: where it turns, how big.
const RECORD_AT: Vec2 = Vec2::new(-410.0, 10.0);
const RECORD_R: f32 = 150.0;
/// 33⅓ turns a minute, in radians a second.
const SPIN: f32 = 33.333 / 60.0 * std::f32::consts::TAU;

#[derive(Resource, Default)]
struct MenuRow(usize);

#[derive(Component)]
struct Row(usize);

#[derive(Component)]
enum Info {
    Title,
    Details,
    Chart,
    Audio,
    Import,
    /// The record on this tune at this difficulty, under its vinyl.
    Best,
}

fn enter(mut commands: Commands, fonts: Res<Fonts>, mut row: ResMut<MenuRow>) {
    // The texts are only rewritten on change: make sure the fresh ones get filled.
    row.set_changed();
    commands.spawn(screen_root(Screen::Songs)).with_children(|screen| {
        screen
            .spawn(centred_on(MENU_X, -175.0, 600.0, 20.0))
            .with_child(label("SELECT A TUNE", 13.0, palette::MUTED));
        screen.spawn(centred_on(MENU_X, -135.0, 760.0, 50.0)).with_child((
            Info::Title,
            Text::new(""),
            TextFont {
                font: fonts.display.clone().into(),
                ..TextFont::from_font_size(34.0)
            },
            TextColor(palette::FLYER_YELLOW),
        ));
        screen
            .spawn(centred_on(MENU_X, -95.0, 760.0, 22.0))
            .with_child((Info::Details, label("", 15.0, palette::MUTED)));
        for row in 0..ROWS {
            let y = -60.0 + row as f32 * 32.0;
            screen
                .spawn(centred_on(MENU_X, y, 680.0, 30.0))
                .with_child((Row(row), label("", 19.0, palette::INK)));
        }
        screen
            .spawn(centred_on(MENU_X, 204.0, 760.0, 22.0))
            .with_child((Info::Chart, centred_label("", 14.0, palette::SIGNAL)));
        screen
            .spawn(centred_on(MENU_X, 228.0, 760.0, 22.0))
            .with_child((Info::Audio, centred_label("", 13.0, palette::MUTED)));
        screen
            .spawn(centred_on(MENU_X, 252.0, 760.0, 22.0))
            .with_child((Info::Import, centred_label("", 13.0, palette::FLYER_YELLOW)));
        screen
            .spawn(centred_on(RECORD_AT.x, -RECORD_AT.y + RECORD_R + 28.0, 360.0, 40.0))
            .with_child((Info::Best, centred_label("", 14.0, palette::FLYER_YELLOW)));
        screen.spawn(centred_on(0.0, 290.0, 1000.0, 20.0)).with_child(label(
            "↑ ↓ choose · ← → change · ✕ / Space play · drop a tune on this window to play it",
            14.0,
            palette::MUTED,
        ));
    });
}

/// The offered note speed closest to `speed` (the settings file may hold any).
fn nearest_speed(speed: f32) -> f32 {
    NOTE_SPEEDS
        .into_iter()
        .min_by(|a, b| (a - speed).abs().total_cmp(&(b - speed).abs()))
        .unwrap_or(1.0)
}

fn step<T: Copy + PartialEq>(options: &[T], current: T, by: i32) -> T {
    let i = options.iter().position(|&o| o == current).unwrap_or(0) as i32;
    options[(i + by).rem_euclid(options.len() as i32) as usize]
}

#[allow(clippy::too_many_arguments)]
fn navigate(
    mut raw: MessageReader<RawInput>,
    mut row: ResMut<MenuRow>,
    mut session: ResMut<Session>,
    mut settings: ResMut<SettingsStore>,
    library: Res<SongLibrary>,
    recordings: Res<Recordings>,
    audio: NonSend<AudioLink>,
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
                // A tune of the player's plays once its recording is ready.
                let ready = library.get(session.song).is_some_and(|song| {
                    song.recording
                        .as_ref()
                        .is_none_or(|r| recordings.get(&r.path, audio.sample_rate()).is_some())
                });
                if ready {
                    session.from_tour = false;
                    next.set(Screen::Rhythm);
                }
                0
            }
            MenuKey::Back => 0,
        };
        if change != 0 {
            match row.0 {
                0 => {
                    let count = library.songs.len().max(1) as i32;
                    session.song = (session.song as i32 + change).rem_euclid(count) as usize;
                    // Another song has other sections.
                    session.practice = None;
                }
                1 => session.difficulty = step(&PLAYABLE, session.difficulty, change),
                2 => {
                    // The whole song, then each section in turn.
                    let sections = library.get(session.song).map_or(0, |song| song.sections.len());
                    let options: Vec<Option<usize>> = std::iter::once(None).chain((0..sections).map(Some)).collect();
                    session.practice = step(&options, session.practice, change);
                }
                3 => {
                    let tempo = session.tempo_percent as i32 + 10 * change;
                    session.tempo_percent = tempo.clamp(50, 150) as u32;
                }
                4 => {
                    let speed = step(&NOTE_SPEEDS, nearest_speed(settings.note_speed()), change);
                    settings.set_note_speed(speed);
                }
                5 => session.autoplay = !session.autoplay,
                6 => session.no_fail = !session.no_fail,
                // An imported tune always plays as recorded.
                _ if library.get(session.song).is_some_and(|song| song.recording.is_some()) => {}
                _ => {
                    let mode = match settings.audio_mode() {
                        AudioMode::Live => AudioMode::Classic,
                        AudioMode::Classic => AudioMode::Live,
                    };
                    settings.set_audio_mode(mode);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn show(
    session: Res<Session>,
    settings: Res<SettingsStore>,
    audio: NonSend<AudioLink>,
    row: Res<MenuRow>,
    library: Res<SongLibrary>,
    importing: Res<Importing>,
    recordings: Res<Recordings>,
    records: Res<RecordsStore>,
    mut rows: Query<(&Row, &mut Text, &mut TextColor), Without<Info>>,
    mut infos: Query<(&Info, &mut Text), Without<Row>>,
) {
    if !session.is_changed()
        && !row.is_changed()
        && !library.is_changed()
        && !settings.is_changed()
        && !importing.is_changed()
        && !recordings.is_changed()
    {
        return;
    }
    let mode = settings.audio_mode();
    let on_off = |on: bool| if on { "on" } else { "off" };
    let song_name = match library.get(session.song) {
        Some(song) if song.meta.title.chars().count() > 18 => {
            format!("{}…", song.meta.title.chars().take(17).collect::<String>())
        }
        Some(song) => song.meta.title.clone(),
        None => "?".to_owned(),
    };
    let recorded = library.get(session.song).is_some_and(|song| song.recording.is_some());
    let lesson = library.get(session.song).is_some_and(Song::is_lesson);
    // The section looped, with its bars counted from one.
    let practice = session
        .practice
        .and_then(|index| library.get(session.song)?.sections.get(index))
        .map(|(name, start, end)| (name.as_str(), start.bar() + 1, end.bar()));
    let values = [
        (
            "Song",
            format!("{} of {}: {song_name}", session.song + 1, library.songs.len()),
        ),
        (
            "Difficulty",
            if lesson {
                "a lesson".to_owned()
            } else {
                session.difficulty.name().to_owned()
            },
        ),
        (
            "Practice",
            match practice {
                Some((name, from, to)) => format!("loop {name}, bars {from}–{to}"),
                None => "off: the whole song".to_owned(),
            },
        ),
        ("Tempo", format!("{} %", session.tempo_percent)),
        ("Note speed", format!("{}×", settings.note_speed())),
        ("Autoplay (selecta bot)", on_off(session.autoplay).to_owned()),
        (
            "No-Fail",
            if lesson || practice.is_some() {
                "always".to_owned()
            } else {
                on_off(session.no_fail).to_owned()
            },
        ),
        (
            "Audio",
            if recorded {
                "Recorded".to_owned()
            } else {
                mode.name().to_owned()
            },
        ),
    ];
    for (r, mut text, mut colour) in &mut rows {
        let (name, value) = &values[r.0];
        let selected = r.0 == row.0;
        text.0 = format!("{} {name:<24} ◀ {value:^26} ▶", if selected { "›" } else { " " });
        colour.0 = if selected { palette::FLYER_YELLOW } else { palette::INK };
    }
    let song = library.get(session.song);
    for (info, mut text) in &mut infos {
        text.0 = match (info, song) {
            (Info::Title, Some(song)) => song.meta.title.clone(),
            (Info::Details, Some(song)) => {
                let seconds = song.tempo.seconds_at(song.length.0 as f64) * 100.0 / f64::from(session.tempo_percent);
                let bpm = song.tempo.bpm_at(wu_time::Tick::ZERO) * f64::from(session.tempo_percent) / 100.0;
                let length = format!("{}:{:02}", (seconds / 60.0) as u32, (seconds % 60.0) as u32);
                let bpm = format!("{bpm:.0} BPM");
                [
                    song.meta.artist.as_str(),
                    bpm.as_str(),
                    &song.meta.key,
                    &song.meta.subgenre,
                    &length,
                ]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" · ")
            }
            (Info::Chart, Some(song)) if practice.is_some() => {
                let (name, start, end) = session
                    .practice
                    .and_then(|index| song.sections.get(index))
                    .cloned()
                    .unwrap_or_default();
                let looped = wu_game::play::chart_between(wu_game::play::chart(song, session.difficulty), start, end);
                format!(
                    "practice: {name}'s {} notes, round and round until you leave · no fail, no record",
                    looped.notes.len() + looped.holds.len()
                )
            }
            (Info::Chart, Some(song)) if song.is_lesson() => {
                let chart = wu_game::play::chart(song, session.difficulty);
                format!(
                    "{} lessons, {} notes: each control in turn, no fail, timing loose",
                    song.lessons.len(),
                    chart.notes.len() + chart.holds.len()
                )
            }
            (Info::Chart, Some(song)) => {
                let chart = auto_chart(&song.drums, &song.bass, &song.tempo, session.difficulty);
                let lanes = Difficulty::rules(session.difficulty).pads.len();
                let rolls = match chart.rolls.len() {
                    0 => String::new(),
                    1 => " · 1 roll (L1 / R1 join in)".to_owned(),
                    n => format!(" · {n} rolls (L1 / R1 join in)"),
                };
                let rails = Difficulty::rules(session.difficulty).rails;
                let bass = match rails.len() {
                    0 => String::new(),
                    1 => format!(" · the bass on R2: {} holds", chart.holds.len()),
                    _ => format!(" · the bass on L2 and R2: {} holds", chart.holds.len()),
                };
                format!("{} notes on {lanes} pads{rolls}{bass}", chart.notes.len())
            }
            (Info::Import, _) => importing.status.clone().unwrap_or_default(),
            (Info::Best, Some(song)) if song.is_lesson() => "HOW TO PLAY\nstart here".to_owned(),
            (Info::Best, Some(_)) => library
                .id(session.song)
                .and_then(|id| records.best(id, session.difficulty))
                .map_or_else(
                    || format!("no record yet on {}", session.difficulty.name()),
                    |best| {
                        format!(
                            "BEST ON {}\n{}",
                            session.difficulty.name().to_uppercase(),
                            describe(best)
                        )
                    },
                ),
            (Info::Audio, Some(song)) if song.recording.is_some() => {
                let ready = song
                    .recording
                    .as_ref()
                    .is_some_and(|r| recordings.get(&r.path, audio.sample_rate()).is_some());
                match &recordings.problem {
                    Some(problem) => problem.clone(),
                    None if ready => "Your tune plays as recorded; a miss muffles it until your next hit".to_owned(),
                    None => "Getting your tune ready…".to_owned(),
                }
            }
            (Info::Audio, Some(_)) => match (mode, audio.info().bluetooth) {
                (AudioMode::Live, true) => {
                    "Bluetooth output: its delay makes playing live hard. Try Audio: Classic".to_owned()
                }
                (AudioMode::Live, false) => "Live audio: your presses play your part".to_owned(),
                (AudioMode::Classic, _) => {
                    "Classic audio: the whole song plays; a miss mutes your part until your next hit".to_owned()
                }
            },
            (_, None) => library
                .songs
                .get(session.song)
                .and_then(|s| s.song.as_ref().err())
                .cloned()
                .unwrap_or_default(),
        };
    }
}

/// The selected tune's record: it turns while its drop plays.
#[derive(Component)]
struct Record {
    /// Radians a second, now.
    speed: f32,
}

/// The record's label, in the tune's colour; and the light around the record.
#[derive(Component)]
struct RecordLabel;

#[derive(Component)]
struct RecordGlow;

/// A dubplate: black vinyl with its grooves catching the light, a label in the
/// tune's colour, a mark on the label to show it turning, and a halo.
fn spawn_record(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let at = RECORD_AT.extend(5.0);
    let label = materials.add(ColorMaterial::from(palette::FLYER_YELLOW));
    let glow = materials.add(see_through(glowing(palette::FLYER_YELLOW, 1.4), 0.6));
    commands.spawn((
        DespawnOnExit(Screen::Songs),
        RecordGlow,
        Mesh2d(meshes.add(Annulus::new(RECORD_R + 2.0, RECORD_R + 6.0))),
        MeshMaterial2d(glow),
        Transform::from_translation(at - Vec3::Z),
    ));
    spawn_vinyl(
        &mut commands,
        &mut meshes,
        &mut materials,
        RECORD_R,
        label,
        (
            DespawnOnExit(Screen::Songs),
            Record { speed: 0.0 },
            Transform::from_translation(at),
        ),
        RecordLabel,
    );
}

/// Turns the record while the preview plays (it gets up to speed and winds
/// down like a deck), in the selected tune's colour, its halo on the kick.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn spin_record(
    time: Res<Time>,
    session: Res<Session>,
    library: Res<SongLibrary>,
    preview: Res<Preview>,
    settings: Res<SettingsStore>,
    mood: Res<StageMood>,
    mut record: Query<(&mut Record, &mut Transform)>,
    labels: Query<&MeshMaterial2d<ColorMaterial>, (With<RecordLabel>, Without<RecordGlow>)>,
    glows: Query<&MeshMaterial2d<ColorMaterial>, (With<RecordGlow>, Without<RecordLabel>)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let dt = time.delta_secs();
    // Reduced motion: the record stays still.
    let target = if preview.is_playing() && !settings.reduced_motion() {
        SPIN
    } else {
        0.0
    };
    for (mut record, mut transform) in &mut record {
        // A deck's platter: up to speed in about half a second, down in one.
        let rate = if target > record.speed { 4.0 } else { 2.0 };
        record.speed += (target - record.speed) * (1.0 - (-rate * dt).exp());
        transform.rotate_z(-record.speed * dt);
    }
    let colour = library
        .get(session.song)
        .map_or(palette::MUTED, |song| palette::subgenre(&song.meta.subgenre));
    for label in &labels {
        if let Some(mut material) = materials.get_mut(&label.0) {
            material.color = colour;
        }
    }
    for glow in &glows {
        if let Some(mut material) = materials.get_mut(&glow.0) {
            material.color = glowing(colour, 1.0 + 1.5 * mood.pulse).with_alpha(0.25 + 0.5 * mood.pulse);
        }
    }
}
