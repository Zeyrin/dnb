//! Where raw input comes from.

#[cfg(feature = "gilrs")]
mod gilrs;
mod scripted;

#[cfg(feature = "gilrs")]
pub use self::gilrs::GilrsBackend;
pub use scripted::ScriptedBackend;

use std::time::Duration;

use crate::event::{DeviceInfo, InputEvent};

/// A source of controller events. Lives on the input thread.
pub trait Backend {
    fn name(&self) -> &'static str;
    /// Waits up to `timeout` for input, then appends everything available,
    /// stamped on the shared clock.
    fn wait(&mut self, events: &mut Vec<InputEvent>, timeout: Duration);
    /// The controllers connected right now.
    fn devices(&self) -> Vec<DeviceInfo>;
}
