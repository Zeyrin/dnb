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
use wu_audio::{Command as EngineCommand, engine, render_offline, write_wav};
use wu_content::demo::{DEMO_BARS, DEMO_BPM, demo_program};
use wu_content::songs::BUILTIN;
use wu_time::Tick;

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
    /// List the built-in songs.
    Songs,
    /// Generate a song's chart for each difficulty, validate it, and report its density.
    Chart {
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
                let program = compiled.program(sample_rate, &compiled.tempo, 0, |_, _| false);
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
        }
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

fn input_monitor(seconds: f64) -> anyhow::Result<()> {
    use wu_input::backend::GilrsBackend;
    use wu_input::{InputKind, InputThread, IntervalStats, Layout};

    let mut thread = InputThread::spawn(GilrsBackend::new, Layout::Reel, |_, _| {})?;
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

fn chart(id: &str, only: Option<&str>, show_bars: i64) -> anyhow::Result<()> {
    use wu_chart::{Difficulty, auto_chart, validate};
    use wu_instruments::Pad;

    let song = builtin(id)?.load()?;
    let seconds = song.tempo.seconds_at(song.length.0 as f64);
    for difficulty in Difficulty::ALL {
        if only.is_some_and(|name| !name.eq_ignore_ascii_case(difficulty.name())) {
            continue;
        }
        let chart = auto_chart(&song.drums, &song.tempo, difficulty);
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
            "{:<9} {:>4} notes · {} rolls · {:.2} notes/s on average · {:.2} at the busiest · {verdict}",
            difficulty.name(),
            chart.notes.len(),
            chart.rolls.len(),
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
    println!("{} · overhits {}", counts.join(" · "), score.overhits);
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
