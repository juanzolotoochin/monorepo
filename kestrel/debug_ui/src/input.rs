//! Routed hardware input: what gameplay, global shortcuts and the game's own
//! menus may read once the debug UI has taken what it owns.
//!
//! Every button is owned from its press to its release:
//!
//! - **Game**: the press reached gameplay; its hold and release do too.
//! - **Ui**: the debug UI had the device when it went down (or claimed the
//!   key, like the `/` that toggles the overlay). Gameplay never sees it,
//!   even after the capture ends or the overlay hides.
//! - **Suppressed**: a gameplay hold that something interrupted (UI capture,
//!   the pause overlay, losing window focus). Gameplay sees one release edge
//!   when the interruption starts, then nothing until a fresh press.
//!
//! Gameplay acts on presses, holds and releases as it always has: an
//! interrupted walk sees its keys released and stops through the usual path,
//! and an interrupted jetpack sees Space released and switches off. Nothing
//! is ever pressed on the game's behalf.
//!
//! Without the interactive backend nothing captures, so [`GameInput`]
//! mirrors the raw devices: scripted runs and tests that press
//! `ButtonInput` directly keep working.

use std::hash::Hash;

use bevy::ecs::message::MessageCursor;
use bevy::input::keyboard::{KeyboardFocusLost, KeyboardInput};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;

/// Who a held button belongs to (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Game,
    Ui,
    Suppressed,
}

/// What the debug UI held this frame, measured after its pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capture {
    /// A debug widget has keyboard focus: new key presses belong to it, and
    /// held gameplay keys are interrupted.
    pub keyboard: bool,
    /// The pointer is over the debug UI, or the UI is using it or has a
    /// popup open: new button presses and the wheel belong to it. Held
    /// gameplay buttons keep going (hovering takes nothing over).
    pub pointer: bool,
    /// The UI is actively using the pointer (dragging, pressing a widget):
    /// held gameplay buttons are interrupted.
    pub pointer_interrupt: bool,
}

/// One frame of raw device state for [`GameInput::route`].
pub struct Frame<'a> {
    pub keys: &'a ButtonInput<KeyCode>,
    pub mouse: &'a ButtonInput<MouseButton>,
    pub scroll: Vec2,
    pub motion: Vec2,
    pub keyboard: &'a [KeyboardInput],
    pub capture: Capture,
    /// The window lost keyboard focus this frame.
    pub focus_lost: bool,
}

/// The input gameplay reads instead of Bevy's raw resources. Read it like
/// `ButtonInput`: `input.keys().just_pressed(KeyCode::Space)`.
#[derive(Resource, Default)]
pub struct GameInput {
    keys: ButtonInput<KeyCode>,
    mouse: ButtonInput<MouseButton>,
    scroll: Vec2,
    motion: Vec2,
    keyboard: Vec<KeyboardInput>,
    capture: Capture,
    key_owners: HashMap<KeyCode, Owner>,
    button_owners: HashMap<MouseButton, Owner>,
    /// Keys the debug UI takes for itself this frame (the overlay toggle, a
    /// closing Escape), whatever the capture says.
    claims: HashSet<KeyCode>,
    /// Interrupted gameplay holds since the start (release edges sent while
    /// the button was still physically down).
    cancellations: u64,
}

impl GameInput {
    /// Keys as gameplay sees them.
    pub fn keys(&self) -> &ButtonInput<KeyCode> {
        &self.keys
    }

    /// Mouse buttons as gameplay sees them.
    pub fn mouse(&self) -> &ButtonInput<MouseButton> {
        &self.mouse
    }

    /// This frame's wheel movement, or zero while the pointer is over the UI.
    pub fn scroll(&self) -> Vec2 {
        self.scroll
    }

    /// This frame's mouse movement, or zero while the pointer is over the UI.
    pub fn motion(&self) -> Vec2 {
        self.motion
    }

    /// This frame's keyboard messages for keys gameplay owns: presses,
    /// repeats and releases, with their text. Text fields read these.
    pub fn keyboard(&self) -> &[KeyboardInput] {
        &self.keyboard
    }

    /// What the debug UI captured this frame.
    pub fn capture(&self) -> Capture {
        self.capture
    }

    /// Whether the debug UI owns a held or new mouse button.
    pub fn ui_owns_pointer(&self) -> bool {
        self.button_owners.values().any(|&owner| owner == Owner::Ui)
    }

    /// How many gameplay holds have been interrupted so far.
    pub fn cancellations(&self) -> u64 {
        self.cancellations
    }

    /// Takes a key for the debug UI this frame: its press (and everything up
    /// to its release) never reaches gameplay.
    pub fn claim(&mut self, key: KeyCode) {
        self.claims.insert(key);
    }

    /// Takes everything from gameplay for the rest of this frame, as the
    /// pause overlay does while it is open. Held gameplay buttons see one
    /// release and stay off until pressed again; presses made this frame are
    /// dropped without a release.
    pub fn block(&mut self) {
        self.cancellations += interrupt(&mut self.keys, &mut self.key_owners);
        self.cancellations += interrupt(&mut self.mouse, &mut self.button_owners);
        self.keyboard.clear();
        self.scroll = Vec2::ZERO;
        self.motion = Vec2::ZERO;
    }

    /// Routes one frame: called once per frame, before any gameplay input.
    pub fn route(&mut self, frame: Frame) {
        self.capture = frame.capture;
        let claims = std::mem::take(&mut self.claims);
        let keyboard_capture = frame.capture.keyboard;
        let (cancelled, game_keys) = route_device(
            frame.keys,
            &mut self.keys,
            &mut self.key_owners,
            |key| if claims.contains(&key) || keyboard_capture { Owner::Ui } else { Owner::Game },
            |_| keyboard_capture || frame.focus_lost,
        );
        self.cancellations += cancelled;
        let pointer = frame.capture.pointer;
        let (cancelled, _) = route_device(
            frame.mouse,
            &mut self.mouse,
            &mut self.button_owners,
            |_| if pointer { Owner::Ui } else { Owner::Game },
            // A button the UI just took interrupts held gameplay buttons too:
            // the player clicked a debug window mid-drag.
            |owners| {
                frame.capture.pointer_interrupt
                    || frame.focus_lost
                    || frame.mouse.get_just_pressed().any(|b| owners.get(b) == Some(&Owner::Ui))
            },
        );
        self.cancellations += cancelled;
        self.keyboard = frame.keyboard.iter().filter(|m| game_keys.contains(&m.key_code)).cloned().collect();
        let over_ui = frame.capture.pointer;
        self.scroll = if over_ui { Vec2::ZERO } else { frame.scroll };
        self.motion = if over_ui { Vec2::ZERO } else { frame.motion };
    }
}

/// Routes one device's buttons into `routed`. Returns the interruptions and
/// the buttons gameplay owned during the frame (for filtering messages).
fn route_device<T: Copy + Eq + Hash + Send + Sync + 'static>(
    raw: &ButtonInput<T>,
    routed: &mut ButtonInput<T>,
    owners: &mut HashMap<T, Owner>,
    new_owner: impl Fn(T) -> Owner,
    interrupted: impl Fn(&HashMap<T, Owner>) -> bool,
) -> (u64, HashSet<T>) {
    routed.clear();
    // A press starts a new ownership; a held button nobody has seen pressed
    // (injected by a test, or held at startup) is treated as pressed now.
    for &button in raw.get_just_pressed() {
        owners.insert(button, new_owner(button));
    }
    for &button in raw.get_pressed() {
        owners.entry(button).or_insert_with(|| new_owner(button));
    }
    if interrupted(owners) {
        for owner in owners.values_mut() {
            if *owner == Owner::Game {
                *owner = Owner::Suppressed;
            }
        }
    }
    let mut cancelled = 0;
    let mut game = HashSet::new();
    let buttons: Vec<T> = raw
        .get_pressed()
        .chain(raw.get_just_pressed())
        .chain(raw.get_just_released())
        .chain(routed.get_pressed())
        .copied()
        .collect();
    for button in buttons {
        // Owners outlive a release by this frame, so a release is routed to
        // whoever owned the press.
        let owned = owners.get(&button) == Some(&Owner::Game);
        if owned {
            game.insert(button);
            if raw.just_pressed(button) || (raw.pressed(button) && !routed.pressed(button)) {
                routed.press(button);
            }
        }
        if routed.pressed(button) && !(owned && raw.pressed(button)) {
            if raw.pressed(button) {
                cancelled += 1;
            }
            routed.release(button);
        }
    }
    owners.retain(|button, _| raw.pressed(*button));
    (cancelled, game)
}

/// Interrupts every gameplay hold of one device (see [`GameInput::block`]).
fn interrupt<T: Copy + Eq + Hash + Send + Sync + 'static>(routed: &mut ButtonInput<T>, owners: &mut HashMap<T, Owner>) -> u64 {
    let mut cancelled = 0;
    let fresh: Vec<T> = routed.get_just_pressed().copied().collect();
    for button in fresh {
        routed.reset(button);
    }
    let held: Vec<T> = routed.get_pressed().copied().collect();
    for button in held {
        routed.release(button);
        cancelled += 1;
    }
    for owner in owners.values_mut() {
        if *owner == Owner::Game {
            *owner = Owner::Suppressed;
        }
    }
    cancelled
}

/// The debug UI's measurement of the current frame, which the backend's pass
/// writes and routing reads. Without a backend it stays empty.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct CaptureSnapshot(pub Capture);

/// Builds this frame's [`GameInput`]. Every raw resource is optional: runs
/// without Bevy's input plugin (simulation) still route.
#[allow(clippy::too_many_arguments)]
pub(crate) fn route(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    scroll: Option<Res<AccumulatedMouseScroll>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    keyboard: Option<Res<Messages<KeyboardInput>>>,
    focus: Option<Res<Messages<KeyboardFocusLost>>>,
    capture: Res<CaptureSnapshot>,
    mut keyboard_cursor: Local<MessageCursor<KeyboardInput>>,
    mut focus_cursor: Local<MessageCursor<KeyboardFocusLost>>,
    mut input: ResMut<GameInput>,
) {
    let no_keys = ButtonInput::default();
    let no_buttons = ButtonInput::default();
    let messages: Vec<KeyboardInput> = keyboard.as_deref().map(|m| keyboard_cursor.read(m).cloned().collect()).unwrap_or_default();
    let focus_lost = focus.as_deref().is_some_and(|m| focus_cursor.read(m).count() > 0);
    input.route(Frame {
        keys: keys.as_deref().unwrap_or(&no_keys),
        mouse: mouse.as_deref().unwrap_or(&no_buttons),
        scroll: scroll.map_or(Vec2::ZERO, |s| s.delta),
        motion: motion.map_or(Vec2::ZERO, |m| m.delta),
        keyboard: &messages,
        capture: capture.0,
        focus_lost,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::keyboard::Key;
    use bevy::input::ButtonState;

    /// Raw devices plus routing, stepped frame by frame like Bevy does.
    #[derive(Default)]
    struct Rig {
        keys: ButtonInput<KeyCode>,
        mouse: ButtonInput<MouseButton>,
        messages: Vec<KeyboardInput>,
        capture: Capture,
        focus_lost: bool,
        input: GameInput,
    }

    impl Rig {
        fn press(&mut self, key: KeyCode) {
            self.keys.press(key);
            self.messages.push(message(key, ButtonState::Pressed));
        }
        fn release(&mut self, key: KeyCode) {
            self.keys.release(key);
            self.messages.push(message(key, ButtonState::Released));
        }
        fn frame(&mut self) {
            self.input.route(Frame {
                keys: &self.keys,
                mouse: &self.mouse,
                scroll: Vec2::new(0.0, 1.0),
                motion: Vec2::ZERO,
                keyboard: &self.messages,
                capture: self.capture,
                focus_lost: std::mem::take(&mut self.focus_lost),
            });
            self.keys.clear();
            self.mouse.clear();
            self.messages.clear();
        }
        fn game(&self) -> &ButtonInput<KeyCode> {
            self.input.keys()
        }
    }

    fn message(key: KeyCode, state: ButtonState) -> KeyboardInput {
        KeyboardInput { key_code: key, logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified), state, text: None, repeat: false, window: Entity::PLACEHOLDER }
    }

    #[test]
    fn without_capture_input_passes_through() {
        let mut rig = Rig::default();
        rig.press(KeyCode::KeyW);
        rig.frame();
        assert!(rig.game().just_pressed(KeyCode::KeyW) && rig.game().pressed(KeyCode::KeyW));
        assert_eq!(rig.input.keyboard().len(), 1);
        assert_eq!(rig.input.scroll(), Vec2::new(0.0, 1.0));
        rig.frame();
        assert!(rig.game().pressed(KeyCode::KeyW) && !rig.game().just_pressed(KeyCode::KeyW));
        rig.release(KeyCode::KeyW);
        rig.frame();
        assert!(rig.game().just_released(KeyCode::KeyW) && !rig.game().pressed(KeyCode::KeyW));
        assert_eq!(rig.input.cancellations(), 0);
    }

    #[test]
    fn a_tap_within_one_frame_is_pressed_and_released() {
        let mut rig = Rig::default();
        rig.press(KeyCode::KeyR);
        rig.release(KeyCode::KeyR);
        rig.frame();
        assert!(rig.game().just_pressed(KeyCode::KeyR) && rig.game().just_released(KeyCode::KeyR));
        assert!(!rig.game().pressed(KeyCode::KeyR));
    }

    #[test]
    fn keyboard_capture_interrupts_a_hold_once_and_it_never_resumes() {
        let mut rig = Rig::default();
        rig.press(KeyCode::Space);
        rig.frame();
        rig.capture.keyboard = true;
        rig.frame();
        // One release edge: the jetpack switches off through its usual path.
        assert!(rig.game().just_released(KeyCode::Space));
        assert_eq!(rig.input.cancellations(), 1);
        rig.frame();
        assert!(!rig.game().just_released(KeyCode::Space) && !rig.game().pressed(KeyCode::Space));
        // The capture ends with Space still held: it stays off.
        rig.capture.keyboard = false;
        for _ in 0..3 {
            rig.frame();
            assert!(!rig.game().pressed(KeyCode::Space));
        }
        // Its release is not gameplay's either.
        rig.release(KeyCode::Space);
        rig.frame();
        assert!(!rig.game().just_released(KeyCode::Space));
        // A fresh press is.
        rig.press(KeyCode::Space);
        rig.frame();
        assert!(rig.game().just_pressed(KeyCode::Space));
        assert_eq!(rig.input.cancellations(), 1);
    }

    #[test]
    fn keys_pressed_during_capture_belong_to_the_ui_until_released() {
        let mut rig = Rig::default();
        rig.capture.keyboard = true;
        rig.press(KeyCode::KeyW);
        rig.frame();
        assert!(!rig.game().pressed(KeyCode::KeyW) && rig.input.keyboard().is_empty());
        rig.capture.keyboard = false;
        rig.frame();
        assert!(!rig.game().pressed(KeyCode::KeyW));
        rig.release(KeyCode::KeyW);
        rig.frame();
        assert!(!rig.game().just_released(KeyCode::KeyW));
        assert_eq!(rig.input.cancellations(), 0);
    }

    #[test]
    fn a_claimed_key_is_hidden_for_its_whole_press() {
        let mut rig = Rig::default();
        rig.input.claim(KeyCode::Escape);
        rig.press(KeyCode::Escape);
        rig.frame();
        assert!(!rig.game().just_pressed(KeyCode::Escape));
        // Repeats and the release stay with the UI.
        rig.messages.push(KeyboardInput { repeat: true, ..message(KeyCode::Escape, ButtonState::Pressed) });
        rig.frame();
        assert!(rig.input.keyboard().is_empty());
        rig.release(KeyCode::Escape);
        rig.frame();
        assert!(!rig.game().just_released(KeyCode::Escape) && rig.input.keyboard().is_empty());
        rig.press(KeyCode::Escape);
        rig.frame();
        assert!(rig.game().just_pressed(KeyCode::Escape), "a claim lasts one press");
    }

    #[test]
    fn hovering_the_ui_does_not_take_a_held_drag_but_using_it_does() {
        let mut rig = Rig::default();
        rig.mouse.press(MouseButton::Right);
        rig.frame();
        assert!(rig.input.mouse().pressed(MouseButton::Right));
        // The steering drag crosses a debug window.
        rig.capture.pointer = true;
        rig.frame();
        assert!(rig.input.mouse().pressed(MouseButton::Right), "hover alone takes nothing over");
        assert_eq!(rig.input.scroll(), Vec2::ZERO, "the wheel over the UI is the UI's");
        // A click on the window while steering interrupts it.
        rig.mouse.press(MouseButton::Left);
        rig.frame();
        assert!(!rig.input.mouse().pressed(MouseButton::Right) && rig.input.mouse().just_released(MouseButton::Right));
        assert!(!rig.input.mouse().just_pressed(MouseButton::Left));
        assert!(rig.input.ui_owns_pointer());
        rig.capture.pointer = false;
        rig.frame();
        assert!(!rig.input.mouse().pressed(MouseButton::Right) && !rig.input.mouse().pressed(MouseButton::Left));
    }

    #[test]
    fn a_ui_drag_never_becomes_gameplay_after_the_pointer_leaves() {
        let mut rig = Rig::default();
        rig.capture.pointer = true;
        rig.capture.pointer_interrupt = true;
        rig.mouse.press(MouseButton::Left);
        rig.frame();
        rig.capture = Capture::default();
        for _ in 0..3 {
            rig.frame();
            assert!(!rig.input.mouse().pressed(MouseButton::Left));
        }
        rig.mouse.release(MouseButton::Left);
        rig.frame();
        assert!(!rig.input.mouse().just_released(MouseButton::Left));
        rig.mouse.press(MouseButton::Left);
        rig.frame();
        assert!(rig.input.mouse().just_pressed(MouseButton::Left));
    }

    #[test]
    fn losing_focus_retires_holds_without_resuming_them() {
        let mut rig = Rig::default();
        rig.press(KeyCode::KeyW);
        rig.mouse.press(MouseButton::Right);
        rig.frame();
        rig.focus_lost = true;
        rig.frame();
        assert!(rig.game().just_released(KeyCode::KeyW) && rig.input.mouse().just_released(MouseButton::Right));
        // Focus comes back with the buttons still down: nothing restarts.
        rig.frame();
        assert!(!rig.game().pressed(KeyCode::KeyW) && !rig.input.mouse().pressed(MouseButton::Right));
        assert_eq!(rig.input.cancellations(), 2);
    }

    #[test]
    fn blocking_releases_holds_once_and_drops_new_presses() {
        let mut rig = Rig::default();
        rig.press(KeyCode::Space);
        rig.frame();
        rig.press(KeyCode::KeyW);
        rig.frame();
        rig.input.block();
        assert!(rig.game().just_released(KeyCode::Space), "a held key is released");
        assert!(!rig.game().just_pressed(KeyCode::KeyW) && !rig.game().just_released(KeyCode::KeyW), "a new press is dropped");
        assert!(rig.input.keyboard().is_empty());
        rig.frame();
        rig.input.block();
        assert!(!rig.game().just_released(KeyCode::Space), "only once");
        // Unblocked with both still held: neither resumes.
        rig.frame();
        assert!(!rig.game().pressed(KeyCode::Space) && !rig.game().pressed(KeyCode::KeyW));
    }

    #[test]
    fn modifiers_held_for_the_ui_do_not_leak_into_gameplay_clicks() {
        let mut rig = Rig::default();
        rig.capture.keyboard = true;
        rig.press(KeyCode::ShiftLeft);
        rig.frame();
        rig.capture.keyboard = false;
        rig.mouse.press(MouseButton::Left);
        rig.frame();
        assert!(rig.input.mouse().just_pressed(MouseButton::Left));
        assert!(!rig.game().pressed(KeyCode::ShiftLeft), "the click is a plain click");
    }

    #[test]
    fn keyboard_and_pointer_capture_are_independent() {
        let mut rig = Rig::default();
        rig.capture.pointer = true;
        rig.press(KeyCode::KeyW);
        rig.frame();
        assert!(rig.game().pressed(KeyCode::KeyW), "keys work while the pointer is over the UI");
        rig.capture = Capture { keyboard: true, ..default() };
        rig.mouse.press(MouseButton::Right);
        rig.frame();
        assert!(rig.input.mouse().pressed(MouseButton::Right), "buttons work while a widget has the keyboard");
    }
}
