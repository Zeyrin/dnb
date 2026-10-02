//! The RHYTHM screen: the highway. Notes fall toward the hit line: pads in the
//! middle, the bass line on the two rails at the edges. The player's pads and
//! rails sound at once (from the input thread) and are judged here, from their
//! timestamps, against the audio clock, minus the calibrated offset.

use bevy::prelude::*;
use wu_audio::{Command, Report};
use wu_chart::Rail;
use wu_content::settings::AudioMode;
use wu_game::judge::{Judgement, LANE_COUNT, Lane, Outcome, TimedNote};
use wu_game::play::{chart as play_chart, new_run, practice_tempo};
use wu_game::run::{HYPE_TO_WHEEL_UP, Run, SETTLE_MS};
use wu_input::{Action, Button, Hand, Phase, RailNote};
use wu_instruments::{PAD_COUNT, Pad};
use wu_time::{TICKS_PER_BAR, TempoMap, Tick};

use crate::audio::{AudioLink, EngineReport};
use crate::fonts::Fonts;
use crate::imports::Recordings;
use crate::input::{InputLink, PlayerAction};
use crate::palette;
use crate::screens::Screen;
use crate::session::{LastRun, Session};
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
                (play, draw_hype, draw_rolls, draw_notes, draw_hud)
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
/// A WHEEL UP!'s silence while the record is pulled back, in beats.
const REWIND_GAP_BEATS: f64 = 2.0;
/// How far ahead a WHEEL UP! cuts at the soonest: the engine needs the jump
/// before the transport gets there.
const REWIND_NOTICE_MS: f64 = 150.0;
/// Hype phrases are drawn with this many bands, reused as they scroll past.
const HYPE_BANDS: usize = 4;
/// How long the WHEEL UP! banner stays up.
const BANNER_NS: u64 = 1_800_000_000;

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

/// A WHEEL UP! asked for: the song cuts, then goes back `back_ms`.
#[derive(Clone, Copy, Debug)]
struct PendingRewind {
    back_ms: f64,
    /// The cut on the run's timeline.
    cut_ms: f64,
    /// The device frame the engine cut on, once it has.
    cut_device: Option<f64>,
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
    /// The song's id, for the replay.
    song: String,
    title: String,
    run: Run,
    notes: Vec<TimedNote>,
    rolls: Vec<TimedRoll>,
    cues: Vec<RailCue>,
    column_of: [usize; LANE_COUNT],
    column_colour: [Color; COLUMNS],
    /// The practice tempo, for bar lines and frames.
    tempo: TempoMap,
    sample_rate: u32,
    song_length: Tick,
    /// One beat at the practice tempo, for the count-in.
    beat_ms: f64,
    sections: Vec<(String, f64)>,
    /// When the run ends, in song time.
    end_song_ms: f64,
    /// The engine is playing this run's program (until then the clock describes
    /// whatever played before).
    started: bool,
    /// Rewinds so far: the device frame of each cut, and how far it went back.
    /// The run's timeline is song time plus every rewind before that instant.
    rewinds: Vec<(f64, f64)>,
    pending_rewind: Option<PendingRewind>,
    /// Note indices in time order: for drawing them and for the selecta bot.
    order: Vec<usize>,
    /// Hype as last shown, to flash when it rises; when the banner went up.
    hype_seen: f32,
    hype_flash_ns: u64,
    banner_ns: Option<u64>,
    audio_offset_ms: f64,
    visual_lead_ms: f64,
    autoplay: bool,
    /// Classic audio: the song plays the player's part, and a miss mutes it
    /// (an imported tune's recording, a miss muffles).
    classic: bool,
    muted: bool,
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
    /// Song time now (negative in the count-in), and the run's timeline.
    now_song_ms: f64,
    now_ms: f64,
}

impl Play {
    /// The run's timeline at a device frame: song time plus every rewind cut before it.
    fn offset_at(&self, device_frame: f64) -> f64 {
        self.rewinds
            .iter()
            .filter(|&&(cut, _)| cut <= device_frame)
            .map(|&(_, back)| back)
            .sum()
    }

    fn song_ms_at(&self, tick: Tick) -> f64 {
        self.tempo.seconds_at(tick.0 as f64) * 1000.0
    }

    /// Note indices sorted by time, after the notes changed.
    fn reorder(&mut self) {
        let notes = &self.notes;
        let mut order: Vec<usize> = (0..notes.len()).collect();
        order.sort_by(|&a, &b| notes[a].ms.total_cmp(&notes[b].ms).then(a.cmp(&b)));
        self.order = order;
        self.next_spawn = 0;
        self.autoplay_next = 0;
    }
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
struct HypeFill;

#[derive(Component)]
struct HypeBand(usize);

#[derive(Component)]
enum Hud {
    Score,
    Combo,
    Status,
    Centre,
    Hype,
}

#[allow(clippy::too_many_arguments)]
fn enter(
    mut commands: Commands,
    mut audio: NonSendMut<AudioLink>,
    input: NonSend<InputLink>,
    session: Res<Session>,
    library: Res<SongLibrary>,
    recordings: Res<Recordings>,
    settings: Res<SettingsStore>,
    fonts: Res<Fonts>,
    mut next: ResMut<NextState<Screen>>,
) {
    let (Some(song), Some(id)) = (library.get(session.song).cloned(), library.id(session.song)) else {
        next.set(Screen::Songs);
        return;
    };
    // An imported tune plays its recording, at the output's rate: readied on
    // the songs screen, or read now when the game starts straight into it.
    let backing = match &song.recording {
        Some(recording) => {
            let rate = audio.sample_rate();
            let ready = recordings.get(&recording.path, rate).or_else(|| {
                wu_import::decode(&recording.path)
                    .ok()
                    .map(|tune| wu_dsp::resample(&tune.stereo, 2, tune.sample_rate, rate).into())
            });
            let Some(ready) = ready else {
                next.set(Screen::Songs);
                return;
            };
            Some(wu_audio::Backing::new(
                ready,
                rate,
                recording.first_bar_s,
                song.tempo.bpm_at(Tick::ZERO),
                wu_dsp::db_to_gain(recording.gain_db),
            ))
        }
        None => None,
    };
    let tempo = practice_tempo(&song, session.tempo_percent);
    let chart = play_chart(&song, session.difficulty);
    let ms_at = |tick: Tick| tempo.seconds_at(tick.0 as f64) * 1000.0;
    let run = new_run(&song, &chart, &tempo, session.no_fail());
    let notes = run.judge().notes().to_vec();

    let sample_rate = audio.sample_rate();
    let autoplay = session.autoplay;
    let mode = settings.audio_mode();
    // Live: the player's part is left out of the backing, their presses play it.
    // Classic: the song plays it, marked so a miss can mute it.
    let mut program = song.program(
        sample_rate,
        &tempo,
        COUNT_IN_BARS,
        |tick, pad| !autoplay && chart.contains(tick, pad),
        |tick, key| !autoplay && chart.holds_note(tick, key),
        mode,
    );
    let recorded = backing.is_some();
    if let Some(backing) = backing {
        program = program.with_backing(backing);
    }
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
    let end_song_ms = ms_at(song.length).max(last_note_ms) + 1500.0;
    let sections = song
        .sections
        .iter()
        .map(|(name, start, _)| (name.clone(), ms_at(*start)))
        .collect();
    let note_count = notes.len();
    let mut play = Play {
        generation,
        song: id.to_owned(),
        title: song.meta.title.clone(),
        run,
        notes,
        rolls: rolls.clone(),
        cues,
        column_of,
        column_colour,
        beat_ms: 60_000.0 / tempo.bpm_at(Tick::ZERO),
        tempo: tempo.clone(),
        sample_rate,
        song_length: song.length,
        sections,
        end_song_ms,
        started: false,
        rewinds: Vec::new(),
        pending_rewind: None,
        order: Vec::new(),
        hype_seen: 0.0,
        hype_flash_ns: 0,
        banner_ns: None,
        audio_offset_ms: calibration.audio_ms,
        visual_lead_ms: calibration.visual_lead_ms(),
        autoplay,
        classic: (mode == AudioMode::Classic || recorded) && !autoplay,
        muted: false,
        autoplay_next: 0,
        autoplay_releases: Vec::new(),
        next_spawn: 0,
        entities: vec![None; note_count],
        paused: false,
        failed_at_ns: None,
        popups: [Popup::default(); COLUMNS],
        pressed_at_ns: [0; COLUMNS],
        rail_down: [false; 2],
        now_song_ms: f64::NEG_INFINITY,
        now_ms: f64::NEG_INFINITY,
    };
    play.reorder();
    commands.insert_resource(play);

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
        // Hype phrases: a gold band across the pads, under the rolls and notes.
        let left = column_x(0) - LANE_W / 2.0;
        let right = column_x(PAD_COUNT - 1) + LANE_W / 2.0;
        for band in 0..HYPE_BANDS {
            screen
                .spawn((
                    HypeBand(band),
                    Visibility::Hidden,
                    Node {
                        border: UiRect::vertical(px(2)),
                        justify_content: JustifyContent::FlexStart,
                        align_items: AlignItems::FlexStart,
                        padding: UiRect::left(px(6)),
                        ..centred_on((left + right) / 2.0, 0.0, right - left, NOTE_H)
                    },
                    BorderColor::all(palette::FLYER_YELLOW),
                    BackgroundColor(palette::mix(palette::BACKDROP, palette::FLYER_YELLOW, 0.1)),
                ))
                .with_child(label("HYPE", 11.0, palette::FLYER_YELLOW));
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
            // Judgements show over the notes passing under them.
            screen
                .spawn((
                    centred_on(column_x(column), HIT_Y - 44.0, LANE_W + 30.0, 18.0),
                    GlobalZIndex(5),
                ))
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
        // The hype meter, under the score: a notch where WHEEL UP! becomes possible.
        screen
            .spawn(centred_on(460.0, -62.0, 300.0, 16.0))
            .with_child((Hud::Hype, label("", 12.0, palette::MUTED)));
        screen
            .spawn((
                Node {
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(5)),
                    overflow: Overflow::clip(),
                    justify_content: JustifyContent::FlexStart,
                    ..centred_on(460.0, -42.0, 220.0, 12.0)
                },
                BorderColor::all(palette::MUTED),
            ))
            .with_child((
                HypeFill,
                Node {
                    width: percent(0),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(palette::FLYER_YELLOW),
            ));
        screen.spawn((
            centred_on(460.0 - 110.0 + 220.0 * HYPE_TO_WHEEL_UP, -42.0, 2.0, 18.0),
            BackgroundColor(palette::INK),
        ));
        screen
            .spawn(centred_on(-470.0, -130.0, 280.0, 80.0))
            .with_child((Hud::Status, label("", 14.0, palette::MUTED)));
        screen
            .spawn((centred_on(0.0, 40.0, 900.0, 90.0), GlobalZIndex(5)))
            .with_child((
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

/// Where an instant falls for this run.
#[derive(Clone, Copy, Debug)]
struct Moment {
    song_ms: f64,
    /// The run's timeline: song time plus every rewind before this instant.
    timeline_ms: f64,
    /// Not paused, not before the start, not in a rewind's gap.
    playing: bool,
    device_frame: f64,
}

fn moment(play: &Play, audio: &AudioLink, at_ns: u64) -> Option<Moment> {
    let point = audio.transport_at(at_ns)?;
    let song_ms = point.song_frame / f64::from(play.sample_rate) * 1000.0;
    // A cut the run hasn't taken yet: from it on, the song is in the gap.
    let in_gap = play
        .pending_rewind
        .and_then(|p| p.cut_device)
        .is_some_and(|cut| point.device_frame >= cut);
    Some(Moment {
        song_ms,
        timeline_ms: song_ms + play.offset_at(point.device_frame),
        playing: point.playing && !in_gap,
        device_frame: point.device_frame,
    })
}

/// Plans a WHEEL UP!: the cut on the next bar line far enough ahead for the
/// engine, back to the start of the 8-bar phrase that bar line ends.
fn plan_rewind(play: &mut Play) -> Option<Command> {
    if play.pending_rewind.is_some() || !play.run.can_wheel_up() || play.now_song_ms < 0.0 {
        return None;
    }
    let soonest = play
        .tempo
        .tick_at_seconds((play.now_song_ms + REWIND_NOTICE_MS) / 1000.0);
    let cut_bar = (soonest / TICKS_PER_BAR as f64).ceil() as i64;
    let cut = Tick::from_bars(cut_bar);
    let to = Tick::from_bars((cut_bar - 1).div_euclid(8) * 8);
    if cut >= play.song_length || to >= cut {
        return None;
    }
    let cut_song_ms = play.song_ms_at(cut);
    let back_ms = cut_song_ms - play.song_ms_at(to);
    let beat_s = 60.0 / play.tempo.bpm_at(cut);
    let gap_frames = (REWIND_GAP_BEATS * beat_s * f64::from(play.sample_rate)).round() as u32;
    play.pending_rewind = Some(PendingRewind {
        back_ms,
        cut_ms: cut_song_ms + play.offset_at(f64::INFINITY),
        cut_device: None,
    });
    Some(Command::Jump {
        at: cut,
        to,
        gap_frames,
    })
}

/// Classic audio: a miss mutes the player's part, the next hit brings it back.
fn classic_mute(play: &mut Play, outcomes: &[Outcome], audio: &mut AudioLink) {
    if !play.classic {
        return;
    }
    let latest = outcomes.iter().rev().find_map(|outcome| match outcome {
        Outcome::Hit { .. } => Some(false),
        Outcome::Missed { .. } => Some(true),
        _ => None,
    });
    if let Some(muted) = latest
        && muted != play.muted
    {
        play.muted = muted;
        audio.send(Command::MutePlayer(muted));
    }
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

#[allow(clippy::too_many_arguments)]
fn play(
    mut commands: Commands,
    play: Option<ResMut<Play>>,
    mut actions: MessageReader<PlayerAction>,
    mut reports: MessageReader<EngineReport>,
    mut audio: NonSendMut<AudioLink>,
    mut input: NonSendMut<InputLink>,
    session: Res<Session>,
    mut next: ResMut<NextState<Screen>>,
) {
    let Some(mut play) = play else { return };
    let play = &mut *play;
    // Until the engine plays this run's program, the clock still describes
    // whatever played before (and would miss every note up to its position).
    if !play.started {
        if audio.is_live(play.generation) {
            play.started = true;
        } else {
            actions.clear();
            reports.clear();
            return;
        }
    }
    for EngineReport(report) in reports.read() {
        if let Report::Jumped { device_frame, .. } = *report
            && let Some(pending) = play.pending_rewind.as_mut()
        {
            pending.cut_device = Some(device_frame as f64);
        }
    }
    let now_ns = wu_time::mono::now_ns();
    let reach = play.run.judge().windows().safe;
    let Some(now) = moment(play, &audio, now_ns) else {
        return;
    };
    play.now_song_ms = now.song_ms;
    // Through a rewind's gap the highway holds still at the cut.
    play.now_ms = match play.pending_rewind {
        Some(PendingRewind {
            cut_device: Some(cut),
            cut_ms,
            ..
        }) if now.device_frame >= cut => cut_ms,
        _ => now.timeline_ms,
    };
    // What the player did this frame: (timeline ms, lane, let go).
    let mut inputs: Vec<(f64, Lane, bool)> = Vec::new();
    let mut wheel_up = false;
    for PlayerAction(action) in actions.read() {
        let playing = !play.autoplay && !play.paused;
        // When it happened, in song time and on the timeline; nothing in a gap or a pause.
        let at = moment(play, &audio, action.at_ns)
            .filter(|m| m.playing)
            .map(|m| (m.song_ms - play.audio_offset_ms, m.timeline_ms - play.audio_offset_ms));
        match (action.action, action.phase) {
            (Action::Pad(pad), Phase::Pressed) if playing => {
                let lane = Lane::Pad(pad);
                play.pressed_at_ns[play.column_of[lane.index()]] = now_ns;
                if let Some((_, ms)) = at {
                    inputs.push((ms, lane, false));
                }
            }
            // Inside a roll, the hand's shoulder button plays the roll's lane.
            (Action::Roll(hand), Phase::Pressed) if playing => {
                let Some((song_ms, ms)) = at else { continue };
                if let Some(roll) = play
                    .rolls
                    .iter()
                    .find(|r| r.hand == hand && r.start_ms - reach <= song_ms && song_ms <= r.end_ms + reach)
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
                if let Some((_, ms)) = at {
                    inputs.push((ms, Lane::Rail(rail), !down));
                }
            }
            (Action::WheelUp, Phase::Pressed) if playing => wheel_up = true,
            (Action::Pause, Phase::Pressed) if play.failed_at_ns.is_none() && play.pending_rewind.is_none() => {
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
    // Arm the shoulders with the lane of the roll coming up, and the rails with
    // the next bass note each holds, so the input thread plays them straight away.
    let now_song_ms = play.now_song_ms;
    for hand in [Hand::Left, Hand::Right] {
        let roll = play
            .rolls
            .iter()
            .find(|r| r.hand == hand && r.start_ms - ROLL_ARM_MS <= now_song_ms && now_song_ms <= r.end_ms + reach)
            .map(|r| r.pad)
            .filter(|_| !play.autoplay);
        input.set_roll_pad(hand, roll);
        let cue = play
            .cues
            .iter()
            .find(|c| hand_of(c.rail) == hand && c.start_ms + reach >= now_song_ms)
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
    // WHEEL UP!, asked for by both sticks, or by the selecta bot at the end of a phrase.
    let bot_pulls_up = play.autoplay && {
        let bar = play.tempo.tick_at_seconds(now_song_ms / 1000.0) / TICKS_PER_BAR as f64;
        bar >= 0.0 && (bar.floor() as i64).rem_euclid(8) == 7
    };
    if (wheel_up || bot_pulls_up)
        && let Some(command) = plan_rewind(play)
    {
        audio.send(command);
    }
    if play.autoplay {
        // The selecta bot: every note dead on time, every hold to its end.
        while let Some(&index) = play.order.get(play.autoplay_next) {
            let note = play.notes[index];
            if note.ms > play.now_ms {
                break;
            }
            play.autoplay_next += 1;
            if play.run.judge().judgement(index).is_some() {
                continue;
            }
            inputs.push((note.ms, note.lane, false));
            if let Some(span) = note.hold {
                play.autoplay_releases.push((span.end_ms, note.lane));
            }
        }
        let now_ms = play.now_ms;
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
        classic_mute(play, &outcomes, &mut audio);
        note_feedback(play, &outcomes, now_ns, &mut commands);
    }
    let missed = play.run.settle(play.now_ms);
    classic_mute(play, &missed, &mut audio);
    note_feedback(play, &missed, now_ns, &mut commands);
    // The cut reached the run once every press before it has surely arrived.
    let settle_frames = SETTLE_MS / 1000.0 * f64::from(play.sample_rate);
    if let Some(pending) = play.pending_rewind
        && let Some(cut) = pending.cut_device
        && now.device_frame >= cut + settle_frames
    {
        play.pending_rewind = None;
        if let Some((outcomes, _)) = play.run.wheel_up(pending.cut_ms, pending.back_ms) {
            play.rewinds.push((cut, pending.back_ms));
            note_feedback(play, &outcomes, now_ns, &mut commands);
            play.notes = play.run.judge().notes().to_vec();
            play.entities.resize(play.notes.len(), None);
            play.autoplay_releases.clear();
            play.rail_down = [false; 2];
            play.reorder();
        }
    }
    if play.run.hype() > play.hype_seen + 1e-6 {
        play.hype_flash_ns = now_ns;
    }
    play.hype_seen = play.run.hype();
    if play.pending_rewind.is_some_and(|p| p.cut_device.is_some())
        && play.banner_ns.is_none_or(|at| now_ns - at > BANNER_NS)
    {
        play.banner_ns = Some(now_ns);
    }
    if play.run.score().failed {
        // PLUG PULLED: the power cuts, the run is over.
        play.failed_at_ns = Some(now_ns);
        audio.send(Command::Stop);
    } else if now_song_ms > play.end_song_ms && play.pending_rewind.is_none() {
        finish(play, &session, &mut commands, &mut next, false);
    }
}

fn finish(play: &mut Play, session: &Session, commands: &mut Commands, next: &mut NextState<Screen>, failed: bool) {
    play.run.finish();
    commands.insert_resource(LastRun {
        song: play.song.clone(),
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

/// Lays the hype bands over the phrases in view: gold while clean, grey once broken.
fn draw_hype(
    play: Option<Res<Play>>,
    mut bands: Query<(
        &HypeBand,
        &mut Node,
        &mut Visibility,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    let Some(play) = play else { return };
    if !play.now_ms.is_finite() {
        return;
    }
    let view_ms = play.now_ms + play.visual_lead_ms;
    let in_view: Vec<(f64, f64, bool)> = play
        .run
        .phrases()
        .filter(|&(start, end, _)| start - view_ms <= LOOKAHEAD_MS && note_y(end, view_ms) < HIT_Y)
        .collect();
    for (band, mut node, mut visibility, mut background, mut border) in &mut bands {
        let Some(&(start, end, clean)) = in_view.get(band.0) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let top = note_y(end, view_ms).max(TOP_Y);
        let bottom = note_y(start, view_ms).min(HIT_Y);
        if bottom <= top {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Inherited;
        node.margin.top = px(top);
        node.height = px(bottom - top);
        let colour = if clean { palette::FLYER_YELLOW } else { palette::MUTED };
        background.0 = palette::mix(palette::BACKDROP, colour, 0.1);
        *border = BorderColor::all(colour);
    }
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
    while let Some(&index) = play.order.get(play.next_spawn) {
        let note = play.notes[index];
        if note.ms - view_ms > LOOKAHEAD_MS + 100.0 {
            break;
        }
        play.next_spawn += 1;
        let done = play.run.judge().judgement(index).is_some() && !play.run.judge().is_held(index);
        if done || play.entities[index].is_some() {
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
    mut fill: Query<
        (&mut Node, &mut BackgroundColor),
        (With<VibeFill>, Without<HypeFill>, Without<Receptor>, Without<NoteMark>),
    >,
    mut hype_fill: Query<&mut Node, (With<HypeFill>, Without<VibeFill>, Without<Receptor>, Without<NoteMark>)>,
    mut huds: Query<(&Hud, &mut Text, &mut TextColor), Without<PopupText>>,
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
    // During a WHEEL UP! replay the meter shows the boost draining instead.
    let boost_left = play
        .run
        .boost()
        .filter(|&(from, to)| from <= play.now_ms && play.now_ms < to)
        .map(|(from, to)| ((to - play.now_ms) / (to - from)) as f32);
    if let Ok(mut node) = hype_fill.single_mut() {
        node.width = percent(100.0 * boost_left.unwrap_or(play.run.hype()));
    }
    let section = play
        .sections
        .iter()
        .rev()
        .find(|(_, start)| *start <= play.now_song_ms)
        .map_or("Count-in", |(name, _)| name.as_str());
    let last_offset = play
        .popups
        .iter()
        .filter(|p| p.judgement.is_some_and(|j| j != Judgement::Miss))
        .max_by_key(|p| p.at_ns)
        .map(|p| p.offset_ms);
    let beats_to_go = (-play.now_song_ms / play.beat_ms).ceil();
    let banner = play.banner_ns.is_some_and(|at| now_ns.saturating_sub(at) < BANNER_NS);
    for (hud, mut text, mut colour) in &mut huds {
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
                } else if banner {
                    "WHEEL UP!".to_owned()
                } else if play.now_song_ms < 0.0 && play.now_song_ms.is_finite() {
                    format!("{}", beats_to_go.max(1.0))
                } else {
                    String::new()
                }
            }
            Hud::Hype => {
                let flash = now_ns.saturating_sub(play.hype_flash_ns) < 600_000_000;
                colour.0 = if flash || play.run.can_wheel_up() || boost_left.is_some() {
                    palette::FLYER_YELLOW
                } else {
                    palette::MUTED
                };
                if boost_left.is_some() {
                    "WHEEL UP!  multiplier doubled".to_owned()
                } else if play.pending_rewind.is_some() {
                    "pulling up…".to_owned()
                } else if play.run.can_wheel_up() {
                    format!("HYPE {:.0} %  ·  L3 + R3: WHEEL UP!", play.run.hype() * 100.0)
                } else {
                    format!("HYPE {:.0} %", play.run.hype() * 100.0)
                }
            }
        };
    }
}
