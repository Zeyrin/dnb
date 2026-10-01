//! The input thread.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rtrb::{Consumer, RingBuffer};
use wu_instruments::Pad;

use crate::backend::Backend;
use crate::event::{Button, DeviceInfo, InputEvent, InputKind};
use crate::mapping::{Hand, Layout};

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

/// Switches shared with the input thread, flipped by the game as screens change.
#[derive(Debug, Default)]
pub struct LiveControl {
    enabled: AtomicBool,
    layout: AtomicU8,
    /// Per hand: 0 for nothing, else 1 + the index of the pad its shoulder plays.
    roll_pads: [AtomicU8; 2],
}

impl LiveControl {
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

    /// The pad a button plays right now, shoulders included.
    pub fn pad_for(&self, button: Button) -> Option<Pad> {
        match button {
            Button::L1 => self.roll_pad(Hand::Left),
            Button::R1 => self.roll_pad(Hand::Right),
            other => self.layout().pad_for(other),
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
    /// must live where they were created). `on_pad` runs there too, for every
    /// pad press while live play is enabled: hand it to the audio engine.
    pub fn spawn<B, F, P>(make_backend: F, layout: Layout, mut on_pad: P) -> Result<InputThread, InputError>
    where
        B: Backend,
        F: FnOnce() -> Result<B, InputError> + Send + 'static,
        P: FnMut(Pad, u64) + Send + 'static,
    {
        let (mut tx, events) = RingBuffer::new(EVENT_SLOTS);
        let live = Arc::new(LiveControl::default());
        live.set_layout(layout);
        live.set_enabled(true);
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
                    let mut batch = Vec::with_capacity(64);
                    while !stop.load(Ordering::Relaxed) {
                        batch.clear();
                        backend.wait(&mut batch, WAIT);
                        let mut devices_changed = false;
                        for event in &batch {
                            if let InputKind::Pressed(button) = event.kind
                                && live.enabled()
                                && let Some(pad) = live.pad_for(button)
                            {
                                on_pad(pad, event.at_ns);
                            }
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
