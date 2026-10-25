//! Controller state as the front end sees it: a [`GamepadSnapshot`] per
//! device and frame, from the native reader or the synthetic source, and the
//! filtering both share (radial dead zones with hysteresis, trigger
//! thresholds, edges, and re-arming after a suspension).
//!
//! Device identities are local to this process. They never enter commands,
//! saves or input logs.

use std::collections::BTreeSet;

use bevy::math::Vec2;
use serde::{Deserialize, Serialize};

use super::tuning;

/// A device's identity within its source (native or synthetic).
pub type DeviceId = u64;

/// Digital controls, named by their position on an Xbox-style pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Button {
    /// A.
    South,
    /// B.
    East,
    /// X.
    West,
    /// Y.
    North,
    /// LB.
    LeftBumper,
    /// RB.
    RightBumper,
    /// View (Back, Select).
    View,
    /// Menu (Start).
    Menu,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    LeftStick,
    RightStick,
}

/// One device's raw state for one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GamepadSnapshot {
    pub device: DeviceId,
    pub connected: bool,
    pub buttons: BTreeSet<Button>,
    /// Raw axes in [-1, 1]; +y is up.
    pub left_stick: Vec2,
    pub right_stick: Vec2,
    /// Raw triggers in [0, 1].
    pub left_trigger: f32,
    pub right_trigger: f32,
}

impl GamepadSnapshot {
    pub fn connected(device: DeviceId) -> GamepadSnapshot {
        GamepadSnapshot { device, connected: true, ..Default::default() }
    }

    /// Nothing held, sticks centered, triggers up (by the filters' measure).
    fn at_rest(&self) -> bool {
        self.buttons.is_empty()
            && self.left_stick.length() < tuning::MOVE_STICK_EXIT
            && self.right_stick.length() < tuning::AIM_STICK_EXIT
            && self.left_trigger < tuning::TRIGGER_RELEASE
            && self.right_trigger < tuning::TRIGGER_RELEASE
    }
}

/// A stick past a radial dead zone with hysteresis: it becomes active at
/// `enter` and stays active until it falls under `exit`.
#[derive(Clone, Copy, Debug, Default)]
pub struct StickFilter {
    active: bool,
}

impl StickFilter {
    /// The unit direction while active, else `None`.
    pub fn update(&mut self, raw: Vec2, enter: f32, exit: f32) -> Option<Vec2> {
        let length = raw.length();
        self.active = if self.active { length >= exit } else { length >= enter };
        self.active.then(|| raw / length)
    }
}

/// An analog trigger read as a button: pressed at `TRIGGER_PRESS`, released
/// only under the lower `TRIGGER_RELEASE`.
#[derive(Clone, Copy, Debug, Default)]
pub struct TriggerFilter {
    pressed: bool,
}

impl TriggerFilter {
    pub fn update(&mut self, raw: f32) -> bool {
        self.pressed = if self.pressed { raw >= tuning::TRIGGER_RELEASE } else { raw >= tuning::TRIGGER_PRESS };
        self.pressed
    }
}

/// A control, digital or derived from an analog one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Control {
    Button(Button),
    RightTrigger,
    LeftTrigger,
}

/// One device's filtered state for one frame.
#[derive(Clone, Debug, Default)]
pub struct PadFrame {
    pub held: BTreeSet<Control>,
    pub pressed: BTreeSet<Control>,
    pub released: BTreeSet<Control>,
    /// The left stick's unit direction past its dead zone.
    pub movement: Option<Vec2>,
    /// The right stick's unit direction past its dead zone.
    pub aim: Option<Vec2>,
    /// The left stick crossed into its dead zone's outside this frame.
    pub movement_started: bool,
    pub aim_started: bool,
}

impl PadFrame {
    pub fn pressed(&self, button: Button) -> bool {
        self.pressed.contains(&Control::Button(button))
    }

    pub fn released(&self, button: Button) -> bool {
        self.released.contains(&Control::Button(button))
    }

    pub fn held(&self, button: Button) -> bool {
        self.held.contains(&Control::Button(button))
    }

    /// Deliberate input: a fresh press or a stick leaving its dead zone.
    /// Noise inside the dead zones, and holds, are not.
    pub fn deliberate(&self) -> bool {
        !self.pressed.is_empty() || self.movement_started || self.aim_started
    }
}

/// Tracks one device across frames.
#[derive(Clone, Debug, Default)]
pub struct PadTracker {
    previous: BTreeSet<Control>,
    movement: StickFilter,
    aim: StickFilter,
    left_trigger: TriggerFilter,
    right_trigger: TriggerFilter,
    /// Suspended: everything reads as released until the device is at rest.
    rearm: bool,
    /// The last snapshot had nothing held and the sticks centered.
    at_rest: bool,
    /// The last frame's filtered state.
    pub frame: PadFrame,
}

impl PadTracker {
    /// After pause, focus loss or a UI capture: holds end now, and nothing
    /// counts again until every button is up and both sticks are centered.
    pub fn suspend(&mut self) {
        // A device already at rest has nothing to re-arm.
        self.rearm = !self.at_rest;
    }

    #[cfg(test)]
    pub fn rearming(&self) -> bool {
        self.rearm
    }

    pub fn update(&mut self, snapshot: &GamepadSnapshot) -> &PadFrame {
        let was_moving = self.frame.movement.is_some();
        let was_aiming = self.frame.aim.is_some();
        self.at_rest = snapshot.at_rest();
        let mut held: BTreeSet<Control> = snapshot.buttons.iter().map(|&b| Control::Button(b)).collect();
        if self.right_trigger.update(snapshot.right_trigger) {
            held.insert(Control::RightTrigger);
        }
        if self.left_trigger.update(snapshot.left_trigger) {
            held.insert(Control::LeftTrigger);
        }
        let mut movement = self.movement.update(snapshot.left_stick, tuning::MOVE_STICK_ENTER, tuning::MOVE_STICK_EXIT);
        let mut aim = self.aim.update(snapshot.right_stick, tuning::AIM_STICK_ENTER, tuning::AIM_STICK_EXIT);
        if self.rearm {
            if snapshot.at_rest() {
                self.rearm = false;
            } else {
                held.clear();
                movement = None;
                aim = None;
            }
        }
        let pressed = held.difference(&self.previous).copied().collect();
        let released = self.previous.difference(&held).copied().collect();
        self.previous = held.clone();
        self.frame = PadFrame {
            held,
            pressed,
            released,
            movement,
            aim,
            movement_started: movement.is_some() && !was_moving,
            aim_started: aim.is_some() && !was_aiming,
        };
        &self.frame
    }
}

/// Keeps a continuous direction steady: it changes only when the new one is
/// more than `step` radians away, so stick noise cannot flood commands.
#[derive(Clone, Copy, Debug, Default)]
pub struct DirectionCoalescer {
    last: Option<Vec2>,
}

impl DirectionCoalescer {
    pub fn update(&mut self, direction: Option<Vec2>, step: f32) -> Option<Vec2> {
        match (direction, self.last) {
            (None, _) => self.last = None,
            (Some(new), Some(old)) if new.angle_to(old).abs() <= step => {}
            (Some(new), _) => self.last = Some(new),
        }
        self.last
    }

    pub fn reset(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_dead_zone_has_hysteresis() {
        let mut stick = StickFilter::default();
        let x = |v: f32| Vec2::new(v, 0.0);
        assert_eq!(stick.update(x(0.25), 0.3, 0.2), None, "under the entry threshold");
        assert_eq!(stick.update(x(0.31), 0.3, 0.2), Some(Vec2::X));
        assert_eq!(stick.update(x(0.22), 0.3, 0.2), Some(Vec2::X), "stays active above the exit threshold");
        assert_eq!(stick.update(x(0.19), 0.3, 0.2), None);
        assert_eq!(stick.update(x(0.29), 0.3, 0.2), None, "must re-enter past the entry threshold");
    }

    #[test]
    fn diagonals_are_unit_directions() {
        let mut stick = StickFilter::default();
        let d = stick.update(Vec2::new(0.6, 0.6), 0.3, 0.2).unwrap();
        assert!((d.length() - 1.0).abs() < 1e-6);
        assert!((d.x - d.y).abs() < 1e-6);
    }

    #[test]
    fn triggers_press_and_release_at_distinct_thresholds() {
        let mut trigger = TriggerFilter::default();
        assert!(!trigger.update(tuning::TRIGGER_PRESS - 0.01));
        assert!(trigger.update(tuning::TRIGGER_PRESS));
        assert!(trigger.update(tuning::TRIGGER_RELEASE + 0.01), "noise above the release threshold holds");
        assert!(!trigger.update(tuning::TRIGGER_RELEASE - 0.01));
    }

    #[test]
    fn noisy_trigger_near_the_press_threshold_presses_once() {
        let mut pad = PadTracker::default();
        let mut snapshot = GamepadSnapshot::connected(1);
        let mut presses = 0;
        for i in 0..40 {
            // Wobbles around the press threshold without reaching release.
            snapshot.right_trigger = tuning::TRIGGER_PRESS + if i % 2 == 0 { 0.03 } else { -0.12 };
            presses += pad.update(&snapshot).pressed.contains(&Control::RightTrigger) as u32;
        }
        assert_eq!(presses, 1);
    }

    #[test]
    fn edges_follow_held_state() {
        let mut pad = PadTracker::default();
        let mut snapshot = GamepadSnapshot::connected(1);
        snapshot.buttons.insert(Button::South);
        assert!(pad.update(&snapshot).pressed(Button::South));
        assert!(!pad.update(&snapshot).pressed(Button::South), "a hold is not a new press");
        snapshot.buttons.clear();
        assert!(pad.update(&snapshot).released(Button::South));
    }

    #[test]
    fn a_suspended_pad_reads_released_until_it_is_at_rest() {
        let mut pad = PadTracker::default();
        let mut snapshot = GamepadSnapshot::connected(1);
        snapshot.buttons.insert(Button::RightBumper);
        snapshot.left_stick = Vec2::Y;
        assert!(pad.update(&snapshot).held(Button::RightBumper));
        pad.suspend();
        let frame = pad.update(&snapshot).clone();
        assert!(frame.released(Button::RightBumper), "one release when the suspension starts");
        assert!(frame.movement.is_none());
        // Still held: nothing.
        let frame = pad.update(&snapshot).clone();
        assert!(frame.held.is_empty() && frame.pressed.is_empty() && frame.movement.is_none());
        // Stick back to center but the button still down: still nothing.
        snapshot.left_stick = Vec2::ZERO;
        assert!(pad.update(&snapshot).held.is_empty());
        snapshot.buttons.clear();
        pad.update(&snapshot);
        assert!(!pad.rearming());
        snapshot.buttons.insert(Button::RightBumper);
        assert!(pad.update(&snapshot).pressed(Button::RightBumper), "a fresh press counts");
    }

    #[test]
    fn small_direction_changes_are_coalesced() {
        let mut c = DirectionCoalescer::default();
        let at = |deg: f32| Some(Vec2::from_angle(deg.to_radians()));
        let first = c.update(at(10.0), tuning::MOVE_DIRECTION_STEP).unwrap();
        for deg in [10.5, 9.4, 11.0, 9.0] {
            assert_eq!(c.update(at(deg), tuning::MOVE_DIRECTION_STEP), Some(first), "{deg}° is noise");
        }
        assert_ne!(c.update(at(30.0), tuning::MOVE_DIRECTION_STEP), Some(first));
        assert_eq!(c.update(None, tuning::MOVE_DIRECTION_STEP), None);
    }
}
