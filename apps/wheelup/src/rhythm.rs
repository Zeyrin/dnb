//! The RHYTHM screen: the highway. Notes fall toward the hit line: pads in the
//! middle, the bass line on the two rails at the edges. The player's pads and
//! rails sound at once (from the input thread) and are judged here, from their
//! timestamps, against the audio clock, minus the calibrated offset.

use bevy::prelude::*;
use wu_audio::{Command, Hit, Report};
use wu_chart::Rail;
use wu_content::settings::{AudioMode, Language};
use wu_game::judge::{Judgement, LANE_COUNT, Lane, Outcome, TimedNote};
use wu_game::play::{chart as play_chart, chart_between, new_run, practice_tempo};
use wu_game::run::{HYPE_TO_WHEEL_UP, Run, SETTLE_MS};
use wu_game::score::Score;
use wu_input::{Action, Button, Hand, Layout, Phase, RailNote};
use wu_instruments::{PAD_COUNT, Pad};
use wu_time::{TICKS_PER_BAR, TempoMap, Tick};

use crate::audio::{AudioLink, EngineReport};
use crate::drawn::Drawn;
use crate::fonts::Fonts;
use crate::highway::{
    Burst, GEM_GLOW, Looks, Z_BAND, Z_BEAM, Z_BURST, Z_FIELD, Z_HIT_LINE, Z_LANE_GLOW, Z_NOTE, Z_RECEPTOR, fade_bursts,
    glowing, see_through, spawn_vinyl,
};
use crate::imports::Recordings;
use crate::input::{InputLink, PlayerAction};
use crate::palette;
use crate::screens::Screen;
use crate::session::{LastRun, Session};
use crate::settings::SettingsStore;
use crate::songs_screen::SongLibrary;
use crate::stage::{Scene, StageMood};
use crate::tour_screen::TourData;
use crate::ui::{centred_label, centred_on, label, screen_root};
use crate::words::{decimal, fill, tr};

#[derive(Debug)]
pub struct RhythmPlugin;

impl Plugin for RhythmPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Rhythm), enter)
            .add_systems(OnExit(Screen::Rhythm), exit)
            .add_systems(
                Update,
                (
                    play,
                    draw_bands,
                    draw_notes,
                    set_the_mood,
                    spin_back,
                    light_receptors,
                    spawn_bursts,
                    fade_bursts,
                    draw_hud,
                )
                    .chain()
                    .run_if(in_state(Screen::Rhythm)),
            );
    }
}

const COUNT_IN_BARS: i64 = 2;
/// Frames in a row with everything drawn before the music starts.
const DRAWN_FRAMES: u32 = 4;
/// The music never holds longer than this for the stage, in nanoseconds.
const MOST_HELD_NS: u64 = 5_000_000_000;
/// How far ahead notes appear at note speed 1, in song milliseconds.
const LOOKAHEAD_MS: f64 = 2000.0;
/// Where notes meet the hit line, and where they appear: in the world, up
/// from the centre of the screen (the interface counts down: see `ui_y`).
const HIT_Y: f32 = -250.0;
const TOP_Y: f32 = 330.0;
const LANE_W: f32 = 66.0;
const HAND_GAP: f32 = 44.0;
const RAIL_W: f32 = 40.0;
/// Between a rail and the pad lanes beside it.
const RAIL_GAP: f32 = 18.0;
const NOTE_W: f32 = 56.0;
const NOTE_H: f32 = 20.0;
/// How long a kick lights the stage (time constant).
const KICK_GLOW_S: f64 = 0.11;
/// How long a press lights its receptor (time constant).
const PRESS_GLOW_S: f64 = 0.08;
/// How long a judgement stays on screen.
const POPUP_NS: u64 = 450_000_000;
/// After the plug is pulled, how long before the results.
const FAIL_PAUSE_NS: u64 = 2_500_000_000;
/// How long before a roll its shoulder button starts playing the roll's lane.
const ROLL_ARM_MS: f64 = 400.0;
/// A WHEEL UP!'s gap while the record is pulled back, in beats: a bar, so the
/// tune drops in again on the one.
const REWIND_GAP_BEATS: f64 = 4.0;
/// How far a WHEEL UP! pulls the tune back: a whole phrase, eight bars, about
/// ten seconds at jungle tempo, and the same bar of the phrase comes round.
const REWIND_BARS: i64 = 8;
/// How far ahead a WHEEL UP! cuts at the soonest: the engine needs the jump
/// before the transport gets there.
const REWIND_NOTICE_MS: f64 = 150.0;
/// Hype phrases are drawn with this many bands, reused as they scroll past.
const HYPE_BANDS: usize = 4;
/// How long the WHEEL UP! banner stays up.
const BANNER_NS: u64 = 1_800_000_000;
/// The record a WHEEL UP! pulls back: how big, how fast it spins back at first
/// (radians a second), how long it spins, and when it is gone (seconds).
const SPINBACK_R: f32 = 170.0;
const SPINBACK_SPEED: f32 = 16.0;
const SPINBACK_S: f32 = 1.0;
const SPINBACK_GONE_S: f32 = 1.35;
/// Over the notes and their bursts.
const Z_SPINBACK: f32 = 30.0;

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

impl Play {
    /// Where a note at `note_ms` is, seen at `view_ms`.
    fn note_y(&self, note_ms: f64, view_ms: f64) -> f32 {
        let speed = f64::from(TOP_Y - HIT_Y) / self.lookahead_ms;
        HIT_Y + ((note_ms - view_ms) * speed) as f32
    }
}

/// The interface's height for a height in the world: it counts down from the centre.
fn ui_y(world_y: f32) -> f32 {
    -world_y
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

/// Why the song goes back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rewind {
    /// The player pulled the tune up: hype spent, the multiplier doubled.
    WheelUp,
    /// Practice: the loop came round.
    Loop,
}

/// A WHEEL UP! asked for (the song cuts, then goes back `back_ms`), or a
/// practice loop come round.
#[derive(Clone, Copy, Debug)]
struct PendingRewind {
    kind: Rewind,
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

/// Practice: one section, looped, a bar of the song before it as a run-up.
#[derive(Clone, Debug)]
struct Practice {
    name: String,
    /// The section, in song milliseconds.
    start_ms: f64,
    end_ms: f64,
    /// Once round the loop, run-up and all.
    loop_ms: f64,
    /// Each pass's accuracy so far, and the judgements counted before this one.
    passes: Vec<f64>,
    counts_before: [u32; 4],
}

/// A lesson's words, in song milliseconds: shown from when its notes come into view.
#[derive(Clone, Debug)]
struct LessonCue {
    title: String,
    caption: String,
    start_ms: f64,
}

/// After the pass number: the best pass so far, then the last few, the latest last.
fn passes_line(passes: &[f64], language: Language) -> String {
    let Some(best) = passes.iter().copied().reduce(f64::max) else {
        return String::new();
    };
    let recent: Vec<String> = passes
        .iter()
        .rev()
        .take(3)
        .rev()
        .map(|a| format!("{:.0}", a * 100.0))
        .collect();
    fill(
        tr(language, " · best {} %\n{} %"),
        &[&format!("{:.0}", best * 100.0), &recent.join(" → ")],
    )
}

/// A caption with `{P1}`–`{P8}` replaced by the buttons that play those pads.
fn buttons_named(caption: &str, layout: Layout) -> String {
    Pad::ALL.into_iter().fold(caption.to_owned(), |text, pad| {
        text.replace(&format!("{{P{}}}", pad.index() + 1), layout.button_for(pad).glyph())
    })
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
    /// What each section teaches, in a lesson.
    lessons: Vec<LessonCue>,
    /// The section looped, in practice.
    practice: Option<Practice>,
    /// The venue it plays at: its stop on the tour.
    scene: Scene,
    /// The language the HUD speaks.
    language: Language,
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
    /// When the music was asked for, while it holds for the stage to be drawn.
    held_since_ns: Option<u64>,
    /// Practice's Wait mode is on: the song stands still on a note until it is hit.
    wait: bool,
    /// The note the song stands still on.
    waiting: Option<usize>,
    failed_at_ns: Option<u64>,
    popups: [Popup; COLUMNS],
    pressed_at_ns: [u64; COLUMNS],
    /// The rails held down right now.
    rail_down: [bool; 2],
    /// When each kick lands, in song time: the stage pulses with them.
    kicks_ms: Vec<f64>,
    /// Hits and misses still to light up: their column and judgement.
    bursts: Vec<(usize, Judgement)>,
    /// How far ahead notes appear, at the player's note speed.
    lookahead_ms: f64,
    /// Song time now (negative in the count-in), and the run's timeline.
    now_song_ms: f64,
    now_ms: f64,
}

impl Play {
    /// A lesson: charted from what each section teaches, never failed.
    fn lesson(&self) -> bool {
        !self.lessons.is_empty()
    }

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

/// A note's lit body, dimmed once it is missed.
#[derive(Component)]
struct NoteGem;

/// A hold's tail, stretched from its head up to where it ends.
#[derive(Component)]
struct HoldBody;

/// A part of a band laid over the highway: a roll's or a hype phrase's.
#[derive(Component, Clone, Copy)]
struct BandPart {
    band: Band,
    part: Part,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Band {
    Roll(usize),
    Hype(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Fill,
    Top,
    Bottom,
    Left,
    Right,
    Label,
}

#[derive(Component)]
struct HitLine;

/// The record a WHEEL UP! pulls back, over the highway.
#[derive(Component)]
struct Spinback;

/// The materials the highway's lights are drawn in, per column.
#[derive(Resource)]
struct Lights {
    gem: Vec<Handle<ColorMaterial>>,
    dim: Vec<Handle<ColorMaterial>>,
    hold: Vec<Handle<ColorMaterial>>,
    /// A receptor's shape, brightening as it is pressed.
    receptor: Vec<Handle<ColorMaterial>>,
    /// A lane's glow from below.
    glow: Vec<Handle<ColorMaterial>>,
    hit_line: Handle<ColorMaterial>,
}

#[derive(Component)]
struct PopupText(usize);

#[derive(Component)]
struct VibeFill;

#[derive(Component)]
struct HypeFill;

#[derive(Component)]
enum Hud {
    Score,
    /// The combo, big and faint behind the notes' path.
    BigCombo,
    Combo,
    Status,
    Centre,
    Hype,
    /// In a lesson: which step it is, what it teaches, and what to do.
    LessonStep,
    LessonTitle,
    Lesson,
}

#[allow(clippy::too_many_arguments)]
fn enter(
    mut commands: Commands,
    mut audio: NonSendMut<AudioLink>,
    input: NonSend<InputLink>,
    session: Res<Session>,
    library: Res<SongLibrary>,
    tour: Res<TourData>,
    recordings: Res<Recordings>,
    settings: Res<SettingsStore>,
    fonts: Res<Fonts>,
    mut next: ResMut<NextState<Screen>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut drawn: ResMut<Drawn>,
) {
    let (Some(mut song), Some(id)) = (library.get(session.song).cloned(), library.id(session.song)) else {
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
    let ms_at = |tick: Tick| tempo.seconds_at(tick.0 as f64) * 1000.0;
    // Practice loops one section, a bar of the song before it as its run-up;
    // only its notes are charted, and it can't be failed.
    let language = settings.language();
    let section = session.practice.and_then(|index| {
        let (_, start, end) = song.sections.get(index)?;
        Some((song.section_name(index, language)?.to_owned(), *start, *end))
    });
    let mut chart = match &section {
        Some((_, start, end)) => chart_between(play_chart(&song, session.difficulty), *start, *end),
        None => play_chart(&song, session.difficulty),
    };
    // The drums are the player's; the bass only if they asked for it too.
    if !settings.bass_on_triggers() {
        chart.holds.clear();
    }
    // Only the section's own hype phrases: the others, empty, would pay out at once.
    if let Some((_, start, end)) = &section {
        song.hype.retain(|&(from, to)| from < *end && *start < to);
    }
    let run = new_run(&song, &chart, &tempo, session.no_fail() || section.is_some());
    let run_up = Tick::from_bars(1);
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
    // Practice keeps the count-in's click going, on every beat of the loop.
    if let Some((_, start, end)) = &section {
        let beat = Tick::from_beats(1).0;
        let clicks = ((*start - run_up).0.div_euclid(beat)..end.0.div_euclid(beat)).map(|b| Hit {
            tick: Tick::from_beats(b),
            pad: Pad::P4,
            velocity: if b.rem_euclid(4) == 0 { 0.8 } else { 0.55 },
        });
        program = program.with_hits(clicks);
    }
    let recorded = backing.is_some();
    if let Some(backing) = backing {
        program = program.with_backing(backing);
    }
    let generation = audio.load(program);
    match &section {
        // The first time round, the count-in's length of run-up.
        Some((_, start, end)) => {
            audio.send(Command::Seek(*start - Tick::from_bars(COUNT_IN_BARS)));
            audio.send(Command::SetLoop(Some((*start - run_up, *end))));
        }
        None => audio.send(Command::Seek(Tick::from_bars(-COUNT_IN_BARS))),
    }
    // The music holds until the stage is on screen.
    drawn.restart();

    let layout = input.layout();
    let lessons = song
        .lessons
        .iter()
        .map(|lesson| LessonCue {
            title: lesson.name_in(language).to_uppercase(),
            caption: buttons_named(lesson.caption_in(language), layout),
            start_ms: ms_at(lesson.start),
        })
        .collect();
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
    // Practice goes round until the player leaves.
    let end_song_ms = if section.is_some() {
        f64::INFINITY
    } else {
        ms_at(song.length).max(last_note_ms) + 1500.0
    };
    let wait = session.wait && section.is_some() && !autoplay;
    let practice = section.map(|(name, start, end)| Practice {
        name,
        start_ms: ms_at(start),
        end_ms: ms_at(end),
        loop_ms: ms_at(end) - ms_at(start - run_up),
        passes: Vec::new(),
        counts_before: [0; 4],
    });
    let sections = song
        .sections
        .iter()
        .enumerate()
        .map(|(index, (name, start, _))| {
            let name = song.section_name(index, language).unwrap_or(name);
            (name.to_owned(), ms_at(*start))
        })
        .collect();
    let note_count = notes.len();
    let kicks_ms = song
        .drums
        .iter()
        .filter(|hit| hit.pad == Pad::P1)
        .map(|hit| ms_at(hit.tick))
        .collect();
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
        lessons,
        practice,
        scene: Scene::of_song(&tour.0, id),
        language,
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
        held_since_ns: Some(wu_time::mono::now_ns()),
        wait,
        waiting: None,
        failed_at_ns: None,
        popups: [Popup::default(); COLUMNS],
        pressed_at_ns: [0; COLUMNS],
        rail_down: [false; 2],
        kicks_ms,
        bursts: Vec::new(),
        lookahead_ms: LOOKAHEAD_MS / f64::from(settings.note_speed().clamp(0.5, 4.0)),
        now_song_ms: f64::NEG_INFINITY,
        now_ms: f64::NEG_INFINITY,
    };
    play.reorder();
    commands.insert_resource(play);

    // Rails are only drawn when the chart uses them.
    let shown: Vec<usize> = (0..COLUMNS)
        .filter(|&c| c < PAD_COUNT || chart.holds.iter().any(|h| rail_column(h.rail) == c))
        .collect();
    let looks = Looks::new(&LANES, &mut meshes, &mut materials, &mut images);
    let lights = Lights {
        gem: column_colour
            .iter()
            .map(|&c| materials.add(ColorMaterial::from(glowing(c, GEM_GLOW))))
            .collect(),
        dim: column_colour
            .iter()
            .map(|&c| materials.add(ColorMaterial::from(palette::mix(palette::BACKDROP, c, 0.3))))
            .collect(),
        hold: column_colour
            .iter()
            .map(|&c| materials.add(see_through(glowing(c, 0.8), 0.55)))
            .collect(),
        receptor: column_colour
            .iter()
            .map(|&c| materials.add(ColorMaterial::from(c)))
            .collect(),
        glow: column_colour
            .iter()
            .map(|&c| {
                materials.add(ColorMaterial {
                    texture: Some(looks.fade.clone()),
                    ..see_through(c, 0.15)
                })
            })
            .collect(),
        hit_line: materials.add(ColorMaterial::from(glowing(Color::WHITE, 1.4))),
    };
    spawn_highway(&mut commands, &looks, &lights, &shown, &column_colour, &mut materials);
    // A hot pink label, so the yellow WHEEL UP! over it stands out.
    let label = materials.add(ColorMaterial::from(glowing(palette::pad(Pad::P2), 1.3)));
    spawn_vinyl(
        &mut commands,
        &mut meshes,
        &mut materials,
        SPINBACK_R,
        label,
        (
            DespawnOnExit(Screen::Rhythm),
            Spinback,
            Visibility::Hidden,
            Transform::from_xyz(0.0, 40.0, Z_SPINBACK),
        ),
        (),
    );
    spawn_bands(
        &mut commands,
        &looks,
        &rolls,
        &column_of,
        &column_colour,
        &fonts,
        &mut materials,
    );
    spawn_hud(&mut commands, &shown, &fonts, song.is_lesson());
    commands.insert_resource(looks);
    commands.insert_resource(lights);
}

/// The highway's furniture, in the world: dark glass over the venue, lane
/// lines, each lane's glow, the hit line, and a receptor shaped like its button.
fn spawn_highway(
    commands: &mut Commands,
    looks: &Looks,
    lights: &Lights,
    shown: &[usize],
    column_colour: &[Color; COLUMNS],
    materials: &mut Assets<ColorMaterial>,
) {
    let place = |x: f32, y: f32, width: f32, height: f32, z: f32| Transform {
        translation: Vec3::new(x, y, z),
        scale: Vec3::new(width, height, 1.0),
        ..default()
    };
    let piece = |material: &Handle<ColorMaterial>, at: Transform| {
        (
            DespawnOnExit(Screen::Rhythm),
            Mesh2d(looks.unit.clone()),
            MeshMaterial2d(material.clone()),
            at,
        )
    };
    let (left, right) = pads_span();
    let (bottom, top) = (HIT_Y - 46.0, TOP_Y + 30.0);
    let glass = materials.add(see_through(Color::srgb(0.012, 0.008, 0.025), 0.88));
    commands.spawn(piece(
        &glass,
        place(
            (left + right) / 2.0,
            (top + bottom) / 2.0,
            right - left,
            top - bottom,
            Z_FIELD,
        ),
    ));
    for &column in shown.iter().filter(|&&c| c >= PAD_COUNT) {
        commands.spawn(piece(
            &glass,
            place(
                column_x(column),
                (top + bottom) / 2.0,
                RAIL_W + 8.0,
                top - bottom,
                Z_FIELD,
            ),
        ));
    }
    // Lane lines, faint; the one between the hands a little brighter.
    let line = materials.add(see_through(Color::srgb(0.55, 0.5, 0.75), 0.14));
    let hands = materials.add(see_through(Color::srgb(0.55, 0.5, 0.75), 0.32));
    for lane in 1..PAD_COUNT {
        let (x, material) = if lane == 4 {
            ((column_x(3) + column_x(4)) / 2.0, &hands)
        } else {
            (column_x(lane) - LANE_W / 2.0, &line)
        };
        commands.spawn(piece(
            material,
            place(x, (top + bottom) / 2.0, 1.5, top - bottom, Z_FIELD + 0.5),
        ));
    }
    for &column in shown {
        commands.spawn(piece(
            &lights.glow[column],
            place(
                column_x(column),
                HIT_Y + 110.0,
                column_width(column) - 6.0,
                220.0,
                Z_LANE_GLOW,
            ),
        ));
    }
    commands.spawn((
        HitLine,
        piece(
            &lights.hit_line,
            place((left + right) / 2.0, HIT_Y, right - left - 12.0, 3.0, Z_HIT_LINE),
        ),
    ));
    for &column in shown {
        let base = materials.add(ColorMaterial::from(palette::mix(
            palette::BACKDROP,
            column_colour[column],
            0.14,
        )));
        commands
            .spawn((
                DespawnOnExit(Screen::Rhythm),
                Transform::from_xyz(column_x(column), HIT_Y, Z_RECEPTOR),
                Visibility::default(),
            ))
            .with_children(|receptor| {
                receptor.spawn((
                    Mesh2d(looks.unit.clone()),
                    MeshMaterial2d(base),
                    Transform::from_scale(Vec3::new(column_width(column) - 12.0, 40.0, 1.0)),
                ));
                if column < PAD_COUNT {
                    looks.spawn_glyph(receptor, column, 24.0, 0.5, &lights.receptor[column], ());
                } else {
                    // A rail: its trigger, as a bar.
                    receptor.spawn((
                        Mesh2d(looks.unit.clone()),
                        MeshMaterial2d(lights.receptor[column].clone()),
                        Transform {
                            translation: Vec3::new(0.0, 0.0, 0.5),
                            scale: Vec3::new(RAIL_W - 20.0, 6.0, 1.0),
                            ..default()
                        },
                    ));
                }
            });
    }
}

/// Where the pad lanes start and end, left to right.
fn pads_span() -> (f32, f32) {
    (
        column_x(0) - LANE_W / 2.0 - 8.0,
        column_x(PAD_COUNT - 1) + LANE_W / 2.0 + 8.0,
    )
}

/// Bands over the highway, placed as they scroll by, edged in light and
/// labelled: a hype phrase framed in gold across the pads (framed, not filled:
/// over black, the faintest fill shows plainly, and a drop is all hype), a
/// roll tinting its lane.
fn spawn_bands(
    commands: &mut Commands,
    looks: &Looks,
    rolls: &[TimedRoll],
    column_of: &[usize; LANE_COUNT],
    column_colour: &[Color; COLUMNS],
    fonts: &Fonts,
    materials: &mut Assets<ColorMaterial>,
) {
    let mut spawn = |commands: &mut Commands, band: Band, colour: Color, label: String| {
        let fill = materials.add(see_through(colour, 0.012));
        let edge = materials.add(ColorMaterial::from(glowing(colour, 1.5)));
        let parts: &[(Part, &Handle<ColorMaterial>)] = match band {
            Band::Hype(_) => &[
                (Part::Top, &edge),
                (Part::Bottom, &edge),
                (Part::Left, &edge),
                (Part::Right, &edge),
            ],
            Band::Roll(_) => &[(Part::Fill, &fill), (Part::Top, &edge), (Part::Bottom, &edge)],
        };
        for &(part, material) in parts {
            commands.spawn((
                DespawnOnExit(Screen::Rhythm),
                BandPart { band, part },
                Visibility::Hidden,
                Mesh2d(looks.unit.clone()),
                MeshMaterial2d(material.clone()),
                Transform::from_xyz(0.0, 0.0, Z_BAND),
            ));
        }
        commands.spawn((
            DespawnOnExit(Screen::Rhythm),
            BandPart {
                band,
                part: Part::Label,
            },
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                top: percent(50),
                ..default()
            },
            Text::new(label),
            TextFont {
                font: fonts.bold.clone().into(),
                ..TextFont::from_font_size(13.0)
            },
            TextColor(colour),
        ));
    };
    for band in 0..HYPE_BANDS {
        spawn(commands, Band::Hype(band), palette::FLYER_YELLOW, "HYPE".to_owned());
    }
    for (index, roll) in rolls.iter().enumerate() {
        let shoulder = match roll.hand {
            Hand::Left => Button::L1,
            Hand::Right => Button::R1,
        };
        let colour = column_colour[column_of[Lane::Pad(roll.pad).index()]];
        spawn(commands, Band::Roll(index), colour, shoulder.glyph().to_owned());
    }
}

/// The lesson card: left of the vibe meter, clear of it.
const LESSON_X: f32 = -510.0;
const LESSON_W: f32 = 236.0;

/// The interface over the highway: judgements, the vibe and hype meters, the
/// score, where the song is, the big words in the middle, and a lesson's card.
fn spawn_hud(commands: &mut Commands, shown: &[usize], fonts: &Fonts, lesson: bool) {
    commands.spawn(screen_root(Screen::Rhythm)).with_children(|screen| {
        for &column in shown {
            // Judgements show over the notes passing under them.
            screen
                .spawn((
                    centred_on(column_x(column), ui_y(HIT_Y + 46.0), LANE_W + 30.0, 20.0),
                    GlobalZIndex(5),
                ))
                .with_child((
                    PopupText(column),
                    Text::new(""),
                    TextFont {
                        font: fonts.bold.clone().into(),
                        ..TextFont::from_font_size(15.0)
                    },
                    TextColor(palette::INK),
                    UiTransform::IDENTITY,
                ));
        }
        // The vibe meter, left of the highway.
        let meter_x = column_x(PAD_COUNT) - RAIL_W / 2.0 - 30.0;
        let meter_h = TOP_Y - HIT_Y;
        screen
            .spawn((
                Node {
                    flex_direction: FlexDirection::ColumnReverse,
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(6)),
                    overflow: Overflow::clip(),
                    ..centred_on(meter_x, ui_y(HIT_Y + meter_h / 2.0), 16.0, meter_h)
                },
                BorderColor::all(palette::MUTED),
                BackgroundColor(palette::BACKDROP.with_alpha(0.7)),
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
            .spawn(centred_on(meter_x, ui_y(HIT_Y - 34.0), 60.0, 16.0))
            .with_child(label("VIBE", 11.0, palette::MUTED));
        // The trigger each rail is played with, under it.
        for (rail, trigger) in [(Rail::Left, "L2"), (Rail::Right, "R2")] {
            let column = rail_column(rail);
            if shown.contains(&column) {
                screen
                    .spawn(centred_on(column_x(column), ui_y(HIT_Y - 34.0), 40.0, 16.0))
                    .with_child(label(trigger, 11.0, palette::MUTED));
            }
        }
        let right_x = 480.0;
        screen.spawn(centred_on(right_x, -270.0, 300.0, 48.0)).with_child((
            Hud::Score,
            Text::new(""),
            TextFont {
                font: fonts.display.clone().into(),
                ..TextFont::from_font_size(36.0)
            },
            TextColor(palette::FLYER_YELLOW),
        ));
        screen
            .spawn(centred_on(right_x, -215.0, 300.0, 50.0))
            .with_child((Hud::Combo, label("", 15.0, palette::INK)));
        // The hype meter, under the score: a notch where WHEEL UP! becomes possible.
        screen
            .spawn(centred_on(right_x, -170.0, 300.0, 16.0))
            .with_child((Hud::Hype, label("", 12.0, palette::MUTED)));
        screen
            .spawn((
                Node {
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(5)),
                    overflow: Overflow::clip(),
                    justify_content: JustifyContent::FlexStart,
                    ..centred_on(right_x, -150.0, 220.0, 12.0)
                },
                BorderColor::all(palette::MUTED),
                BackgroundColor(palette::BACKDROP.with_alpha(0.7)),
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
            centred_on(right_x - 110.0 + 220.0 * HYPE_TO_WHEEL_UP, -150.0, 2.0, 18.0),
            BackgroundColor(palette::INK),
        ));
        screen
            .spawn(centred_on(LESSON_X, -250.0, LESSON_W, 96.0))
            .with_child((Hud::Status, label("", 14.0, palette::MUTED)));
        // A lesson's card, left of the vibe meter: which step, what it is, what to do.
        if lesson {
            screen
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::FlexStart,
                        row_gap: px(10),
                        padding: UiRect::all(px(14)),
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::all(px(8)),
                        ..centred_on(LESSON_X, -30.0, LESSON_W, 270.0)
                    },
                    BorderColor::all(palette::FLYER_YELLOW.with_alpha(0.45)),
                    BackgroundColor(palette::BACKDROP.with_alpha(0.82)),
                ))
                .with_children(|card| {
                    card.spawn((Hud::LessonStep, centred_label("", 12.0, palette::MUTED)));
                    card.spawn((
                        Hud::LessonTitle,
                        Text::new(""),
                        TextFont {
                            font: fonts.display.clone().into(),
                            ..TextFont::from_font_size(26.0)
                        },
                        TextColor(palette::FLYER_YELLOW),
                        TextLayout::justify(Justify::Center),
                    ));
                    card.spawn((
                        Hud::Lesson,
                        Text::new(""),
                        TextFont {
                            font: fonts.bold.clone().into(),
                            ..TextFont::from_font_size(17.0)
                        },
                        TextColor(palette::INK),
                        TextLayout::justify(Justify::Center),
                    ));
                });
        }
        screen.spawn(centred_on(0.0, ui_y(150.0), 500.0, 110.0)).with_child((
            Hud::BigCombo,
            Text::new(""),
            TextFont {
                font: fonts.display.clone().into(),
                ..TextFont::from_font_size(84.0)
            },
            TextColor(palette::FLYER_YELLOW.with_alpha(0.2)),
            TextLayout::justify(Justify::Center),
        ));
        screen
            .spawn((centred_on(0.0, -40.0, 900.0, 90.0), GlobalZIndex(5)))
            .with_child((
                Hud::Centre,
                Text::new(""),
                TextFont {
                    font: fonts.display.clone().into(),
                    ..TextFont::from_font_size(64.0)
                },
                TextColor(palette::FLYER_YELLOW),
                UiTransform::IDENTITY,
            ));
    });
}

fn exit(
    mut commands: Commands,
    mut audio: NonSendMut<AudioLink>,
    mut input: NonSendMut<InputLink>,
    mut mood: ResMut<StageMood>,
) {
    audio.send(Command::Stop);
    *mood = StageMood::default();
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
    if play.practice.is_some() || play.pending_rewind.is_some() || !play.run.can_wheel_up() || play.now_song_ms < 0.0 {
        return None;
    }
    let soonest = play
        .tempo
        .tick_at_seconds((play.now_song_ms + REWIND_NOTICE_MS) / 1000.0);
    let cut_bar = (soonest / TICKS_PER_BAR as f64).ceil() as i64;
    let cut = Tick::from_bars(cut_bar);
    let to = Tick::from_bars((cut_bar - REWIND_BARS).max(0));
    if cut >= play.song_length || to >= cut {
        return None;
    }
    let cut_song_ms = play.song_ms_at(cut);
    let back_ms = cut_song_ms - play.song_ms_at(to);
    let beat_s = 60.0 / play.tempo.bpm_at(cut);
    let gap_frames = (REWIND_GAP_BEATS * beat_s * f64::from(play.sample_rate)).round() as u32;
    play.pending_rewind = Some(PendingRewind {
        kind: Rewind::WheelUp,
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
        play.bursts.push((column, judgement));
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
    drawn: Res<Drawn>,
) {
    let Some(mut play) = play else { return };
    let play = &mut *play;
    // Until the engine plays this run's program, the clock still describes
    // whatever played before (and would miss every note up to its position).
    if !play.started {
        // The music starts once the stage is drawn: the first time it shows,
        // its shaders compile, and notes falling on a black screen are missed.
        if let Some(since) = play.held_since_ns
            && !crate::esc_menu::is_open()
            && (drawn.for_frames(DRAWN_FRAMES) || wu_time::mono::now_ns().saturating_sub(since) > MOST_HELD_NS)
        {
            play.held_since_ns = None;
            audio.send(Command::Play);
        }
        if audio.is_live(play.generation) {
            play.started = true;
        } else {
            actions.clear();
            reports.clear();
            return;
        }
    }
    for EngineReport(report) in reports.read() {
        match *report {
            Report::Jumped { device_frame, .. } => {
                if let Some(pending) = play.pending_rewind.as_mut() {
                    pending.cut_device = Some(device_frame as f64);
                }
            }
            // Practice's loop came round: its notes come round with it.
            Report::Looped { device_frame, .. } => {
                if let Some(practice) = &play.practice {
                    play.pending_rewind = Some(PendingRewind {
                        kind: Rewind::Loop,
                        back_ms: practice.loop_ms,
                        cut_ms: practice.end_ms + play.offset_at(f64::INFINITY),
                        cut_device: Some(device_frame as f64),
                    });
                }
            }
            _ => {}
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
    // The Esc menu pauses the song while it is up.
    let menu = crate::esc_menu::is_open();
    let hold = menu || play.waiting.is_some();
    if hold != play.paused && play.failed_at_ns.is_none() && play.pending_rewind.is_none() {
        play.paused = hold;
        audio.send(if hold { Command::Stop } else { Command::Play });
    }
    for PlayerAction(action) in actions.read() {
        // Wait mode: the note the song stands still on, hit at last.
        if !menu
            && let (Some(index), Action::Pad(pad), Phase::Pressed) = (play.waiting, action.action, action.phase)
            && play.notes[index].lane == Lane::Pad(pad)
        {
            let lane = Lane::Pad(pad);
            play.pressed_at_ns[play.column_of[lane.index()]] = now_ns;
            // Judged at the edge of its window: late, but hit (the song kept
            // going a moment after it stopped).
            // Judged here: the song is still paused, and paused presses go nowhere.
            let late = play.run.judge().windows().big;
            let outcomes = play.run.press(lane, play.notes[index].ms + late);
            classic_mute(play, &outcomes, &mut audio);
            note_feedback(play, &outcomes, now_ns, &mut commands);
            play.waiting = None;
            continue;
        }
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
            // OPTIONS pulls the menu up too; it pauses the song next frame.
            (Action::Pause, Phase::Pressed) if play.failed_at_ns.is_none() && play.pending_rewind.is_none() => {
                crate::esc_menu::set_open(!menu);
            }
            (Action::Select, Phase::Pressed) => {
                next.set(if session.from_tour { Screen::Tour } else { Screen::Songs });
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
    // Wait mode: a note about to be missed stops the song until it is hit.
    if play.wait && play.waiting.is_none() && !play.paused {
        let late = play.run.judge().windows().big;
        play.waiting = play.order.iter().copied().find(|&i| {
            let note = play.notes[i];
            matches!(note.lane, Lane::Pad(_)) && note.ms + late < play.now_ms && play.run.judge().judgement(i).is_none()
        });
    }
    // Nothing is missed while the song waits.
    let missed = if play.waiting.is_some() {
        Vec::new()
    } else {
        play.run.settle(play.now_ms)
    };
    classic_mute(play, &missed, &mut audio);
    note_feedback(play, &missed, now_ns, &mut commands);
    // The cut reached the run once every press before it has surely arrived.
    let settle_frames = SETTLE_MS / 1000.0 * f64::from(play.sample_rate);
    if let Some(pending) = play.pending_rewind
        && let Some(cut) = pending.cut_device
        && now.device_frame >= cut + settle_frames
    {
        play.pending_rewind = None;
        let went_back = match pending.kind {
            Rewind::WheelUp => play.run.wheel_up(pending.cut_ms, pending.back_ms),
            Rewind::Loop => Some(play.run.again(pending.cut_ms, pending.back_ms)),
        };
        if let Some((outcomes, _)) = went_back {
            // A pass done: its accuracy, from what it was judged.
            if let Some(practice) = play.practice.as_mut() {
                let counts = play.run.score().counts;
                let pass: [u32; 4] = std::array::from_fn(|i| counts[i] - practice.counts_before[i]);
                practice.passes.push(Score::accuracy_of(pass));
                practice.counts_before = counts;
            }
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
    if play
        .pending_rewind
        .is_some_and(|p| p.kind == Rewind::WheelUp && p.cut_device.is_some())
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
        no_fail: session.no_fail() || play.lesson(),
        autoplay: play.autoplay,
        lesson: play.lesson(),
        score: play.run.score().clone(),
        failed,
        presses: play.run.presses().to_vec(),
        notes: play.notes.len(),
    });
    next.set(Screen::Results);
}

/// Lays the bands over what is in view: each hype phrase's gold across the
/// pads (grey once broken), each roll's over its lane.
#[allow(clippy::type_complexity)]
fn draw_bands(
    play: Option<Res<Play>>,
    mut parts: Query<(
        &BandPart,
        &mut Visibility,
        Option<&mut Transform>,
        Option<&mut Node>,
        Option<&mut TextColor>,
    )>,
) {
    let Some(play) = play else { return };
    if !play.now_ms.is_finite() {
        return;
    }
    let view_ms = play.now_ms + play.visual_lead_ms;
    let phrases: Vec<(f64, f64, bool)> = play
        .run
        .phrases()
        .filter(|&(start, end, _)| start - view_ms <= play.lookahead_ms && play.note_y(end, view_ms) > HIT_Y)
        .collect();
    let (left, right) = pads_span();
    for (part, mut visibility, transform, node, text_colour) in &mut parts {
        // Where the band runs, from bottom to top, across which columns, in what colour.
        let span = match part.band {
            Band::Hype(band) => phrases.get(band).map(|&(start, end, clean)| {
                let colour = if clean { palette::FLYER_YELLOW } else { palette::MUTED };
                (start, end, (left + right) / 2.0, right - left - 4.0, colour)
            }),
            Band::Roll(index) => {
                let roll = play.rolls[index];
                let column = play.column_of[Lane::Pad(roll.pad).index()];
                (roll.start_ms - view_ms <= play.lookahead_ms).then(|| {
                    (
                        roll.start_ms,
                        roll.end_ms,
                        column_x(column),
                        LANE_W - 4.0,
                        play.column_colour[column],
                    )
                })
            }
        };
        let Some((start, end, x, width, colour)) = span else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        // Clipped to the highway: from the hit line up to where notes appear.
        let bottom = play.note_y(start, view_ms).max(HIT_Y) - NOTE_H / 2.0;
        let top = play.note_y(end, view_ms).min(TOP_Y) + NOTE_H / 2.0;
        if top <= bottom {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        visibility.set_if_neq(Visibility::Inherited);
        if let Some(mut transform) = transform {
            let middle = (top + bottom) / 2.0;
            let (at, size) = match part.part {
                Part::Fill => (Vec2::new(x, middle), Vec2::new(width, top - bottom)),
                Part::Top => (Vec2::new(x, top), Vec2::new(width, 2.0)),
                Part::Bottom | Part::Label => (Vec2::new(x, bottom), Vec2::new(width, 2.0)),
                Part::Left => (Vec2::new(x - width / 2.0, middle), Vec2::new(3.0, top - bottom)),
                Part::Right => (Vec2::new(x + width / 2.0, middle), Vec2::new(3.0, top - bottom)),
            };
            transform.translation.x = at.x;
            transform.translation.y = at.y;
            transform.scale = size.extend(1.0);
        }
        if let Some(mut node) = node {
            node.margin.left = px(x - width / 2.0 + 4.0);
            node.margin.top = px(ui_y(top) + 3.0);
        }
        if let Some(mut text_colour) = text_colour {
            text_colour.0 = colour;
        }
    }
}

/// Spawns the notes coming into view, moves them, and lets go of the ones gone by.
#[allow(clippy::type_complexity)]
fn draw_notes(
    mut commands: Commands,
    play: Option<ResMut<Play>>,
    looks: Option<Res<Looks>>,
    lights: Option<Res<Lights>>,
    mut heads: Query<(&mut Transform, &Children), (With<NoteMark>, Without<HoldBody>)>,
    mut bodies: Query<&mut Transform, (With<HoldBody>, Without<NoteMark>)>,
    mut gems: Query<&mut MeshMaterial2d<ColorMaterial>, With<NoteGem>>,
) {
    let (Some(mut play), Some(looks), Some(lights)) = (play, looks, lights) else {
        return;
    };
    let play = &mut *play;
    if !play.now_ms.is_finite() {
        return;
    }
    let view_ms = play.now_ms + play.visual_lead_ms;
    while let Some(&index) = play.order.get(play.next_spawn) {
        let note = play.notes[index];
        if note.ms - view_ms > play.lookahead_ms + 100.0 {
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
                Transform::from_xyz(column_x(column), play.note_y(note.ms, view_ms), Z_NOTE),
                Visibility::default(),
            ))
            .with_children(|head| {
                if note.hold.is_some() {
                    head.spawn((
                        HoldBody,
                        Mesh2d(looks.unit.clone()),
                        MeshMaterial2d(lights.hold[column].clone()),
                        Transform::from_scale(Vec3::new(width - 18.0, 0.0, 1.0)),
                    ));
                }
                head.spawn((
                    NoteGem,
                    Mesh2d(looks.unit.clone()),
                    MeshMaterial2d(lights.gem[column].clone()),
                    Transform {
                        translation: Vec3::new(0.0, 0.0, 0.2),
                        scale: Vec3::new(width, NOTE_H, 1.0),
                        ..default()
                    },
                ));
                // A white-hot core along the note.
                head.spawn((
                    Mesh2d(looks.unit.clone()),
                    MeshMaterial2d(looks.core.clone()),
                    Transform {
                        translation: Vec3::new(0.0, 0.0, 0.3),
                        scale: Vec3::new(width - 10.0, 3.0, 1.0),
                        ..default()
                    },
                ));
                if column < PAD_COUNT {
                    looks.spawn_glyph(head, column, 13.0, 0.4, &looks.ink, ());
                }
            })
            .id();
        play.entities[index] = Some(entity);
    }
    for index in 0..play.entities.len() {
        let Some(entity) = play.entities[index] else { continue };
        let note = play.notes[index];
        let judged = play.run.judge().judgement(index);
        let held = play.run.judge().is_held(index);
        // A note's head; while a hold is held, the hit line eats it from below.
        let mut head_y = play.note_y(note.ms, view_ms);
        if held {
            head_y = head_y.max(HIT_Y);
        }
        let tail_y = note
            .hold
            .map_or(head_y, |span| play.note_y(span.end_ms, view_ms).min(TOP_Y));
        if tail_y < HIT_Y - 120.0 {
            commands.entity(entity).despawn();
            play.entities[index] = None;
            continue;
        }
        let Ok((mut transform, children)) = heads.get_mut(entity) else {
            continue;
        };
        transform.translation.y = head_y;
        let column = play.column_of[note.lane.index()];
        for child in children {
            if let Ok(mut body) = bodies.get_mut(*child) {
                let length = (tail_y - head_y).max(0.0);
                body.translation.y = length / 2.0;
                body.scale.y = length;
            }
            if let Ok(mut gem) = gems.get_mut(*child) {
                let wanted = match judged {
                    Some(_) if !held => &lights.dim[column],
                    _ => &lights.gem[column],
                };
                if gem.0 != *wanted {
                    gem.0 = wanted.clone();
                }
            }
        }
    }
}

/// Lights each receptor as it is pressed, and each lane's glow with it and
/// with the kick; the hit line swells with the kick too.
fn light_receptors(
    play: Option<Res<Play>>,
    lights: Option<Res<Lights>>,
    mood: Res<StageMood>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let (Some(play), Some(lights)) = (play, lights) else {
        return;
    };
    let now_ns = wu_time::mono::now_ns();
    for column in 0..COLUMNS {
        let colour = play.column_colour[column];
        // Pads flash on each press; a rail glows for as long as it is held.
        let glow = if column >= PAD_COUNT {
            if play.rail_down[column - PAD_COUNT] { 1.0 } else { 0.0 }
        } else {
            let since = now_ns.saturating_sub(play.pressed_at_ns[column]) as f64 / 1e9;
            (-since / PRESS_GLOW_S).exp() as f32
        };
        if let Some(mut material) = materials.get_mut(&lights.receptor[column]) {
            material.color = glowing(colour, 0.55 + 2.2 * glow);
        }
        if let Some(mut material) = materials.get_mut(&lights.glow[column]) {
            material.color = colour.with_alpha(0.12 + 0.45 * glow + 0.08 * mood.pulse);
        }
    }
    if let Some(mut material) = materials.get_mut(&lights.hit_line) {
        material.color = glowing(Color::WHITE, 1.2 + 0.8 * mood.pulse);
    }
}

/// Throws light from the hit line for every hit: a ring, and up the lane a
/// beam for the best ones; a miss darkens its receptor in red.
fn spawn_bursts(
    mut commands: Commands,
    time: Res<Time>,
    play: Option<ResMut<Play>>,
    looks: Option<Res<Looks>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let (Some(mut play), Some(looks)) = (play, looks) else {
        return;
    };
    let now = time.elapsed_secs();
    let bursts = std::mem::take(&mut play.bursts);
    for (column, judgement) in bursts {
        let colour = play.column_colour[column];
        let x = column_x(column);
        let width = column_width(column);
        let (ring, brightness) = match judgement {
            Judgement::Wicked => (palette::mix(colour, Color::WHITE, 0.5), 3.2),
            Judgement::Big => (colour, 2.4),
            Judgement::Safe => (palette::mix(colour, palette::MUTED, 0.5), 1.4),
            Judgement::Miss => (palette::WARNING, 1.1),
        };
        let ring = glowing(ring, brightness).to_linear();
        commands.spawn((
            DespawnOnExit(Screen::Rhythm),
            Burst {
                born_s: now,
                life_s: if judgement == Judgement::Miss { 0.2 } else { 0.28 },
                from_scale: Vec2::splat(width * 0.6),
                to_scale: Vec2::splat(width * if judgement == Judgement::Miss { 0.9 } else { 1.7 }),
                colour: ring,
            },
            Mesh2d(looks.ring.clone()),
            MeshMaterial2d(materials.add(see_through(Color::LinearRgba(ring), 1.0))),
            Transform::from_xyz(x, HIT_Y, Z_BURST),
        ));
        if matches!(judgement, Judgement::Wicked | Judgement::Big) {
            let beam = glowing(colour, if judgement == Judgement::Wicked { 1.2 } else { 0.8 }).to_linear();
            commands.spawn((
                DespawnOnExit(Screen::Rhythm),
                Burst {
                    born_s: now,
                    life_s: 0.18,
                    from_scale: Vec2::new(width - 8.0, 240.0),
                    to_scale: Vec2::new(width - 14.0, 260.0),
                    colour: beam,
                },
                Mesh2d(looks.unit.clone()),
                MeshMaterial2d(materials.add(ColorMaterial {
                    texture: Some(looks.fade.clone()),
                    ..see_through(Color::LinearRgba(beam), 1.0)
                })),
                Transform::from_xyz(x, HIT_Y + 120.0, Z_BEAM),
            ));
        }
    }
}

/// A WHEEL UP!: the record leaps up over the highway and spins back, slowing
/// as the music returns, then drops away.
fn spin_back(
    time: Res<Time>,
    play: Option<Res<Play>>,
    settings: Res<SettingsStore>,
    mut record: Query<(&mut Transform, &mut Visibility), With<Spinback>>,
) {
    let Some(play) = play else { return };
    let Ok((mut transform, mut visibility)) = record.single_mut() else {
        return;
    };
    // Reduced motion: the banner says it, no record leaps.
    let since = play
        .banner_ns
        .map(|at| wu_time::mono::now_ns().saturating_sub(at) as f32 / 1e9)
        .filter(|&t| t < SPINBACK_GONE_S && !settings.reduced_motion());
    let Some(t) = since else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    visibility.set_if_neq(Visibility::Inherited);
    // In with a bounce, out with a drop.
    let scale = if t < 0.22 {
        let x = t / 0.22;
        let back = 1.7;
        1.0 + (back + 1.0) * (x - 1.0).powi(3) + back * (x - 1.0).powi(2)
    } else if t > SPINBACK_S {
        1.0 - (t - SPINBACK_S) / (SPINBACK_GONE_S - SPINBACK_S)
    } else {
        1.0
    };
    transform.scale = Vec3::splat(scale.max(0.01));
    // Backwards, slowing to a stop.
    let speed = SPINBACK_SPEED * (1.0 - t / SPINBACK_S).max(0.0);
    transform.rotate_z(speed * time.delta_secs());
}

/// Tells the stage how the night is going: the kick's pulse, a drop's lasers
/// in the hype phrases, the hype meter, a WHEEL UP!'s flare.
fn set_the_mood(play: Option<Res<Play>>, mut mood: ResMut<StageMood>) {
    let Some(play) = play else { return };
    if !play.now_song_ms.is_finite() {
        return;
    }
    let now = play.now_song_ms;
    let last_kick = play.kicks_ms.partition_point(|&k| k <= now);
    let pulse = last_kick
        .checked_sub(1)
        .map_or(0.0, |i| (-(now - play.kicks_ms[i]) / 1000.0 / KICK_GLOW_S).exp());
    let in_phrase = play
        .run
        .phrases()
        .any(|(start, end, _)| start <= play.now_ms && play.now_ms < end);
    let now_ns = wu_time::mono::now_ns();
    let flash = play.banner_ns.map_or(0.0, |at| {
        let since = now_ns.saturating_sub(at) as f64 / 1e9;
        (-since / 0.35).exp()
    });
    let wheeling = flash > 0.05;
    *mood = StageMood {
        pulse: pulse as f32,
        intensity: if in_phrase || wheeling { 1.0 } else { 0.4 },
        lasers: if in_phrase || wheeling { 1.0 } else { 0.0 },
        flash: flash as f32,
        hype: play.run.hype(),
        scene: play.scene,
    };
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
    mut popups: Query<(&PopupText, &mut Text, &mut TextColor, &mut UiTransform), Without<Hud>>,
    mut vibe_fill: Query<(&mut Node, &mut BackgroundColor), (With<VibeFill>, Without<HypeFill>)>,
    mut hype_fill: Query<&mut Node, (With<HypeFill>, Without<VibeFill>)>,
    mut huds: Query<(&Hud, &mut Text, &mut TextColor, Option<&mut UiTransform>), Without<PopupText>>,
) {
    let Some(play) = play else { return };
    let now_ns = wu_time::mono::now_ns();
    let score = play.run.score();
    for (popup, mut text, mut colour, mut transform) in &mut popups {
        let entry = play.popups[popup.0];
        let age = now_ns.saturating_sub(entry.at_ns);
        match entry.judgement {
            Some(judgement) if age < POPUP_NS => {
                text.0 = judgement.label().to_owned();
                colour.0 = judgement_colour(judgement);
                // It pops out, then settles.
                let t = age as f32 / 1e9;
                transform.scale = Vec2::splat(1.0 + 0.45 * (-t / 0.05).exp());
            }
            _ => text.0.clear(),
        }
    }
    if let Ok((mut node, mut background)) = vibe_fill.single_mut() {
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
        .map_or(tr(play.language, "Count-in"), |(name, _)| name.as_str());
    let language = play.language;
    let last_offset = play
        .popups
        .iter()
        .filter(|p| p.judgement.is_some_and(|j| j != Judgement::Miss))
        .max_by_key(|p| p.at_ns)
        .map(|p| p.offset_ms);
    // A lesson shows as its notes come into view; the first one through the count-in.
    let step = play
        .lessons
        .iter()
        .rposition(|cue| cue.start_ms - play.lookahead_ms <= play.now_song_ms)
        .unwrap_or(0);
    let lesson = play.lessons.get(step);
    // Beats to go before the notes: the count-in, or a practice loop's run-up.
    let lead_in_ms = play.practice.as_ref().map_or(0.0, |p| p.start_ms);
    let beats_to_go = ((lead_in_ms - play.now_song_ms) / play.beat_ms).ceil();
    let banner = play.banner_ns.is_some_and(|at| now_ns.saturating_sub(at) < BANNER_NS);
    for (hud, mut text, mut colour, transform) in &mut huds {
        // The banner pops out and settles.
        if let (Hud::Centre, Some(mut transform)) = (hud, transform) {
            let age = play
                .banner_ns
                .map_or(f32::MAX, |at| now_ns.saturating_sub(at) as f32 / 1e9);
            transform.scale = Vec2::splat(1.0 + 0.6 * (-age / 0.09).exp());
        }
        text.0 = match hud {
            Hud::Score => format!("{:>9}", score.points),
            Hud::BigCombo => {
                if score.combo >= 10 {
                    format!("{}", score.combo)
                } else {
                    String::new()
                }
            }
            Hud::Combo => fill(
                tr(language, "{} combo · ×{}\naccuracy {} %"),
                &[
                    &score.combo,
                    &score.multiplier(),
                    &decimal(language, score.accuracy() * 100.0, 1),
                ],
            ),
            Hud::Status => {
                let offset = last_offset.map_or(String::new(), |o| {
                    if o >= 0.0 {
                        fill(tr(language, "{} ms late"), &[&format!("{o:.0}")])
                    } else {
                        fill(tr(language, "{} ms early"), &[&format!("{:.0}", -o)])
                    }
                });
                match &play.practice {
                    Some(practice) => format!(
                        "{}\n{}\n{}{}\n{offset}",
                        play.title,
                        fill(tr(language, "PRACTICE · {}"), &[&practice.name]),
                        fill(tr(language, "pass {}"), &[&(practice.passes.len() + 1)]),
                        passes_line(&practice.passes, language),
                    ),
                    None => format!("{}\n{section}\n{offset}", play.title),
                }
            }
            Hud::Centre => {
                if play.failed_at_ns.is_some() {
                    tr(language, "PLUG PULLED").to_owned()
                } else if banner {
                    "WHEEL UP!".to_owned()
                } else if play.now_song_ms < lead_in_ms
                    && play.now_song_ms.is_finite()
                    && (play.practice.is_none() || beats_to_go <= 4.0)
                {
                    format!("{}", beats_to_go.max(1.0))
                } else {
                    String::new()
                }
            }
            Hud::LessonStep => fill(tr(language, "LESSON {} OF {}"), &[&(step + 1), &play.lessons.len()]),
            Hud::LessonTitle => lesson.map_or_else(String::new, |cue| cue.title.clone()),
            Hud::Lesson => lesson.map_or_else(String::new, |cue| cue.caption.clone()),
            Hud::Hype => {
                let flash = now_ns.saturating_sub(play.hype_flash_ns) < 600_000_000;
                // Practice has no WHEEL UP!: the loop is the replay.
                let can_wheel_up = play.run.can_wheel_up() && play.practice.is_none();
                colour.0 = if flash || can_wheel_up || boost_left.is_some() {
                    palette::FLYER_YELLOW
                } else {
                    palette::MUTED
                };
                if boost_left.is_some() {
                    tr(language, "WHEEL UP!  multiplier doubled").to_owned()
                } else if play.pending_rewind.is_some_and(|p| p.kind == Rewind::WheelUp) {
                    tr(language, "pulling up…").to_owned()
                } else if can_wheel_up {
                    let hype = format!("{:.0}", play.run.hype() * 100.0);
                    fill(tr(language, "HYPE {} %  ·  L3 + R3: WHEEL UP!"), &[&hype])
                } else {
                    fill(tr(language, "HYPE {} %"), &[&format!("{:.0}", play.run.hype() * 100.0)])
                }
            }
        };
    }
}
