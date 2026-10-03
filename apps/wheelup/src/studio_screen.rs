//! The STUDIO tab: make a tune on the pad. The Pattern view's step grid (the
//! eight pads, sixteen steps a bar) and the Live view, where the pads play and
//! record onto the loop, pass by pass; a template per subgenre, play and stop,
//! undo and redo, an autosave every minute, and Chart It, which plays the tune
//! as a chart.

use bevy::prelude::*;
use wu_audio::{Command, Hit, Report};
use wu_input::{Action, Button, LiveAction, Phase};
use wu_instruments::kits::KITS;
use wu_instruments::{PAD_COUNT, Pad};
use wu_studio::grid::{Cell, MOST_BARS};
use wu_studio::templates::{Template, templates};
use wu_studio::{Edit, Studio, store};
use wu_time::{STEPS_PER_BAR, TICKS_PER_STEP, Tick};

use crate::audio::{AudioLink, EngineReport};
use crate::fonts::Fonts;
use crate::input::{InputLink, PlayerAction, RawInput};
use crate::palette;
use crate::screens::Screen;
use crate::session::Session;
use crate::settings::SettingsStore;
use crate::songs_screen::{SongLibrary, fresh_presses};
use crate::ui::{centred_label, centred_on, label, screen_root};
use crate::words::{fill, tr};

/// The rows above the grid.
const MENU: [&str; 6] = ["Template", "Tempo", "Kit", "Bars", "Record to", "Chart It"];
const TEMPLATE: usize = 0;
const TEMPO: usize = 1;
const KIT: usize = 2;
const BARS: usize = 3;
const QUANTIZE: usize = 4;
const CHART_IT: usize = 5;
/// Two bars in view at a time.
const COLUMNS: usize = 2 * STEPS_PER_BAR as usize;
const CELL: f32 = 20.0;
const PITCH: f32 = 24.0;
/// The first cell's centre, and the first pad row's.
const GRID_X: f32 = -330.0;
const GRID_Y: f32 = 4.0;
const ROW_PITCH: f32 = 26.0;
const AUTOSAVE_S: f64 = 60.0;

#[derive(Debug)]
pub struct StudioPlugin;

impl Plugin for StudioPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Desk::load())
            .add_systems(OnEnter(Screen::Studio), enter)
            .add_systems(OnExit(Screen::Studio), leave)
            .add_systems(Update, (drive, show).chain().run_if(in_state(Screen::Studio)));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    /// The step grid, edited a step at a time.
    Pattern,
    /// The pads played, and recorded onto the loop.
    Live,
}

/// The tune on the desk, and where the cursor is.
#[derive(Resource)]
pub struct Desk {
    view: View,
    /// Recording in the Live view: each pass's hits land on the grid when it ends.
    recording: bool,
    /// The pass being recorded: pad and step, landed in time.
    pass: Vec<(Pad, usize)>,
    /// Recordings land on eighths instead of sixteenths.
    eighths: bool,
    studio: Studio,
    templates: Vec<Template>,
    /// The template ✕ starts a new tune from.
    template: usize,
    /// A menu row, or `MENU.len()` and on for the pads, top to bottom.
    row: usize,
    step: usize,
    playing: bool,
    saved_at: f64,
    /// What just happened: saved, couldn't save, why it won't play.
    status: String,
}

impl Desk {
    /// The tune last worked on, or the first template.
    fn load() -> Desk {
        let templates = templates();
        let newest = store::studio_dir().and_then(|dir| store::list(&dir).into_iter().next());
        let studio = newest
            .and_then(|folder| Some(Studio::new(store::load(&folder).ok()?, Some(folder))))
            .or_else(|| templates.first().map(|t| Studio::new(t.project.clone(), None)))
            .unwrap_or_else(|| Studio::new(wu_content::project::Project::from_ron(DEMO).expect("valid"), None));
        let template = templates
            .iter()
            .position(|t| t.subgenre == studio.project.meta.subgenre)
            .unwrap_or(0);
        Desk {
            view: View::Pattern,
            recording: false,
            pass: Vec::new(),
            eighths: false,
            studio,
            templates,
            template,
            row: MENU.len(),
            step: 0,
            playing: false,
            saved_at: 0.0,
            status: String::new(),
        }
    }

    fn pad(&self) -> Option<Pad> {
        self.row.checked_sub(MENU.len()).and_then(Pad::from_index)
    }

    /// Saves now, when there is something to save (or, `always`, to give a
    /// tune never saved its folder).
    fn save(&mut self, now: f64, always: bool) {
        self.saved_at = now;
        if !self.studio.dirty && (self.studio.folder.is_some() || !always) {
            return;
        }
        self.status = match store::studio_dir().map(|dir| self.studio.save(&dir)) {
            Some(Ok(_)) => "saved".to_owned(),
            Some(Err(error)) => format!("not saved: {error}"),
            None => "not saved: no data folder".to_owned(),
        };
    }

    /// The pass recorded so far lands on the grid, as one edit.
    fn land_pass(&mut self) -> bool {
        let hits: Vec<(Pad, usize, Cell)> = self.pass.drain(..).map(|(pad, step)| (pad, step, Cell::Hit)).collect();
        !hits.is_empty() && self.studio.apply(Edit::Hits(hits))
    }

    /// The tune's id in the song library: "studio/<its folder>".
    fn id(&self) -> String {
        let folder = self
            .studio
            .folder
            .as_ref()
            .and_then(|f| f.file_name())
            .map(|n| n.to_string_lossy().into_owned());
        format!(
            "studio/{}",
            folder.unwrap_or_else(|| store::slug(&self.studio.project.meta.title))
        )
    }
}

/// Only if no template could be built (none can fail: a test checks them).
const DEMO: &str = r#"Project(version: 1, meta: (title: "New tune", artist: "You"), bpm: 170.0, kit: "ragga-93",
    patterns: {"beat": Drums(bars: 2, steps: {"P1": "X... .... ..X. ....", "P2": ".... X... .... X..."})},
    arrangement: [(name: "Drop", bars: 16, play: ["beat"], hype: true)])"#;

#[derive(Component)]
enum Text2 {
    Title,
    Details,
    Menu(usize),
    PadName(usize),
    Page,
    Status,
    Hint,
}

#[derive(Component)]
struct GridCell {
    pad: usize,
    column: usize,
}

/// A ratchet's count, written on its step.
#[derive(Component)]
struct CellDigit {
    pad: usize,
    column: usize,
}

#[derive(Component)]
struct Playhead;

fn enter(
    mut commands: Commands,
    fonts: Res<Fonts>,
    settings: Res<SettingsStore>,
    mut desk: ResMut<Desk>,
    mut audio: NonSendMut<AudioLink>,
) {
    let language = settings.language();
    desk.playing = false;
    desk.status.clear();
    desk.recording = false;
    reload(&mut desk, &mut audio, false);
    commands.spawn(screen_root(Screen::Studio)).with_children(|screen| {
        screen.spawn(centred_on(0.0, -186.0, 900.0, 36.0)).with_child((
            Text2::Title,
            Text::new(""),
            TextFont {
                font: fonts.display.clone().into(),
                ..TextFont::from_font_size(26.0)
            },
            TextColor(palette::FLYER_YELLOW),
        ));
        screen
            .spawn(centred_on(0.0, -160.0, 900.0, 20.0))
            .with_child((Text2::Details, centred_label("", 14.0, palette::MUTED)));
        for row in 0..MENU.len() {
            screen
                .spawn(centred_on(0.0, -128.0 + row as f32 * 22.0, 760.0, 22.0))
                .with_child((Text2::Menu(row), label("", 16.0, palette::INK)));
        }
        for pad in 0..PAD_COUNT {
            let y = GRID_Y + pad as f32 * ROW_PITCH;
            screen
                .spawn(centred_on(GRID_X - 100.0, y, 150.0, 24.0))
                .with_child((Text2::PadName(pad), label("", 15.0, palette::INK)));
            for column in 0..COLUMNS {
                screen
                    .spawn((
                        GridCell { pad, column },
                        Node {
                            border: UiRect::all(px(2)),
                            ..centred_on(cell_x(column), y, CELL, CELL)
                        },
                        BackgroundColor(palette::BACKDROP),
                        BorderColor::all(Color::NONE),
                    ))
                    .with_child((
                        CellDigit { pad, column },
                        Text::new(""),
                        TextFont::from_font_size(11.0),
                        TextColor(palette::BACKDROP),
                    ));
            }
        }
        screen.spawn((
            Playhead,
            Node {
                ..centred_on(cell_x(0), GRID_Y - 17.0, CELL, 4.0)
            },
            BackgroundColor(palette::FLYER_YELLOW),
            Visibility::Hidden,
        ));
        screen
            .spawn(centred_on(0.0, GRID_Y + PAD_COUNT as f32 * ROW_PITCH, 900.0, 20.0))
            .with_child((Text2::Page, centred_label("", 14.0, palette::MUTED)));
        screen
            .spawn(centred_on(
                0.0,
                GRID_Y + PAD_COUNT as f32 * ROW_PITCH + 22.0,
                900.0,
                20.0,
            ))
            .with_child((Text2::Status, centred_label("", 14.0, palette::SIGNAL)));
        screen
            .spawn(centred_on(0.0, 290.0, 1100.0, 20.0))
            .with_child((Text2::Hint, centred_label("", 14.0, palette::MUTED)));
    });
    let _ = language;
}

fn leave(time: Res<Time>, mut desk: ResMut<Desk>, mut audio: NonSendMut<AudioLink>) {
    audio.send(Command::Stop);
    desk.playing = false;
    desk.recording = false;
    desk.land_pass();
    desk.save(time.elapsed_secs_f64(), false);
}

/// Where column `column` of the grid sits: a little gap after each beat.
fn cell_x(column: usize) -> f32 {
    GRID_X + column as f32 * PITCH + (column / 4) as f32 * 4.0
}

/// The engine takes the beat again, looped, from where it was; while
/// recording, with a click on every beat (and, `count_in`, a bar of them first).
fn reload(desk: &mut Desk, audio: &mut AudioLink, count_in: bool) {
    let song = match desk.studio.loop_song() {
        Ok(song) => song,
        Err(error) => {
            desk.status = format!("won't play: {error}");
            return;
        }
    };
    let at = if desk.playing && !count_in {
        audio.tick_now().unwrap_or(0.0)
    } else {
        0.0
    };
    let length = song.length.0.max(1);
    let mut program = song.whole_program(audio.sample_rate(), &song.tempo, i64::from(count_in));
    if desk.recording {
        let beat = Tick::from_beats(1).0;
        program = program.with_hits((0..length / beat).map(|b| Hit {
            tick: Tick::from_beats(b),
            pad: Pad::P4,
            velocity: if b % 4 == 0 { 0.8 } else { 0.55 },
        }));
    }
    audio.load(program);
    audio.send(Command::SetLoop(Some((Tick::ZERO, song.length))));
    let from = if count_in {
        -Tick::from_bars(1).0
    } else {
        (at as i64).rem_euclid(length)
    };
    audio.send(Command::Seek(Tick(from)));
    if desk.playing {
        audio.send(Command::Play);
    }
}

#[allow(clippy::too_many_arguments)]
fn drive(
    mut raw: MessageReader<RawInput>,
    mut actions: MessageReader<PlayerAction>,
    mut reports: MessageReader<EngineReport>,
    time: Res<Time>,
    mut desk: ResMut<Desk>,
    mut audio: NonSendMut<AudioLink>,
    mut input: NonSendMut<InputLink>,
    mut library: ResMut<SongLibrary>,
    mut session: ResMut<Session>,
    mut next: ResMut<NextState<Screen>>,
) {
    let now = time.elapsed_secs_f64();
    // In the Live view the pads sound as they are pressed.
    input.set_live(desk.view == View::Live);
    // Each time the loop comes round, the pass just played lands on the grid.
    let looped = reports
        .read()
        .any(|EngineReport(report)| matches!(report, Report::Looped { .. }));
    if looped && desk.land_pass() {
        reload(&mut desk, &mut audio, false);
    }
    // The pads, landed on the grid as they are played.
    let steps = desk.studio.grid().steps();
    for PlayerAction(action) in actions.read() {
        let (Action::Pad(pad), Phase::Pressed) = (action.action, action.phase) else {
            continue;
        };
        if desk.view != View::Live || !desk.recording || crate::esc_menu::is_open() {
            continue;
        }
        let Some(tick) = audio.tick_at(action.at_ns) else {
            continue;
        };
        let grain = if desk.eighths { 2 } else { 1 };
        let step = ((tick / TICKS_PER_STEP as f64 / f64::from(grain)).round() as i64) * i64::from(grain);
        // Early for the loop's first step, in the count-in, counts as on it.
        if step < 0 {
            continue;
        }
        let step = (step as usize) % steps;
        if !desk.pass.contains(&(pad, step)) {
            desk.pass.push((pad, step));
        }
    }
    let rows = MENU.len() + PAD_COUNT;
    for button in fresh_presses(&mut raw) {
        let pad = desk.pad();
        let step = desk.step;
        let live = desk.view == View::Live;
        let changed = match button {
            // Both views: L1 / R1 switch, OPTIONS plays.
            Button::L1 | Button::R1 => {
                desk.view = if live { View::Pattern } else { View::Live };
                if live && desk.recording {
                    desk.recording = false;
                    desk.land_pass();
                    true
                } else {
                    false
                }
            }
            Button::Start => {
                desk.playing = !desk.playing;
                let landed = !desk.playing && std::mem::take(&mut desk.recording) && desk.land_pass();
                audio.send(if desk.playing { Command::Play } else { Command::Stop });
                landed
            }
            // The Live view: R3 records, L3 takes the last pass back.
            Button::R3 if live => {
                desk.recording = !desk.recording;
                if desk.recording {
                    desk.pass.clear();
                    let count_in = !desk.playing;
                    desk.playing = true;
                    reload(&mut desk, &mut audio, count_in);
                    false
                } else {
                    desk.land_pass();
                    true
                }
            }
            Button::L3 if live => {
                desk.pass.clear();
                desk.studio.undo()
            }
            _ if live => false,
            Button::DPadUp => {
                desk.row = (desk.row + rows - 1) % rows;
                false
            }
            Button::DPadDown => {
                desk.row = (desk.row + 1) % rows;
                false
            }
            Button::DPadLeft | Button::DPadRight => {
                let by: i64 = if button == Button::DPadLeft { -1 } else { 1 };
                match desk.row {
                    TEMPLATE => {
                        let count = desk.templates.len().max(1) as i64;
                        desk.template = (desk.template as i64 + by).rem_euclid(count) as usize;
                        false
                    }
                    TEMPO => {
                        let bpm = desk.studio.project.bpm + by as f64;
                        desk.studio.apply(Edit::Tempo(bpm))
                    }
                    KIT => {
                        let i = KITS.iter().position(|k| k.id == desk.studio.project.kit).unwrap_or(0) as i64;
                        let kit = KITS[(i + by).rem_euclid(KITS.len() as i64) as usize].id;
                        desk.studio.apply(Edit::Kit(kit.to_owned()))
                    }
                    BARS => {
                        let bars = desk.studio.grid().bars + by;
                        desk.studio.apply(Edit::Bars(bars))
                    }
                    QUANTIZE => {
                        desk.eighths = !desk.eighths;
                        false
                    }
                    CHART_IT => false,
                    _ => {
                        desk.step = (step as i64 + by).rem_euclid(steps as i64) as usize;
                        false
                    }
                }
            }
            Button::South => match (desk.row, pad) {
                (TEMPLATE, _) => {
                    // A new tune from the template: the one on the desk is kept.
                    desk.save(now, false);
                    if let Some(template) = desk.templates.get(desk.template) {
                        desk.studio = Studio::new(template.project.clone(), None);
                        desk.step = 0;
                        desk.status = "a new tune".to_owned();
                    }
                    true
                }
                (CHART_IT, _) => {
                    desk.save(now, true);
                    match desk.studio.compile() {
                        Ok(song) => {
                            let id = desk.id();
                            session.song = library.put(id, song);
                            session.practice = None;
                            session.from_tour = false;
                            next.set(Screen::Rhythm);
                        }
                        Err(error) => desk.status = format!("won't chart: {error}"),
                    }
                    false
                }
                (_, Some(pad)) => {
                    let changed = desk.studio.apply(Edit::Cycle { pad, step });
                    // Heard as it is set, when the beat isn't playing it already.
                    if !desk.playing && desk.studio.grid().cell(pad, step) != Cell::Rest {
                        audio.play_live(LiveAction::Pad {
                            pad,
                            at_ns: wu_time::mono::now_ns(),
                        });
                    }
                    changed
                }
                _ => false,
            },
            Button::East => match pad {
                Some(pad) => desk.studio.apply(Edit::Set {
                    pad,
                    step,
                    cell: Cell::Rest,
                }),
                None => false,
            },
            // A ratchet of two, three, four, then back to a plain hit.
            Button::R3 => match pad {
                Some(pad) => {
                    let cell = desk.studio.grid().cell(pad, step).next_ratchet();
                    desk.studio.apply(Edit::Set { pad, step, cell })
                }
                None => false,
            },
            Button::West => desk.studio.undo(),
            Button::North => desk.studio.redo(),
            _ => false,
        };
        let steps_now = desk.studio.grid().steps();
        desk.step = desk.step.min(steps_now - 1);
        if changed {
            reload(&mut desk, &mut audio, false);
        }
    }
    if desk.studio.dirty && now - desk.saved_at > AUTOSAVE_S {
        desk.save(now, false);
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn show(
    desk: Res<Desk>,
    settings: Res<SettingsStore>,
    input: NonSend<InputLink>,
    audio: NonSend<AudioLink>,
    mut texts: Query<(&Text2, &mut Text, &mut TextColor), Without<CellDigit>>,
    mut digits: Query<(&CellDigit, &mut Text), Without<Text2>>,
    mut cells: Query<(&GridCell, &mut BackgroundColor, &mut BorderColor, &mut Visibility), Without<Playhead>>,
    mut playhead: Query<(&mut Node, &mut Visibility), (With<Playhead>, Without<GridCell>)>,
) {
    let language = settings.language();
    let grid = desk.studio.grid();
    let project = &desk.studio.project;
    let kit = wu_instruments::kit_def(&project.kit);
    let layout = input.layout();
    // The playhead, while the beat plays (not in a count-in).
    let playing_step = desk
        .playing
        .then(|| audio.tick_now())
        .flatten()
        .filter(|&tick| tick >= 0.0)
        .map(|tick| (tick as i64 / TICKS_PER_STEP).rem_euclid(grid.steps() as i64) as usize);
    // The Live view follows the playhead; the Pattern view, the cursor.
    let page = match desk.view {
        View::Live => playing_step.map_or(0, |step| step / COLUMNS),
        View::Pattern => desk.step / COLUMNS,
    };
    if desk.is_changed() || playing_step.is_some() {
        for (digit, mut text) in &mut digits {
            let step = page * COLUMNS + digit.column;
            let pad = Pad::from_index(digit.pad).unwrap_or(Pad::P1);
            let wanted = match grid.cell(pad, step) {
                Cell::Ratchet(strokes) if step < grid.steps() => strokes.to_string(),
                _ => String::new(),
            };
            if text.0 != wanted {
                text.0 = wanted;
            }
        }
        for (cell, mut background, mut border, mut visibility) in &mut cells {
            let step = page * COLUMNS + cell.column;
            if step >= grid.steps() {
                visibility.set_if_neq(Visibility::Hidden);
                continue;
            }
            visibility.set_if_neq(Visibility::Inherited);
            let pad = Pad::from_index(cell.pad).unwrap_or(Pad::P1);
            let colour = palette::pad(pad);
            let empty = if step.is_multiple_of(4) {
                Color::srgb(0.17, 0.15, 0.24)
            } else {
                Color::srgb(0.10, 0.09, 0.15)
            };
            let fill = match grid.cell(pad, step) {
                Cell::Rest => empty,
                Cell::Ghost => palette::mix(empty, colour, 0.35),
                Cell::Hit => palette::mix(empty, colour, 0.75),
                Cell::Accent | Cell::Ratchet(_) => colour,
            };
            let fill = if playing_step == Some(step) {
                palette::mix(fill, palette::INK, 0.35)
            } else {
                fill
            };
            background.0 = fill;
            let cursor = desk.view == View::Pattern && desk.pad() == Some(pad) && desk.step == step;
            let recorded = desk.pass.contains(&(pad, step));
            *border = BorderColor::all(if cursor {
                palette::FLYER_YELLOW
            } else if recorded {
                palette::WARNING
            } else {
                Color::NONE
            });
        }
    }
    if let Ok((mut node, mut visibility)) = playhead.single_mut() {
        match playing_step.filter(|&s| s / COLUMNS == page) {
            Some(step) => {
                let column = step % COLUMNS;
                node.margin.left = px(cell_x(column) - CELL / 2.0);
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
    if !desk.is_changed() {
        return;
    }
    let template_name = desk.templates.get(desk.template).map_or("", |t| t.subgenre.as_str());
    let kit_name = kit.map_or(project.kit.as_str(), |k| k.name);
    for (text, mut value, mut colour) in &mut texts {
        value.0 = match text {
            Text2::Title => project.meta.title.clone(),
            Text2::Details => fill(
                tr(language, "{} · {} BPM · {} · {} · {}"),
                &[
                    &project.meta.subgenre,
                    &project.bpm,
                    &kit_name,
                    &words_bars(language, grid.bars),
                    &if desk.studio.dirty {
                        tr(language, "not saved yet")
                    } else {
                        tr(language, "saved")
                    },
                ],
            ),
            Text2::Menu(row) => {
                let value = match *row {
                    TEMPLATE => fill(tr(language, "{} · ✕ starts a new tune"), &[&template_name]),
                    TEMPO => format!("{} BPM", project.bpm),
                    KIT => kit_name.to_owned(),
                    BARS => format!("{} / {MOST_BARS}", grid.bars),
                    QUANTIZE => tr(language, if desk.eighths { "eighths (1/8)" } else { "sixteenths (1/16)" }).to_owned(),
                    _ => tr(language, "✕ plays it as a chart").to_owned(),
                };
                let selected = desk.view == View::Pattern && desk.row == *row;
                colour.0 = if selected { palette::FLYER_YELLOW } else { palette::INK };
                format!(
                    "{} {:<14} ◀ {:^36} ▶",
                    if selected { "›" } else { " " },
                    tr(language, MENU[*row]),
                    value
                )
            }
            Text2::PadName(index) => {
                let pad = Pad::from_index(*index).unwrap_or(Pad::P1);
                let name = kit.map_or("", |k| k.pads[*index].name);
                colour.0 = if desk.view == View::Pattern && desk.pad() == Some(pad) {
                    palette::FLYER_YELLOW
                } else {
                    palette::pad(pad)
                };
                format!("{} {name}", layout.button_for(pad).glyph())
            }
            Text2::Page if desk.view == View::Live => {
                colour.0 = if desk.recording { palette::WARNING } else { palette::MUTED };
                fill(
                    tr(
                        language,
                        if desk.recording {
                            "LIVE · ● RECORDING onto {} · this pass: {} hits"
                        } else {
                            "LIVE · the pads play · records onto {} · {} hits waiting"
                        },
                    ),
                    &[
                        &tr(language, if desk.eighths { "eighths (1/8)" } else { "sixteenths (1/16)" }),
                        &desk.pass.len(),
                    ],
                )
            }
            Text2::Hint => tr(
                language,
                match desk.view {
                    View::Pattern => {
                        "↑ ↓ ← → move · ✕ step · R3 / M ratchet · ○ erase · □ undo · △ redo · L1 R1 live · OPTIONS / Space play"
                    }
                    View::Live => {
                        "pads play · R3 / M record · L3 / X take the last pass back · L1 R1 pattern view · OPTIONS / Space play"
                    }
                },
            )
            .to_owned(),
            Text2::Page => {
                colour.0 = palette::MUTED;
                let first = page * 2 + 1;
                let last = (first + 1).min(grid.bars as usize);
                let bar = desk.step / STEPS_PER_BAR as usize + 1;
                fill(
                    tr(language, "bars {}–{} of {} · step {} of bar {}"),
                    &[
                        &first,
                        &last,
                        &grid.bars,
                        &(desk.step % STEPS_PER_BAR as usize + 1),
                        &bar,
                    ],
                )
            }
            Text2::Status => {
                if desk.status.starts_with("not saved") || desk.status.starts_with("won't") {
                    colour.0 = palette::WARNING;
                    desk.status.clone()
                } else {
                    colour.0 = palette::SIGNAL;
                    tr(language, status_key(&desk.status)).to_owned()
                }
            }
        };
    }
}

/// The status lines that are always the same, as the word table keys them.
fn status_key(status: &str) -> &'static str {
    match status {
        "saved" => "saved",
        "a new tune" => "a new tune",
        _ => "",
    }
}

fn words_bars(language: wu_content::settings::Language, bars: i64) -> String {
    if bars == 1 {
        tr(language, "1 bar").to_owned()
    } else {
        fill(tr(language, "{} bars"), &[&bars])
    }
}
