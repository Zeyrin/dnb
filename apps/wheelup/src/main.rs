//! WHEEL UP!: the game. This crate is rendering, UI and glue only; the music,
//! the timing and the rules live in the `wu-*` crates, which run without it.

#![forbid(unsafe_code)]
// Release builds on Windows open the game alone, without a console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod audio;
mod calibrate;
mod capture;
mod fonts;
mod highway;
mod imports;
mod input;
mod monitor;
mod overlay;
mod pads;
mod palette;
mod preview;
mod records;
mod results;
mod rhythm;
mod screens;
mod session;
mod settings;
mod songs_screen;
mod stage;
mod title;
mod ui;

use std::path::PathBuf;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;
use bevy::window::WindowResolution;
use clap::Parser;
use wu_audio::output::OutputOptions;

/// With `rt-check`, allocations on the audio thread are reported.
#[cfg(feature = "rt-check")]
#[global_allocator]
static ALLOCATOR: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

#[derive(Debug, Parser)]
#[command(
    name = "wheelup",
    version,
    about = "WHEEL UP!: a junglist rhythm game and controller-first DAW"
)]
struct Args {
    /// Part of the output device's name; the default output otherwise.
    #[arg(long)]
    audio_device: Option<String>,
    /// Audio buffer size in frames. Smaller is lower latency; the driver may refuse.
    #[arg(long)]
    buffer: Option<u32>,
    /// Run without a sound card (the engine still runs, silently).
    #[arg(long)]
    silent: bool,
    /// Start the jam groove straight away, and let the selecta bot play charts.
    #[arg(long)]
    autoplay: bool,
    /// The screen to open on. `rhythm` starts the first song at once.
    #[arg(long, value_enum, default_value_t = StartScreen::Songs)]
    screen: StartScreen,
    /// Difficulty for `--screen rhythm`.
    #[arg(long, value_enum, default_value_t = StartDifficulty::Easy)]
    difficulty: StartDifficulty,
    /// Practice tempo in percent (50–150).
    #[arg(long, default_value_t = 100)]
    tempo: u32,
    /// The song to select: a built-in song's id, or an imported one's (see
    /// `wheelup-cli songs`).
    #[arg(long)]
    song: Option<String>,
    /// Practice: loop this section of the song, by name (`Drop 2`, say).
    #[arg(long)]
    practice: Option<String>,
    /// Save a PNG of the window to this path once the scene has settled, then quit.
    #[arg(long, value_name = "PATH")]
    screenshot: Option<PathBuf>,
    /// Frames to wait before taking the screenshot.
    #[arg(long, default_value_t = 30)]
    screenshot_after: u32,
    /// Take the screenshot this many seconds after starting instead: the same
    /// moment of a song however slowly frames come.
    #[arg(long, value_name = "SECONDS")]
    screenshot_at: Option<f32>,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum StartScreen {
    Songs,
    Jam,
    Controller,
    Calibrate,
    Rhythm,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum StartDifficulty {
    Beginner,
    Easy,
    Medium,
    Hard,
    Junglist,
}

fn main() -> AppExit {
    // Fix the shared clock's epoch before any input or audio timestamp exists.
    wu_time::mono::epoch();
    let args = Args::parse();

    let mut app = App::new();
    let start = match args.screen {
        StartScreen::Songs => screens::Screen::Songs,
        StartScreen::Jam => screens::Screen::Jam,
        StartScreen::Controller => screens::Screen::Controller,
        StartScreen::Calibrate => screens::Screen::Calibrate,
        StartScreen::Rhythm => screens::Screen::Rhythm,
    };
    let session = session::Session {
        difficulty: match args.difficulty {
            StartDifficulty::Beginner => wu_chart::Difficulty::Beginner,
            StartDifficulty::Easy => wu_chart::Difficulty::Easy,
            StartDifficulty::Medium => wu_chart::Difficulty::Medium,
            StartDifficulty::Hard => wu_chart::Difficulty::Hard,
            StartDifficulty::Junglist => wu_chart::Difficulty::Junglist,
        },
        tempo_percent: args.tempo.clamp(50, 150),
        autoplay: args.autoplay,
        ..session::Session::default()
    };
    app.insert_resource(ClearColor(palette::BACKDROP))
        .insert_resource(settings::SettingsStore::load())
        .insert_resource(records::RecordsStore::load())
        .insert_resource(session)
        .insert_resource(songs_screen::WantedSong(args.song))
        .insert_resource(songs_screen::WantedPractice(args.practice))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "WHEEL UP!".into(),
                resolution: WindowResolution::new(1280, 720),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((FrameTimeDiagnosticsPlugin::default(), fonts::FontsPlugin))
        .add_plugins((
            audio::AudioPlugin {
                options: OutputOptions {
                    device: args.audio_device,
                    buffer_frames: args.buffer,
                },
                silent: args.silent,
            },
            input::InputPlugin,
            screens::ScreensPlugin { start },
        ))
        .add_plugins((
            stage::StagePlugin,
            title::TitlePlugin,
            songs_screen::SongsPlugin,
            preview::PreviewPlugin,
            imports::ImportPlugin,
            rhythm::RhythmPlugin,
            results::ResultsPlugin,
            pads::PadsPlugin {
                autoplay: args.autoplay,
            },
            monitor::MonitorPlugin,
            calibrate::CalibratePlugin,
            overlay::OverlayPlugin,
        ));

    let wait = args
        .screenshot_at
        .map_or(capture::Wait::Frames(args.screenshot_after), capture::Wait::Seconds);
    app.add_plugins(capture::CapturePlugin {
        auto: args.screenshot.map(|path| (path, wait)),
    })
    .add_systems(Startup, warm_up_kits);
    if let Some(dir) = wu_content::kits::default_cache_dir() {
        wu_content::kits::set_cache_dir(dir);
    }
    app.run()
}

/// Bakes (or loads) every kit on a thread of its own, so no song waits for one.
fn warm_up_kits(audio: NonSend<audio::AudioLink>) {
    let sample_rate = audio.sample_rate();
    let spawned = std::thread::Builder::new()
        .name("kits".into())
        .spawn(move || wu_content::kits::warm_up(sample_rate));
    if let Err(error) = spawned {
        warn!("kits will bake as songs need them: {error}");
    }
}
