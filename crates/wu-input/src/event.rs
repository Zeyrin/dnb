//! Raw input: physical controls, named by where they sit on the pad.

use std::fmt;

/// A controller, as numbered by its backend. The keyboard is [`KEYBOARD`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(pub u32);

/// The keyboard, which the game feeds through the same mapping as controllers.
pub const KEYBOARD: DeviceId = DeviceId(u32::MAX);

/// Buttons by position. PlayStation names in the comments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Button {
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    /// △
    North,
    /// □
    West,
    /// ✕
    South,
    /// ○
    East,
    L1,
    R1,
    L3,
    R3,
    /// Options
    Start,
    /// Create
    Select,
    /// The PS button
    Mode,
}

impl Button {
    /// The symbol printed on a PlayStation controller.
    pub fn glyph(self) -> &'static str {
        match self {
            Button::DPadUp => "↑",
            Button::DPadDown => "↓",
            Button::DPadLeft => "←",
            Button::DPadRight => "→",
            Button::North => "△",
            Button::West => "□",
            Button::South => "✕",
            Button::East => "○",
            Button::L1 => "L1",
            Button::R1 => "R1",
            Button::L3 => "L3",
            Button::R3 => "R3",
            Button::Start => "OPTIONS",
            Button::Select => "CREATE",
            Button::Mode => "PS",
        }
    }
}

/// Analog controls. Sticks run -1..1 (up is positive), triggers 0..1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Axis {
    LeftX,
    LeftY,
    RightX,
    RightY,
    L2,
    R2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputKind {
    Pressed(Button),
    Released(Button),
    Axis(Axis, f32),
    Connected,
    Disconnected,
}

/// One change on one device, stamped on the shared clock (`wu_time::mono`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputEvent {
    pub at_ns: u64,
    pub device: DeviceId,
    pub kind: InputKind,
}

/// Which button glyphs a controller should show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    PlayStation,
    Xbox,
    Nintendo,
    Steam,
    Generic,
    Keyboard,
}

impl Family {
    /// Guesses from the USB vendor id, then from the name.
    pub fn detect(vendor_id: Option<u16>, name: &str) -> Family {
        let name = name.to_lowercase();
        match vendor_id {
            Some(0x054C) => Family::PlayStation,
            Some(0x045E) => Family::Xbox,
            Some(0x057E) => Family::Nintendo,
            Some(0x28DE) => Family::Steam,
            _ if name.contains("dualsense") || name.contains("dualshock") || name.contains("playstation") => {
                Family::PlayStation
            }
            _ if name.contains("xbox") => Family::Xbox,
            _ if name.contains("nintendo") || name.contains("switch") => Family::Nintendo,
            _ if name.contains("steam") => Family::Steam,
            _ => Family::Generic,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: String,
    pub family: Family,
}

impl fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (#{})", self.name, self.id.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_come_from_vendor_ids_then_names() {
        assert_eq!(Family::detect(Some(0x054C), "Wireless Controller"), Family::PlayStation);
        assert_eq!(
            Family::detect(None, "DualSense Wireless Controller"),
            Family::PlayStation
        );
        assert_eq!(Family::detect(Some(0x045E), "Controller"), Family::Xbox);
        assert_eq!(Family::detect(None, "Some Pad"), Family::Generic);
    }
}
