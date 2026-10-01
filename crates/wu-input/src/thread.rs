//! The input thread.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rtrb::{Consumer, RingBuffer};
use wu_instruments::Pad;

use crate::backend::Backend;
use crate::event::{DeviceInfo, InputEvent, InputKind};
use crate::mapping::{Action, ActionEvent, Hand, Layout, Mapper, Phase};

/// Raw events waiting for the main thread. A second of mashing at 1 kHz fits.
const EVENT_SLOTS: usize = 4096;
/// How long the thread waits for input before checking whether it should stop.
const WAIT: Duration = Duration::from_millis(4);

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("controller backend unavailable: {0}")]
    Backend(String),
    #[error("could not start the input thread: {0}")]
    Thread(String),
}

/// The bass note a rail plays when pressed, and the transport frame its hold
/// ends on (the engine stops it there even if the trigger stays down).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RailNote {
    pub key: u8,
    pub until_frame: i64,
}

/// What a control plays straight away, without waiting for a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveAction {
    /// A pad, or a shoulder button inside a roll.
    Pad {
        pad: Pad,
        at_ns: u64,
    },
    /// A rail pressed with a bass note armed on it.
    RailOn {
        hand: Hand,
        note: RailNote,
        at_ns: u64,
    },
    RailOff {
        hand: Hand,
        at_ns: u64,
    },
}

/// A rail note packed in one atomic: the armed flag, the end frame, the key.
const RAIL_ARMED: u64 = 1 << 63;
const RAIL_FRAME_MASK: u64 = (1 << 55) - 1;

/// Switches shared with the input thread, flipped by the game as screens change.
#[derive(Debug, Default)]
pub struct LiveControl {
    enabled: AtomicBool,
    layout: AtomicU8,
    /// Per hand: 0 for nothing, else 1 + the index of the pad its shoulder plays.
    roll_pads: [AtomicU8; 2],
    /// Per hand: the bass note its rail plays, packed (see `RAIL_ARMED`).
    rail_notes: [AtomicU64; 2],
}

impl LiveControl {
    pub fn new(layout: Layout) -> LiveControl {
        let live = LiveControl::default();
        live.set_layout(layout);
        live.set_enabled(true);
        live
    }

    /// Whether pad presses should make sound straight away.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_layout(&self, layout: Layout) {
        self.layout.store(layout.as_u8(), Ordering::Relaxed);
    }

    pub fn layout(&self) -> Layout {
        Layout::from_u8(self.layout.load(Ordering::Relaxed))
    }

    /// What a hand's shoulder button (L1, R1) plays: the lane of a roll in
    /// reach, or nothing.
    pub fn set_roll_pad(&self, hand: Hand, pad: Option<Pad>) {
        let value = pad.map_or(0, |p| p.index() as u8 + 1);
        self.roll_pads[hand.index()].store(value, Ordering::Relaxed);
    }

    pub fn roll_pad(&self, hand: Hand) -> Option<Pad> {
        let value = self.roll_pads[hand.index()].load(Ordering::Relaxed);
        value.checked_sub(1).and_then(|i| Pad::from_index(usize::from(i)))
    }

    /// What a hand's rail plays when pressed: the next bass note it holds.
    pub fn set_rail_note(&self, hand: Hand, note: Option<RailNote>) {
        let value = note.map_or(0, |n| {
            RAIL_ARMED | ((n.until_frame.max(0) as u64 & RAIL_FRAME_MASK) << 8) | u64::from(n.key)
        });
        self.rail_notes[hand.index()].store(value, Ordering::Relaxed);
    }

    pub fn rail_note(&self, hand: Hand) -> Option<RailNote> {
        let value = self.rail_notes[hand.index()].load(Ordering::Relaxed);
        (value & RAIL_ARMED != 0).then_some(RailNote {
            key: (value & 0xFF) as u8,
            until_frame: ((value & !RAIL_ARMED) >> 8) as i64,
        })
    }

    /// What an action plays straight away, if anything (nothing while live play is off).
    pub fn respond(&self, action: &ActionEvent) -> Option<LiveAction> {
        if !self.enabled() {
            return None;
        }
        let at_ns = action.at_ns;
        match (action.action, action.phase) {
            (Action::Pad(pad), Phase::Pressed) => Some(LiveAction::Pad { pad, at_ns }),
            (Action::Roll(hand), Phase::Pressed) => self.roll_pad(hand).map(|pad| LiveAction::Pad { pad, at_ns }),
            (Action::Rail(hand), Phase::Pressed) => {
                self.rail_note(hand)
                    .map(|note| LiveAction::RailOn { hand, note, at_ns })
            }
            (Action::Rail(hand), Phase::Released) => Some(LiveAction::RailOff { hand, at_ns }),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct InputThread {
    /// Every raw event, in arrival order.
    pub events: Consumer<InputEvent>,
    pub live: Arc<LiveControl>,
    devices: Arc<Mutex<Vec<DeviceInfo>>>,
    backend_name: Arc<Mutex<Result<String, String>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl InputThread {
    /// Starts the thread. `make_backend` runs on the new thread (some backends
    /// must live where they were created). `on_live` runs there too, for
    /// everything `live` says should sound at once: hand it to the audio engine.
    pub fn spawn<B, F, P>(make_backend: F, live: Arc<LiveControl>, mut on_live: P) -> Result<InputThread, InputError>
    where
        B: Backend,
        F: FnOnce() -> Result<B, InputError> + Send + 'static,
        P: FnMut(LiveAction) + Send + 'static,
    {
        let (mut tx, events) = RingBuffer::new(EVENT_SLOTS);
        let devices = Arc::new(Mutex::new(Vec::new()));
        let backend_name = Arc::new(Mutex::new(Err("starting".to_owned())));
        let stop = Arc::new(AtomicBool::new(false));

        let handle = {
            let (live, devices, backend_name, stop) = (
                Arc::clone(&live),
                Arc::clone(&devices),
                Arc::clone(&backend_name),
                Arc::clone(&stop),
            );
            thread::Builder::new()
                .name("wheelup-input".into())
                .spawn(move || {
                    let mut backend = match make_backend() {
                        Ok(backend) => backend,
                        Err(error) => {
                            set(&backend_name, Err(error.to_string()));
                            return;
                        }
                    };
                    set(&backend_name, Ok(backend.name().to_owned()));
                    set(&devices, backend.devices());
                    // The thread's own mapper: the rails' press and release thresholds
                    // are applied here, so a trigger sounds without waiting for a frame.
                    let mut mapper = Mapper::new(live.layout());
                    let mut batch = Vec::with_capacity(64);
                    while !stop.load(Ordering::Relaxed) {
                        batch.clear();
                        backend.wait(&mut batch, WAIT);
                        let mut devices_changed = false;
                        mapper.layout = live.layout();
                        for event in &batch {
                            mapper.map(event, |action| {
                                if let Some(sound) = live.respond(&action) {
                                    on_live(sound);
                                }
                            });
                            devices_changed |= matches!(event.kind, InputKind::Connected | InputKind::Disconnected);
                            // A full queue means the game stalled; drop rather than block.
                            let _ = tx.push(*event);
                        }
                        if devices_changed {
                            set(&devices, backend.devices());
                        }
                    }
                })
                .map_err(|e| InputError::Thread(e.to_string()))?
        };
        Ok(InputThread {
            events,
            live,
            devices,
            backend_name,
            stop,
            handle: Some(handle),
        })
    }

    /// The connected controllers.
    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.devices.lock().map(|d| d.clone()).unwrap_or_default()
    }

    /// The backend's name once it started, or why it couldn't.
    pub fn backend(&self) -> Result<String, String> {
        self.backend_name
            .lock()
            .map_or_else(|_| Err("input thread crashed".to_owned()), |b| b.clone())
    }
}

fn set<T>(slot: &Mutex<T>, value: T) {
    if let Ok(mut guard) = slot.lock() {
        *guard = value;
    }
}

impl Drop for InputThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
