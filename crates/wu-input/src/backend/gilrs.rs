//! Gamepads through gilrs, on every desktop platform.
//!
//! On Linux gilrs passes on the kernel's own timestamp for each event; that is
//! converted to the shared clock and used when it is plausible (earlier than the
//! read, by less than 50 ms). Elsewhere the read time is the timestamp, and
//! blocking reads keep it within a fraction of a millisecond of the event.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ::gilrs::{Axis as GAxis, Button as GButton, EventType, Gilrs};

use super::Backend;
use crate::event::{Axis, Button, DeviceId, DeviceInfo, Family, InputEvent, InputKind};
use crate::thread::InputError;

/// The oldest an OS timestamp may be, relative to the read, and still be trusted.
const MAX_OS_AGE_NS: u64 = 50_000_000;
/// How often the wall-clock offset is re-measured (NTP may slew the wall clock).
const RESYNC: Duration = Duration::from_secs(5);

pub struct GilrsBackend {
    gilrs: Gilrs,
    wall_offset_ns: i128,
    synced_at: Instant,
}

impl std::fmt::Debug for GilrsBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GilrsBackend").finish_non_exhaustive()
    }
}

impl GilrsBackend {
    pub fn new() -> Result<GilrsBackend, InputError> {
        let gilrs = Gilrs::new().map_err(|e| InputError::Backend(e.to_string()))?;
        Ok(GilrsBackend {
            gilrs,
            wall_offset_ns: wall_offset_ns(),
            synced_at: Instant::now(),
        })
    }

    fn translate(&self, event: ::gilrs::Event, read_ns: u64) -> Option<InputEvent> {
        let os_ns = event
            .time
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|since| u64::try_from(since.as_nanos() as i128 - self.wall_offset_ns).ok());
        let at_ns = os_ns
            .filter(|&t| t <= read_ns && read_ns - t < MAX_OS_AGE_NS)
            .unwrap_or(read_ns);
        let kind = match event.event {
            EventType::ButtonPressed(button, _) => InputKind::Pressed(button_of(button)?),
            EventType::ButtonReleased(button, _) => InputKind::Released(button_of(button)?),
            EventType::ButtonChanged(GButton::LeftTrigger2, value, _) => InputKind::Axis(Axis::L2, value),
            EventType::ButtonChanged(GButton::RightTrigger2, value, _) => InputKind::Axis(Axis::R2, value),
            EventType::AxisChanged(axis, value, _) => InputKind::Axis(axis_of(axis)?, value),
            EventType::Connected => InputKind::Connected,
            EventType::Disconnected => InputKind::Disconnected,
            _ => return None,
        };
        Some(InputEvent {
            at_ns,
            device: DeviceId(usize::from(event.id) as u32),
            kind,
        })
    }
}

/// Wall clock minus the shared monotonic clock, in nanoseconds.
fn wall_offset_ns() -> i128 {
    let wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i128);
    wall - i128::from(wu_time::mono::now_ns())
}

/// Triggers are read as axes (`ButtonChanged`), never as digital buttons, so the
/// mapper's own thresholds decide when they press.
fn button_of(button: GButton) -> Option<Button> {
    Some(match button {
        GButton::DPadUp => Button::DPadUp,
        GButton::DPadDown => Button::DPadDown,
        GButton::DPadLeft => Button::DPadLeft,
        GButton::DPadRight => Button::DPadRight,
        GButton::North => Button::North,
        GButton::West => Button::West,
        GButton::South => Button::South,
        GButton::East => Button::East,
        GButton::LeftTrigger => Button::L1,
        GButton::RightTrigger => Button::R1,
        GButton::LeftThumb => Button::L3,
        GButton::RightThumb => Button::R3,
        GButton::Start => Button::Start,
        GButton::Select => Button::Select,
        GButton::Mode => Button::Mode,
        _ => return None,
    })
}

fn axis_of(axis: GAxis) -> Option<Axis> {
    Some(match axis {
        GAxis::LeftStickX => Axis::LeftX,
        GAxis::LeftStickY => Axis::LeftY,
        GAxis::RightStickX => Axis::RightX,
        GAxis::RightStickY => Axis::RightY,
        // Some platforms report analog triggers as Z axes in -1..1.
        GAxis::LeftZ => return None,
        GAxis::RightZ => return None,
        _ => return None,
    })
}

impl Backend for GilrsBackend {
    fn name(&self) -> &'static str {
        "gilrs"
    }

    fn wait(&mut self, events: &mut Vec<InputEvent>, timeout: Duration) {
        if self.synced_at.elapsed() > RESYNC {
            self.wall_offset_ns = wall_offset_ns();
            self.synced_at = Instant::now();
        }
        let mut next = self.gilrs.next_event_blocking(Some(timeout));
        while let Some(event) = next {
            let read_ns = wu_time::mono::now_ns();
            if let Some(event) = self.translate(event, read_ns) {
                events.push(event);
            }
            next = self.gilrs.next_event();
        }
    }

    fn devices(&self) -> Vec<DeviceInfo> {
        self.gilrs
            .gamepads()
            .map(|(id, pad)| DeviceInfo {
                id: DeviceId(usize::from(id) as u32),
                name: pad.name().to_owned(),
                family: Family::detect(pad.vendor_id(), pad.name()),
            })
            .collect()
    }
}
