//! The Calibrate screen: tap along to clicks, then to flashes. The first gives
//! the audio offset (subtracted from every tap before judging), the second the
//! video offset (how far visuals must run ahead). Saved per audio output device.

use bevy::prelude::*;
use wu_audio::{Command, Report, VoiceSource};
use wu_content::demo::metronome_program;
use wu_content::settings::{Calibration, Language};
use wu_game::calibration::{Estimate, MAX_SPREAD_MS, estimate};
use wu_input::{Action, Phase};

use crate::audio::{AudioLink, EngineReport};
use crate::input::PlayerAction;
use crate::notice::FirstLaunch;
use crate::palette;
use crate::screens::Screen;
use crate::settings::SettingsStore;
use crate::ui::{centred_label, centred_on, label, screen_root};
use crate::words::{decimal, fill, tr};

#[derive(Debug)]
pub struct CalibratePlugin;

impl Plugin for CalibratePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Calibrating>()
            .add_systems(OnEnter(Screen::Calibrate), enter)
            .add_systems(
                OnExit(Screen::Calibrate),
                |mut audio: NonSendMut<AudioLink>, mut first_launch: ResMut<FirstLaunch>| {
                    audio.send(Command::Stop);
                    first_launch.0 = false;
                },
            )
            .add_systems(Update, (run, show).chain().run_if(in_state(Screen::Calibrate)));
    }
}

/// Taps needed per test.
const TAPS: usize = 16;
const BPM: f64 = 100.0;
const BEAT_NS: u64 = (60.0 / BPM * 1e9) as u64;
/// How long each flash stays lit.
const FLASH_NS: u64 = 90_000_000;
/// After a test ends, taps are ignored this long, so a few extra beats of
/// tapping don't skip the results.
const COOLDOWN_NS: u64 = 1_500_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Stage {
    #[default]
    Intro,
    Audio,
    AudioDone,
    Video,
    Done,
    Saved,
}

#[derive(Resource, Debug, Default)]
struct Calibrating {
    stage: Stage,
    beats_ns: Vec<u64>,
    taps_ns: Vec<u64>,
    audio: Option<Estimate>,
    video: Option<Estimate>,
    flashes_from_ns: u64,
    /// Presses before this are ignored (see `COOLDOWN_NS`).
    ready_at_ns: u64,
}

#[derive(Component)]
enum Part {
    Instructions,
    Lamp,
    Progress,
    Results,
}

fn enter(
    mut commands: Commands,
    mut audio: NonSendMut<AudioLink>,
    mut state: ResMut<Calibrating>,
    settings: Res<SettingsStore>,
) {
    let language = settings.language();
    let sample_rate = audio.sample_rate();
    audio.load(metronome_program(sample_rate, BPM));
    *state = Calibrating::default();
    commands.spawn(screen_root(Screen::Calibrate)).with_children(|screen| {
        screen
            .spawn(centred_on(0.0, -150.0, 1100.0, 60.0))
            .with_child((Part::Instructions, centred_label("", 19.0, palette::INK)));
        screen.spawn((
            Part::Lamp,
            Node {
                border: UiRect::all(px(3)),
                border_radius: BorderRadius::MAX,
                ..centred_on(0.0, -10.0, 150.0, 150.0)
            },
            BorderColor::all(palette::FLYER_YELLOW),
            BackgroundColor(palette::BACKDROP),
        ));
        screen
            .spawn(centred_on(0.0, 95.0, 600.0, 24.0))
            .with_child((Part::Progress, label("", 18.0, palette::FLYER_YELLOW)));
        screen
            .spawn(centred_on(0.0, 185.0, 1000.0, 110.0))
            .with_child((Part::Results, centred_label("", 15.0, palette::SIGNAL)));
        screen.spawn(centred_on(0.0, 320.0, 1000.0, 18.0)).with_child(label(
            tr(
                language,
                "any pad, Space or OPTIONS: continue · Tab / CREATE: next screen · Esc menu",
            ),
            13.0,
            palette::MUTED,
        ));
    });
}

fn run(
    mut state: ResMut<Calibrating>,
    mut actions: MessageReader<PlayerAction>,
    mut reports: MessageReader<EngineReport>,
    mut audio: NonSendMut<AudioLink>,
    mut settings: ResMut<SettingsStore>,
    first_launch: Res<FirstLaunch>,
    mut next: ResMut<NextState<Screen>>,
) {
    let state = &mut *state;
    if state.stage == Stage::Audio {
        for EngineReport(report) in reports.read() {
            if let Report::VoiceStarted(start) = report
                && matches!(start.source, VoiceSource::Sequence { .. })
                && let Some(ns) = audio.estimator.ns_at_device_frame(start.device_frame as f64)
            {
                state.beats_ns.push(ns.max(0.0) as u64);
            }
        }
    } else {
        reports.clear();
    }
    let now = wu_time::mono::now_ns();
    for PlayerAction(action) in actions.read() {
        let tap = matches!(action.action, Action::Pad(_) | Action::Pause) && action.phase == Phase::Pressed;
        if !tap || action.at_ns < state.ready_at_ns {
            continue;
        }
        match state.stage {
            // The first time, the songs come once the offsets are saved.
            Stage::Saved if first_launch.0 => next.set(Screen::Songs),
            Stage::Intro | Stage::Saved => {
                *state = Calibrating {
                    stage: Stage::Audio,
                    ..Calibrating::default()
                };
                audio.send(Command::Seek(wu_time::Tick::ZERO));
                audio.send(Command::Play);
            }
            Stage::Audio | Stage::Video => state.taps_ns.push(action.at_ns),
            Stage::AudioDone => {
                state.stage = Stage::Video;
                state.taps_ns.clear();
                state.flashes_from_ns = now + 1_000_000_000;
            }
            Stage::Done => {
                if let (Some(audio_estimate), Some(video_estimate)) = (state.audio, state.video)
                    && audio_estimate.is_steady()
                    && video_estimate.is_steady()
                {
                    let device = audio.info().device.clone();
                    settings.set_calibration(
                        &device,
                        Calibration {
                            audio_ms: audio_estimate.offset_ms,
                            video_ms: video_estimate.offset_ms,
                        },
                    );
                    state.stage = Stage::Saved;
                } else {
                    // Too uneven to trust: back to the start, nothing saved.
                    *state = Calibrating::default();
                }
                state.ready_at_ns = now + COOLDOWN_NS;
            }
        }
    }
    match state.stage {
        Stage::Audio if state.taps_ns.len() >= TAPS => {
            audio.send(Command::Stop);
            state.audio = estimate(&state.beats_ns, &state.taps_ns);
            state.stage = Stage::AudioDone;
            state.ready_at_ns = now + COOLDOWN_NS;
        }
        Stage::Video if state.taps_ns.len() >= TAPS => {
            let beats = flash_times(state.flashes_from_ns, now + BEAT_NS);
            state.video = estimate(&beats, &state.taps_ns);
            state.stage = Stage::Done;
            state.ready_at_ns = now + COOLDOWN_NS;
        }
        _ => {}
    }
}

/// Every flash from `from` up to `until`.
fn flash_times(from: u64, until: u64) -> Vec<u64> {
    (0..).map(|k| from + k * BEAT_NS).take_while(|&t| t <= until).collect()
}

fn describe(name: &'static str, estimate: Option<Estimate>, language: Language) -> String {
    let name = tr(language, name);
    let tenths = |ms: f64| decimal(language, ms, 1);
    match estimate {
        Some(e) if !e.is_steady() => fill(
            tr(language, "{}: taps too uneven (spread {} ms, want under {}), try again"),
            &[&name, &format!("{:.0}", e.spread_ms), &format!("{MAX_SPREAD_MS:.0}")],
        ),
        Some(e) if e.offset_ms >= 0.0 => fill(
            tr(language, "{}: you tap {} ms after it (spread {} ms, {} taps)"),
            &[&name, &tenths(e.offset_ms), &tenths(e.spread_ms), &e.used],
        ),
        Some(e) => fill(
            tr(language, "{}: you tap {} ms before it (spread {} ms, {} taps)"),
            &[&name, &tenths(-e.offset_ms), &tenths(e.spread_ms), &e.used],
        ),
        None => fill(tr(language, "{}: not enough steady taps, try again"), &[&name]),
    }
}

/// Milliseconds to a tenth, with their sign.
fn signed_ms(language: Language, ms: f64) -> String {
    let sign = if ms >= 0.0 { "+" } else { "" };
    format!("{sign}{}", decimal(language, ms, 1))
}

fn show(
    state: Res<Calibrating>,
    audio: NonSend<AudioLink>,
    settings: Res<SettingsStore>,
    first_launch: Res<FirstLaunch>,
    mut parts: Query<(&Part, Option<&mut Text>, Option<&mut BackgroundColor>)>,
) {
    let now = wu_time::mono::now_ns();
    let language = settings.language();
    let device = &audio.info().device;
    let steady = state.audio.is_some_and(|e| e.is_steady()) && state.video.is_some_and(|e| e.is_steady());
    let instructions = match state.stage {
        Stage::Intro if first_launch.0 => {
            "Welcome! First, a minute to fit the game to your speakers and screen.\n\
             Tap any pad exactly on each click you hear. Press a pad to start (Tab / CREATE skips)."
        }
        Stage::Intro => "Two short tests. First: tap any pad exactly on each click you hear.\nPress a pad to start.",
        Stage::Audio => "Tap any pad on every click. Listen, don't look.",
        Stage::AudioDone => "Now the screen: tap on every flash of the circle, without sound.\nPress a pad to start.",
        Stage::Video => "Tap any pad on every flash.",
        Stage::Done if steady => "Done. Press a pad to save these offsets for this audio output.",
        Stage::Done => "Not steady enough to save. Press a pad to start again.",
        Stage::Saved if first_launch.0 => "Saved: the game now plays in time with you. Press a pad for the songs.",
        Stage::Saved => "Saved. Press a pad to run it again.",
    };
    let instructions = tr(language, instructions);
    let progress = match state.stage {
        Stage::Audio | Stage::Video => format!("{} / {TAPS}", state.taps_ns.len()),
        _ => String::new(),
    };
    let lit = state.stage == Stage::Video
        && now >= state.flashes_from_ns
        && (now - state.flashes_from_ns) % BEAT_NS < FLASH_NS;
    let saved = settings.calibration(device);
    let mut results = vec![
        describe("sound", state.audio, language),
        describe("screen", state.video, language),
    ];
    if state.stage == Stage::Intro || state.stage == Stage::Saved {
        results = vec![fill(
            tr(
                language,
                "saved for \"{}\": sound {} ms, screen {} ms, visuals lead by {} ms",
            ),
            &[
                device,
                &signed_ms(language, saved.audio_ms),
                &signed_ms(language, saved.video_ms),
                &signed_ms(language, saved.visual_lead_ms()),
            ],
        )];
    }
    for (part, text, background) in &mut parts {
        match (part, text, background) {
            (Part::Instructions, Some(mut text), _) => text.0 = instructions.to_owned(),
            (Part::Progress, Some(mut text), _) => text.0 = progress.clone(),
            (Part::Results, Some(mut text), _) => text.0 = results.join("\n"),
            (Part::Lamp, _, Some(mut background)) => {
                background.0 = if lit { palette::FLYER_YELLOW } else { palette::BACKDROP };
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flashes_follow_the_tempo() {
        let flashes = flash_times(1_000, 1_000 + 3 * BEAT_NS);
        assert_eq!(
            flashes,
            vec![1_000, 1_000 + BEAT_NS, 1_000 + 2 * BEAT_NS, 1_000 + 3 * BEAT_NS]
        );
    }
}
