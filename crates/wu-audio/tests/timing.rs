//! Hits land on the exact sample frame the tempo map gives them, whatever the
//! buffer size, through loops and seeks; live hits land where their timing mode says.

use std::sync::Arc;

use wu_audio::{
    BufferTiming, Command, Hit, LiveHit, LiveMode, Program, Report, VoiceSource, VoiceStart, engine, render_offline,
};
use wu_dsp::Sample;
use wu_instruments::{Kit, PAD_COUNT, Pad, PadSound};
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
        }),
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
    assert_eq!((snapshot.playback_ns, snapshot.output_latency_ns), (7_003, 3));
}
