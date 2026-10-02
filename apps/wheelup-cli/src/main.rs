//! `wheelup-cli`: the headless side of WHEEL UP!. Rendering and inspection run
//! without a window, an audio device or a controller, so CI and agents can use
//! them; `play` and `devices` are there for checking a real sound card.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use wu_audio::output::{OutputOptions, list_outputs, prepare};
use wu_audio::{Command as EngineCommand, MixSettings, Note, Program, engine, render_offline, write_wav};
use wu_content::demo::{DEMO_BARS, DEMO_BPM, audition_phrase, demo_program};
use wu_content::notes::parse_notes;
use wu_content::songs::BUILTIN;
use wu_instruments::{INSTRUMENTS, Instrument, Kit};
use wu_time::{STEPS_PER_BAR, TempoMap, Tick};

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
        /// `demo`, or a built-in song id (see `songs`).
        song: String,
        #[arg(long, short)]
        out: PathBuf,
        /// Bars of the demo to render (its two-bar pattern repeats). Songs render whole.
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
    /// List the songs: the built-in ones, then the tunes imported into the game.
    Songs,
    /// Listen to a tune (MP3, WAV, FLAC, OGG or M4A) and keep it in the game's
    /// library as a song to play: print what was heard and the charts made from it.
    Import {
        file: PathBuf,
        /// Where to keep it, if not the game's own library.
        #[arg(long)]
        library: Option<PathBuf>,
    },
    /// List the built-in instruments.
    Instruments,
    /// List the built-in kits: their pads and breaks.
    Kits,
    /// Play a kit to a WAV file: each pad a beat apart, then each break, looped.
    Kit {
        /// A kit's id (see `kits`).
        id: String,
        #[arg(long, short)]
        out: PathBuf,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
    },
    /// List the breaks the kits are cut from.
    Breaks,
    /// Perform a break to a WAV file, looped, then each of its slices.
    Break {
        /// A break's name, as `breaks` lists it ("Rough Rider" or rough-rider).
        name: String,
        #[arg(long, short)]
        out: PathBuf,
        /// How many times round the loop.
        #[arg(long, default_value_t = 4)]
        loops: usize,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
    },
    /// Play one instrument to a WAV file, alone or over the demo beat.
    Audition {
        /// An instrument (see `instruments`).
        instrument: String,
        #[arg(long, short)]
        out: PathBuf,
        /// What to play, in the songs' note notation ("F3+Ab3+C4:16 | …");
        /// a phrase that suits the instrument otherwise.
        #[arg(long)]
        notes: Option<String>,
        /// Over the demo beat.
        #[arg(long)]
        beat: bool,
        #[arg(long, default_value_t = 168.0)]
        bpm: f64,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
    },
    /// Generate a song's chart for each difficulty, validate it, and report its density.
    Chart {
        /// A built-in song's id, or an imported one's (see `songs`).
        song: String,
        /// Only this difficulty (Beginner, Easy, Medium, Hard, Junglist).
        #[arg(long)]
        difficulty: Option<String>,
        /// Print this many bars of each chart, from the first drop, as a lane diagram.
        #[arg(long, default_value_t = 0)]
        show_bars: i64,
    },
    /// Judge a saved replay again, from its presses alone, and print the score.
    Replay { file: PathBuf },
    /// Measure loudness (EBU R128) and true peak: a built-in song or `demo`,
    /// rendered as it ships, or a WAV file.
    Lufs {
        /// A song id, `demo`, or a path to a .wav file.
        what: String,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
    },
    /// List the audio output devices.
    Devices,
    /// Print controller events as they arrive, with timestamps and the report
    /// rate and jitter measured from them.
    InputMonitor {
        /// Stop after this many seconds.
        #[arg(long, default_value_t = 30.0)]
        seconds: f64,
    },
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
            let (program, length) = if song == "demo" {
                (demo_program(sample_rate, bpm, bars, false), Tick::from_bars(bars))
            } else {
                let compiled = builtin(&song)?.load()?;
                let program = compiled.whole_program(sample_rate, &compiled.tempo, 0);
                (program, compiled.length)
            };
            // One extra bar so the last hits ring out.
            let frames = program.tempo.frame_at(length + Tick::from_bars(1), sample_rate);
            let started = Instant::now();
            let render = render_offline(program, usize::try_from(frames)?, block);
            write_wav(&out, &render.audio, sample_rate).with_context(|| format!("writing {}", out.display()))?;
            let peak = render.audio.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            println!(
                "{}: {:.2} s, {} drum hits, peak {:.1} dBFS, rendered in {:.0} ms",
                out.display(),
                frames as f64 / f64::from(sample_rate),
                render.starts.len(),
                20.0 * peak.max(1e-9).log10(),
                started.elapsed().as_secs_f64() * 1000.0,
            );
        }
        Command::Songs => {
            for song in BUILTIN {
                match song.load() {
                    Ok(compiled) => println!(
                        "{:<24} {} · {} · {:.0} BPM · {} · {} bars",
                        song.id,
                        compiled.meta.title,
                        compiled.meta.artist,
                        compiled.tempo.bpm_at(Tick::ZERO),
                        compiled.meta.key,
                        compiled.length.bar()
                    ),
                    Err(error) => println!("{:<24} BROKEN: {error}", song.id),
                }
            }
            for imported in imported_songs() {
                match imported {
                    Ok(imported) => {
                        let song = imported.song();
                        println!(
                            "{:<24} {} · {} · {:.1} BPM · imported · {} bars",
                            imported.id,
                            song.meta.title,
                            if song.meta.artist.is_empty() {
                                "?"
                            } else {
                                &song.meta.artist
                            },
                            song.tempo.bpm_at(Tick::ZERO),
                            song.length.bar()
                        );
                    }
                    Err((folder, error)) => println!("{:<24} BROKEN: {error}", folder.display()),
                }
            }
        }
        Command::Import { file, library } => import(&file, library)?,
        Command::Instruments => {
            for name in INSTRUMENTS {
                if let Some(instrument) = Instrument::named(name, 48_000) {
                    let kind = match instrument {
                        Instrument::Sampled(_) => "sampled",
                        Instrument::Synth(_) => "synth",
                    };
                    println!(
                        "{name:<14} {:<14} {kind:<8} {:?} bus",
                        instrument.name(),
                        instrument.bus()
                    );
                }
            }
        }
        Command::Kits => {
            for def in &wu_instruments::KITS {
                let pads: Vec<&str> = def.pads.iter().map(|pad| pad.name).collect();
                let breaks: Vec<&str> = def.breaks.iter().map(|b| b.name).collect();
                println!("{:<16} {:<16} {}", def.id, def.name, pads.join(" · "));
                println!("{:<33} breaks: {}", "", breaks.join(", "));
            }
        }
        Command::Kit { id, out, sample_rate } => play_kit(&id, &out, sample_rate)?,
        Command::Breaks => {
            for def in wu_instruments::breaks::library::ALL {
                println!(
                    "{:<16} played at {:.0} BPM, sampled up to {:.0} · {} slices",
                    def.name,
                    def.played_bpm,
                    def.bpm,
                    def.slices.len()
                );
            }
        }
        Command::Break {
            name,
            out,
            loops,
            sample_rate,
        } => perform_break(&name, &out, loops, sample_rate)?,
        Command::Audition {
            instrument,
            out,
            notes,
            beat,
            bpm,
            sample_rate,
        } => audition(&instrument, &out, notes.as_deref(), beat, bpm, sample_rate)?,
        Command::Chart {
            song,
            difficulty,
            show_bars,
        } => chart(&song, difficulty.as_deref(), show_bars)?,
        Command::Replay { file } => replay(&file)?,
        Command::Lufs { what, sample_rate } => lufs(&what, sample_rate)?,
        Command::InputMonitor { seconds } => input_monitor(seconds)?,
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

fn audition(name: &str, out: &Path, notes: Option<&str>, beat: bool, bpm: f64, sample_rate: u32) -> anyhow::Result<()> {
    let Some(instrument) = Instrument::named(name, sample_rate) else {
        bail!("no instrument called \"{name}\": try `wheelup-cli instruments`");
    };
    let phrase = notes.unwrap_or_else(|| audition_phrase(name, instrument.bus()));
    let (parsed, steps) = parse_notes(phrase).map_err(|e| anyhow::anyhow!("--notes: {e}"))?;
    let bars = (steps + STEPS_PER_BAR - 1) / STEPS_PER_BAR;
    let notes = parsed.iter().map(|n| Note {
        tick: Tick::from_steps(n.step),
        length: Tick::from_steps(n.length),
        key: n.key,
        velocity: 1.0,
    });
    let program = if beat {
        demo_program(sample_rate, bpm, bars, false)
    } else {
        Program::new(sample_rate, TempoMap::constant(bpm), Kit::ragga_93(sample_rate)).with_mix(MixSettings {
            master_db: -6.0,
            ..MixSettings::default()
        })
    };
    let track = u8::try_from(program.instruments.len())?;
    let program = program
        .with_instrument(instrument.at_tempo(bpm))
        .with_track_notes(track, notes);
    // Two bars more, for the tails to ring out.
    let frames = program.tempo.frame_at(Tick::from_bars(bars + 2), sample_rate);
    let render = render_offline(program, usize::try_from(frames)?, 256);
    write_wav(out, &render.audio, sample_rate).with_context(|| format!("writing {}", out.display()))?;
    let peak = render.audio.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let rms = (render.audio.iter().map(|x| x * x).sum::<f32>() / render.audio.len().max(1) as f32).sqrt();
    println!(
        "{}: {name}, {:.2} s, peak {:.1} dBFS, RMS {:.1} dBFS",
        out.display(),
        frames as f64 / f64::from(sample_rate),
        20.0 * peak.max(1e-9).log10(),
        20.0 * rms.max(1e-9).log10(),
    );
    Ok(())
}

fn play_kit(id: &str, out: &Path, sample_rate: u32) -> anyhow::Result<()> {
    let started = Instant::now();
    let Some(kit) = wu_content::kits::kit(id, sample_rate) else {
        bail!("no kit called \"{id}\": try `wheelup-cli kits`");
    };
    let baked_in = started.elapsed();
    // Each pad at its own level and pan, a beat apart at 170 BPM.
    let beat = (60.0 / 170.0 * f64::from(sample_rate)) as usize;
    let mut stereo = Vec::new();
    for pad in &kit.pads {
        let angle = (pad.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        let start = stereo.len();
        for &x in pad.sample.data() {
            stereo.extend([x * pad.gain * angle.cos(), x * pad.gain * angle.sin()]);
        }
        stereo.resize(start + 2 * beat.max(pad.sample.frames()), 0.0);
    }
    for brk in &kit.breaks {
        stereo.resize(stereo.len() + 2 * beat, 0.0);
        for _ in 0..2 {
            stereo.extend(brk.sample.data().iter().flat_map(|&x| [0.8 * x, 0.8 * x]));
        }
    }
    write_wav(out, &stereo, sample_rate).with_context(|| format!("writing {}", out.display()))?;
    let breaks: Vec<String> = kit
        .breaks
        .iter()
        .map(|b| format!("{} ({} bars at {:.0} BPM)", b.name, b.bars, b.bpm))
        .collect();
    println!(
        "{}: {}, 8 pads then {}; ready in {:.0} ms",
        out.display(),
        kit.name,
        breaks.join(", "),
        baked_in.as_secs_f64() * 1000.0
    );
    Ok(())
}

fn perform_break(name: &str, out: &Path, loops: usize, sample_rate: u32) -> anyhow::Result<()> {
    let wanted = name.to_lowercase().replace('-', " ");
    let Some(def) = wu_instruments::breaks::library::ALL
        .into_iter()
        .find(|def| def.name.to_lowercase() == wanted)
    else {
        bail!("no break called \"{name}\": try `wheelup-cli breaks`");
    };
    let started = Instant::now();
    let take = def.perform(sample_rate)?;
    let elapsed = started.elapsed();
    // The loop round and round, a beat of silence, then each slice a beat apart.
    let beat = (60.0 / def.bpm * f64::from(sample_rate)) as usize;
    let mut mono: Vec<f32> = take
        .audio
        .iter()
        .copied()
        .cycle()
        .take(take.audio.len() * loops.max(1))
        .collect();
    mono.resize(mono.len() + beat, 0.0);
    for (_, slice) in &take.slices {
        let start = mono.len();
        mono.extend_from_slice(slice);
        mono.resize(start + beat.max(slice.len()), 0.0);
    }
    let stereo: Vec<f32> = mono.iter().flat_map(|&x| [0.8 * x, 0.8 * x]).collect();
    write_wav(out, &stereo, sample_rate).with_context(|| format!("writing {}", out.display()))?;
    let slices: Vec<&str> = take.slices.iter().map(|(name, _)| *name).collect();
    println!(
        "{}: {}, {} bars at {:.0} BPM, looped {loops}×, then {}; performed in {:.0} ms",
        out.display(),
        def.name,
        take.bars,
        def.bpm,
        slices.join(", "),
        elapsed.as_secs_f64() * 1000.0
    );
    Ok(())
}

fn input_monitor(seconds: f64) -> anyhow::Result<()> {
    use std::sync::Arc;

    use wu_input::backend::GilrsBackend;
    use wu_input::{InputKind, InputThread, IntervalStats, Layout, LiveControl};

    let mut thread = InputThread::spawn(GilrsBackend::new, Arc::new(LiveControl::new(Layout::Reel)), |_| {})?;
    let until = Instant::now() + Duration::from_secs_f64(seconds.max(0.0));
    let mut stats = IntervalStats::default();
    let mut last_report = Instant::now();
    let mut first_ns = None;
    thread::sleep(Duration::from_millis(200));
    match thread.backend() {
        Ok(name) => println!("backend: {name}"),
        Err(error) => bail!("no controller backend: {error}"),
    }
    let devices = thread.devices();
    if devices.is_empty() {
        println!("no controller connected yet: plug one in");
    }
    for device in devices {
        println!("found {device} [{:?}]", device.family);
    }
    while Instant::now() < until {
        while let Ok(event) = thread.events.pop() {
            stats.observe(&event);
            let t0 = *first_ns.get_or_insert(event.at_ns);
            let what = match event.kind {
                InputKind::Pressed(button) => format!("pressed  {}", button.glyph()),
                InputKind::Released(button) => format!("released {}", button.glyph()),
                InputKind::Axis(axis, value) => format!("{axis:?} {value:+.3}"),
                InputKind::Connected => "connected".to_owned(),
                InputKind::Disconnected => "disconnected".to_owned(),
            };
            if !matches!(event.kind, InputKind::Axis(..)) {
                println!(
                    "{:>10.3} ms  #{}  {what}",
                    (event.at_ns - t0) as f64 / 1e6,
                    event.device.0
                );
            }
        }
        if last_report.elapsed() > Duration::from_secs(2) {
            last_report = Instant::now();
            if let (Some(median), Some(p95), Some(rate)) =
                (stats.quantile_ms(0.5), stats.quantile_ms(0.95), stats.rate_hz())
            {
                println!("interval median {median:.2} ms · p95 {p95:.2} ms · ≈ {rate:.0} reports/s");
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

fn builtin(id: &str) -> anyhow::Result<&'static wu_content::songs::BuiltinSong> {
    BUILTIN.iter().find(|s| s.id == id).ok_or_else(|| {
        let ids: Vec<&str> = BUILTIN.iter().map(|s| s.id).collect();
        anyhow::anyhow!("no song \"{id}\"; built in: {}", ids.join(", "))
    })
}

/// The tunes imported into the game's library.
fn imported_songs() -> Vec<Result<wu_import::ImportedSong, (PathBuf, wu_import::ImportError)>> {
    wu_import::library::default_dir().map_or_else(Vec::new, |dir| wu_import::library::load_all(&dir))
}

/// A built-in song by its id, or an imported one by its.
fn song_by_id(id: &str) -> anyhow::Result<wu_content::project::Song> {
    if let Some(song) = BUILTIN.iter().find(|s| s.id == id) {
        return Ok(song.load()?);
    }
    imported_songs()
        .into_iter()
        .flatten()
        .find(|imported| imported.id == id)
        .map(|imported| imported.song())
        .ok_or_else(|| anyhow::anyhow!("no song \"{id}\" (see `songs`)"))
}

fn import(file: &Path, library: Option<PathBuf>) -> anyhow::Result<()> {
    use std::io::Write;
    use wu_chart::{Difficulty, auto_chart, validate};
    use wu_import::hits::Drum;

    let library = library
        .or_else(wu_import::library::default_dir)
        .context("this system has no data folder to keep imported tunes in; pass --library")?;
    let started = Instant::now();
    print!("{}:", file.display());
    let imported = wu_import::import(file, &library, |stage| {
        print!(" {}…", stage.describe());
        let _ = std::io::stdout().flush();
    });
    println!();
    let imported = imported.with_context(|| format!("importing {}", file.display()))?;
    let kept = &imported.imported;
    let song = imported.song();
    let seconds = song.tempo.seconds_at(song.length.0 as f64);
    println!(
        "{}{} · {:.2} BPM, first bar line at {:.3} s · {} bars ({}:{:02}) · heard in {:.1} s",
        kept.title,
        if kept.artist.is_empty() {
            String::new()
        } else {
            format!(" — {}", kept.artist)
        },
        kept.bpm,
        kept.first_bar_s,
        kept.bars,
        (seconds / 60.0) as u32,
        (seconds % 60.0) as u32,
        started.elapsed().as_secs_f64()
    );
    let count = |drum: Drum| kept.drums.iter().filter(|h| h.1 == drum).count();
    println!(
        "  drums: {} kicks, {} snares, {} ghosts, {} hats",
        count(Drum::Kick),
        count(Drum::Snare),
        count(Drum::Ghost),
        count(Drum::Hat)
    );
    let keys = kept.bass.iter().map(|n| n.2);
    let (low, high) = (keys.clone().min(), keys.max());
    println!(
        "  bass line: {} notes{}",
        kept.bass.len(),
        match (low, high) {
            (Some(low), Some(high)) => format!(", {} to {}", key_name(low), key_name(high)),
            _ => String::new(),
        }
    );
    let shape: Vec<String> = kept
        .sections
        .iter()
        .map(|(name, start, end, _)| format!("{name} {start}–{end}"))
        .collect();
    println!("  {}", shape.join(" · "));
    println!(
        "  played {:.1} dB {} to sit at the game's level",
        kept.gain_db.abs(),
        if kept.gain_db < 0.0 { "down" } else { "up" }
    );
    for difficulty in Difficulty::ALL {
        let chart = auto_chart(&song.drums, &song.bass, &song.tempo, difficulty);
        let problems = validate(&chart, &song.tempo);
        println!(
            "  {:<9} {:>5} notes · {:>3} holds{}",
            difficulty.name(),
            chart.notes.len(),
            chart.holds.len(),
            if problems.is_empty() {
                String::new()
            } else {
                format!(" · {} PROBLEMS", problems.len())
            }
        );
    }
    println!("kept as {} in {}", imported.id, imported.folder.display());
    Ok(())
}

/// A MIDI key as a note name: 29 is F1.
fn key_name(key: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];
    format!("{}{}", NAMES[usize::from(key % 12)], i32::from(key) / 12 - 1)
}

fn chart(id: &str, only: Option<&str>, show_bars: i64) -> anyhow::Result<()> {
    use wu_chart::{Difficulty, auto_chart, validate};
    use wu_instruments::Pad;

    let song = song_by_id(id)?;
    let seconds = song.tempo.seconds_at(song.length.0 as f64);
    for difficulty in Difficulty::ALL {
        if only.is_some_and(|name| !name.eq_ignore_ascii_case(difficulty.name())) {
            continue;
        }
        let chart = auto_chart(&song.drums, &song.bass, &song.tempo, difficulty);
        let problems = validate(&chart, &song.tempo);
        let busiest = (0..song.length.bar())
            .map(|bar| {
                let (from, to) = (Tick::from_bars(bar), Tick::from_bars(bar + 2));
                let n = chart.notes.iter().filter(|n| n.tick >= from && n.tick < to).count();
                n as f64 / (song.tempo.seconds_at(to.0 as f64) - song.tempo.seconds_at(from.0 as f64))
            })
            .fold(0.0f64, f64::max);
        let verdict = if problems.is_empty() {
            "playable".to_owned()
        } else {
            format!("{} PROBLEMS: {problems:?}", problems.len())
        };
        println!(
            "{:<9} {:>4} notes · {} rolls · {} holds · {:.2} notes/s on average · {:.2} at the busiest · {verdict}",
            difficulty.name(),
            chart.notes.len(),
            chart.rolls.len(),
            chart.holds.len(),
            chart.notes.len() as f64 / seconds,
            busiest,
        );
        if show_bars > 0 {
            // One line per 16th step from the first drop: an o for each pad to press
            // (r inside a roll), P1 to P8.
            let drop = song
                .sections
                .iter()
                .find(|s| s.0.starts_with("Drop"))
                .map_or(Tick::ZERO, |s| s.1);
            for step in 0..show_bars * 16 {
                let tick = drop + Tick::from_steps(step);
                let line: String = Pad::ALL
                    .iter()
                    .map(|&pad| match (chart.contains(tick, pad), chart.roll_of(tick, pad)) {
                        (true, Some(_)) => 'r',
                        (true, None) => 'o',
                        _ => '.',
                    })
                    .collect();
                println!("    {tick:>10}  {line}");
            }
        }
    }
    Ok(())
}

fn replay(file: &Path) -> anyhow::Result<()> {
    use wu_game::judge::Judgement;
    use wu_game::play::replay_score;
    use wu_game::replay::Replay;

    let replay = Replay::load(file).with_context(|| format!("reading {}", file.display()))?;
    let song = builtin(&replay.song)?.load()?;
    let score = replay_score(&song, &replay)
        .ok_or_else(|| anyhow::anyhow!("no difficulty called \"{}\"", replay.difficulty))?;
    let counts: Vec<String> = Judgement::ALL
        .iter()
        .map(|j| format!("{} {}", j.label(), score.counts[j.index()]))
        .collect();
    println!(
        "{} · {} · {} % tempo{}{}",
        song.meta.title,
        replay.difficulty,
        replay.tempo_percent,
        if replay.no_fail { " · No-Fail" } else { "" },
        if replay.autoplay { " · selecta bot" } else { "" },
    );
    println!(
        "{} · score {} · accuracy {:.2} % · max combo {} · {} presses",
        if score.failed {
            "PLUG PULLED"
        } else {
            score.grade().label()
        },
        score.points,
        score.accuracy() * 100.0,
        score.max_combo,
        replay.presses.len(),
    );
    println!(
        "{} · overhits {} · holds kept {} / {}",
        counts.join(" · "),
        score.overhits,
        score.holds_completed,
        score.holds_completed + score.holds_dropped
    );
    Ok(())
}

fn lufs(what: &str, sample_rate: u32) -> anyhow::Result<()> {
    use wu_content::mastering::{Loudness, MAX_TRUE_PEAK_DB, TARGET_LUFS, TOLERANCE_LU, measure};

    let loudness = if what.ends_with(".wav") {
        let (audio, rate) = read_wav(Path::new(what))?;
        Loudness::of(&audio, rate)
    } else if what == "demo" {
        let program = demo_program(sample_rate, DEMO_BPM, DEMO_BARS, false);
        let frames = program.tempo.frame_at(Tick::from_bars(DEMO_BARS), sample_rate);
        let render = render_offline(program, usize::try_from(frames)?, 512);
        Loudness::of(&render.audio, sample_rate)
    } else {
        measure(&builtin(what)?.load()?, sample_rate)
    }
    .ok_or_else(|| anyhow::anyhow!("{what} is silent"))?;
    // Shorter than 3 s: no short-term window to report.
    let short_term = if loudness.max_short_term.is_finite() {
        format!("{:.1} LUFS", loudness.max_short_term)
    } else {
        "n/a".to_owned()
    };
    println!(
        "{what}: {:.1} LUFS integrated · loudest 3 s {short_term} · loudest 400 ms {:.1} LUFS",
        loudness.integrated, loudness.max_momentary
    );
    println!(
        "true peak {:.2} dBTP · sample peak {:.2} dBFS · {:.1} s",
        loudness.true_peak_db, loudness.sample_peak_db, loudness.seconds
    );
    let excess = loudness.excess();
    let verdict = if loudness.on_target() {
        "on target".to_owned()
    } else if excess.abs() > TOLERANCE_LU {
        let (word, change) = if excess > 0.0 {
            ("loud", "lower")
        } else {
            ("quiet", "raise")
        };
        format!(
            "too {word} by {:.1} LU: {change} the song's mix.master by about that many dB",
            excess.abs()
        )
    } else {
        "true peak too high: lower mix.master".to_owned()
    };
    println!("target {TARGET_LUFS} LUFS ± {TOLERANCE_LU}, at most {MAX_TRUE_PEAK_DB} dBTP: {verdict}");
    Ok(())
}

/// Reads a WAV as interleaved stereo (a mono file measures as one channel).
fn read_wav(path: &Path) -> anyhow::Result<(Vec<f32>, u32)> {
    let mut reader = hound::WavReader::open(path).with_context(|| format!("reading {}", path.display()))?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|s| s as f32 * scale))
                .collect::<Result<_, _>>()?
        }
    };
    let audio = match spec.channels {
        1 => samples.iter().flat_map(|&x| [x, 0.0]).collect(),
        2 => samples,
        n => bail!("{} has {n} channels; only mono and stereo are measured", path.display()),
    };
    Ok((audio, spec.sample_rate))
}

fn require_demo(song: &str) -> anyhow::Result<()> {
    if song != "demo" {
        bail!("unknown song \"{song}\": only \"demo\" exists until the song format lands");
    }
    Ok(())
}
