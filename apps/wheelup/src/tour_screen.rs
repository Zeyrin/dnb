//! The TOUR screen: the Pirate Radio Tour's six stops, the stars earned at each
//! (◆, from the records at the tour's difficulty), its encore and its challenge.
//! Pick a stop and a tune, and play it for real: full tempo, No-Fail off.

use bevy::prelude::*;
use wu_chart::Difficulty;
use wu_content::settings::Language;
use wu_content::tour::{Challenge, Tour};
use wu_game::tour::{Progress, progress, stars};

use crate::fonts::Fonts;
use crate::input::RawInput;
use crate::palette;
use crate::records::RecordsStore;
use crate::screens::Screen;
use crate::session::{PLAYABLE, Session};
use crate::settings::SettingsStore;
use crate::songs_screen::{MenuKey, SongLibrary, menu_keys};
use crate::stage::{Scene, StageMood};
use crate::ui::{centred_on, label, screen_root};
use crate::words::{self, fill, tr};

#[derive(Debug)]
pub struct TourPlugin;

impl Plugin for TourPlugin {
    fn build(&self, app: &mut App) {
        let tour = Tour::load().unwrap_or_else(|error| {
            warn!("the tour won't load: {error}");
            Tour { venues: Vec::new() }
        });
        app.insert_resource(TourData(tour))
            .init_resource::<TourCursor>()
            .add_systems(OnEnter(Screen::Tour), enter)
            .add_systems(OnExit(Screen::Tour), |mut mood: ResMut<StageMood>| {
                *mood = StageMood::default()
            })
            .add_systems(
                Update,
                (navigate, show, set_the_scene).chain().run_if(in_state(Screen::Tour)),
            );
    }
}

#[derive(Resource, Debug)]
pub struct TourData(pub Tour);

/// Where the player is on the screen: the difficulty row (0) or a stop (1 on),
/// and which of the stop's tunes.
#[derive(Resource, Debug, Default)]
struct TourCursor {
    row: usize,
    tune: usize,
}

/// The stop's list on the left, the chosen stop on the right.
const LIST_X: f32 = -330.0;
const PANEL_X: f32 = 320.0;
const TOP_Y: f32 = -150.0;
const ROW_H: f32 = 38.0;

/// A box like `centred_on`'s, its text set from its left edge.
fn left_on(x: f32, y: f32, width: f32, height: f32) -> Node {
    Node {
        justify_content: JustifyContent::FlexStart,
        ..centred_on(x, y, width, height)
    }
}

#[derive(Component)]
enum Part {
    Heading,
    Row(usize),
    Name,
    Line,
    Set,
    Help,
}

fn enter(mut commands: Commands, fonts: Res<Fonts>, tour: Res<TourData>, mut cursor: ResMut<TourCursor>) {
    cursor.set_changed();
    let rows = 1 + tour.0.venues.len();
    commands.spawn(screen_root(Screen::Tour)).with_children(|screen| {
        screen
            .spawn(left_on(LIST_X, TOP_Y - 34.0, 560.0, 22.0))
            .with_child((Part::Heading, label("", 14.0, palette::MUTED)));
        for row in 0..rows {
            screen
                .spawn(left_on(LIST_X, TOP_Y + row as f32 * ROW_H, 560.0, 30.0))
                .with_child((Part::Row(row), label("", 18.0, palette::INK)));
        }
        screen.spawn(left_on(PANEL_X, TOP_Y - 30.0, 560.0, 40.0)).with_child((
            Part::Name,
            Text::new(""),
            TextFont {
                font: fonts.display.clone().into(),
                ..TextFont::from_font_size(28.0)
            },
            TextColor(palette::FLYER_YELLOW),
        ));
        screen
            .spawn(left_on(PANEL_X, TOP_Y + 14.0, 560.0, 44.0))
            .with_child((Part::Line, label("", 14.0, palette::MUTED)));
        screen
            .spawn(left_on(PANEL_X, TOP_Y + 160.0, 560.0, 240.0))
            .with_child((Part::Set, label("", 16.0, palette::INK)));
        screen
            .spawn(centred_on(0.0, 290.0, 1000.0, 20.0))
            .with_child((Part::Help, label("", 14.0, palette::MUTED)));
    });
}

/// The tunes a stop offers now: its set, then its encore once the set has earned it.
fn tunes<'a>(tour: &'a Tour, progress: &Progress, stop: usize) -> Vec<&'a str> {
    let Some(venue) = tour.venues.get(stop) else {
        return Vec::new();
    };
    let encore_open = progress.stops.get(stop).is_some_and(|s| s.encore_open);
    venue
        .set
        .iter()
        .map(String::as_str)
        .chain(venue.encore.as_deref().filter(|_| encore_open))
        .collect()
}

fn step<T: Copy + PartialEq>(options: &[T], current: T, by: i32) -> T {
    let i = options.iter().position(|&o| o == current).unwrap_or(0) as i32;
    options[(i + by).rem_euclid(options.len() as i32) as usize]
}

#[allow(clippy::too_many_arguments)]
fn navigate(
    mut raw: MessageReader<RawInput>,
    mut cursor: ResMut<TourCursor>,
    mut session: ResMut<Session>,
    tour: Res<TourData>,
    records: Res<RecordsStore>,
    library: Res<SongLibrary>,
    mut next: ResMut<NextState<Screen>>,
) {
    let rows = 1 + tour.0.venues.len();
    for key in menu_keys(&mut raw) {
        let progress = progress(&tour.0, records.records(), session.difficulty);
        let stop = cursor.row.checked_sub(1);
        let offered = stop.map_or_else(Vec::new, |stop| tunes(&tour.0, &progress, stop));
        match key {
            MenuKey::Up => {
                cursor.row = (cursor.row + rows - 1) % rows;
                cursor.tune = 0;
            }
            MenuKey::Down => {
                cursor.row = (cursor.row + 1) % rows;
                cursor.tune = 0;
            }
            MenuKey::Left | MenuKey::Right => {
                let by = if key == MenuKey::Left { -1 } else { 1 };
                if stop.is_none() {
                    session.difficulty = step(&PLAYABLE, session.difficulty, by);
                } else if !offered.is_empty() {
                    cursor.tune = (cursor.tune as i32 + by).rem_euclid(offered.len() as i32) as usize;
                }
            }
            MenuKey::Confirm => {
                let open = stop.is_some_and(|stop| progress.stops.get(stop).is_some_and(|s| s.open));
                let song = offered
                    .get(cursor.tune)
                    .and_then(|id| library.songs.iter().position(|s| s.id == *id));
                if let (true, Some(song)) = (open, song) {
                    // The real thing: the song's own tempo, the whole of it, No-Fail off.
                    session.song = song;
                    session.practice = None;
                    session.tempo_percent = 100;
                    session.no_fail = false;
                    session.from_tour = true;
                    next.set(Screen::Rhythm);
                }
            }
            MenuKey::Back => {}
        }
    }
}

/// Behind the tour, the venue of the stop chosen.
fn set_the_scene(cursor: Res<TourCursor>, tour: Res<TourData>, mut mood: ResMut<StageMood>) {
    let stop = cursor.row.saturating_sub(1);
    let scene = tour
        .0
        .venues
        .get(stop)
        .map_or(Scene::Rooftop, |v| Scene::of_stop(&v.id));
    if mood.scene != scene {
        mood.scene = scene;
    }
}

/// What a stop asks, in the player's language.
fn challenge_line(challenge: &Challenge, language: Language) -> String {
    match challenge {
        Challenge::FullCombo => tr(language, "a full combo on any tune of the set").to_owned(),
        Challenge::AllAtLeast(grade) => fill(tr(language, "every tune of the set at {} or better"), &[grade]),
        Challenge::Stars(n) => fill(tr(language, "{} stars from this stop"), &[n]),
    }
}

/// So many stars of five, as diamonds.
fn diamonds(earned: u32) -> String {
    (0..5).map(|i| if i < earned { '◆' } else { '◇' }).collect()
}

#[allow(clippy::type_complexity)]
fn show(
    cursor: Res<TourCursor>,
    settings: Res<SettingsStore>,
    session: Res<Session>,
    tour: Res<TourData>,
    records: Res<RecordsStore>,
    library: Res<SongLibrary>,
    mut parts: Query<(&Part, &mut Text, &mut TextColor)>,
) {
    if !cursor.is_changed() && !session.is_changed() && !records.is_changed() && !settings.is_changed() {
        return;
    }
    let tour = &tour.0;
    let language = settings.language();
    let difficulty: Difficulty = session.difficulty;
    let progress = progress(tour, records.records(), difficulty);
    let stop = cursor.row.saturating_sub(1);
    let venue = tour.venues.get(stop);
    let standing = progress.stops.get(stop).copied();
    let title_of = |id: &str| {
        library
            .songs
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.song.as_ref().ok())
            .map_or_else(|| id.to_owned(), |song| song.meta.title.clone())
    };
    let tune_line = |id: &str, selected: bool| {
        let best = records.best(id, difficulty);
        format!(
            "{} {:<24} {}  {}",
            if selected { "›" } else { " " },
            title_of(id),
            diamonds(best.map_or(0, stars)),
            best.map_or("", |b| b.grade.as_str())
        )
    };
    for (part, mut text, mut colour) in &mut parts {
        match part {
            Part::Heading => {
                text.0 = fill(
                    tr(
                        language,
                        "PIRATE RADIO TOUR · {} ◆ earned on {} · {} dubplates to spend",
                    ),
                    &[
                        &progress.stars,
                        &words::difficulty(language, difficulty),
                        &records.dubplates_left(tour),
                    ],
                );
            }
            Part::Row(0) => {
                let selected = cursor.row == 0;
                text.0 = format!(
                    "{} {:<24} ◀ {:^10} ▶",
                    if selected { "›" } else { " " },
                    tr(language, "Difficulty"),
                    words::difficulty(language, difficulty)
                );
                colour.0 = if selected { palette::FLYER_YELLOW } else { palette::INK };
            }
            Part::Row(row) => {
                let index = row - 1;
                let (Some(venue), Some(stand)) = (tour.venues.get(index), progress.stops.get(index)) else {
                    continue;
                };
                let selected = cursor.row == *row;
                let state = if venue.set.is_empty() {
                    tr(language, "on the way").to_owned()
                } else if !stand.open {
                    fill(tr(language, "{} ◆ to get in"), &[&venue.opens_at])
                } else {
                    format!(
                        "◆ {}/{}{}",
                        stand.stars,
                        stand.most,
                        if stand.challenge_done { " ✓" } else { "" }
                    )
                };
                text.0 = format!(
                    "{} {} {:<24} {state}",
                    if selected { "›" } else { " " },
                    index + 1,
                    venue.name
                );
                colour.0 = match (selected, stand.open) {
                    (true, _) => palette::FLYER_YELLOW,
                    (false, true) => palette::INK,
                    (false, false) => palette::MUTED,
                };
            }
            Part::Name => text.0 = venue.map_or_else(String::new, |v| v.name.to_uppercase()),
            Part::Line => {
                text.0 = venue.map_or_else(String::new, |v| match (language, &v.line_fr) {
                    (Language::French, Some(line)) => line.clone(),
                    _ => v.line.clone(),
                });
            }
            Part::Set => {
                let (Some(venue), Some(stand)) = (venue, standing) else {
                    text.0.clear();
                    continue;
                };
                let mut lines = Vec::new();
                if venue.set.is_empty() {
                    lines.push(tr(language, "Its tunes are still being cut.").to_owned());
                } else {
                    let picking = cursor.row > 0 && stand.open;
                    lines.push(tr(language, "SET").to_owned());
                    for (i, id) in venue.set.iter().enumerate() {
                        lines.push(tune_line(id, picking && cursor.tune == i));
                    }
                    if let Some(encore) = &venue.encore {
                        lines.push(String::new());
                        if stand.encore_open {
                            lines.push(tr(language, "ENCORE").to_owned());
                            lines.push(tune_line(encore, picking && cursor.tune == venue.set.len()));
                        } else {
                            lines.push(fill(
                                tr(language, "ENCORE: {} ◆ from the set opens it"),
                                &[&venue.encore_stars],
                            ));
                        }
                    }
                    lines.push(String::new());
                    lines.push(
                        fill(
                            tr(language, "CHALLENGE: {}"),
                            &[&challenge_line(&venue.challenge, language)],
                        ) + if stand.challenge_done {
                            tr(language, "  ✓ done")
                        } else {
                            ""
                        },
                    );
                    if !stand.open {
                        lines.push(String::new());
                        lines.push(fill(
                            tr(language, "Closed: {} ◆ on the tour opens it ({} so far)."),
                            &[&venue.opens_at, &progress.stars],
                        ));
                    }
                }
                text.0 = lines.join("\n");
            }
            Part::Help => {
                text.0 = tr(
                    language,
                    "↑ ↓ choose · ← → difficulty, or a stop's tune · ✕ / Space play it · every real run counts here, on the tour or not",
                )
                .to_owned();
            }
        }
    }
}
