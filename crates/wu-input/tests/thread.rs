//! The input thread delivers every event in order with its timestamp, and plays
//! pads live only while live play is on.

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use wu_input::backend::ScriptedBackend;
use wu_input::{
    Axis, Button, DeviceId, Hand, InputEvent, InputKind, InputThread, Layout, LiveAction, LiveControl, RailNote,
};
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
        Arc::new(LiveControl::new(Layout::Reel)),
        move |action| {
            let _ = tx.send(action);
        },
    )
    .expect("thread starts");

    let events = drain(&mut thread, 4);
    assert_eq!(
        events.iter().map(|e| e.at_ns).collect::<Vec<_>>(),
        vec![1_000, 2_000, 3_000, 4_000]
    );
    let live: Vec<LiveAction> = rx.try_iter().collect();
    assert_eq!(
        live,
        vec![
            LiveAction::Pad {
                pad: Pad::P1,
                at_ns: 1_000
            },
            LiveAction::Pad {
                pad: Pad::P7,
                at_ns: 2_000
            },
        ],
        "L1 plays nothing outside a roll"
    );
    assert_eq!(thread.backend(), Ok("scripted".to_owned()));
}

#[test]
fn no_live_sound_while_disabled() {
    let (tx, rx) = mpsc::channel::<LiveAction>();
    let script = vec![press(1_000, Button::DPadUp)];
    let live = Arc::new(LiveControl::new(Layout::Drummer));
    live.set_enabled(false);
    let mut thread = InputThread::spawn(
        move || Ok(ScriptedBackend::new(script, Vec::new())),
        live,
        move |action| {
            let _ = tx.send(action);
        },
    )
    .expect("thread starts");
    assert_eq!(drain(&mut thread, 1).len(), 1);
    assert!(rx.try_iter().next().is_none());
}

#[test]
fn shoulders_play_the_armed_roll_and_rails_the_armed_bass_note() {
    let device = DeviceId(3);
    let axis = |at_ns, value| InputEvent {
        at_ns,
        device,
        kind: InputKind::Axis(Axis::R2, value),
    };
    let script = vec![
        press(1_000, Button::R1),
        axis(2_000, 0.6),
        axis(3_000, 0.15),
        axis(4_000, 0.05),
    ];
    let live = Arc::new(LiveControl::new(Layout::Reel));
    live.set_roll_pad(Hand::Right, Some(Pad::P7));
    let note = RailNote {
        key: 29,
        until_frame: 96_000,
    };
    live.set_rail_note(Hand::Right, Some(note));
    assert_eq!(live.rail_note(Hand::Right), Some(note));
    assert_eq!(live.rail_note(Hand::Left), None);
    let (tx, rx) = mpsc::channel();
    let mut thread = InputThread::spawn(
        move || Ok(ScriptedBackend::new(script, Vec::new())),
        live,
        move |action| {
            let _ = tx.send(action);
        },
    )
    .expect("thread starts");
    drain(&mut thread, 4);
    let played: Vec<LiveAction> = rx.try_iter().collect();
    assert_eq!(
        played,
        vec![
            LiveAction::Pad {
                pad: Pad::P7,
                at_ns: 1_000
            },
            LiveAction::RailOn {
                hand: Hand::Right,
                note,
                at_ns: 2_000
            },
            // 0.15 is above the release threshold: still held until 0.05.
            LiveAction::RailOff {
                hand: Hand::Right,
                at_ns: 4_000
            },
        ]
    );
}
