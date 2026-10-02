//! Hits land on the exact sample frame the tempo map gives them, whatever the
//! buffer size, through loops and seeks; live hits land where their timing mode says.

use std::sync::Arc;

use wu_audio::{
    BufferTiming, Command, Hit, LiveHit, LiveMode, LiveNote, Note, Program, Report, VoiceSource, VoiceStart, engine,
    render_offline,
};
use wu_dsp::Adsr;
use wu_dsp::Sample;
use wu_instruments::synth::{Filter, Wave};
use wu_instruments::{Bus, Instrument, Kit, PAD_COUNT, Pad, PadSound, Patch, Sends, Tone};
use wu_time::{TempoMap, Tick};

const SR: u32 = 48_000;

/// Every pad plays a single-sample click, so onsets are unmistakable.
fn click_kit() -> Kit {
    Kit {
        name: "clicks".into(),
        pads: std::array::from_fn::<_, PAD_COUNT, _>(|_| PadSound {
            name: "click".into(),
            sample: Arc::new(Sample::mono(vec![1.0], SR)),
            gain: 1.0,
            pan: 0.0,
            choke: None,
            bus: Bus::Drums,
            sidechain: false,
            sends: Sends::DRY,
        }),
        breaks: Vec::new(),
    }
}

fn sequenced(starts: &[VoiceStart]) -> Vec<(Tick, i64, u64)> {
    starts
        .iter()
        .filter_map(|s| match s.source {
            VoiceSource::Sequence { tick, transport_frame } => Some((tick, transport_frame, s.device_frame)),
            VoiceSource::Live { .. } => None,
        })
        .collect()
}

#[test]
fn hits_start_on_their_exact_frames_across_odd_buffer_sizes() {
    let tempo = TempoMap::constant(174.0);
    let hits: Vec<Hit> = (0..32)
        .map(|step| Hit {
            tick: Tick::from_steps(step).swung(0.12),
            pad: Pad::from_index(step as usize % 8).expect("eight pads"),
            velocity: 0.9,
        })
        .collect();
    let program = Program::new(SR, tempo.clone(), click_kit()).with_hits(hits.clone());
    let frames = tempo.frame_at(Tick::from_bars(2), SR) as usize + 10;

    for block in [1, 97, 256, 4096] {
        let render = render_offline(program.clone(), frames, block);
        let starts = sequenced(&render.starts);
        assert_eq!(starts.len(), hits.len(), "block {block}");
        for (hit, (tick, transport_frame, device_frame)) in hits.iter().zip(starts) {
            let expected = tempo.frame_at(hit.tick, SR);
            assert_eq!(tick, hit.tick);
            assert_eq!(transport_frame, expected, "block {block}, tick {tick}");
            assert_eq!(device_frame, expected as u64, "block {block}, tick {tick}");
            // The click is audible exactly there and not a frame before.
            let at = expected as usize;
            assert!(render.audio[2 * at] > 0.5, "block {block}: silent at {at}");
            if at > 0 {
                assert_eq!(render.audio[2 * at - 2], 0.0, "block {block}: early at {at}");
            }
        }
    }
}

#[test]
fn a_loop_repeats_seamlessly() {
    let tempo = TempoMap::constant(174.0);
    let program = Program::new(SR, tempo.clone(), click_kit())
        .with_hits([
            Hit {
                tick: Tick::ZERO,
                pad: Pad::P1,
                velocity: 1.0,
            },
            Hit {
                tick: Tick::from_beats(2),
                pad: Pad::P2,
                velocity: 1.0,
            },
            Hit {
                tick: Tick::from_bars(1),
                pad: Pad::P3,
                velocity: 1.0,
            },
        ])
        .with_loop(Tick::ZERO, Tick::from_bars(1));
    let bar = tempo.frame_at(Tick::from_bars(1), SR);
    let half = tempo.frame_at(Tick::from_beats(2), SR);
    let render = render_offline(program, (bar * 3) as usize, 333);
    let starts = sequenced(&render.starts);
    let devices: Vec<u64> = starts.iter().map(|s| s.2).collect();
    let expected: Vec<u64> = (0..3)
        .flat_map(|k| [k * bar, k * bar + half])
        .map(|f| f as u64)
        .collect();
    assert_eq!(devices, expected[..devices.len()].to_vec());
    assert_eq!(devices.len(), 6, "the hit at the loop end never plays");
    assert!(starts.iter().all(|s| s.0 != Tick::from_bars(1)));
}

#[test]
fn seeking_moves_the_next_hit() {
    let tempo = TempoMap::constant(170.0);
    let program = Program::new(SR, tempo.clone(), click_kit()).with_hits((0..8).map(|bar| Hit {
        tick: Tick::from_bars(bar),
        pad: Pad::P1,
        velocity: 1.0,
    }));
    let mut parts = engine(SR);
    for command in [
        Command::Load(Box::new(program)),
        Command::Seek(Tick::from_bars(5)),
        Command::Play,
    ] {
        parts.handle.send(command).expect("room in the queue");
    }
    let mut buffer = vec![0.0; 512];
    parts.engine.process(&mut buffer, BufferTiming::default());
    let mut first = None;
    parts.handle.poll(|report| {
        if let Report::VoiceStarted(start) = report {
            first.get_or_insert(start);
        }
    });
    let start = first.expect("the bar-5 hit plays at once");
    assert_eq!(
        start.source,
        VoiceSource::Sequence {
            tick: Tick::from_bars(5),
            transport_frame: tempo.frame_at(Tick::from_bars(5), SR)
        }
    );
    assert_eq!(start.device_frame, 0);
}

fn live_start(mode: LiveMode, hit_at_ns: u64, timing: BufferTiming) -> u64 {
    let mut parts = engine(SR);
    let program = Program::new(SR, TempoMap::constant(174.0), click_kit());
    for command in [Command::Load(Box::new(program)), Command::SetLiveMode(mode)] {
        parts.handle.send(command).expect("room in the queue");
    }
    assert!(parts.live.hit(LiveHit {
        pad: Pad::P5,
        velocity: 1.0,
        at_ns: hit_at_ns
    }));
    let mut buffer = vec![0.0; 2 * 256];
    parts.engine.process(&mut buffer, timing);
    let mut device_frame = None;
    parts.handle.poll(|report| {
        if let Report::VoiceStarted(start) = report {
            device_frame = Some(start.device_frame);
        }
    });
    device_frame.expect("the live hit played")
}

#[test]
fn live_hits_follow_their_scheduling_mode() {
    let latency_ns = 4_000_000;
    let playback_ns = 1_000_000_000;
    let callback_ns = playback_ns - latency_ns;
    let timing = BufferTiming {
        playback_ns,
        output_latency_ns: latency_ns,
    };
    // Pressed 2 ms before the callback ran.
    let pressed = callback_ns - 2_000_000;
    assert_eq!(live_start(LiveMode::Asap, pressed, timing), 0);
    // Stable: press + one buffer (5.33 ms) + latency, which is 3.33 ms into this buffer.
    assert_eq!(live_start(LiveMode::Stable, pressed, timing), 160);
}

#[test]
fn a_program_at_the_wrong_rate_is_refused() {
    let mut parts = engine(SR);
    let program = Program::new(44_100, TempoMap::constant(174.0), click_kit());
    parts
        .handle
        .send(Command::Load(Box::new(program)))
        .expect("room in the queue");
    parts.engine.process(&mut [0.0; 64], BufferTiming::default());
    let mut reports = Vec::new();
    parts.handle.poll(|r| reports.push(r));
    assert_eq!(
        reports,
        vec![Report::ProgramRejected {
            expected_rate: SR,
            got_rate: 44_100
        }]
    );
}

#[test]
fn the_clock_reports_what_is_playing() {
    let tempo = TempoMap::constant(174.0);
    let mut parts = engine(SR);
    parts
        .handle
        .send(Command::Load(Box::new(Program::new(SR, tempo, click_kit()))))
        .expect("room");
    parts.handle.send(Command::Play).expect("room");
    let mut buffer = vec![0.0; 2 * 128];
    for k in 0..4u64 {
        parts.engine.process(
            &mut buffer,
            BufferTiming {
                playback_ns: 7_000 + k,
                output_latency_ns: 3,
            },
        );
    }
    let snapshot = parts.handle.clock();
    assert!(snapshot.playing);
    assert_eq!(
        snapshot.generation,
        parts.handle.loads_sent(),
        "the load has taken effect"
    );
    assert_eq!((snapshot.device_frame, snapshot.transport_frame), (384, 384));
    // The limiter's look-ahead counts as output latency.
    let look_ahead_ns = parts.engine.latency_frames() as f64 * 1e9 / f64::from(SR);
    assert!(look_ahead_ns > 1e6, "about 1.6 ms");
    assert!((snapshot.playback_ns as f64 - 7_003.0 - look_ahead_ns).abs() <= 1.0);
    assert_eq!(snapshot.playback_ns - 7_003, snapshot.output_latency_ns - 3);
}

/// Plays live rail notes on a steady tone and returns the left channel, with
/// the limiter's look-ahead taken off so frame `f` of the result is frame `f`.
fn rail_output(frames: usize, events: impl Fn(usize, &mut wu_audio::LiveSender)) -> Vec<f32> {
    let tone = Tone {
        name: "steady".into(),
        sample: Arc::new(Sample::mono(vec![0.5; 64], SR)),
        root_key: 60,
        sustain: Some((0, 64)),
        gain: 1.0,
        pan: 0.0,
        bus: Bus::Bass,
        sends: Sends::DRY,
        tune: 1.0,
    };
    let program = Program::new(SR, TempoMap::constant(120.0), click_kit()).with_tone(tone);
    let mut parts = engine(SR);
    for command in [Command::Load(Box::new(program)), Command::Play] {
        parts.handle.send(command).expect("room in the queue");
    }
    let latency = parts.engine.latency_frames();
    let mut left = Vec::new();
    let mut buffer = vec![0.0; 2 * 256];
    let mut done = 0;
    while left.len() < frames + latency {
        events(done, &mut parts.live);
        parts.engine.process(&mut buffer, BufferTiming::default());
        left.extend(buffer.iter().step_by(2));
        done += 256;
    }
    left.drain(..latency);
    left
}

#[test]
fn a_rail_note_stops_at_its_charted_end() {
    let out = rail_output(9_600, |done, live| {
        if done == 0 {
            assert!(live.note_on(LiveNote {
                rail: 1,
                key: 60,
                velocity: 1.0,
                at_ns: 0,
                until_frame: Some(4_800),
            }));
        }
    });
    assert!(out[2_400] > 0.3, "sounding while held");
    assert!(out[4_799] > 0.3, "still sounding just before the end");
    assert_eq!(out[4_800 + 720 + 8], 0.0, "silent once its release is over");
}

#[test]
fn letting_go_of_the_rail_releases_the_note() {
    let out = rail_output(9_600, |done, live| match done {
        0 => assert!(live.note_on(LiveNote {
            rail: 0,
            key: 60,
            velocity: 1.0,
            at_ns: 0,
            until_frame: None,
        })),
        1_280 => assert!(live.note_off(0, 0)),
        _ => {}
    });
    assert!(out[1_279] > 0.3, "held until the release");
    assert_eq!(out[1_280 + 720 + 8], 0.0, "gone after the release");
}

#[test]
fn a_muted_player_part_stays_silent_until_unmuted() {
    let tempo = TempoMap::constant(120.0);
    let beat = |b: i64, pad: Pad| Hit {
        tick: Tick::from_beats(b),
        pad,
        velocity: 1.0,
    };
    let program = Program::new(SR, tempo, click_kit())
        .with_hits([beat(0, Pad::P1), beat(1, Pad::P1), beat(2, Pad::P1)])
        .with_player_hits([beat(0, Pad::P2), beat(1, Pad::P2), beat(2, Pad::P2)]);
    let mut parts = engine(SR);
    for command in [Command::Load(Box::new(program)), Command::Play] {
        parts.handle.send(command).expect("room in the queue");
    }
    let mut started = Vec::new();
    // 480-frame buffers: exactly 50 to a beat (half a second at 120 BPM).
    let mut buffer = vec![0.0; 2 * 480];
    // A beat each: unmuted, muted, unmuted again.
    for (beat_index, muted) in [false, true, false].into_iter().enumerate() {
        parts.handle.send(Command::MutePlayer(muted)).expect("room");
        for _ in 0..50 {
            parts.engine.process(&mut buffer, BufferTiming::default());
        }
        parts.handle.poll(|report| {
            if let Report::VoiceStarted(start) = report {
                started.push((beat_index, start.pad));
            }
        });
    }
    let player: Vec<usize> = started.iter().filter(|s| s.1 == Pad::P2).map(|s| s.0).collect();
    assert_eq!(player, vec![0, 2], "the player's beat-two hit was muted");
    assert_eq!(
        started.iter().filter(|s| s.1 == Pad::P1).count(),
        3,
        "the backing never is"
    );
}

#[test]
fn a_rewind_waits_out_its_gap_then_plays_the_phrase_again() {
    // A click on every beat at 120 BPM: 24 000 frames apart.
    let tempo = TempoMap::constant(120.0);
    let program = Program::new(SR, tempo.clone(), click_kit()).with_hits((0..16).map(|beat| Hit {
        tick: Tick::from_beats(beat),
        pad: Pad::P1,
        velocity: 1.0,
    }));
    let mut parts = engine(SR);
    let gap = 12_000;
    for command in [
        Command::Load(Box::new(program)),
        Command::Play,
        Command::Jump {
            at: Tick::from_beats(4),
            to: Tick::ZERO,
            gap_frames: gap,
        },
    ] {
        parts.handle.send(command).expect("room in the queue");
    }
    let mut starts = Vec::new();
    let mut transport = Vec::new();
    let mut buffer = vec![0.0; 2 * 333];
    for _ in 0..(8 * 24_000 + gap as usize) / 333 + 1 {
        parts.engine.process(&mut buffer, BufferTiming::default());
        parts.handle.poll(|report| match report {
            Report::VoiceStarted(start) => starts.push(start.device_frame),
            Report::Transport {
                device_frame,
                transport_frame,
                playing,
            } => transport.push((device_frame, transport_frame, playing)),
            _ => {}
        });
    }
    let beat = 24_000u64;
    let gap = u64::from(gap);
    let expected: Vec<u64> = (0..4)
        .map(|b| b * beat)
        .chain((0..4).map(|b| 4 * beat + gap + b * beat))
        .collect();
    assert_eq!(&starts[..8], &expected[..], "beats 0-3, the gap, then beats 0-3 again");
    assert!(
        transport.contains(&(4 * beat, 0, false)),
        "the cut, at its exact frame: {transport:?}"
    );
    assert!(
        transport.contains(&(4 * beat + gap, 0, true)),
        "the drop: {transport:?}"
    );
}

#[test]
fn bass_notes_play_every_bass_sound_and_track_notes_their_own() {
    // A sampled bass sound hard left, a synth one hard right, and a synth
    // track hard right: which side sounds tells them apart.
    let left = Tone {
        name: "left".into(),
        sample: Arc::new(Sample::mono(vec![0.5; 64], SR)),
        root_key: 60,
        sustain: Some((0, 64)),
        gain: 1.0,
        pan: -1.0,
        bus: Bus::Bass,
        sends: Sends::DRY,
        tune: 1.0,
    };
    let right = Patch {
        wave: Wave::Sine,
        filter: Filter::low(20_000.0, 0.707),
        amp: Adsr::new(0.001, 0.01, 1.0, 0.01),
        pan: 1.0,
        ..Patch::BASIC
    };
    let beat = Tick::from_beats(1);
    let program = Program::new(SR, TempoMap::constant(120.0), click_kit())
        .with_tone(left)
        .with_bass_sound(Instrument::Synth(right))
        .with_instrument(Instrument::Synth(right))
        .with_notes([Note {
            tick: Tick::ZERO,
            length: beat,
            key: 60,
            velocity: 1.0,
        }])
        .with_track_notes(
            2,
            [Note {
                tick: Tick::from_beats(2),
                length: beat,
                key: 72,
                velocity: 1.0,
            }],
        );
    let render = render_offline(program, 4 * 24_000, 256);
    let level = |side: usize, from_beat: usize| {
        let frames = &render.audio[2 * (from_beat * 24_000 + 2_400)..2 * (from_beat * 24_000 + 21_600)];
        frames.iter().skip(side).step_by(2).map(|x| x * x).sum::<f32>()
    };
    assert!(
        level(0, 0) > 100.0 && level(1, 0) > 100.0,
        "the bass line on both its sounds"
    );
    assert!(level(0, 2) < 1e-6, "the track leaves the bass sounds alone");
    assert!(level(1, 2) > 100.0, "and plays its own");
}
