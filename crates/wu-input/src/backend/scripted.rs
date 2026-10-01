//! A backend that replays a fixed list of events: for tests and replays.

use std::collections::VecDeque;
use std::thread;
use std::time::Duration;

use super::Backend;
use crate::event::{DeviceInfo, InputEvent};

#[derive(Debug, Default)]
pub struct ScriptedBackend {
    events: VecDeque<InputEvent>,
    devices: Vec<DeviceInfo>,
}

impl ScriptedBackend {
    pub fn new(events: impl IntoIterator<Item = InputEvent>, devices: Vec<DeviceInfo>) -> ScriptedBackend {
        ScriptedBackend {
            events: events.into_iter().collect(),
            devices,
        }
    }
}

impl Backend for ScriptedBackend {
    fn name(&self) -> &'static str {
        "scripted"
    }

    /// Hands over every remaining event at once, keeping their scripted timestamps.
    fn wait(&mut self, events: &mut Vec<InputEvent>, timeout: Duration) {
        if self.events.is_empty() {
            thread::sleep(timeout);
        }
        events.extend(self.events.drain(..));
    }

    fn devices(&self) -> Vec<DeviceInfo> {
        self.devices.clone()
    }
}
