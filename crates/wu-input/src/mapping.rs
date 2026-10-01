//! From buttons to musical actions.

use std::collections::BTreeMap;

use wu_instruments::Pad;

use crate::event::{Axis, Button, DeviceId, InputEvent, InputKind};

/// Analog triggers count as pressed once they rise past this…
pub const RAIL_PRESS: f32 = 0.20;
/// …and as released once they fall below this, so a resting finger can't flicker.
pub const RAIL_RELEASE: f32 = 0.10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Hand {
    Left,
    Right,
}

impl Hand {
    pub const fn index(self) -> usize {
        match self {
            Hand::Left => 0,
            Hand::Right => 1,
        }
    }
}

/// What a control means in play.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Pad(Pad),
    /// L1 / R1: an alternate stroke inside roll segments.
    Roll(Hand),
    /// L2 / R2 crossing the press threshold: the sub and bass rails.
    Rail(Hand),
    Pause,
    Select,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActionEvent {
    pub at_ns: u64,
    pub device: DeviceId,
    pub action: Action,
    pub phase: Phase,
    /// 1.0 for buttons; the trigger's travel for rails.
    pub value: f32,
}

/// Which pad each button plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Layout {
    /// Exactly the reel's mapping: ↑ kick, ↓ snare, ← ghost, → rim; △ □ ✕ ○ on the right.
    #[default]
    Reel,
    /// Kick on ↓, under the thumb's resting point, snare on ↑.
    Drummer,
}

impl Layout {
    pub const ALL: [Layout; 2] = [Layout::Reel, Layout::Drummer];

    pub fn name(self) -> &'static str {
        match self {
            Layout::Reel => "Reel",
            Layout::Drummer => "Drummer",
        }
    }

    pub fn pad_for(self, button: Button) -> Option<Pad> {
        let pad = match button {
            Button::DPadUp => Pad::P1,
            Button::DPadDown => Pad::P2,
            Button::DPadLeft => Pad::P3,
            Button::DPadRight => Pad::P4,
            Button::North => Pad::P5,
            Button::West => Pad::P6,
            Button::South => Pad::P7,
            Button::East => Pad::P8,
            _ => return None,
        };
        Some(match (self, pad) {
            (Layout::Drummer, Pad::P1) => Pad::P2,
            (Layout::Drummer, Pad::P2) => Pad::P1,
            (_, pad) => pad,
        })
    }

    /// The button that plays `pad`.
    pub fn button_for(self, pad: Pad) -> Button {
        const BUTTONS: [Button; 8] = [
            Button::DPadUp,
            Button::DPadDown,
            Button::DPadLeft,
            Button::DPadRight,
            Button::North,
            Button::West,
            Button::South,
            Button::East,
        ];
        BUTTONS
            .into_iter()
            .find(|&b| self.pad_for(b) == Some(pad))
            .unwrap_or(Button::South)
    }

    /// Which hand plays `pad`: D-pad on the left thumb, face buttons on the right.
    pub fn hand_for(self, pad: Pad) -> Hand {
        match self.button_for(pad) {
            Button::DPadUp | Button::DPadDown | Button::DPadLeft | Button::DPadRight => Hand::Left,
            _ => Hand::Right,
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            Layout::Reel => 0,
            Layout::Drummer => 1,
        }
    }

    pub fn from_u8(value: u8) -> Layout {
        match value {
            1 => Layout::Drummer,
            _ => Layout::Reel,
        }
    }
}

/// Turns raw events into actions, tracking each trigger's pressed state.
#[derive(Clone, Debug, Default)]
pub struct Mapper {
    pub layout: Layout,
    rails: BTreeMap<DeviceId, [bool; 2]>,
}

impl Mapper {
    pub fn new(layout: Layout) -> Mapper {
        Mapper {
            layout,
            rails: BTreeMap::new(),
        }
    }

    pub fn map(&mut self, event: &InputEvent, mut out: impl FnMut(ActionEvent)) {
        let emit = |action: Action, phase: Phase, value: f32| ActionEvent {
            at_ns: event.at_ns,
            device: event.device,
            action,
            phase,
            value,
        };
        let layout = self.layout;
        let button_action = |button: Button| match button {
            Button::L1 => Some(Action::Roll(Hand::Left)),
            Button::R1 => Some(Action::Roll(Hand::Right)),
            Button::Start => Some(Action::Pause),
            Button::Select => Some(Action::Select),
            other => layout.pad_for(other).map(Action::Pad),
        };
        match event.kind {
            InputKind::Pressed(button) => {
                if let Some(action) = button_action(button) {
                    out(emit(action, Phase::Pressed, 1.0));
                }
            }
            InputKind::Released(button) => {
                if let Some(action) = button_action(button) {
                    out(emit(action, Phase::Released, 0.0));
                }
            }
            InputKind::Axis(axis @ (Axis::L2 | Axis::R2), value) => {
                let hand = if axis == Axis::L2 { Hand::Left } else { Hand::Right };
                let held = &mut self.rails.entry(event.device).or_default()[hand.index()];
                if !*held && value >= RAIL_PRESS {
                    *held = true;
                    out(emit(Action::Rail(hand), Phase::Pressed, value));
                } else if *held && value < RAIL_RELEASE {
                    *held = false;
                    out(emit(Action::Rail(hand), Phase::Released, value));
                }
            }
            InputKind::Disconnected => {
                // A trigger held when the pad vanished is let go.
                if let Some(rails) = self.rails.remove(&event.device) {
                    for hand in [Hand::Left, Hand::Right] {
                        if rails[hand.index()] {
                            out(emit(Action::Rail(hand), Phase::Released, 0.0));
                        }
                    }
                }
            }
            InputKind::Axis(..) | InputKind::Connected => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: InputKind) -> InputEvent {
        InputEvent {
            at_ns: 42,
            device: DeviceId(0),
            kind,
        }
    }

    fn actions(mapper: &mut Mapper, kinds: &[InputKind]) -> Vec<(Action, Phase)> {
        let mut out = Vec::new();
        for &kind in kinds {
            mapper.map(&event(kind), |a| out.push((a.action, a.phase)));
        }
        out
    }

    #[test]
    fn the_reel_layout_matches_the_video() {
        let reel = Layout::Reel;
        assert_eq!(reel.pad_for(Button::DPadUp), Some(Pad::P1));
        assert_eq!(reel.pad_for(Button::South), Some(Pad::P7));
        assert_eq!(reel.pad_for(Button::L1), None);
        assert_eq!(reel.hand_for(Pad::P2), Hand::Left);
        assert_eq!(reel.hand_for(Pad::P8), Hand::Right);
        for pad in Pad::ALL {
            assert_eq!(reel.pad_for(reel.button_for(pad)), Some(pad));
        }
    }

    #[test]
    fn the_drummer_layout_swaps_kick_and_snare() {
        let drummer = Layout::Drummer;
        assert_eq!(drummer.pad_for(Button::DPadDown), Some(Pad::P1));
        assert_eq!(drummer.pad_for(Button::DPadUp), Some(Pad::P2));
        assert_eq!(drummer.button_for(Pad::P1), Button::DPadDown);
        assert_eq!(Layout::from_u8(drummer.as_u8()), drummer);
    }

    #[test]
    fn buttons_map_to_pads_rolls_and_menu_actions() {
        let mut mapper = Mapper::new(Layout::Reel);
        let out = actions(
            &mut mapper,
            &[
                InputKind::Pressed(Button::DPadUp),
                InputKind::Released(Button::DPadUp),
                InputKind::Pressed(Button::R1),
                InputKind::Pressed(Button::Start),
                InputKind::Pressed(Button::L3),
            ],
        );
        assert_eq!(
            out,
            vec![
                (Action::Pad(Pad::P1), Phase::Pressed),
                (Action::Pad(Pad::P1), Phase::Released),
                (Action::Roll(Hand::Right), Phase::Pressed),
                (Action::Pause, Phase::Pressed),
            ]
        );
    }

    #[test]
    fn triggers_press_and_release_with_hysteresis() {
        let mut mapper = Mapper::new(Layout::Reel);
        let out = actions(
            &mut mapper,
            &[
                InputKind::Axis(Axis::R2, 0.15),
                InputKind::Axis(Axis::R2, 0.25),
                InputKind::Axis(Axis::R2, 0.15),
                InputKind::Axis(Axis::R2, 0.90),
                InputKind::Axis(Axis::R2, 0.05),
                InputKind::Axis(Axis::L2, 0.50),
                InputKind::Disconnected,
            ],
        );
        assert_eq!(
            out,
            vec![
                (Action::Rail(Hand::Right), Phase::Pressed),
                (Action::Rail(Hand::Right), Phase::Released),
                (Action::Rail(Hand::Left), Phase::Pressed),
                (Action::Rail(Hand::Left), Phase::Released),
            ]
        );
    }
}
