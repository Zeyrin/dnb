//! WHEEL UP!: the game. This crate is rendering, UI and glue only; the music,
//! the timing and the rules live in the `wu-*` crates, which run without it.

#![forbid(unsafe_code)]

mod audio;
mod capture;
mod fonts;
mod overlay;
mod pads;
mod palette;
mod title;

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
    /// Save a PNG of the window to this path once the scene has settled, then quit.
    #[arg(long, value_name = "PATH")]
    screenshot: Option<PathBuf>,
    /// Frames to wait before taking the screenshot.
    #[arg(long, default_value_t = 30)]
    screenshot_after: u32,
}

fn main() -> AppExit {
    // Fix the shared clock's epoch before any input or audio timestamp exists.
    wu_time::mono::epoch();
    let args = Args::parse();

    let mut app = App::new();
    app.insert_resource(ClearColor(palette::BACKDROP))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "WHEEL UP!".into(),
                resolution: WindowResolution::new(1280, 720),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((FrameTimeDiagnosticsPlugin::default(), fonts::FontsPlugin))
        .add_plugins(audio::AudioPlugin {
            options: OutputOptions {
                device: args.audio_device,
                buffer_frames: args.buffer,
            },
            silent: args.silent,
            autoplay: args.autoplay,
        })
        .add_plugins((title::TitlePlugin, pads::PadsPlugin, overlay::OverlayPlugin));

    if let Some(path) = args.screenshot {
        app.add_plugins(capture::CapturePlugin {
            path,
            after_frames: args.screenshot_after,
        });
    }
    app.run()
}
