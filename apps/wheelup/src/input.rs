//! Controllers and keyboard, turned into timestamped events and pad actions.
//!
//! Controllers are read on `wu-input`'s thread, which also plays pads straight
//! into the audio engine. The keyboard comes through Bevy, so its timestamps are
//! only as fine as the frame rate: good enough to develop with, not to compete.

use std::collections::{BTreeMap, VecDeque};

use bevy::prelude::*;
use wu_input::backend::GilrsBackend;
use wu_input::{
    ActionEvent, Axis, Button, DeviceId, DeviceInfo, InputEvent, InputKind, InputThread, IntervalStats, KEYBOARD,
    Layout, Mapper,
};

use crate::audio::AudioLink;
use crate::settings::SettingsStore;

#[derive(Debug)]
pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        // Built now, not in a startup system: the first screen's OnEnter runs
        // before Startup and already needs it.
        start_input(app.world_mut());
        app.add_message::<RawInput>()
            .add_message::<PlayerAction>()
            .add_systems(PreUpdate, pump_input);
    }
}

/// Every raw event, controllers and keyboard alike.
#[derive(Message, Clone, Copy, Debug)]
pub struct RawInput(pub InputEvent);

/// Pads, rolls, rails and menu buttons, after the layout is applied.
#[derive(Message, Clone, Copy, Debug)]
pub struct PlayerAction(pub ActionEvent);

/// What one controller is doing right now.
#[derive(Clone, Debug, Default)]
pub struct ControllerState {
    pub held: Vec<Button>,
    pub axes: BTreeMap<Axis, f32>,
}

impl ControllerState {
    pub fn axis(&self, axis: Axis) -> f32 {
        self.axes.get(&axis).copied().unwrap_or(0.0)
    }

    pub fn is_held(&self, button: Button) -> bool {
        self.held.contains(&button)
    }
}

/// How many raw events the controller screen's log keeps.
const LOG_LENGTH: usize = 10;

#[derive(Debug)]
pub struct InputLink {
    thread: Option<InputThread>,
    /// Why there is no controller thread, if there isn't one.
    pub error: Option<String>,
    pub mapper: Mapper,
    pub stats: IntervalStats,
    pub states: BTreeMap<DeviceId, ControllerState>,
    pub log: VecDeque<InputEvent>,
    live: bool,
}

impl InputLink {
    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.thread.as_ref().map(InputThread::devices).unwrap_or_default()
    }

    pub fn backend(&self) -> Result<String, String> {
        match (&self.thread, &self.error) {
            (Some(thread), _) => thread.backend(),
            (None, Some(error)) => Err(error.clone()),
            (None, None) => Err("not started".to_owned()),
        }
    }

    pub fn layout(&self) -> Layout {
        self.mapper.layout
    }

    pub fn set_layout(&mut self, layout: Layout) {
        self.mapper.layout = layout;
        if let Some(thread) = &self.thread {
            thread.live.set_layout(layout);
        }
    }

    /// Whether pad presses make sound immediately (off while calibrating).
    pub fn set_live(&mut self, live: bool) {
        self.live = live;
        if let Some(thread) = &self.thread {
            thread.live.set_enabled(live);
        }
    }

    /// The controller to show: the most recently active one, else the keyboard.
    pub fn focus(&self) -> DeviceId {
        self.log
            .iter()
            .rev()
            .map(|e| e.device)
            .find(|&d| d != KEYBOARD)
            .unwrap_or(KEYBOARD)
    }
}

fn start_input(world: &mut World) {
    let layout = world.resource::<SettingsStore>().layout();
    let sender = world.non_send_mut::<AudioLink>().take_input_sender();
    let (thread, error) = match sender {
        Some(mut live) => {
            let on_pad = move |pad, at_ns| {
                live.hit(wu_audio::LiveHit {
                    pad,
                    velocity: 1.0,
                    at_ns,
                });
            };
            match InputThread::spawn(GilrsBackend::new, layout, on_pad) {
                Ok(thread) => (Some(thread), None),
                Err(error) => (None, Some(error.to_string())),
            }
        }
        None => (None, Some("the live sender was already taken".to_owned())),
    };
    if let Some(error) = &error {
        warn!("no controllers: {error}");
    }
    world.insert_non_send(InputLink {
        thread,
        error,
        mapper: Mapper::new(layout),
        stats: IntervalStats::default(),
        states: BTreeMap::new(),
        log: VecDeque::with_capacity(LOG_LENGTH),
        live: true,
    });
}

/// Keyboard stand-ins for controller buttons.
const KEYS: [(KeyCode, Button); 15] = [
    (KeyCode::ArrowUp, Button::DPadUp),
    (KeyCode::ArrowDown, Button::DPadDown),
    (KeyCode::ArrowLeft, Button::DPadLeft),
    (KeyCode::ArrowRight, Button::DPadRight),
    (KeyCode::KeyI, Button::North),
    (KeyCode::KeyJ, Button::West),
    (KeyCode::KeyK, Button::South),
    (KeyCode::KeyL, Button::East),
    (KeyCode::KeyE, Button::L1),
    (KeyCode::KeyO, Button::R1),
    (KeyCode::Space, Button::Start),
    (KeyCode::Enter, Button::Start),
    (KeyCode::Tab, Button::Select),
    (KeyCode::KeyX, Button::L3),
    (KeyCode::KeyM, Button::R3),
];

/// Keyboard stand-ins for the analog triggers: fully pressed or released.
const TRIGGER_KEYS: [(KeyCode, Axis); 2] = [(KeyCode::KeyZ, Axis::L2), (KeyCode::KeyN, Axis::R2)];

fn pump_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut input: NonSendMut<InputLink>,
    mut audio: NonSendMut<AudioLink>,
    mut raw: MessageWriter<RawInput>,
    mut actions: MessageWriter<PlayerAction>,
) {
    let input = &mut *input;
    let mut events = Vec::new();
    if let Some(thread) = input.thread.as_mut() {
        while let Ok(event) = thread.events.pop() {
            events.push(event);
        }
    }
    let now = wu_time::mono::now_ns();
    let keyboard = |kind| InputEvent {
        at_ns: now,
        device: KEYBOARD,
        kind,
    };
    for (key, button) in KEYS {
        if keys.just_pressed(key) {
            events.push(keyboard(InputKind::Pressed(button)));
            // The keyboard has no input thread: play its pads from here.
            if input.live
                && let Some(pad) = input.mapper.layout.pad_for(button)
            {
                audio.hit(pad, now);
            }
        }
        if keys.just_released(key) {
            events.push(keyboard(InputKind::Released(button)));
        }
    }
    for (key, axis) in TRIGGER_KEYS {
        if keys.just_pressed(key) {
            events.push(keyboard(InputKind::Axis(axis, 1.0)));
        }
        if keys.just_released(key) {
            events.push(keyboard(InputKind::Axis(axis, 0.0)));
        }
    }

    for event in events {
        input.stats.observe(&event);
        let state = input.states.entry(event.device).or_default();
        match event.kind {
            InputKind::Pressed(button) if !state.held.contains(&button) => state.held.push(button),
            InputKind::Released(button) => state.held.retain(|&b| b != button),
            InputKind::Axis(axis, value) => {
                state.axes.insert(axis, value);
            }
            InputKind::Disconnected => {
                input.states.remove(&event.device);
            }
            _ => {}
        }
        // Stick noise would flood the log; it shows in the readouts instead.
        if !matches!(event.kind, InputKind::Axis(..)) {
            if input.log.len() == LOG_LENGTH {
                input.log.pop_front();
            }
            input.log.push_back(event);
        }
        input.mapper.map(&event, |action| {
            actions.write(PlayerAction(action));
        });
        raw.write(RawInput(event));
    }
}
