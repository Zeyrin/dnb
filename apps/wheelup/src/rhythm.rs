//! The RHYTHM screen: the highway. Notes fall toward the hit line: pads in the
//! middle, the bass line on the two rails at the edges. The player's pads and
//! rails sound at once (from the input thread) and are judged here, from their
//! timestamps, against the audio clock, minus the calibrated offset.

use bevy::prelude::*;
use wu_audio::Command;
use wu_chart::Rail;
use wu_content::songs::BUILTIN;
use wu_game::judge::{Judgement, LANE_COUNT, Lane, Outcome, TimedNote};
use wu_game::play::{chart as play_chart, practice_tempo, timed_notes};
use wu_game::run::Run;
use wu_input::{Action, Button, Hand, Phase, RailNote};
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
                (play, draw_rolls, draw_notes, draw_hud)
                    .chain()
                    .run_if(in_state(Screen::Rhythm)),
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
const RAIL_W: f32 = 40.0;
/// Between a rail and the pad lanes beside it.
const RAIL_GAP: f32 = 18.0;
const NOTE_W: f32 = 56.0;
const NOTE_H: f32 = 18.0;
/// How long a judgement stays on screen.
const POPUP_NS: u64 = 450_000_000;
/// After the plug is pulled, how long before the results.
const FAIL_PAUSE_NS: u64 = 2_500_000_000;
/// How long before a roll its shoulder button starts playing the roll's lane.
const ROLL_ARM_MS: f64 = 400.0;

/// Pad lanes left to right, by button: the left thumb's D-pad, then the right
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

/// Highway columns: the eight pad lanes, then the left and right rails.
const COLUMNS: usize = PAD_COUNT + 2;

/// Horizontal centre of a pad lane, relative to the centre of the screen.
fn lane_x(lane: usize) -> f32 {
    let width = LANES.len() as f32 * LANE_W + HAND_GAP;
    -width / 2.0 + (lane as f32 + 0.5) * LANE_W + if lane >= 4 { HAND_GAP } else { 0.0 }
}

/// Horizontal centre of a column: rails sit outside the pad lanes.
fn column_x(column: usize) -> f32 {
    let outside = LANE_W / 2.0 + RAIL_GAP + RAIL_W / 2.0;
    match column {
        c if c < PAD_COUNT => lane_x(c),
        c if c == PAD_COUNT => lane_x(0) - outside,
        _ => lane_x(PAD_COUNT - 1) + outside,
    }
}

fn column_width(column: usize) -> f32 {
    if column < PAD_COUNT { LANE_W } else { RAIL_W }
}

fn rail_column(rail: Rail) -> usize {
    PAD_COUNT + rail.index()
}

fn rail_of(hand: Hand) -> Rail {
    match hand {
        Hand::Left => Rail::Left,
        Hand::Right => Rail::Right,
    }
}

fn hand_of(rail: Rail) -> Hand {
    match rail {
        Rail::Left => Hand::Left,
        Rail::Right => Hand::Right,
    }
}

fn note_y(note_ms: f64, view_ms: f64) -> f32 {
    let speed = f64::from(HIT_Y - TOP_Y) / LOOKAHEAD_MS;
    HIT_Y - ((note_ms - view_ms) * speed) as f32
}

/// A roll in song milliseconds, with the hand whose shoulder joins in.
#[derive(Clone, Copy, Debug)]
struct TimedRoll {
    pad: Pad,
    hand: Hand,
    start_ms: f64,
    end_ms: f64,
}

/// What a rail plays when pressed for a hold: its key, and the program frame it ends on.
#[derive(Clone, Copy, Debug)]
struct RailCue {
    rail: Rail,
    start_ms: f64,
    note: RailNote,
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
    rolls: Vec<TimedRoll>,
    cues: Vec<RailCue>,
    column_of: [usize; LANE_COUNT],
    column_colour: [Color; COLUMNS],
    /// One beat at the practice tempo, for the count-in.
    beat_ms: f64,
    sections: Vec<(String, f64)>,
    end_ms: f64,
    audio_offset_ms: f64,
    visual_lead_ms: f64,
    autoplay: bool,
    autoplay_next: usize,
    /// Holds the selecta bot is holding: when to let go, and where.
    autoplay_releases: Vec<(f64, Lane)>,
    next_spawn: usize,
    entities: Vec<Option<Entity>>,
    paused: bool,
    failed_at_ns: Option<u64>,
    popups: [Popup; COLUMNS],
    pressed_at_ns: [u64; COLUMNS],
    /// The rails held down right now.
    rail_down: [bool; 2],
    now_ms: f64,
}

#[derive(Component)]
struct NoteMark;

#[derive(Component)]
struct RollMark(usize);

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
    // The player's part is left out of the backing: their presses play it.
    let program = song.program(
        sample_rate,
        &tempo,
        COUNT_IN_BARS,
        |tick, pad| !autoplay && chart.contains(tick, pad),
        |tick, key| !autoplay && chart.holds_note(tick, key),
    );
    let generation = audio.load(program);
    audio.send(Command::Seek(Tick::from_bars(-COUNT_IN_BARS)));
    audio.send(Command::Play);

    let layout = input.layout();
    let mut column_of = [0; LANE_COUNT];
    let mut column_colour = [palette::BASS; COLUMNS];
    for (lane, button) in LANES.into_iter().enumerate() {
        column_colour[lane] = layout.pad_for(button).map_or(palette::MUTED, palette::pad);
        if let Some(pad) = layout.pad_for(button) {
            column_of[Lane::Pad(pad).index()] = lane;
        }
    }
    for rail in Rail::ALL {
        column_of[Lane::Rail(rail).index()] = rail_column(rail);
    }
    let rolls: Vec<TimedRoll> = chart
        .rolls
        .iter()
        .map(|roll| TimedRoll {
            pad: roll.pad,
            hand: layout.hand_for(roll.pad),
            start_ms: ms_at(roll.start),
            end_ms: ms_at(roll.end),
        })
        .collect();
    let cues = chart
        .holds
        .iter()
        .map(|hold| RailCue {
            rail: hold.rail,
            start_ms: ms_at(hold.start),
            note: RailNote {
                key: hold.key,
                until_frame: tempo.frame_at(hold.end, sample_rate),
            },
        })
        .collect();
    let calibration = settings.calibration(&audio.info().device);
    let last_note_ms = notes
        .iter()
        .map(|n| n.hold.map_or(n.ms, |span| span.end_ms))
        .fold(0.0, f64::max);
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
        rolls: rolls.clone(),
        cues,
        column_of,
        column_colour,
        beat_ms: 60_000.0 / tempo.bpm_at(Tick::ZERO),
        sections,
        end_ms,
        audio_offset_ms: calibration.audio_ms,
        visual_lead_ms: calibration.visual_lead_ms(),
        autoplay,
        autoplay_next: 0,
        autoplay_releases: Vec::new(),
        next_spawn: 0,
        entities: vec![None; note_count],
        paused: false,
        failed_at_ns: None,
        popups: [Popup::default(); COLUMNS],
        pressed_at_ns: [0; COLUMNS],
        rail_down: [false; 2],
        now_ms: f64::NEG_INFINITY,
    });

    // Rails are only drawn when the chart uses them.
    let shown: Vec<usize> = (0..COLUMNS)
        .filter(|&c| c < PAD_COUNT || chart.holds.iter().any(|h| rail_column(h.rail) == c))
        .collect();
    commands.spawn(screen_root(Screen::Rhythm)).with_children(|screen| {
        // The lanes, faint, from the top of the highway to the hit line.
        for &column in &shown {
            let height = HIT_Y - TOP_Y + 40.0;
            screen.spawn((
                centred_on(
                    column_x(column),
                    TOP_Y + height / 2.0 - 20.0,
                    column_width(column) - 6.0,
                    height,
                ),
                BackgroundColor(palette::mix(palette::BACKDROP, column_colour[column], 0.06)),
            ));
        }
        // Rolls: a band over the lane and under the receptors, marked at the top
        // with the shoulder button that joins in.
        for (index, roll) in rolls.iter().enumerate() {
            let colour = palette::pad(roll.pad);
            let shoulder = match roll.hand {
                Hand::Left => Button::L1,
                Hand::Right => Button::R1,
            };
            screen
                .spawn((
                    RollMark(index),
                    Visibility::Hidden,
                    Node {
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::all(px(8)),
                        align_items: AlignItems::FlexStart,
                        padding: UiRect::top(px(NOTE_H + 2.0)),
                        ..centred_on(
                            column_x(column_of[Lane::Pad(roll.pad).index()]),
                            0.0,
                            LANE_W - 4.0,
                            NOTE_H,
                        )
                    },
                    BorderColor::all(colour),
                    BackgroundColor(palette::mix(palette::BACKDROP, colour, 0.22)),
                ))
                .with_child((
                    Text::new(shoulder.glyph()),
                    TextFont {
                        font: fonts.bold.clone().into(),
                        ..TextFont::from_font_size(15.0)
                    },
                    TextColor(colour),
                ));
        }
        for &column in &shown {
            let colour = column_colour[column];
            let glyph = match column {
                c if c < PAD_COUNT => LANES[c].glyph(),
                c if c == PAD_COUNT => "L2",
                _ => "R2",
            };
            screen
                .spawn((
                    Receptor(column),
                    Node {
                        border: UiRect::all(px(3)),
                        border_radius: BorderRadius::all(px(10)),
                        ..centred_on(column_x(column), HIT_Y, column_width(column) - 8.0, 40.0)
                    },
                    BorderColor::all(colour),
                    BackgroundColor(palette::dim(colour)),
                ))
                .with_child((
                    Text::new(glyph),
                    TextFont {
                        font: fonts.bold.clone().into(),
                        ..TextFont::from_font_size(if column < PAD_COUNT { 22.0 } else { 14.0 })
                    },
                    TextColor(palette::INK),
                ));
            screen
                .spawn(centred_on(column_x(column), HIT_Y - 44.0, LANE_W + 30.0, 18.0))
                .with_child((PopupText(column), label("", 13.0, palette::INK)));
        }
        // The vibe meter, left of the highway.
        let meter_x = column_x(PAD_COUNT) - RAIL_W / 2.0 - 30.0;
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
        screen.spawn(centred_on(460.0, -150.0, 300.0, 40.0)).with_child((
            Hud::Score,
            Text::new(""),
            TextFont {
                font: fonts.display.clone().into(),
                ..TextFont::from_font_size(30.0)
            },
            TextColor(palette::FLYER_YELLOW),
        ));
        screen
            .spawn(centred_on(460.0, -105.0, 300.0, 50.0))
            .with_child((Hud::Combo, label("", 15.0, palette::INK)));
        screen
            .spawn(centred_on(-470.0, -130.0, 280.0, 80.0))
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

fn exit(mut commands: Commands, mut audio: NonSendMut<AudioLink>, mut input: NonSendMut<InputLink>) {
    audio.send(Command::Stop);
    for hand in [Hand::Left, Hand::Right] {
        input.set_roll_pad(hand, None);
        input.set_rail_note(hand, None);
    }
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
            Outcome::HoldEnd { note, .. } => {
                // Kept to its end or let go early, the hold's tail is gone.
                if let Some(entity) = play.entities[note].take() {
                    commands.entity(entity).despawn();
                }
                continue;
            }
            Outcome::Overhit { .. } => continue,
        };
        let column = play.column_of[play.notes[note].lane.index()];
        play.popups[column] = Popup {
            judgement: Some(judgement),
            offset_ms,
            at_ns: now_ns,
        };
        // A tap is done once hit; a hold stays on the highway while it is held.
        if judgement != Judgement::Miss
            && play.notes[note].hold.is_none()
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
    mut input: NonSendMut<InputLink>,
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
    let reach = play.run.judge().windows().safe;
    // What the player did this frame: (song ms, lane, let go).
    let mut inputs: Vec<(f64, Lane, bool)> = Vec::new();
    for PlayerAction(action) in actions.read() {
        let playing = !play.autoplay && !play.paused;
        let at_ms = || song_ms(&audio, action.at_ns).map(|ms| ms - play.audio_offset_ms);
        match (action.action, action.phase) {
            (Action::Pad(pad), Phase::Pressed) if playing => {
                let lane = Lane::Pad(pad);
                play.pressed_at_ns[play.column_of[lane.index()]] = now_ns;
                if let Some(ms) = at_ms() {
                    inputs.push((ms, lane, false));
                }
            }
            // Inside a roll, the hand's shoulder button plays the roll's lane.
            (Action::Roll(hand), Phase::Pressed) if playing => {
                let Some(ms) = at_ms() else { continue };
                if let Some(roll) = play
                    .rolls
                    .iter()
                    .find(|r| r.hand == hand && r.start_ms - reach <= ms && ms <= r.end_ms + reach)
                {
                    let lane = Lane::Pad(roll.pad);
                    play.pressed_at_ns[play.column_of[lane.index()]] = now_ns;
                    inputs.push((ms, lane, false));
                }
            }
            (Action::Rail(hand), phase) if playing => {
                let rail = rail_of(hand);
                let down = phase == Phase::Pressed;
                play.rail_down[rail.index()] = down;
                if let Some(ms) = at_ms() {
                    inputs.push((ms, Lane::Rail(rail), !down));
                }
            }
            (Action::Pause, Phase::Pressed) if play.failed_at_ns.is_none() => {
                play.paused = !play.paused;
                audio.send(if play.paused { Command::Stop } else { Command::Play });
            }
            (Action::Select, Phase::Pressed) => {
                next.set(Screen::Songs);
                return;
            }
            _ => {}
        }
    }
    let Some(now_ms) = song_ms(&audio, now_ns) else { return };
    play.now_ms = now_ms;
    // Arm the shoulders with the lane of the roll coming up, and the rails with
    // the next bass note each holds, so the input thread plays them straight away.
    for hand in [Hand::Left, Hand::Right] {
        let roll = play
            .rolls
            .iter()
            .find(|r| r.hand == hand && r.start_ms - ROLL_ARM_MS <= now_ms && now_ms <= r.end_ms + reach)
            .map(|r| r.pad)
            .filter(|_| !play.autoplay);
        input.set_roll_pad(hand, roll);
        let cue = play
            .cues
            .iter()
            .find(|c| hand_of(c.rail) == hand && c.start_ms + reach >= now_ms)
            .map(|c| c.note)
            .filter(|_| !play.autoplay);
        input.set_rail_note(hand, cue);
    }
    if play.paused || play.failed_at_ns.is_some() {
        if play.failed_at_ns.is_some_and(|at| now_ns - at > FAIL_PAUSE_NS) {
            finish(play, &session, &mut commands, &mut next, true);
        }
        return;
    }
    if play.autoplay {
        // The selecta bot: every note dead on time, every hold to its end.
        while let Some(note) = play.notes.get(play.autoplay_next).copied() {
            if note.ms > now_ms {
                break;
            }
            inputs.push((note.ms, note.lane, false));
            if let Some(span) = note.hold {
                play.autoplay_releases.push((span.end_ms, note.lane));
            }
            play.autoplay_next += 1;
        }
        play.autoplay_releases.retain(|&(end_ms, lane)| {
            let due = end_ms <= now_ms;
            if due {
                inputs.push((end_ms, lane, true));
            }
            !due
        });
    }
    inputs.sort_by(|a, b| a.0.total_cmp(&b.0));
    if play.autoplay {
        for &(_, lane, up) in &inputs {
            if let Lane::Rail(rail) = lane {
                play.rail_down[rail.index()] = !up;
            } else {
                play.pressed_at_ns[play.column_of[lane.index()]] = now_ns;
            }
        }
    }
    for (ms, lane, released) in inputs {
        let outcomes = if released {
            play.run.release(lane, ms)
        } else {
            play.run.press(lane, ms)
        };
        note_feedback(play, &outcomes, now_ns, &mut commands);
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

/// Places each roll's band between its first and last notes, or hides it.
fn draw_rolls(play: Option<Res<Play>>, mut bands: Query<(&RollMark, &mut Node, &mut Visibility)>) {
    let Some(play) = play else { return };
    if !play.now_ms.is_finite() {
        return;
    }
    let view_ms = play.now_ms + play.visual_lead_ms;
    for (mark, mut node, mut visibility) in &mut bands {
        let roll = play.rolls[mark.0];
        // Clipped to the highway: from where notes appear down to the hit line.
        let top = note_y(roll.end_ms, view_ms).max(TOP_Y);
        let bottom = note_y(roll.start_ms, view_ms).min(HIT_Y);
        if roll.start_ms - view_ms > LOOKAHEAD_MS || bottom <= top {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Inherited;
        node.margin.top = px(top - NOTE_H / 2.0);
        node.height = px(bottom - top + NOTE_H);
    }
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
        if play.run.judge().judgement(index).is_some() && !play.run.judge().is_held(index) {
            continue;
        }
        let column = play.column_of[note.lane.index()];
        let width = if column < PAD_COUNT { NOTE_W } else { RAIL_W - 10.0 };
        let entity = commands
            .spawn((
                DespawnOnExit(Screen::Rhythm),
                NoteMark,
                Node {
                    border_radius: BorderRadius::all(px(6)),
                    ..centred_on(column_x(column), note_y(note.ms, view_ms), width, NOTE_H)
                },
                BackgroundColor(play.column_colour[column]),
            ))
            .id();
        play.entities[index] = Some(entity);
    }
    for index in 0..play.entities.len() {
        let Some(entity) = play.entities[index] else { continue };
        let note = play.notes[index];
        // A note's head; a hold reaches up to its end, and while it is held the
        // hit line eats it from below.
        let mut bottom = note_y(note.ms, view_ms) + NOTE_H / 2.0;
        let mut top = bottom - NOTE_H;
        if let Some(span) = note.hold {
            top = top.min(note_y(span.end_ms, view_ms).max(TOP_Y) - NOTE_H / 2.0);
            if play.run.judge().is_held(index) {
                bottom = bottom.min(HIT_Y + NOTE_H / 2.0);
            }
        }
        if top > HIT_Y + 120.0 {
            commands.entity(entity).despawn();
            play.entities[index] = None;
            continue;
        }
        if let Ok((mut node, mut background)) = nodes.get_mut(entity) {
            node.margin.top = px(top);
            node.height = px((bottom - top).max(4.0));
            let colour = play.column_colour[play.column_of[note.lane.index()]];
            background.0 = match play.run.judge().judgement(index) {
                Some(Judgement::Miss) => palette::mix(palette::BACKDROP, colour, 0.25),
                Some(_) if !play.run.judge().is_held(index) => palette::mix(palette::BACKDROP, colour, 0.25),
                _ => colour,
            };
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
        let column = receptor.0;
        let colour = play.column_colour[column];
        // Pads flash on each press; a rail glows for as long as it is held.
        let glow = if column >= PAD_COUNT {
            if play.rail_down[column - PAD_COUNT] { 1.0 } else { 0.0 }
        } else {
            let since = now_ns.saturating_sub(play.pressed_at_ns[column]) as f64 / 1e9;
            (-since / 0.08).exp() as f32
        };
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
