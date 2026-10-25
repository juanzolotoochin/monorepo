//! Synthetic controllers: physical-style device state without hardware, a
//! virtual-device driver, or an OS-level virtual controller.
//!
//! While [`SyntheticGamepads`] exists it is the only controller source: the
//! native reader stands down, so tests and development runs see exactly the
//! devices they describe. Normal player runs never insert it.
//!
//! A [`GamepadScript`] is a timed scenario (seconds of game time since the
//! script started) of connections, buttons, sticks and triggers. Tests play
//! one at any frame rate; `--gamepad-script <file.json>` plays one in a
//! rendered run for visual QA (development only). The JSON is:
//!
//! ```json
//! { "steps": [
//!     { "at": 0.0, "pad": 1, "connect": true },
//!     { "at": 0.1, "pad": 1, "press": "Menu" },
//!     { "at": 0.2, "pad": 1, "release": "Menu" },
//!     { "at": 0.5, "pad": 1, "left_stick": [0.0, 1.0] },
//!     { "at": 1.5, "pad": 1, "right_trigger": 1.0 }
//! ] }
//! ```

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::pad::{Button, DeviceId, GamepadSnapshot};
use super::GamepadSnapshots;

/// Every synthetic controller's current state, by device.
#[derive(Resource, Default, Clone, Debug)]
pub struct SyntheticGamepads {
    pads: BTreeMap<DeviceId, GamepadSnapshot>,
}

impl SyntheticGamepads {
    pub fn connect(&mut self, device: DeviceId) -> &mut Self {
        self.pads.insert(device, GamepadSnapshot::connected(device));
        self
    }

    pub fn disconnect(&mut self, device: DeviceId) -> &mut Self {
        self.pads.remove(&device);
        self
    }

    fn pad(&mut self, device: DeviceId) -> &mut GamepadSnapshot {
        self.pads.get_mut(&device).unwrap_or_else(|| panic!("synthetic controller {device} is not connected"))
    }

    pub fn press(&mut self, device: DeviceId, button: Button) -> &mut Self {
        self.pad(device).buttons.insert(button);
        self
    }

    pub fn release(&mut self, device: DeviceId, button: Button) -> &mut Self {
        self.pad(device).buttons.remove(&button);
        self
    }

    pub fn left_stick(&mut self, device: DeviceId, value: Vec2) -> &mut Self {
        self.pad(device).left_stick = value;
        self
    }

    pub fn right_stick(&mut self, device: DeviceId, value: Vec2) -> &mut Self {
        self.pad(device).right_stick = value;
        self
    }

    pub fn right_trigger(&mut self, device: DeviceId, value: f32) -> &mut Self {
        self.pad(device).right_trigger = value;
        self
    }

    pub fn left_trigger(&mut self, device: DeviceId, value: f32) -> &mut Self {
        self.pad(device).left_trigger = value;
        self
    }

    /// Every connected controller's state.
    #[cfg(test)]
    pub fn snapshots(&self) -> Vec<GamepadSnapshot> {
        self.pads.values().cloned().collect()
    }

    /// Everything released and centered, still connected.
    #[cfg(test)]
    pub fn neutral(&mut self, device: DeviceId) -> &mut Self {
        *self.pad(device) = GamepadSnapshot::connected(device);
        self
    }

    pub fn apply(&mut self, step: &Step) {
        let pad = step.pad;
        match step.connect {
            Some(true) => {
                self.connect(pad);
            }
            Some(false) => {
                self.disconnect(pad);
            }
            None => {}
        }
        if !self.pads.contains_key(&pad) {
            return;
        }
        if let Some(button) = step.press {
            self.press(pad, button);
        }
        if let Some(button) = step.release {
            self.release(pad, button);
        }
        if let Some([x, y]) = step.left_stick {
            self.left_stick(pad, Vec2::new(x, y));
        }
        if let Some([x, y]) = step.right_stick {
            self.right_stick(pad, Vec2::new(x, y));
        }
        if let Some(v) = step.right_trigger {
            self.right_trigger(pad, v);
        }
        if let Some(v) = step.left_trigger {
            self.left_trigger(pad, v);
        }
    }
}

/// One timed change to a synthetic controller.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Seconds of game time since the script started.
    pub at: f32,
    pub pad: DeviceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub press: Option<Button>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<Button>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_stick: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_stick: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_trigger: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_trigger: Option<f32>,
}

/// A timed scenario, played into [`SyntheticGamepads`].
#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GamepadScript {
    pub steps: Vec<Step>,
    #[serde(skip)]
    elapsed: f32,
    #[serde(skip)]
    next: usize,
}

impl GamepadScript {
    pub fn new(mut steps: Vec<Step>) -> GamepadScript {
        steps.sort_by(|a, b| a.at.total_cmp(&b.at));
        GamepadScript { steps, elapsed: 0.0, next: 0 }
    }

    pub fn from_json(bytes: &[u8]) -> Result<GamepadScript, String> {
        let script: GamepadScript = serde_json::from_slice(bytes).map_err(|e| format!("gamepad script: {e}"))?;
        if script.steps.iter().any(|s| !s.at.is_finite() || s.at < 0.0) {
            return Err("gamepad script: step times must be finite and non-negative".into());
        }
        Ok(GamepadScript::new(script.steps))
    }

    #[cfg(test)]
    pub fn finished(&self) -> bool {
        self.next >= self.steps.len()
    }

    /// Advances by `dt` and applies every step now due, in order.
    pub fn advance(&mut self, dt: f32, pads: &mut SyntheticGamepads) {
        // Steps at time zero apply on the first frame.
        while let Some(step) = self.steps.get(self.next).filter(|s| s.at <= self.elapsed) {
            pads.apply(step);
            self.next += 1;
        }
        self.elapsed += dt;
    }
}

/// Plays the script, if one runs, into the synthetic controllers.
pub(super) fn play(time: Res<Time>, script: Option<ResMut<GamepadScript>>, pads: Option<ResMut<SyntheticGamepads>>) {
    if let (Some(mut script), Some(mut pads)) = (script, pads) {
        script.advance(time.delta_secs(), &mut pads);
    }
}

/// The synthetic controllers become this frame's snapshots.
pub(super) fn publish(pads: Res<SyntheticGamepads>, mut snapshots: ResMut<GamepadSnapshots>) {
    snapshots.0 = pads.pads.values().cloned().collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_parse_and_play_in_time_order() {
        let json = br#"{ "steps": [
            { "at": 0.5, "pad": 1, "left_stick": [0.0, 1.0] },
            { "at": 0.0, "pad": 1, "connect": true },
            { "at": 0.2, "pad": 1, "press": "Menu" }
        ] }"#;
        let mut script = GamepadScript::from_json(json).unwrap();
        let mut pads = SyntheticGamepads::default();
        script.advance(0.1, &mut pads);
        assert!(pads.pads[&1].connected && pads.pads[&1].buttons.is_empty());
        script.advance(0.1, &mut pads);
        assert!(pads.pads[&1].buttons.is_empty(), "0.2 s is not due until elapsed reaches it");
        script.advance(0.1, &mut pads);
        assert!(pads.pads[&1].buttons.contains(&Button::Menu));
        for _ in 0..3 {
            script.advance(0.1, &mut pads);
        }
        assert_eq!(pads.pads[&1].left_stick, Vec2::Y);
        assert!(script.finished());
    }

    #[test]
    fn unknown_fields_and_negative_times_are_rejected() {
        assert!(GamepadScript::from_json(br#"{ "steps": [{ "at": 0.0, "pad": 1, "jump": true }] }"#).is_err());
        assert!(GamepadScript::from_json(br#"{ "steps": [{ "at": -1.0, "pad": 1 }] }"#).is_err());
    }
}
