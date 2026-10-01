//! The input thread delivers every event in order with its timestamp, and plays
//! pads live only while live play is on.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use wu_input::backend::ScriptedBackend;
use wu_input::{Button, DeviceId, InputEvent, InputKind, InputThread, Layout};
use wu_instruments::Pad;

fn press(at_ns: u64, button: Button) -> InputEvent {
    InputEvent {
        at_ns,
        device: DeviceId(3),
        kind: InputKind::Pressed(button),
    }
}

fn drain(thread: &mut InputThread, expected: usize) -> Vec<InputEvent> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut events = Vec::new();
    while events.len() < expected && Instant::now() < deadline {
        while let Ok(event) = thread.events.pop() {
            events.push(event);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    events
}

#[test]
fn pads_sound_live_and_every_event_reaches_the_game() {
    let script = vec![
        press(1_000, Button::DPadUp),
        press(2_000, Button::South),
        press(3_000, Button::L1),
        InputEvent {
            at_ns: 4_000,
            device: DeviceId(3),
            kind: InputKind::Released(Button::South),
        },
    ];
    let (tx, rx) = mpsc::channel();
    let mut thread = InputThread::spawn(
        move || Ok(ScriptedBackend::new(script, Vec::new())),
        Layout::Reel,
        move |pad, at_ns| {
            let _ = tx.send((pad, at_ns));
        },
    )
    .expect("thread starts");

    let events = drain(&mut thread, 4);
    assert_eq!(
        events.iter().map(|e| e.at_ns).collect::<Vec<_>>(),
        vec![1_000, 2_000, 3_000, 4_000]
    );
    let live: Vec<(Pad, u64)> = rx.try_iter().collect();
    assert_eq!(live, vec![(Pad::P1, 1_000), (Pad::P7, 2_000)]);
    assert_eq!(thread.backend(), Ok("scripted".to_owned()));
}

#[test]
fn no_live_sound_while_disabled() {
    let (tx, rx) = mpsc::channel::<(Pad, u64)>();
    let script = vec![press(1_000, Button::DPadUp)];
    // The thread may read the script before we disable live play, so start
    // the backend only after a beat.
    let mut thread = InputThread::spawn(
        move || {
            std::thread::sleep(Duration::from_millis(50));
            Ok(ScriptedBackend::new(script, Vec::new()))
        },
        Layout::Drummer,
        move |pad, at_ns| {
            let _ = tx.send((pad, at_ns));
        },
    )
    .expect("thread starts");
    thread.live.set_enabled(false);
    assert_eq!(drain(&mut thread, 1).len(), 1);
    assert!(rx.try_iter().next().is_none());
}
