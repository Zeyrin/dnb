//! `wheelup-cli`: the headless side of WHEEL UP!. Rendering and inspection run
//! without a window, an audio device or a controller, so CI and agents can use
//! them; `play` and `devices` are there for checking a real sound card.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use wu_audio::output::{OutputOptions, list_outputs, prepare};
use wu_audio::{Command as EngineCommand, engine, render_offline, write_wav};
use wu_content::demo::{DEMO_BARS, DEMO_BPM, demo_program};
use wu_time::{TempoMap, Tick};

#[derive(Debug, Parser)]
#[command(name = "wheelup-cli", version, about = "Headless tools for WHEEL UP!")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the musical-time constants this build uses.
    Info,
    /// Render a song to a 16-bit stereo WAV, faster than real time.
    Render {
        /// What to render. Only `demo` exists until songs arrive in M3.
        song: String,
        #[arg(long, short)]
        out: PathBuf,
        /// Bars to render; the demo's two-bar pattern repeats.
        #[arg(long, default_value_t = 8)]
        bars: i64,
        #[arg(long, default_value_t = DEMO_BPM)]
        bpm: f64,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
        /// Frames per engine call, as a sound card would ask for.
        #[arg(long, default_value_t = 256)]
        block: usize,
    },
    /// List the audio output devices.
    Devices,
    /// Play a song on a sound card.
    Play {
        song: String,
        /// Part of the output device's name; the default device otherwise.
        #[arg(long)]
        device: Option<String>,
        /// Frames per callback; smaller is lower latency.
        #[arg(long)]
        buffer: Option<u32>,
        #[arg(long, default_value_t = DEMO_BPM)]
        bpm: f64,
        /// How long to play, looping the pattern.
        #[arg(long, default_value_t = 10.0)]
        seconds: f64,
    },
}

fn main() -> anyhow::Result<()> {
    wu_time::mono::epoch();
    match Cli::parse().command {
        Command::Info => {
            println!("wheelup-cli {}", env!("CARGO_PKG_VERSION"));
            println!("ticks per beat: {}", wu_time::PPQ);
            println!("ticks per 16th step: {}", wu_time::TICKS_PER_STEP);
        }
        Command::Render {
            song,
            out,
            bars,
            bpm,
            sample_rate,
            block,
        } => {
            require_demo(&song)?;
            let program = demo_program(sample_rate, bpm, bars, false);
            // One extra bar so the last hits ring out.
            let frames = TempoMap::constant(bpm).frame_at(Tick::from_bars(bars + 1), sample_rate);
            let started = Instant::now();
            let render = render_offline(program, usize::try_from(frames)?, block);
            write_wav(&out, &render.audio, sample_rate).with_context(|| format!("writing {}", out.display()))?;
            let peak = render.audio.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            println!(
                "{}: {bars} bars at {bpm} BPM, {:.2} s, {} hits, peak {:.1} dBFS, rendered in {:.0} ms",
                out.display(),
                frames as f64 / f64::from(sample_rate),
                render.starts.len(),
                20.0 * peak.max(1e-9).log10(),
                started.elapsed().as_secs_f64() * 1000.0,
            );
        }
        Command::Devices => {
            for name in list_outputs()? {
                println!("{name}");
            }
        }
        Command::Play {
            song,
            device,
            buffer,
            bpm,
            seconds,
        } => {
            require_demo(&song)?;
            let prepared = prepare(&OutputOptions {
                device,
                buffer_frames: buffer,
            })?;
            let info = prepared.info.clone();
            println!(
                "{} ({}): {} Hz, {} channels, {}, buffer {}",
                info.device,
                info.host,
                info.sample_rate,
                info.channels,
                info.sample_format,
                info.buffer_frames
                    .map_or("driver default".into(), |b| format!("{b} frames")),
            );
            if info.bluetooth {
                println!("warning: Bluetooth output adds 100 ms or more of latency");
            }
            let mut parts = engine(prepared.sample_rate());
            let program = demo_program(prepared.sample_rate(), bpm, DEMO_BARS, true);
            for command in [EngineCommand::Load(Box::new(program)), EngineCommand::Play] {
                parts
                    .handle
                    .send(command)
                    .map_err(|_| anyhow::anyhow!("engine queue full"))?;
            }
            let _output = prepared.start(parts.engine)?;
            let until = Instant::now() + Duration::from_secs_f64(seconds.max(0.0));
            while Instant::now() < until {
                parts.handle.poll(|_| {});
                thread::sleep(Duration::from_millis(20));
            }
        }
    }
    Ok(())
}

fn require_demo(song: &str) -> anyhow::Result<()> {
    if song != "demo" {
        bail!("unknown song \"{song}\": only \"demo\" exists until the song format lands");
    }
    Ok(())
}
