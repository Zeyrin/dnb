//! The RHYTHM screen: the highway. Notes fall toward the hit line. The player's
//! pads sound at once (from the input thread) and are judged here, from their
//! timestamps, against the audio clock, minus the calibrated offset.

use bevy::prelude::*;
use wu_audio::Command;
use wu_content::songs::BUILTIN;
use wu_game::judge::{Judgement, Outcome, TimedNote};
use wu_game::play::{chart as play_chart, practice_tempo, timed_notes};
use wu_game::run::Run;
use wu_input::{Action, Button, Phase};
use wu_instruments::{PAD_COUNT, Pad};
use wu_time::Tick;

use crate::audio::AudioLink;
use crate::fonts::Fonts;
use crate::input::{InputLink, PlayerAction};
use crate::palette;
use crate::screens::Screen;
use crate::session::{LastRun, Session, windows};
use crate::settings::SettingsStore;
use crate::songs_screen::SongLibrary;
use crate::ui::{centred_on, label, screen_root};

#[derive(Debug)]
pub struct RhythmPlugin;

impl Plugin for RhythmPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Rhythm), enter)
            .add_systems(OnExit(Screen::Rhythm), exit)
            .add_systems(
                Update,
                (play, draw_notes, draw_hud).chain().run_if(in_state(Screen::Rhythm)),
            );
    }
}

const COUNT_IN_BARS: i64 = 2;
/// How far ahead notes appear, in song milliseconds.
const LOOKAHEAD_MS: f64 = 1500.0;
/// Vertical positions, relative to the centre of the screen.
const HIT_Y: f32 = 240.0;
const TOP_Y: f32 = -150.0;
const LANE_W: f32 = 66.0;
const HAND_GAP: f32 = 44.0;
const NOTE_W: f32 = 56.0;
const NOTE_H: f32 = 18.0;
/// How long a judgement stays on screen.
const POPUP_NS: u64 = 450_000_000;
/// After the plug is pulled, how long before the results.
const FAIL_PAUSE_NS: u64 = 2_500_000_000;

/// Lanes left to right, by button: the left thumb's D-pad, then the right
/// thumb's face buttons, laid out as the hands sit.
const LANES: [Button; PAD_COUNT] = [
    Button::DPadLeft,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadRight,
    Button::West,
    Button::North,
    Button::South,
    Button::East,
];

/// Horizontal centre of a lane, relative to the centre of the screen.
fn lane_x(lane: usize) -> f32 {
    let width = LANES.len() as f32 * LANE_W + HAND_GAP;
    -width / 2.0 + (lane as f32 + 0.5) * LANE_W + if lane >= 4 { HAND_GAP } else { 0.0 }
}

fn note_y(note_ms: f64, view_ms: f64) -> f32 {
    let speed = f64::from(HIT_Y - TOP_Y) / LOOKAHEAD_MS;
    HIT_Y - ((note_ms - view_ms) * speed) as f32
}

#[derive(Clone, Copy, Debug, Default)]
struct Popup {
    judgement: Option<Judgement>,
    offset_ms: f64,
    at_ns: u64,
}

#[derive(Resource)]
struct Play {
    /// The engine program this run plays; nothing is judged until it is live.
    generation: u64,
    song: &'static str,
    title: String,
    run: Run,
    notes: Vec<TimedNote>,
    lane_of: [usize; PAD_COUNT],
    pad_of_lane: [Option<Pad>; PAD_COUNT],
    /// One beat at the practice tempo, for the count-in.
    beat_ms: f64,
    sections: Vec<(String, f64)>,
    end_ms: f64,
    audio_offset_ms: f64,
    visual_lead_ms: f64,
    autoplay: bool,
    autoplay_next: usize,
    next_spawn: usize,
    entities: Vec<Option<Entity>>,
    paused: bool,
    failed_at_ns: Option<u64>,
    popups: [Popup; PAD_COUNT],
    pressed_at_ns: [u64; PAD_COUNT],
    now_ms: f64,
}

#[derive(Component)]
struct NoteMark;

#[derive(Component)]
struct Receptor(usize);

#[derive(Component)]
struct PopupText(usize);

#[derive(Component)]
struct VibeFill;

#[derive(Component)]
enum Hud {
    Score,
    Combo,
    Status,
    Centre,
}

#[allow(clippy::too_many_arguments)]
fn enter(
    mut commands: Commands,
    mut audio: NonSendMut<AudioLink>,
    input: NonSend<InputLink>,
    session: Res<Session>,
    library: Res<SongLibrary>,
    settings: Res<SettingsStore>,
    fonts: Res<Fonts>,
    mut next: ResMut<NextState<Screen>>,
) {
    let (Some(song), Some(builtin)) = (library.get(session.song).cloned(), BUILTIN.get(session.song)) else {
        next.set(Screen::Songs);
        return;
    };
    let tempo = practice_tempo(&song, session.tempo_percent);
    let chart = play_chart(&song, session.difficulty);
    let ms_at = |tick: Tick| tempo.seconds_at(tick.0 as f64) * 1000.0;
    let notes = timed_notes(&chart, &tempo);
    let run = Run::new(notes, windows(session.difficulty), session.score_rules());
    let notes = run.judge().notes().to_vec();

    let sample_rate = audio.sample_rate();
    let autoplay = session.autoplay;
    let program = song.program(sample_rate, &tempo, COUNT_IN_BARS, |tick, pad| {
        !autoplay && chart.contains(tick, pad)
    });
    let generation = audio.load(program);
    audio.send(Command::Seek(Tick::from_bars(-COUNT_IN_BARS)));
    audio.send(Command::Play);

    let layout = input.layout();
    let mut lane_of = [0; PAD_COUNT];
    let mut pad_of_lane = [None; PAD_COUNT];
    for (lane, button) in LANES.into_iter().enumerate() {
        if let Some(pad) = layout.pad_for(button) {
            lane_of[pad.index()] = lane;
            pad_of_lane[lane] = Some(pad);
        }
    }
    let calibration = settings.calibration(&audio.info().device);
    let last_note_ms = notes.last().map_or(0.0, |n| n.ms);
    let end_ms = ms_at(song.length).max(last_note_ms) + 1500.0;
    let sections = song
        .sections
        .iter()
        .map(|(name, start, _)| (name.clone(), ms_at(*start)))
        .collect();
    let note_count = notes.len();
    commands.insert_resource(Play {
        generation,
        song: builtin.id,
        title: song.meta.title.clone(),
        run,
        notes,
        lane_of,
        pad_of_lane,
        beat_ms: 60_000.0 / tempo.bpm_at(Tick::ZERO),
        sections,
        end_ms,
        audio_offset_ms: calibration.audio_ms,
        visual_lead_ms: calibration.visual_lead_ms(),
        autoplay,
        autoplay_next: 0,
        next_spawn: 0,
        entities: vec![None; note_count],
        paused: false,
        failed_at_ns: None,
        popups: [Popup::default(); PAD_COUNT],
        pressed_at_ns: [0; PAD_COUNT],
        now_ms: f64::NEG_INFINITY,
    });

    commands.spawn(screen_root(Screen::Rhythm)).with_children(|screen| {
        // The lanes, faint, from the top of the highway to the hit line.
        for (lane, button) in LANES.into_iter().enumerate() {
            let colour = layout.pad_for(button).map_or(palette::MUTED, palette::pad);
            let height = HIT_Y - TOP_Y + 40.0;
            screen.spawn((
                centred_on(lane_x(lane), TOP_Y + height / 2.0 - 20.0, LANE_W - 6.0, height),
                BackgroundColor(palette::mix(palette::BACKDROP, colour, 0.06)),
            ));
            screen
                .spawn((
                    Receptor(lane),
                    Node {
                        border: UiRect::all(px(3)),
                        border_radius: BorderRadius::all(px(10)),
                        ..centred_on(lane_x(lane), HIT_Y, LANE_W - 8.0, 40.0)
                    },
                    BorderColor::all(colour),
                    BackgroundColor(palette::dim(colour)),
                ))
                .with_child((
                    Text::new(button.glyph()),
                    TextFont {
                        font: fonts.bold.clone().into(),
                        ..TextFont::from_font_size(22.0)
                    },
                    TextColor(palette::INK),
                ));
            screen
                .spawn(centred_on(lane_x(lane), HIT_Y - 44.0, LANE_W + 30.0, 18.0))
                .with_child((PopupText(lane), label("", 13.0, palette::INK)));
        }
        // The vibe meter, left of the highway.
        let meter_x = lane_x(0) - LANE_W / 2.0 - 30.0;
        let meter_h = HIT_Y - TOP_Y;
        screen
            .spawn((
                Node {
                    flex_direction: FlexDirection::ColumnReverse,
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(6)),
                    overflow: Overflow::clip(),
                    ..centred_on(meter_x, TOP_Y + meter_h / 2.0, 16.0, meter_h)
                },
                BorderColor::all(palette::MUTED),
            ))
            .with_child((
                VibeFill,
                Node {
                    width: percent(100),
                    height: percent(50),
                    ..default()
                },
                BackgroundColor(palette::SIGNAL),
            ));
        screen
            .spawn(centred_on(meter_x, HIT_Y + 34.0, 60.0, 16.0))
            .with_child(label("VIBE", 11.0, palette::MUTED));
        screen.spawn(centred_on(430.0, -150.0, 300.0, 40.0)).with_child((
            Hud::Score,
            Text::new(""),
            TextFont {
                font: fonts.display.clone().into(),
                ..TextFont::from_font_size(30.0)
            },
            TextColor(palette::FLYER_YELLOW),
        ));
        screen
            .spawn(centred_on(430.0, -105.0, 300.0, 50.0))
            .with_child((Hud::Combo, label("", 15.0, palette::INK)));
        screen
            .spawn(centred_on(-440.0, -130.0, 300.0, 80.0))
            .with_child((Hud::Status, label("", 14.0, palette::MUTED)));
        screen.spawn(centred_on(0.0, 40.0, 900.0, 90.0)).with_child((
            Hud::Centre,
            Text::new(""),
            TextFont {
                font: fonts.display.clone().into(),
                ..TextFont::from_font_size(54.0)
            },
            TextColor(palette::FLYER_YELLOW),
        ));
    });
}

fn exit(mut commands: Commands, mut audio: NonSendMut<AudioLink>) {
    audio.send(Command::Stop);
    commands.remove_resource::<Play>();
}

fn song_ms(audio: &AudioLink, at_ns: u64) -> Option<f64> {
    let frame = audio.estimator.transport_frame_at(at_ns)?;
    Some(frame / f64::from(audio.sample_rate()) * 1000.0)
}

fn note_feedback(play: &mut Play, outcomes: &[Outcome], now_ns: u64, commands: &mut Commands) {
    for outcome in outcomes {
        let (note, judgement, offset_ms) = match *outcome {
            Outcome::Hit {
                note,
                judgement,
                offset_ms,
            } => (note, judgement, offset_ms),
            Outcome::Missed { note } => (note, Judgement::Miss, 0.0),
            Outcome::Overhit { .. } => continue,
        };
        let lane = play.lane_of[play.notes[note].pad.index()];
        play.popups[lane] = Popup {
            judgement: Some(judgement),
            offset_ms,
            at_ns: now_ns,
        };
        if judgement != Judgement::Miss
            && let Some(entity) = play.entities[note].take()
        {
            commands.entity(entity).despawn();
        }
    }
}

fn play(
    mut commands: Commands,
    play: Option<ResMut<Play>>,
    mut actions: MessageReader<PlayerAction>,
    mut audio: NonSendMut<AudioLink>,
    session: Res<Session>,
    mut next: ResMut<NextState<Screen>>,
) {
    let Some(mut play) = play else { return };
    let play = &mut *play;
    // Until the engine reports this run's program, the clock still describes
    // whatever played before (and would miss every note up to its position).
    if !play.paused && play.failed_at_ns.is_none() && !audio.is_live(play.generation) {
        actions.clear();
        return;
    }
    let now_ns = wu_time::mono::now_ns();
    let mut presses: Vec<(f64, Pad)> = Vec::new();
    for PlayerAction(action) in actions.read() {
        if action.phase != Phase::Pressed {
            continue;
        }
        match action.action {
            Action::Pad(pad) if !play.autoplay && !play.paused => {
                play.pressed_at_ns[play.lane_of[pad.index()]] = now_ns;
                if let Some(ms) = song_ms(&audio, action.at_ns) {
                    presses.push((ms - play.audio_offset_ms, pad));
                }
            }
            Action::Pause if play.failed_at_ns.is_none() => {
                play.paused = !play.paused;
                audio.send(if play.paused { Command::Stop } else { Command::Play });
            }
            Action::Select => {
                next.set(Screen::Songs);
                return;
            }
            _ => {}
        }
    }
    let Some(now_ms) = song_ms(&audio, now_ns) else { return };
    play.now_ms = now_ms;
    if play.paused || play.failed_at_ns.is_some() {
        if play.failed_at_ns.is_some_and(|at| now_ns - at > FAIL_PAUSE_NS) {
            finish(play, &session, &mut commands, &mut next, true);
        }
        return;
    }
    presses.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (ms, pad) in presses {
        let outcomes = play.run.press(pad, ms);
        note_feedback(play, &outcomes, now_ns, &mut commands);
    }
    if play.autoplay {
        while let Some(note) = play.notes.get(play.autoplay_next).copied() {
            if note.ms > now_ms {
                break;
            }
            play.pressed_at_ns[play.lane_of[note.pad.index()]] = now_ns;
            let outcomes = play.run.press(note.pad, note.ms);
            note_feedback(play, &outcomes, now_ns, &mut commands);
            play.autoplay_next += 1;
        }
    }
    let missed = play.run.settle(now_ms);
    note_feedback(play, &missed, now_ns, &mut commands);
    if play.run.score().failed {
        // PLUG PULLED: the power cuts, the run is over.
        play.failed_at_ns = Some(now_ns);
        audio.send(Command::Stop);
    } else if now_ms > play.end_ms {
        finish(play, &session, &mut commands, &mut next, false);
    }
}

fn finish(play: &mut Play, session: &Session, commands: &mut Commands, next: &mut NextState<Screen>, failed: bool) {
    play.run.finish();
    commands.insert_resource(LastRun {
        song: play.song,
        title: play.title.clone(),
        difficulty: session.difficulty,
        tempo_percent: session.tempo_percent,
        no_fail: session.no_fail(),
        autoplay: play.autoplay,
        score: play.run.score().clone(),
        failed,
        presses: play.run.presses().to_vec(),
        notes: play.notes.len(),
    });
    next.set(Screen::Results);
}

fn draw_notes(
    mut commands: Commands,
    play: Option<ResMut<Play>>,
    mut nodes: Query<(&mut Node, &mut BackgroundColor), With<NoteMark>>,
) {
    let Some(mut play) = play else { return };
    let play = &mut *play;
    if !play.now_ms.is_finite() {
        return;
    }
    let view_ms = play.now_ms + play.visual_lead_ms;
    while let Some(note) = play.notes.get(play.next_spawn).copied() {
        if note.ms - view_ms > LOOKAHEAD_MS + 100.0 {
            break;
        }
        let index = play.next_spawn;
        play.next_spawn += 1;
        if play.run.judge().judgement(index).is_some() {
            continue;
        }
        let lane = play.lane_of[note.pad.index()];
        let colour = palette::pad(note.pad);
        let entity = commands
            .spawn((
                DespawnOnExit(Screen::Rhythm),
                NoteMark,
                Node {
                    border_radius: BorderRadius::all(px(6)),
                    ..crate::ui::centred_on(lane_x(lane), note_y(note.ms, view_ms), NOTE_W, NOTE_H)
                },
                BackgroundColor(colour),
            ))
            .id();
        play.entities[index] = Some(entity);
    }
    for index in 0..play.entities.len() {
        let Some(entity) = play.entities[index] else { continue };
        let y = note_y(play.notes[index].ms, view_ms);
        if y > HIT_Y + 140.0 {
            commands.entity(entity).despawn();
            play.entities[index] = None;
            continue;
        }
        if let Ok((mut node, mut background)) = nodes.get_mut(entity) {
            node.margin.top = px(y - NOTE_H / 2.0);
            if play.run.judge().judgement(index) == Some(Judgement::Miss) {
                background.0 = palette::mix(palette::BACKDROP, palette::pad(play.notes[index].pad), 0.25);
            }
        }
    }
}

fn judgement_colour(judgement: Judgement) -> Color {
    match judgement {
        Judgement::Wicked => palette::FLYER_YELLOW,
        Judgement::Big => palette::SIGNAL,
        Judgement::Safe => palette::INK,
        Judgement::Miss => palette::WARNING,
    }
}

#[allow(clippy::type_complexity)]
fn draw_hud(
    play: Option<Res<Play>>,
    mut receptors: Query<(&Receptor, &mut BackgroundColor), (Without<VibeFill>, Without<NoteMark>)>,
    mut popups: Query<(&PopupText, &mut Text, &mut TextColor), Without<Hud>>,
    mut fill: Query<(&mut Node, &mut BackgroundColor), (With<VibeFill>, Without<Receptor>, Without<NoteMark>)>,
    mut huds: Query<(&Hud, &mut Text), Without<PopupText>>,
) {
    let Some(play) = play else { return };
    let now_ns = wu_time::mono::now_ns();
    let score = play.run.score();
    for (receptor, mut background) in &mut receptors {
        let colour = play.pad_of_lane[receptor.0].map_or(palette::MUTED, palette::pad);
        let since = now_ns.saturating_sub(play.pressed_at_ns[receptor.0]) as f64 / 1e9;
        let glow = (-since / 0.08).exp() as f32;
        background.0 = palette::mix(palette::dim(colour), colour, glow);
    }
    for (popup, mut text, mut colour) in &mut popups {
        let entry = play.popups[popup.0];
        match entry.judgement {
            Some(judgement) if now_ns.saturating_sub(entry.at_ns) < POPUP_NS => {
                text.0 = judgement.label().to_owned();
                colour.0 = judgement_colour(judgement);
            }
            _ => text.0.clear(),
        }
    }
    if let Ok((mut node, mut background)) = fill.single_mut() {
        node.height = percent(100.0 * score.vibe);
        background.0 = if score.vibe < 0.25 {
            palette::WARNING
        } else {
            palette::SIGNAL
        };
    }
    let section = play
        .sections
        .iter()
        .rev()
        .find(|(_, start)| *start <= play.now_ms)
        .map_or("Count-in", |(name, _)| name.as_str());
    let last_offset = play
        .popups
        .iter()
        .filter(|p| p.judgement.is_some_and(|j| j != Judgement::Miss))
        .max_by_key(|p| p.at_ns)
        .map(|p| p.offset_ms);
    let beats_to_go = (-play.now_ms / play.beat_ms).ceil();
    for (hud, mut text) in &mut huds {
        text.0 = match hud {
            Hud::Score => format!("{:>9}", score.points),
            Hud::Combo => format!(
                "{} combo · ×{}\naccuracy {:.1} %",
                score.combo,
                score.multiplier(),
                score.accuracy() * 100.0
            ),
            Hud::Status => format!(
                "{}\n{}\n{}",
                play.title,
                section,
                last_offset.map_or(String::new(), |o| if o >= 0.0 {
                    format!("{o:.0} ms late")
                } else {
                    format!("{:.0} ms early", -o)
                })
            ),
            Hud::Centre => {
                if play.failed_at_ns.is_some() {
                    "PLUG PULLED".to_owned()
                } else if play.paused {
                    "PAUSED".to_owned()
                } else if play.now_ms < 0.0 && play.now_ms.is_finite() {
                    format!("{}", beats_to_go.max(1.0))
                } else {
                    String::new()
                }
            }
        };
    }
}
