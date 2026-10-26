//! The native controller reader: Bevy's gamepads become snapshots. It is
//! deliberately thin; everything past the snapshot is shared with the
//! synthetic source and covered by hardware-free tests. Whether the platform
//! backend discovers a device, and whether its buttons and axes match this
//! mapping on a physical controller, is not covered by those tests.

use bevy::input::gamepad::{Gamepad, GamepadButton};
use bevy::prelude::*;

use super::pad::{Button, GamepadSnapshot};
use super::GamepadSnapshots;

/// Bevy's buttons, by their Xbox-layout meaning.
const BUTTONS: [(GamepadButton, Button); 14] = [
    (GamepadButton::South, Button::South),
    (GamepadButton::East, Button::East),
    (GamepadButton::West, Button::West),
    (GamepadButton::North, Button::North),
    (GamepadButton::LeftTrigger, Button::LeftBumper),
    (GamepadButton::RightTrigger, Button::RightBumper),
    (GamepadButton::Select, Button::View),
    (GamepadButton::Start, Button::Menu),
    (GamepadButton::DPadUp, Button::DPadUp),
    (GamepadButton::DPadDown, Button::DPadDown),
    (GamepadButton::DPadLeft, Button::DPadLeft),
    (GamepadButton::DPadRight, Button::DPadRight),
    (GamepadButton::LeftThumb, Button::LeftStick),
    (GamepadButton::RightThumb, Button::RightStick),
];

/// One snapshot of a Bevy gamepad. An entity's bits are its identity for
/// as long as it is connected.
pub(super) fn snapshot(entity: Entity, gamepad: &Gamepad) -> GamepadSnapshot {
    GamepadSnapshot {
        device: entity.to_bits(),
        connected: true,
        buttons: BUTTONS.iter().filter(|(native, _)| gamepad.pressed(*native)).map(|&(_, b)| b).collect(),
        left_stick: gamepad.left_stick(),
        right_stick: gamepad.right_stick(),
        left_trigger: gamepad.get(GamepadButton::LeftTrigger2).unwrap_or(0.0),
        right_trigger: gamepad.get(GamepadButton::RightTrigger2).unwrap_or(0.0),
    }
}

/// Bevy keeps a `Gamepad` entity per connected controller; a disconnected
/// one loses the component, and so drops out of the snapshots.
pub(super) fn read(
    gamepads: Query<(Entity, &Gamepad)>,
    mut snapshots: ResMut<GamepadSnapshots>,
    mut previous_count: Local<usize>,
) {
    snapshots.0 = gamepads.iter().map(|(entity, gamepad)| snapshot(entity, gamepad)).collect();
    if snapshots.0.len() != *previous_count {
        info!("Native gamepads detected: {}", snapshots.0.len());
        *previous_count = snapshots.0.len();
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::gamepad::GamepadAxis;

    use super::*;

    /// The mapping as Bevy reports state. Whether a physical controller
    /// reports these buttons and axes is a hardware check, not this test.
    #[test]
    fn bevy_gamepad_state_maps_to_xbox_layout_snapshots() {
        let mut gamepad = Gamepad::default();
        for button in [GamepadButton::South, GamepadButton::Start, GamepadButton::RightTrigger, GamepadButton::Select, GamepadButton::DPadLeft] {
            gamepad.digital_mut().press(button);
        }
        gamepad.analog_mut().set(GamepadAxis::LeftStickX, 0.5);
        gamepad.analog_mut().set(GamepadAxis::LeftStickY, -0.25);
        gamepad.analog_mut().set(GamepadAxis::RightStickY, 1.0);
        gamepad.analog_mut().set(GamepadButton::RightTrigger2, 0.8);
        gamepad.analog_mut().set(GamepadButton::LeftTrigger2, 0.1);
        let snapshot = snapshot(Entity::from_raw_u32(7).unwrap(), &gamepad);
        assert!(snapshot.connected);
        assert_eq!(
            snapshot.buttons.into_iter().collect::<Vec<_>>(),
            vec![Button::South, Button::RightBumper, Button::View, Button::Menu, Button::DPadLeft]
        );
        assert_eq!(snapshot.left_stick, Vec2::new(0.5, -0.25));
        assert_eq!(snapshot.right_stick, Vec2::new(0.0, 1.0));
        assert_eq!((snapshot.left_trigger, snapshot.right_trigger), (0.1, 0.8));
    }
}
