//! WHEEL UP!: the game. This crate is rendering, UI and glue only; the music,
//! the timing and the rules live in the `wu-*` crates, which run without it.

#![forbid(unsafe_code)]

mod audio;
mod calibrate;
mod capture;
mod fonts;
mod input;
mod monitor;
mod overlay;
mod pads;
mod palette;
mod screens;
mod settings;
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
    /// Start the demo playing straight away.
    #[arg(long)]
    autoplay: bool,
    /// The screen to open on.
    #[arg(long, value_enum, default_value_t = StartScreen::Play)]
    screen: StartScreen,
    /// Save a PNG of the window to this path once the scene has settled, then quit.
    #[arg(long, value_name = "PATH")]
    screenshot: Option<PathBuf>,
    /// Frames to wait before taking the screenshot.
    #[arg(long, default_value_t = 30)]
    screenshot_after: u32,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum StartScreen {
    Play,
    Controller,
    Calibrate,
}

fn main() -> AppExit {
    // Fix the shared clock's epoch before any input or audio timestamp exists.
    wu_time::mono::epoch();
    let args = Args::parse();

    let mut app = App::new();
    let start = match args.screen {
        StartScreen::Play => screens::Screen::Play,
        StartScreen::Controller => screens::Screen::Controller,
        StartScreen::Calibrate => screens::Screen::Calibrate,
    };
    app.insert_resource(ClearColor(palette::BACKDROP))
        .insert_resource(settings::SettingsStore::load())
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
            title::TitlePlugin,
            pads::PadsPlugin {
                autoplay: args.autoplay,
            },
            monitor::MonitorPlugin,
            calibrate::CalibratePlugin,
            overlay::OverlayPlugin,
        ));

    app.add_plugins(capture::CapturePlugin {
        auto: args.screenshot.map(|path| (path, args.screenshot_after)),
    });
    app.run()
}
