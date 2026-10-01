//! The audio callback must never allocate. This test binary installs an
//! allocator that aborts if anything allocates inside `assert_no_alloc`.

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use wu_audio::{BufferTiming, Command, Hit, LiveHit, LiveMode, Note, Program, engine};
use wu_instruments::{Kit, Pad, Tone};
use wu_time::{TempoMap, Tick};

#[global_allocator]
static ALLOCATOR: AllocDisabler = AllocDisabler;

#[test]
fn a_dense_song_with_live_hits_and_voice_stealing_never_allocates() {
    let sample_rate = 48_000;
    let tempo = TempoMap::constant(174.0);
    // Every pad on every 32nd note for four bars: far more voices than the pool
    // holds, so stealing and choking run constantly.
    let hits = (0..4 * 32).flat_map(|n| {
        Pad::ALL.into_iter().map(move |pad| Hit {
            tick: Tick(n * 120),
            pad,
            velocity: 0.8,
        })
    });
    // And a bass note on every beat, overlapping the next: tone voices loop and release too.
    let notes = (0..16).map(|beat| Note {
        tick: Tick::from_beats(beat),
        length: Tick::from_beats(2),
        key: 29 + (beat % 5) as u8,
        velocity: 0.9,
    });
    let program = Program::new(sample_rate, tempo, Kit::ragga_93(sample_rate))
        .with_tone(Tone::sub(sample_rate))
        .with_notes(notes)
        .with_hits(hits)
        .with_loop(Tick::ZERO, Tick::from_bars(4));
    let mut parts = engine(sample_rate);
    for command in [
        Command::Load(Box::new(program)),
        Command::SetLiveMode(LiveMode::Stable),
        Command::Play,
    ] {
        parts.handle.send(command).expect("room in the queue");
    }

    let mut buffer = vec![0.0f32; 2 * 256];
    let mut peak = 0.0f32;
    for k in 0..(sample_rate as u64 * 12 / 256) {
        let playback_ns = k * 5_333_333;
        parts.live.hit(LiveHit {
            pad: Pad::ALL[(k % 8) as usize],
            velocity: 1.0,
            at_ns: playback_ns,
        });
        if k % 100 == 50 {
            parts.handle.send(Command::Seek(Tick::from_bars(1))).expect("room");
        }
        assert_no_alloc(|| {
            parts.engine.process(
                &mut buffer,
                BufferTiming {
                    playback_ns,
                    output_latency_ns: 1_000_000,
                },
            );
        });
        peak = buffer.iter().fold(peak, |m, x| m.max(x.abs()));
        parts.handle.poll(|_| {});
    }
    assert!(peak > 0.1 && peak <= 1.0, "peak {peak}");
    assert_eq!(parts.engine.freed_on_audio_thread(), 0);
}
