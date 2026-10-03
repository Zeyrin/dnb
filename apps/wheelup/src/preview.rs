//! On the songs screen, the selected tune plays: its first drop, looped, a
//! little quieter, once the selection has rested on it a moment. The stage
//! moves with it, the city pulsing on its kicks and the lasers out.

use bevy::prelude::*;
use wu_audio::{Backing, Command};
use wu_content::project::Song;
use wu_instruments::Pad;
use wu_time::Tick;

use crate::audio::AudioLink;
use crate::imports::Recordings;
use crate::screens::Screen;
use crate::session::Session;
use crate::songs_screen::{SongLibrary, picked_kit, picked_stage};
use crate::stage::{Scene, StageMood};
use crate::tour_screen::TourData;

#[derive(Debug)]
pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Preview>()
            .add_systems(Update, play_the_selection.run_if(in_state(Screen::Songs)))
            .add_systems(OnExit(Screen::Songs), stop);
    }
}

/// How long the selection rests on a tune before it plays: scrolling past
/// one does not start it.
const SETTLE_S: f64 = 0.35;
/// How much of the drop loops.
const PREVIEW_BARS: i64 = 8;
/// How much quieter than the game the preview plays, in dB.
const PREVIEW_DB: f32 = -5.0;
/// How long a kick lights the stage (time constant).
const KICK_GLOW_S: f64 = 0.11;

/// What the songs screen is playing.
#[derive(Resource, Default)]
pub struct Preview {
    /// The tune selected, and since when (seconds).
    selected: Option<(usize, f64)>,
    playing: Option<Playing>,
}

impl Preview {
    /// Whether the selected tune is playing.
    pub fn is_playing(&self) -> bool {
        self.playing.is_some()
    }
}

struct Playing {
    song: usize,
    /// The kit it plays on, when not its own.
    kit: Option<&'static str>,
    generation: u64,
    /// When the kicks land, in song time.
    kicks_ms: Vec<f64>,
}

/// The first drop (its first hype phrase), or failing one the middle of the tune.
fn drop_of(song: &Song) -> (Tick, Tick) {
    let from = song
        .hype
        .first()
        .map_or(Tick((song.length.0 / 2).max(0)), |&(start, _)| start);
    let to = (from + Tick::from_bars(PREVIEW_BARS)).min(song.length);
    (from, to)
}

#[allow(clippy::too_many_arguments)]
fn play_the_selection(
    time: Res<Time>,
    session: Res<Session>,
    library: Res<SongLibrary>,
    tour: Res<TourData>,
    recordings: Res<Recordings>,
    mut audio: NonSendMut<AudioLink>,
    mut preview: ResMut<Preview>,
    mut mood: ResMut<StageMood>,
) {
    let now = time.elapsed_secs_f64();
    let selected = session.song;
    if preview.selected.is_none_or(|(song, _)| song != selected) {
        preview.selected = Some((selected, now));
    }
    let rested = preview.selected.is_some_and(|(_, since)| now - since >= SETTLE_S);
    // A kit picked plays at once, pressed or not: the player hears it first.
    let kit = library.get(selected).and_then(|song| picked_kit(&session, song));
    let playing_it = preview
        .playing
        .as_ref()
        .is_some_and(|p| p.song == selected && p.kit == kit);
    if rested
        && !playing_it
        && let Some(song) = library.get(selected)
    {
        let swapped;
        let song = match kit {
            Some(kit) => {
                swapped = Song {
                    kit: kit.to_owned(),
                    ..song.clone()
                };
                &swapped
            }
            None => song,
        };
        let rate = audio.sample_rate();
        // A tune of the player's is its recording: it plays once that is ready.
        let backing = match &song.recording {
            Some(recording) => recordings.get(&recording.path, rate).map(|audio| {
                Some(Backing::new(
                    audio,
                    rate,
                    recording.first_bar_s,
                    song.tempo.bpm_at(Tick::ZERO),
                    wu_dsp::db_to_gain(recording.gain_db + PREVIEW_DB),
                ))
            }),
            None => Some(None),
        };
        if let Some(backing) = backing {
            let mut program = song.whole_program(rate, &song.tempo, 0);
            program.mix.master_db += PREVIEW_DB;
            if let Some(backing) = backing {
                program = program.with_backing(backing);
            }
            let (from, to) = drop_of(song);
            let generation = audio.load(program);
            audio.send(Command::SetLoop(Some((from, to))));
            audio.send(Command::Seek(from));
            audio.send(Command::Play);
            let ms_at = |tick: Tick| song.tempo.seconds_at(tick.0 as f64) * 1000.0;
            preview.playing = Some(Playing {
                song: selected,
                kit,
                generation,
                kicks_ms: song
                    .drums
                    .iter()
                    .filter(|hit| hit.pad == Pad::P1)
                    .map(|hit| ms_at(hit.tick))
                    .collect(),
            });
        }
    }
    // The stage moves with the preview: the venue the tune plays at (or the one
    // picked, pressed or not), pulsing on its kicks.
    let scene = library.id(selected).map_or(Scene::Rooftop, |id| {
        Scene::picked(&tour.0, id, picked_stage(&session, &tour.0, id))
    });
    let Some(playing) = preview.playing.as_ref() else {
        *mood = StageMood {
            scene,
            ..StageMood::default()
        };
        return;
    };
    let point = audio
        .transport_at(wu_time::mono::now_ns())
        .filter(|_| audio.is_live(playing.generation));
    let Some(point) = point else { return };
    let song_ms = point.song_frame / f64::from(audio.sample_rate()) * 1000.0;
    let last = playing.kicks_ms.partition_point(|&k| k <= song_ms);
    let pulse = last
        .checked_sub(1)
        .map_or(0.0, |i| (-(song_ms - playing.kicks_ms[i]) / 1000.0 / KICK_GLOW_S).exp());
    *mood = StageMood {
        pulse: pulse as f32,
        intensity: 0.7,
        lasers: 0.4,
        scene,
        ..StageMood::default()
    };
}

/// Leaving the songs screen: the preview stops, the night calms down.
fn stop(mut audio: NonSendMut<AudioLink>, mut preview: ResMut<Preview>, mut mood: ResMut<StageMood>) {
    if preview.playing.take().is_some() {
        audio.send(Command::SetLoop(None));
        audio.send(Command::Stop);
    }
    preview.selected = None;
    *mood = StageMood::default();
}
