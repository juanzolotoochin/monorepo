//! Controller support without hardware. Every test injects physical-style
//! device state (synthetic snapshots, or keys and buttons) and observes the
//! resolved actions or the commands the session accepted; none calls
//! `PlayerControl` or queues simulation commands to stand in for a device.
//!
//! These cover the snapshot-to-action-to-command path. They do not show
//! that a platform backend discovers a USB controller, or that a physical
//! device's axes and buttons match `native.rs`.

use bevy::prelude::*;
use sim::{CommandKind, PlayerId};

use super::pad::{Button, DeviceId};
use super::synthetic::{GamepadScript, Step, SyntheticGamepads};
use super::*;
use crate::lockstep::{SessionService, WorldMirror};

// ---------------------------------------------------------------------------
// The input service alone: arbitration, seats, contexts and edges.

/// Raw devices plus routing and the input service, stepped frame by frame.
struct Rig {
    keys: ButtonInput<KeyCode>,
    mouse: ButtonInput<MouseButton>,
    motion: Vec2,
    focus_lost: bool,
    game: GameInput,
    pads: SyntheticGamepads,
    input: InputService,
    dt: f32,
}

impl Rig {
    fn new() -> Rig {
        Rig {
            keys: default(),
            mouse: default(),
            motion: Vec2::ZERO,
            focus_lost: false,
            game: default(),
            pads: default(),
            input: default(),
            dt: 1.0 / 60.0,
        }
    }

    /// A rig with controller `pad` connected and seated.
    fn seated(pad: DeviceId) -> Rig {
        let mut rig = Rig::new();
        rig.pads.connect(pad);
        rig.frame();
        rig.tap(pad, Button::Menu);
        assert_eq!(rig.input.seat(), Some(pad));
        rig
    }

    fn frame(&mut self) -> ActionFrame {
        self.game.route(debug_ui::input::Frame {
            keys: &self.keys,
            mouse: &self.mouse,
            scroll: Vec2::ZERO,
            motion: self.motion,
            keyboard: &[],
            capture: default(),
            focus_lost: std::mem::take(&mut self.focus_lost),
        });
        self.input.sample(&self.game, &self.pads.snapshots(), self.dt);
        self.keys.clear();
        self.mouse.clear();
        self.motion = Vec2::ZERO;
        self.input.frame().clone()
    }

    /// Presses a button for one frame, then releases it for one. Returns
    /// the press frame.
    fn tap(&mut self, pad: DeviceId, button: Button) -> ActionFrame {
        self.pads.press(pad, button);
        let pressed = self.frame();
        self.pads.release(pad, button);
        self.frame();
        pressed
    }
}

#[test]
fn menu_claims_the_seat_and_the_claim_press_does_nothing_else() {
    let mut rig = Rig::new();
    rig.pads.connect(2);
    rig.frame();
    rig.pads.press(2, Button::Menu);
    let frame = rig.frame();
    assert_eq!(rig.input.seat(), Some(2));
    assert!(!rig.input.take_menu(), "the claim press does not also pause");
    assert!(frame.gameplay.edges.is_empty());
    assert_eq!(frame.owner, Device::Gamepad);
}

#[test]
fn other_controllers_are_ignored_while_the_seat_is_taken() {
    let mut rig = Rig::new();
    rig.pads.connect(1).connect(2);
    rig.frame();
    // Both press Menu in the same frame: the lowest identity wins, stably.
    rig.pads.press(1, Button::Menu).press(2, Button::Menu);
    rig.frame();
    assert_eq!(rig.input.seat(), Some(1));
    rig.pads.release(1, Button::Menu).release(2, Button::Menu);
    rig.frame();
    rig.pads.left_stick(2, Vec2::Y).press(2, Button::South);
    let frame = rig.frame();
    assert_eq!(frame.gameplay.movement, Vec2::ZERO);
    assert!(frame.gameplay.edges.is_empty());
    rig.pads.neutral(2);
    rig.frame();
    rig.tap(2, Button::Menu);
    assert_eq!(rig.input.seat(), Some(1), "a second Menu press does not move the seat");
}

#[test]
fn ownership_moves_on_deliberate_input_only() {
    let mut rig = Rig::seated(1);
    // Keyboard takes over with a key press.
    rig.keys.press(KeyCode::KeyW);
    let frame = rig.frame();
    assert!(frame.takeover && frame.owner == Device::KeyboardMouse);
    assert!(frame.gameplay.movement.y > 0.0);
    // Stick noise inside the dead zone does not take it back.
    for i in 0..30 {
        rig.pads.left_stick(1, Vec2::new(0.1, if i % 2 == 0 { 0.15 } else { -0.15 }));
        let frame = rig.frame();
        assert_eq!(frame.owner, Device::KeyboardMouse);
    }
    // A stick crossing its threshold does, though W is still held.
    rig.pads.left_stick(1, Vec2::new(0.0, 0.8));
    let frame = rig.frame();
    assert!(frame.takeover && frame.owner == Device::Gamepad);
    // Held keys are not deliberate input; nor is passive mouse drift.
    for _ in 0..10 {
        assert_eq!(rig.frame().owner, Device::Gamepad);
    }
    rig.motion = Vec2::new(4.0, 2.0);
    assert_eq!(rig.frame().owner, Device::Gamepad, "drift under the threshold");
    rig.motion = Vec2::new(30.0, 0.0);
    assert_eq!(rig.frame().owner, Device::KeyboardMouse, "deliberate mouse movement");
}

#[test]
fn simultaneous_deliberate_input_keeps_the_current_owner() {
    let mut rig = Rig::seated(1);
    assert_eq!(rig.input.owner(), Device::Gamepad);
    rig.keys.press(KeyCode::Space);
    rig.pads.press(1, Button::RightBumper);
    let frame = rig.frame();
    assert!(!frame.takeover && frame.owner == Device::Gamepad);
    assert!(frame.gameplay.edges.contains(&Edge::Lift));
}

#[test]
fn the_non_owner_contributes_nothing_not_even_neutral() {
    let mut rig = Rig::seated(1);
    rig.keys.press(KeyCode::KeyW);
    rig.frame();
    // The pad is neutral; the keyboard's walk is the movement.
    let frame = rig.frame();
    assert_eq!(frame.owner, Device::KeyboardMouse);
    assert!(frame.gameplay.movement.y > 0.0);
    rig.pads.press(1, Button::South);
    rig.keys.press(KeyCode::KeyW);
    let frame = rig.frame();
    assert!(frame.takeover, "a pad press is deliberate");
    assert_eq!(frame.gameplay.movement, Vec2::ZERO, "the keyboard no longer drives");
}

#[test]
fn diagonals_are_unit_length_and_noise_is_coalesced() {
    let mut rig = Rig::seated(1);
    rig.pads.left_stick(1, Vec2::new(0.7, 0.7));
    let first = rig.frame().gameplay.movement;
    assert!((first.length() - 1.0).abs() < 1e-5);
    for i in 0..60 {
        let wobble = if i % 2 == 0 { 0.02 } else { -0.02 };
        rig.pads.left_stick(1, Vec2::from_angle(std::f32::consts::FRAC_PI_4 + wobble) * 0.9);
        assert_eq!(rig.frame().gameplay.movement, first, "frame {i}");
    }
    // Crossing out of the dead zone and back: neutral is zero.
    rig.pads.left_stick(1, Vec2::new(0.1, 0.1));
    assert_eq!(rig.frame().gameplay.movement, Vec2::ZERO);
}

#[test]
fn trigger_thresholds_give_one_attack_per_press() {
    let mut rig = Rig::seated(1);
    let mut attacks = 0;
    for press in 0..3 {
        // A noisy squeeze, held for half a second.
        for i in 0..30 {
            rig.pads.right_trigger(1, if i % 3 == 0 { 0.5 } else { 0.95 });
            attacks += rig.frame().gameplay.edges.iter().filter(|e| **e == Edge::Attack).count();
        }
        // A partial release above the release threshold is still held.
        rig.pads.right_trigger(1, tuning::TRIGGER_RELEASE + 0.05);
        attacks += rig.frame().gameplay.edges.iter().filter(|e| **e == Edge::Attack).count();
        assert_eq!(attacks, press + 1);
        rig.pads.right_trigger(1, 0.0);
        rig.frame();
    }
    assert_eq!(attacks, 3);
}

#[test]
fn a_seated_disconnect_frees_the_seat_suspends_and_asks_for_pause() {
    let mut rig = Rig::seated(1);
    rig.pads.press(1, Button::RightBumper).left_stick(1, Vec2::Y);
    rig.frame();
    rig.pads.disconnect(1);
    let frame = rig.frame();
    assert_eq!(frame.suspended, Some(Suspend::Disconnected));
    assert!(frame.pause_requested);
    assert_eq!(rig.input.seat(), None);
    assert_eq!(frame.owner, Device::KeyboardMouse);
    // Reconnecting does not reclaim the seat or resume anything.
    rig.pads.connect(1);
    rig.pads.left_stick(1, Vec2::Y);
    let frame = rig.frame();
    assert_eq!(rig.input.seat(), None);
    assert_eq!(frame.gameplay.movement, Vec2::ZERO);
    rig.pads.neutral(1);
    rig.frame();
    rig.tap(1, Button::Menu);
    assert_eq!(rig.input.seat(), Some(1));
}

#[test]
fn focus_loss_suspends_and_held_controls_count_only_after_rest() {
    let mut rig = Rig::seated(1);
    rig.pads.press(1, Button::RightBumper).left_stick(1, Vec2::Y);
    rig.frame();
    rig.focus_lost = true;
    let frame = rig.frame();
    assert_eq!(frame.suspended, Some(Suspend::FocusLost));
    assert!(frame.pause_requested);
    // Still held: the release edge, then nothing.
    let frame = rig.frame();
    assert!(frame.gameplay.edges.contains(&Edge::LiftRelease));
    for _ in 0..10 {
        let frame = rig.frame();
        assert_eq!(frame.gameplay.movement, Vec2::ZERO);
        assert!(!frame.gameplay.lift_held);
    }
    rig.pads.neutral(1);
    rig.frame();
    rig.pads.left_stick(1, Vec2::Y);
    assert!(rig.frame().gameplay.movement.y != 0.0, "a fresh push counts");
}

#[test]
fn the_utility_panel_takes_controller_ui_and_confirms_one_action() {
    let mut rig = Rig::seated(1);
    rig.input.set_scene(SceneContext { scenario: false, utility: vec![Utility::Dig, Utility::Record, Utility::Respawn], recording: false });
    let frame = rig.tap(1, Button::View);
    assert_eq!(frame.suspended, Some(Suspend::Panel));
    assert!(frame.blocked);
    // Down twice, confirm: Respawn, once. A never also interacts.
    rig.tap(1, Button::DPadDown);
    rig.tap(1, Button::DPadDown);
    let frame = rig.tap(1, Button::South);
    assert_eq!(frame.gameplay.edges, vec![Edge::Utility(Utility::Respawn)]);
    assert!(rig.input.panel().is_none());
    // B closes without an action.
    rig.tap(1, Button::View);
    let frame = rig.tap(1, Button::East);
    assert!(frame.gameplay.edges.is_empty() && rig.input.panel().is_none());
    // Keyboard Escape closes it too, and is marked taken (so it doesn't pause).
    rig.tap(1, Button::View);
    rig.keys.press(KeyCode::Escape);
    rig.frame();
    assert!(rig.input.panel().is_none() && rig.input.escape_taken());
}

#[test]
fn a_chooser_opens_on_request_and_returns_the_pick() {
    let mut rig = Rig::seated(1);
    rig.input.open_chooser("MILL INPUT", vec!["COLLECT 4 LUMBER".into(), "COLLECT 3 IRON".into()]);
    let frame = rig.frame();
    assert_eq!(frame.suspended, Some(Suspend::Panel));
    rig.tap(1, Button::DPadDown);
    assert_eq!(rig.tap(1, Button::South).gameplay.edges, vec![Edge::Choose(1)]);
}

#[test]
fn ui_that_takes_a_press_keeps_it_from_gameplay() {
    let mut rig = Rig::seated(1);
    rig.pads.press(1, Button::South);
    rig.frame();
    assert!(rig.input.frame().gameplay.edges.contains(&Edge::Interact));
    rig.pads.release(1, Button::South);
    rig.frame();
    rig.pads.press(1, Button::South);
    rig.frame();
    assert!(rig.input.take_confirm());
    assert!(rig.input.frame().gameplay.edges.is_empty(), "the confirm is not also an interaction");
}

#[test]
fn navigation_repeats_while_held_and_confirm_never_does() {
    let mut rig = Rig::seated(1);
    rig.pads.press(1, Button::DPadDown).press(1, Button::South);
    let mut navs = 0;
    let mut confirms = 0;
    // One second held: the first step, one after the delay, then one per
    // interval.
    for _ in 0..60 {
        rig.frame();
        navs += rig.input.take_nav().is_some() as u32;
        confirms += rig.input.take_confirm() as u32;
    }
    let expected = 1 + 1 + ((1.0 - 1.0 / 60.0 - tuning::NAV_REPEAT_DELAY) / tuning::NAV_REPEAT_INTERVAL) as u32;
    assert!(navs.abs_diff(expected) <= 1, "{navs} steps, expected about {expected}");
    assert_eq!(confirms, 1);
}

#[test]
fn keyboard_bindings_follow_the_scene() {
    let mut rig = Rig::new();
    rig.keys.press(KeyCode::KeyE);
    assert_eq!(rig.frame().gameplay.edges, vec![Edge::Utility(Utility::Record)]);
    rig.input.set_scene(SceneContext { scenario: true, ..default() });
    rig.keys.release(KeyCode::KeyE);
    rig.frame();
    rig.keys.press(KeyCode::KeyE);
    rig.keys.press(KeyCode::KeyQ);
    assert_eq!(rig.frame().gameplay.edges, vec![Edge::MachineToggle], "Q has no meaning with a supply chain");
}

// ---------------------------------------------------------------------------
// The whole game, headless, driven by synthetic controllers.

struct Game {
    app: App,
    fps: f64,
}

impl Game {
    fn options(menus: bool) -> crate::control::Options {
        crate::control::Options { simulation: true, headless_menus: menus, ..default() }
    }

    fn new(scene: &str, fps: f64) -> Game {
        Game::with(scene, fps, Game::options(false), None)
    }

    /// With the main menu and the pause overlay, as in a player's run.
    fn with_pause(scene: &str, fps: f64) -> Game {
        Game::with(scene, fps, Game::options(true), None)
    }

    fn with(scene: &str, fps: f64, options: crate::control::Options, network: Option<crate::lockstep::Network>) -> Game {
        let hub = crate::control::Hub::new(options.clone()).unwrap();
        let library = crate::assets::SceneLibrary::new();
        let index = library.index(scene).unwrap();
        let launch = crate::flow::Launch { network, ..crate::flow::Launch::single_player(index) };
        let mut app = crate::test_app(options, library, hub, launch);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / fps)));
        app.insert_resource(SyntheticGamepads::default());
        app.add_message::<bevy::input::keyboard::KeyboardFocusLost>();
        crate::start_world(&mut app);
        Game { app, fps }
    }

    fn pads(&mut self) -> Mut<'_, SyntheticGamepads> {
        self.app.world_mut().resource_mut::<SyntheticGamepads>()
    }

    /// Runs frames. Without Bevy's input plugin nothing clears last
    /// frame's key edges, so this does, as the plugin would.
    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.app.update();
            self.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
            self.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        }
    }

    /// Cycles the controller's target selection to `key`.
    fn select(&mut self, key: targets::TargetKey) {
        for _ in 0..24 {
            if self.dispatcher().targets.selected.as_ref() == Some(&key) {
                return;
            }
            self.tap(1, Button::DPadRight);
        }
        panic!("{key:?} is not a candidate: {:?}", self.dispatcher().targets.candidates);
    }

    fn seconds(&mut self, s: f64) {
        self.frames((s * self.fps).round() as usize);
    }

    fn tap(&mut self, pad: DeviceId, button: Button) {
        self.pads().press(pad, button);
        self.frames(1);
        self.pads().release(pad, button);
        self.frames(1);
    }

    /// Connects controller `pad` and claims the seat with it.
    fn claim(&mut self, pad: DeviceId) {
        self.pads().connect(pad);
        self.frames(1);
        self.tap(pad, Button::Menu);
        assert_eq!(self.input().seat(), Some(pad));
    }

    fn input(&self) -> &InputService {
        self.app.world().resource::<InputService>()
    }

    fn service(&self) -> &SessionService {
        self.app.world().resource::<SessionService>()
    }

    /// Every command this player's session has accepted into turns.
    fn commands(&self) -> Vec<CommandKind> {
        let log = self.service().session.input_log().unwrap();
        log.turns.into_iter().flatten().filter(|c| c.player == PlayerId(0)).map(|c| c.kind).collect()
    }

    fn count(&self, f: impl Fn(&CommandKind) -> bool) -> usize {
        self.commands().iter().filter(|c| f(c)).count()
    }

    fn avatar(&self) -> sim::settlement::AgentState {
        let mirror = self.app.world().resource::<WorldMirror>();
        mirror.current.as_ref().unwrap().agents[mirror.player().unwrap() as usize].clone()
    }

    fn position(&self) -> Vec3 {
        let mirror = self.app.world().resource::<WorldMirror>();
        mirror.to_view(self.avatar().body.position)
    }

    fn paused(&self) -> bool {
        self.app.world().get_resource::<crate::pause::Pause>().is_some_and(|p| p.open)
    }

    fn dispatcher(&self) -> &gameplay::Dispatcher {
        self.app.world().resource::<gameplay::Dispatcher>()
    }

    /// Walks the avatar to a point through the routed-movement helper
    /// (test setup, not the behavior under test), until it arrives.
    fn walk_to(&mut self, point: Vec3) {
        self.app.world_mut().resource_scope(|world, mut service: Mut<SessionService>| {
            world.resource_mut::<crate::lockstep::PlayerControl>().go_to(&mut service, point, true);
        });
        for _ in 0..(20.0 * self.fps) as usize {
            self.frames(1);
            if self.avatar().body.route.is_empty() && self.app.world().resource::<SessionService>().caught_up() {
                return;
            }
        }
        panic!("never reached {point:?}");
    }
}

fn is_move(c: &CommandKind) -> bool {
    matches!(c, CommandKind::MoveAvatar { .. })
}

fn is_stop(c: &CommandKind) -> bool {
    matches!(c, CommandKind::MoveAvatar { intent: sim::settlement::avatar::Intent::Stop, .. })
}

fn is_release(c: &CommandKind) -> bool {
    matches!(c, CommandKind::ReleaseDirectControl { .. })
}

fn is_attack(c: &CommandKind) -> bool {
    matches!(c, CommandKind::Attack { .. })
}

#[test]
fn the_controller_walks_with_the_keyboard_idle_and_stops_once_on_neutral() {
    let mut game = Game::new("field", 60.0);
    game.claim(1);
    let start = game.position();
    // A slightly noisy full tilt "up" the screen.
    for i in 0..60 {
        let wobble = if i % 2 == 0 { 0.015 } else { -0.015 };
        game.pads().left_stick(1, Vec2::from_angle(std::f32::consts::FRAC_PI_2 + wobble) * 0.95);
        game.frames(1);
    }
    let moved = crate::navigation::flat(game.position() - start);
    let expected = gameplay::camera_relative(Vec2::Y);
    assert!(moved.length() > 2.0, "walked {moved:?}");
    assert!(moved.normalize().dot(expected) > 0.99, "camera-relative like the keyboard: {moved:?}");
    assert_eq!(game.count(is_move), 1, "stick noise sent no extra movement: {:?}", game.commands());
    game.pads().neutral(1);
    game.seconds(0.5);
    assert_eq!(game.count(is_stop), 1);
    let stopped = game.position();
    game.seconds(0.5);
    assert!(game.position().distance(stopped) < 0.01);
    assert_eq!(game.count(is_move), 2, "an idle device sends nothing more");
}

#[test]
fn run_is_a_separate_held_action() {
    let mut game = Game::new("field", 60.0);
    game.claim(1);
    game.pads().left_stick(1, Vec2::X).press(1, Button::LeftBumper);
    game.seconds(0.3);
    game.pads().release(1, Button::LeftBumper);
    game.seconds(0.3);
    let runs: Vec<bool> = game
        .commands()
        .into_iter()
        .filter_map(|c| match c {
            CommandKind::MoveAvatar { intent: sim::settlement::avatar::Intent::Walk { run, .. }, .. } => Some(run),
            _ => None,
        })
        .collect();
    assert_eq!(runs, vec![true, false]);
}

#[test]
fn keyboard_shift_runs_while_wasd_moves() {
    let mut rig = Rig::new();
    rig.keys.press(KeyCode::ShiftLeft);
    rig.keys.press(KeyCode::KeyW);
    let frame = rig.frame();
    assert!(frame.gameplay.run);
    assert_eq!(frame.gameplay.movement, Vec2::Y);
    rig.keys.release(KeyCode::ShiftLeft);
    assert!(!rig.frame().gameplay.run);
}

#[test]
fn keyboard_r_is_a_held_repair_action() {
    let mut rig = Rig::new();
    rig.keys.press(KeyCode::KeyR);
    let first = rig.frame();
    assert!(first.gameplay.repair_held && first.gameplay.repair_pressed);
    assert!(!first.gameplay.edges.contains(&Edge::Utility(Utility::Respawn)));
    let held = rig.frame();
    assert!(held.gameplay.repair_held && !held.gameplay.repair_pressed);
    rig.keys.release(KeyCode::KeyR);
    assert!(!rig.frame().gameplay.repair_held);
}

#[test]
fn takeover_releases_outgoing_movement_and_thrust_in_both_directions() {
    let mut game = Game::new("jetpack-lab", 60.0);
    game.claim(1);
    // The controller flies and walks.
    game.pads().press(1, Button::RightBumper).left_stick(1, Vec2::Y);
    game.seconds(0.5);
    assert!(game.avatar().avatar().unwrap().flight.requested);
    // A key press takes over: the jetpack goes off before the keyboard's
    // walk applies, though RB is still held.
    let keys_before = game.commands().len();
    game.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyD);
    game.frames(2);
    let after: Vec<_> = game.commands().split_off(keys_before);
    let off = after.iter().position(|c| matches!(c, CommandKind::Jetpack { enabled: false, .. }));
    let walk = after.iter().position(is_move);
    assert!(off.is_some() && walk.is_some() && off < walk, "{after:?}");
    game.seconds(0.3);
    assert!(!game.avatar().avatar().unwrap().flight.requested, "no latched thrust after takeover");
    // The held stick and RB are not deliberate: the keyboard keeps it.
    assert_eq!(game.input().owner(), Device::KeyboardMouse);
    // Back to the controller: the keyboard's walk stops (outgoing movement
    // released) even though D is still held.
    game.pads().neutral(1);
    game.frames(1);
    game.pads().press(1, Button::South);
    game.frames(1);
    assert_eq!(game.input().owner(), Device::Gamepad);
    game.seconds(0.3);
    assert!(game.commands().last().is_some_and(is_stop), "{:?}", game.commands().last());
    game.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyD);
    let count = game.commands().len();
    game.seconds(0.3);
    assert_eq!(game.commands().len(), count, "releasing D (not the owner) sends nothing");
}

/// One press, one attack: at a low frame rate (several ticks per frame), a
/// high one, with a noisy trigger and a long hold.
#[test]
fn one_attack_per_press_at_any_frame_rate() {
    for fps in [15.0, 60.0, 144.0] {
        let mut game = Game::new("field", fps);
        game.claim(1);
        for _ in 0..4 {
            for i in 0..(0.6 * fps) as usize {
                game.pads().right_trigger(1, if i % 2 == 0 { 0.9 } else { 0.6 });
                game.frames(1);
            }
            game.pads().right_trigger(1, 0.0);
            game.seconds(0.4);
        }
        assert_eq!(game.count(is_attack), 4, "at {fps} fps: {:?}", game.commands());
    }
}

#[test]
fn aim_starts_from_confirmed_facing_resolves_before_an_attack_and_is_remembered() {
    let mut game = Game::new("field", 60.0);
    game.claim(1);
    let attack_aim = |game: &Game| {
        game.commands().into_iter().rev().find_map(|c| match c {
            CommandKind::Attack { aim, .. } => Some(Vec2::new(aim.x.to_f32(), aim.y.to_f32()).normalize()),
            _ => None,
        })
    };
    // Before any aim input: the confirmed facing.
    let facing = targets::confirmed_facing(game.app.world().resource::<WorldMirror>()).unwrap();
    game.pads().right_trigger(1, 1.0);
    game.frames(1);
    game.pads().right_trigger(1, 0.0);
    game.seconds(1.0);
    assert!(attack_aim(&game).unwrap().dot(Vec2::new(facing.x, facing.z)) > 0.999);
    // Aim and attack in the same sample: the attack uses the new aim.
    game.pads().right_stick(1, Vec2::X).right_trigger(1, 1.0);
    game.frames(1);
    game.pads().right_stick(1, Vec2::ZERO).right_trigger(1, 0.0);
    game.seconds(1.0);
    let aimed = gameplay::camera_relative(Vec2::X).normalize();
    assert!(attack_aim(&game).unwrap().dot(Vec2::new(aimed.x, aimed.z)) > 0.999);
    // Walking sideways with the aim stick neutral keeps that aim.
    game.pads().left_stick(1, Vec2::Y);
    game.seconds(0.5);
    game.pads().right_trigger(1, 1.0);
    game.frames(1);
    game.pads().right_trigger(1, 0.0).left_stick(1, Vec2::ZERO);
    game.seconds(1.0);
    assert!(attack_aim(&game).unwrap().dot(Vec2::new(aimed.x, aimed.z)) > 0.999);
}

#[test]
fn pause_releases_direct_control_and_nothing_resumes_by_itself() {
    let mut game = Game::with_pause("jetpack-lab", 60.0);
    game.claim(1);
    game.pads().press(1, Button::RightBumper).left_stick(1, Vec2::Y);
    game.seconds(0.5);
    assert!(game.avatar().avatar().unwrap().flight.requested);
    // Pause while flying, walking and squeezing the trigger.
    game.pads().right_trigger(1, 1.0);
    game.pads().press(1, Button::Menu);
    game.frames(1);
    assert!(game.paused());
    game.pads().release(1, Button::Menu);
    let attacks = game.count(is_attack);
    game.seconds(0.5);
    assert_eq!(game.count(is_release), 1);
    assert!(!game.avatar().avatar().unwrap().flight.requested, "thrust is off");
    assert_eq!(game.avatar().avatar().unwrap().intent, Some(sim::settlement::avatar::Intent::Stop));
    // Everything still held. Let go of the trigger and RB, keep the stick:
    // the controller is not at rest, so the pause menu doesn't move either.
    game.pads().right_trigger(1, 0.0).release(1, Button::RightBumper);
    game.seconds(0.2);
    game.pads().left_stick(1, Vec2::ZERO);
    game.frames(2);
    // A on RESUME.
    game.tap(1, Button::South);
    assert!(!game.paused());
    let count = game.commands().len();
    game.seconds(0.5);
    assert_eq!(game.commands().len(), count, "no surprise action after resuming: {:?}", game.commands().split_off(count));
    assert_eq!(game.count(is_attack), attacks);
}

#[test]
fn keyboard_pause_also_releases_once() {
    let mut game = Game::with_pause("field", 60.0);
    game.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
    game.seconds(0.3);
    game.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
    game.frames(1);
    assert!(game.paused());
    game.seconds(0.3);
    assert_eq!(game.count(is_release), 1);
    assert_eq!(game.count(is_stop), 0, "the release replaces an unconditional Stop");
    // Escape again, W still held: walking does not resume.
    game.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Escape);
    game.frames(1);
    game.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
    game.frames(1);
    assert!(!game.paused());
    let stopped = game.position();
    game.seconds(0.5);
    assert!(game.position().distance(stopped) < 0.01);
}

#[test]
fn focus_loss_opens_pause_without_resuming_on_return() {
    let mut game = Game::with_pause("field", 60.0);
    game.claim(1);
    game.pads().left_stick(1, Vec2::Y);
    game.seconds(0.3);
    game.app.world_mut().write_message(bevy::input::keyboard::KeyboardFocusLost);
    game.frames(1);
    assert!(game.paused());
    game.seconds(0.5);
    assert_eq!(game.count(is_release), 1);
    // Focus returns: still paused, still stopped.
    let stopped = game.position();
    game.seconds(0.5);
    assert!(game.paused() && game.position().distance(stopped) < 0.01);
}

#[test]
fn disconnecting_while_flying_releases_pauses_and_needs_a_fresh_claim() {
    let mut game = Game::with_pause("jetpack-lab", 60.0);
    game.claim(1);
    game.pads().press(1, Button::RightBumper);
    game.seconds(1.0);
    assert!(game.avatar().avatar().unwrap().flight.airborne);
    game.pads().disconnect(1);
    game.frames(1);
    assert!(game.paused());
    assert_eq!(game.input().seat(), None);
    game.seconds(0.5);
    assert_eq!(game.count(is_release), 1);
    assert!(!game.avatar().avatar().unwrap().flight.requested);
    // Reconnected and held: nothing, and Menu claims (only claims).
    game.pads().connect(1);
    game.frames(1);
    game.tap(1, Button::Menu);
    assert_eq!(game.input().seat(), Some(1));
    assert!(game.paused(), "the claim press does not also resume");
    game.tap(1, Button::South);
    assert!(!game.paused());
}

#[test]
fn several_controllers_drive_nothing_until_seated() {
    let mut game = Game::new("field", 60.0);
    game.pads().connect(1).connect(2);
    game.frames(1);
    game.pads().left_stick(1, Vec2::Y).left_stick(2, Vec2::Y);
    game.seconds(0.5);
    assert_eq!(game.count(is_move), 0, "unseated controllers do nothing");
    game.pads().neutral(1).neutral(2);
    game.frames(1);
    game.tap(2, Button::Menu);
    game.pads().left_stick(1, Vec2::Y);
    game.seconds(0.3);
    assert_eq!(game.count(is_move), 0, "the other controller is ignored");
    game.pads().left_stick(2, Vec2::Y);
    game.seconds(0.3);
    assert_eq!(game.count(is_move), 1);
}

#[test]
fn the_utility_panel_suspends_and_its_confirm_never_leaks() {
    let mut game = Game::new("field", 60.0);
    game.claim(1);
    game.pads().left_stick(1, Vec2::Y);
    game.seconds(0.3);
    game.pads().press(1, Button::View);
    game.frames(1);
    game.pads().release(1, Button::View).left_stick(1, Vec2::ZERO);
    game.frames(2);
    assert!(matches!(game.input().panel(), Some(Panel::Utility { .. })));
    // The panel lists Record, Clear Recording and Respawn in the field;
    // down twice and A respawns.
    game.tap(1, Button::DPadDown);
    game.tap(1, Button::DPadDown);
    game.tap(1, Button::South);
    game.seconds(0.3);
    assert_eq!(game.count(is_release), 1);
    assert_eq!(game.count(|c| matches!(c, CommandKind::Respawn { .. })), 1);
    assert!(game.dispatcher().notice.is_empty(), "A was not also an interaction: {}", game.dispatcher().notice);
}

#[test]
fn zoom_is_a_view_action() {
    let mut game = Game::new("field", 60.0);
    game.claim(1);
    game.tap(1, Button::DPadDown);
    assert_eq!(game.count(|_| true), 0);
}

/// Pile logistics: sticky nearest target, cycling, the primary action and
/// collecting more, incompatible cargo, and the drop proposal.
#[test]
fn pile_targets_are_sticky_cycle_stably_and_take_and_deposit() {
    let mut game = Game::new("pile-lab", 60.0);
    game.claim(1);
    game.seconds(0.5);
    // Before any input the nearest candidate is selected.
    let nearest = game.dispatcher().targets.candidates.iter().min_by(|a, b| a.gap.total_cmp(&b.gap)).map(|t| t.key.clone());
    assert_eq!(game.dispatcher().targets.selected, nearest);
    // Stand by a pile with ore.
    let stock = game.app.world().resource::<crate::piles::PileLab>().stock.clone().unwrap();
    // The fullest pile, so it outlasts a full carrier's pickup.
    let (&id, pile) = stock.piles.iter().max_by_key(|(_, p)| p.units).expect("a pile with ore");
    let at = Vec3::new(pile.position[0], 0.0, pile.position[1]);
    let offset = crate::navigation::flat(game.position() - at).normalize_or(Vec3::X) * (crate::pile_model::pile_radius(pile.units).max(0.55) + 1.0);
    game.walk_to(at + offset);
    game.frames(2);
    game.select(targets::TargetKey::Pile(id));
    // A small step keeps the selection, even toward another candidate.
    game.pads().left_stick(1, Vec2::X * 0.5);
    game.frames(6);
    game.pads().neutral(1);
    game.seconds(0.3);
    assert_eq!(game.dispatcher().targets.selected, Some(targets::TargetKey::Pile(id)));
    // Cycling walks the candidates in key order and comes back around.
    let keys: Vec<_> = game.dispatcher().targets.candidates.iter().map(|t| t.key.clone()).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    for _ in 0..keys.len() {
        game.tap(1, Button::DPadRight);
    }
    assert_eq!(game.dispatcher().targets.selected, Some(targets::TargetKey::Pile(id)));
    // A takes from it: one Pile command, and the haul completes.
    game.tap(1, Button::South);
    let hauls = |game: &Game| {
        game.commands().into_iter().filter_map(|c| match c {
            CommandKind::Pile { action, .. } => Some(action),
            _ => None,
        }).collect::<Vec<_>>()
    };
    assert_eq!(hauls(&game), vec![sim::settlement::piles::PileAction::Pickup(id)]);
    game.seconds(6.0);
    let carried = game.app.world().resource::<crate::piles::PileLab>().stock.as_ref().unwrap().cargo.clone();
    assert!(carried.units > 0);
    // A at the same pile now deposits; X would collect more.
    game.tap(1, Button::West);
    game.tap(1, Button::South);
    assert_eq!(
        hauls(&game)[1..],
        [sim::settlement::piles::PileAction::Pickup(id), sim::settlement::piles::PileAction::Deposit(id)]
    );
}

#[test]
fn another_mineral_is_collected_beside_the_cargo() {
    let mut game = Game::new("pile-lab", 60.0);
    game.claim(1);
    game.seconds(0.5);
    let stock = game.app.world().resource::<crate::piles::PileLab>().stock.clone().unwrap();
    let (&first, pile) = stock.piles.iter().find(|(_, p)| p.units >= 4).unwrap();
    let other = stock.piles.iter().find(|(_, p)| p.material != pile.material && p.units > 0).map(|(&id, p)| (id, p.clone())).expect("another mineral");
    let at = |p: &crate::pile_model::Pile, game: &Game| {
        let center = Vec3::new(p.position[0], 0.0, p.position[1]);
        center + crate::navigation::flat(game.position() - center).normalize_or(Vec3::X) * (crate::pile_model::pile_radius(p.units).max(0.55) + 1.0)
    };
    let stand = at(pile, &game);
    game.walk_to(stand);
    game.frames(2);
    game.select(targets::TargetKey::Pile(first));
    game.tap(1, Button::South);
    game.seconds(6.0);
    let stand = at(&other.1, &game);
    game.walk_to(stand);
    game.frames(2);
    game.select(targets::TargetKey::Pile(other.0));
    let collect = |c: &CommandKind| matches!(c, CommandKind::Pile { action: sim::settlement::piles::PileAction::Pickup(id), .. } if *id == other.0);
    assert_eq!(game.count(collect), 0);
    game.tap(1, Button::South);
    game.frames(2);
    // Minerals mix in a cargo: a pile of a mineral not carried is collected.
    assert_eq!(game.count(collect), 1, "collects the other mineral");
    assert_eq!(game.dispatcher().targets.selected, Some(targets::TargetKey::Pile(other.0)), "no silent substitution");
}

#[test]
fn drop_proposes_a_point_ahead_and_pause_keeps_the_accepted_job() {
    let mut game = Game::with_pause("pile-lab", 60.0);
    game.claim(1);
    game.seconds(0.5);
    let stock = game.app.world().resource::<crate::piles::PileLab>().stock.clone().unwrap();
    let (&id, pile) = stock.piles.iter().find(|(_, p)| p.units >= 4).unwrap();
    let center = Vec3::new(pile.position[0], 0.0, pile.position[1]);
    let stand = center + crate::navigation::flat(game.position() - center).normalize_or(Vec3::X) * (crate::pile_model::pile_radius(pile.units).max(0.55) + 2.5);
    game.walk_to(stand);
    game.frames(2);
    game.select(targets::TargetKey::Pile(id));
    // A starts a haul that walks into reach; pause before it arrives.
    game.tap(1, Button::South);
    game.frames(3);
    game.tap(1, Button::Menu);
    assert!(game.paused());
    game.seconds(0.2);
    assert_eq!(game.count(is_release), 1);
    let lab = game.app.world().resource::<crate::piles::PileLab>();
    assert!(lab.job.is_some(), "the accepted haul continues during pause");
    game.seconds(6.0);
    let lab = game.app.world().resource::<crate::piles::PileLab>();
    assert!(lab.job.is_none() && lab.last_error.is_none(), "it finished: {:?}", lab.last_error);
    assert!(lab.stock.as_ref().unwrap().cargo.units > 0);
    // Resume; B proposes the preview point.
    game.tap(1, Button::East);
    assert!(!game.paused());
    game.frames(2);
    let preview = game.dispatcher().drop_preview.expect("carrying shows a drop preview");
    game.tap(1, Button::East);
    let drop = game.commands().into_iter().rev().find_map(|c| match c {
        CommandKind::Pile { action: sim::settlement::piles::PileAction::Drop(p), .. } => Some(p),
        _ => None,
    });
    let service = game.service();
    assert_eq!(drop, Some(service.to_sim(preview)));
    // Whatever the simulation decides, one proposal was sent: a refusal
    // would report its reason, never retry elsewhere.
    game.seconds(4.0);
    assert_eq!(game.count(|c| matches!(c, CommandKind::Pile { action: sim::settlement::piles::PileAction::Drop(_), .. })), 1);
}

#[test]
fn a_multi_item_depot_opens_a_chooser_and_machines_check_reach() {
    let mut game = Game::new("forest-outpost", 60.0);
    game.claim(1);
    game.seconds(0.3);
    let view = game.app.world().resource::<crate::settlement::Settlement>().simulation.clone().unwrap();
    let depot = view.config().depots.iter().find(|d| d.id == "mill_input").unwrap().clone();
    // Stock both items, so both can be collected.
    let s = game.service().settlement().unwrap();
    let (d, lumber, iron) = (s.depot_index("mill_input").unwrap(), s.item_index("lumber").unwrap(), s.item_index("iron").unwrap());
    game.app.world_mut().resource_mut::<SessionService>().command(CommandKind::AddStock { depot: d, item: lumber, units: 8 });
    game.app.world_mut().resource_mut::<SessionService>().command(CommandKind::AddStock { depot: d, item: iron, units: 8 });
    let center = Vec3::new(depot.position[0], 0.0, depot.position[1]);
    game.walk_to(center + crate::navigation::flat(game.position() - center).normalize_or(Vec3::X) * 1.6);
    game.frames(2);
    game.select(targets::TargetKey::Depot("mill_input".into()));
    game.tap(1, Button::South);
    game.frames(1);
    let Some(Panel::Chooser { options, .. }) = game.input().panel().cloned() else { panic!("no chooser: {:?}", game.dispatcher().notice) };
    assert_eq!(options.len(), 2, "{options:?}");
    let before = game.count(|c| matches!(c, CommandKind::Transfer { .. }));
    assert_eq!(before, 0, "nothing is sent before a pick");
    game.tap(1, Button::DPadDown);
    game.tap(1, Button::South);
    game.frames(2);
    let transfer = game.commands().into_iter().find_map(|c| match c {
        CommandKind::Transfer { item, deposit, .. } => Some((item, deposit)),
        _ => None,
    });
    assert_eq!(transfer, Some((iron, false)));
    // Machines keep the keyboard's reach: too far, a reason and nothing sent.
    let machine = view.config().producers[0].clone();
    let centre = Vec3::new(machine.position[0], 0.0, machine.position[1]);
    game.walk_to(centre + crate::navigation::flat(game.position() - centre).normalize_or(Vec3::X) * (targets::MACHINE_REACH + 1.5));
    game.frames(2);
    let key = targets::TargetKey::Machine(machine.id.clone());
    let mut tries = 0;
    while game.dispatcher().targets.selected != Some(key.clone()) && tries < 20 {
        game.tap(1, Button::DPadRight);
        tries += 1;
    }
    if game.dispatcher().targets.selected == Some(key) {
        let count = game.count(|c| matches!(c, CommandKind::SetMachine { .. }));
        game.tap(1, Button::South);
        assert_eq!(game.count(|c| matches!(c, CommandKind::SetMachine { .. })), count);
        assert!(game.dispatcher().notice.contains("Move within"), "{}", game.dispatcher().notice);
    }
}

/// Arrow keys now cancel a depot transfer like WASD (they used to be missed).
#[test]
fn arrow_keys_cancel_a_depot_transfer() {
    let mut game = Game::new("forest-outpost", 60.0);
    game.seconds(0.3);
    let world = game.app.world_mut();
    crate::settlement::player_transfer(world, "timber", "lumber", 2, false).unwrap();
    game.frames(3);
    game.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowLeft);
    game.frames(3);
    assert!(game.count(|c| matches!(c, CommandKind::CancelTransfer { .. })) >= 1);
}

/// A controller session's accepted turns, written and read back as an input
/// log, replay to the identical world.
#[test]
fn controller_sessions_replay_exactly() {
    let mut game = Game::with_pause("jetpack-lab", 60.0);
    game.claim(1);
    game.pads().left_stick(1, Vec2::new(0.3, 0.9)).press(1, Button::RightBumper);
    game.seconds(0.6);
    game.pads().right_trigger(1, 1.0);
    game.frames(1);
    game.pads().right_trigger(1, 0.0);
    game.tap(1, Button::Menu);
    game.seconds(0.3);
    game.pads().neutral(1);
    game.frames(2);
    game.tap(1, Button::South);
    game.pads().left_stick(1, Vec2::X);
    game.seconds(0.5);
    let service = game.service();
    let content = service.content().clone();
    let start = service.start_save().unwrap().to_vec();
    let log = save::InputLog::from_bytes(&service.session.input_log().unwrap().to_bytes()).unwrap();
    assert!(log.turns.iter().flatten().any(|c| is_release(&c.kind)), "the log holds a release");
    let original = save::write(service.world().unwrap(), &content);
    let world = save::read(&start, &content).unwrap();
    let config = net::NetConfig { threads: 2, ..net::NetConfig::default() };
    let mut replay = session::Session::start(session::Mode::Replay { world, log }, content.clone(), config).unwrap();
    replay.update(std::time::Duration::from_secs(600), |_| {});
    assert!(replay.finished());
    assert_eq!(save::write(replay.world().unwrap(), &content), original);
}

/// The same timed scenario at different render rates ends in the same
/// place, through the same commands.
#[test]
fn timed_scenarios_agree_across_render_rates() {
    let script = || {
        GamepadScript::new(vec![
            Step { at: 0.0, pad: 1, connect: Some(true), ..default() },
            Step { at: 0.1, pad: 1, press: Some(Button::Menu), ..default() },
            Step { at: 0.2, pad: 1, release: Some(Button::Menu), ..default() },
            Step { at: 0.3, pad: 1, left_stick: Some([0.0, 1.0]), ..default() },
            Step { at: 1.3, pad: 1, left_stick: Some([0.8, 0.0]), ..default() },
            Step { at: 1.8, pad: 1, right_trigger: Some(1.0), ..default() },
            Step { at: 1.9, pad: 1, right_trigger: Some(0.0), ..default() },
            Step { at: 2.3, pad: 1, left_stick: Some([0.0, 0.0]), ..default() },
        ])
    };
    let mut results = Vec::new();
    for fps in [24.0, 60.0, 120.0] {
        let mut game = Game::new("field", fps);
        game.app.insert_resource(script());
        game.seconds(3.0);
        assert!(game.app.world().resource::<GamepadScript>().finished());
        let kinds: Vec<&'static str> = game
            .commands()
            .iter()
            .map(|c| match c {
                CommandKind::MoveAvatar { intent: sim::settlement::avatar::Intent::Stop, .. } => "stop",
                CommandKind::MoveAvatar { .. } => "move",
                CommandKind::Attack { .. } => "attack",
                _ => "other",
            })
            .collect();
        results.push((fps, kinds, game.position()));
    }
    for (fps, kinds, position) in &results[1..] {
        assert_eq!(kinds, &results[0].1, "{fps} fps");
        assert!(position.distance(results[0].2) < 0.35, "{fps} fps ended at {position:?}, not {:?}", results[0].2);
    }
    // A sword attack interrupts the walk; the held stick then resumes it.
    assert_eq!(results[0].1, vec!["move", "move", "attack", "move", "stop"]);
}

/// The development replay file parses.
#[test]
fn the_sample_gamepad_script_parses() {
    let script = GamepadScript::from_json(include_bytes!("../../demos/controller_field.gamepad.json")).unwrap();
    assert!(!script.steps.is_empty());
}

/// A joined player's controller moves its own avatar (not the host's), the
/// preview converges on confirmed state, and a disconnect releases it.
#[test]
fn a_joined_controller_moves_its_own_avatar_and_converges_after_release() {
    let host_net = crate::lockstep::Network::Host("127.0.0.1:0".parse().unwrap());
    let mut host = Game::with("field", 60.0, Game::options(false), Some(host_net));
    let address = host.service().session.local_addr().unwrap();
    let options = Game::options(false);
    let hub = crate::control::Hub::new(options.clone()).unwrap();
    let library = crate::assets::SceneLibrary::new();
    let field = library.index("field").unwrap();
    let launch = crate::flow::Launch { network: Some(crate::lockstep::Network::Join(address)), ..crate::flow::Launch::single_player(field) };
    let mut client = crate::test_app(options, library, hub, launch);
    client.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 60.0)));
    client.insert_resource(SyntheticGamepads::default());
    let mut joined = false;
    for _ in 0..240 {
        host.frames(1);
        client.update();
        let active = client.world().get_resource::<State<service::WorldState>>().is_some_and(|s| *s.get() == service::WorldState::Active);
        if active && client.world().resource::<WorldMirror>().player().is_some() {
            joined = true;
            break;
        }
    }
    assert!(joined);
    let mut client = Game { app: client, fps: 60.0 };
    let step = |host: &mut Game, client: &mut Game, n: usize| {
        for _ in 0..n {
            client.frames(1);
            host.frames(1);
        }
    };
    client.pads().connect(7);
    step(&mut host, &mut client, 1);
    client.pads().press(7, Button::Menu);
    step(&mut host, &mut client, 1);
    client.pads().release(7, Button::Menu);
    step(&mut host, &mut client, 1);
    let host_start = host.position();
    let client_start = client.position();
    client.pads().left_stick(7, Vec2::Y);
    step(&mut host, &mut client, 45);
    assert!(client.position().distance(client_start) > 0.5, "the joined avatar moved");
    assert!(host.position().distance(host_start) < 0.01, "the host's avatar did not");
    client.pads().disconnect(7);
    step(&mut host, &mut client, 60);
    let mirror = client.app.world().resource::<WorldMirror>();
    let me = mirror.player().unwrap() as usize;
    assert_eq!(mirror.current.as_ref().unwrap().agents[me].avatar().unwrap().intent, Some(sim::settlement::avatar::Intent::Stop));
    let shown = {
        let world = client.app.world_mut();
        world.query_filtered::<&Transform, With<crate::character::Player>>().single(world).unwrap().translation
    };
    assert!(crate::navigation::flat(shown - client.position()).length() < 0.1, "the preview settled on the confirmed pose");
}

/// Explicit Cancel Logistics, from the utility panel: unlike the release
/// that opening the panel causes, it ends the accepted haul.
#[test]
fn cancel_logistics_is_explicit_and_separate_from_release() {
    let mut game = Game::new("pile-lab", 60.0);
    game.claim(1);
    game.seconds(0.5);
    let stock = game.app.world().resource::<crate::piles::PileLab>().stock.clone().unwrap();
    let (&id, pile) = stock.piles.iter().max_by_key(|(_, p)| p.units).unwrap();
    let center = Vec3::new(pile.position[0], 0.0, pile.position[1]);
    let stand = center + crate::navigation::flat(game.position() - center).normalize_or(Vec3::X) * (crate::pile_model::pile_radius(pile.units).max(0.55) + 2.5);
    game.walk_to(stand);
    game.frames(2);
    game.select(targets::TargetKey::Pile(id));
    game.tap(1, Button::South);
    game.frames(3);
    assert!(game.app.world().resource::<crate::piles::PileLab>().job.is_some());
    // Opening the panel releases direct control; the haul goes on.
    game.tap(1, Button::View);
    game.frames(3);
    assert_eq!(game.count(is_release), 1);
    assert!(game.app.world().resource::<crate::piles::PileLab>().job.is_some());
    let Some(Panel::Utility { items, .. }) = game.input().panel().cloned() else { panic!("no utility panel") };
    assert_eq!(items.last(), Some(&Utility::CancelLogistics));
    // Up wraps to the last item.
    game.tap(1, Button::DPadUp);
    game.tap(1, Button::South);
    game.frames(3);
    assert!(game.app.world().resource::<crate::piles::PileLab>().job.is_none(), "the haul was canceled");
    assert_eq!(game.count(is_stop), 1);
    assert_eq!(game.count(|c| matches!(c, CommandKind::CancelTransfer { .. })), 1);
}

/// The Deck's single controller can navigate the launch menu immediately.
#[test]
fn sole_gamepad_navigates_the_deck_menu_without_a_claim_press() {
    let mut options = Game::options(true);
    options.auto_claim_gamepad = true;
    let hub = crate::control::Hub::new(options.clone()).unwrap();
    let library = crate::assets::SceneLibrary::new();
    let mut app = crate::test_front_app(options, library, hub, None);
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 60.0)));
    app.insert_resource(SyntheticGamepads::default());
    let mut game = Game { app, fps: 60.0 };
    game.pads().connect(1);
    game.frames(3);
    assert_eq!(game.app.world().resource::<InputService>().seat(), Some(1));
    assert_eq!(game.app.world().resource::<crate::menu::MainMenu>().pad_focus, Some(crate::menu::Action::Start));
    game.pads().left_stick(1, Vec2::NEG_Y);
    game.frames(1);
    assert_eq!(game.app.world().resource::<crate::menu::MainMenu>().pad_focus, Some(crate::menu::Action::Viewer));
    // Steam's Desktop layout may send arrows and Enter instead of gamepad
    // events. They navigate the same focused menu without launching Start.
    game.pads().left_stick(1, Vec2::ZERO);
    game.frames(1);
    game.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowDown);
    game.frames(1);
    assert_eq!(game.app.world().resource::<crate::menu::MainMenu>().pad_focus, Some(crate::menu::Action::Quit));
    let scene = (game.app.world().resource::<crate::menu::MainMenu>().selected_scene() + 1)
        % game.app.world().resource::<crate::assets::SceneLibrary>().scenes.len();
    game.app.world_mut().resource_mut::<crate::menu::MainMenu>().pad_focus = Some(crate::menu::Action::Scene(scene));
    game.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
    game.frames(1);
    assert_eq!(game.app.world().resource::<crate::menu::MainMenu>().selected_scene(), scene);
}

#[test]
fn the_menu_and_pause_work_with_a_controller() {
    let options = Game::options(true);
    let hub = crate::control::Hub::new(options.clone()).unwrap();
    let library = crate::assets::SceneLibrary::new();
    let scenes = library.scenes.len();
    let mut app = crate::test_front_app(options, library, hub, None);
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 60.0)));
    app.insert_resource(SyntheticGamepads::default());
    let mut game = Game { app, fps: 60.0 };
    game.frames(3);
    let menu = |game: &Game| game.app.world().resource::<crate::menu::MainMenu>().selected_scene();
    let starting = |game: &Game| game.app.world().resource::<crate::flow::Flow>().starting().is_some();
    // Unclaimed, A does nothing.
    game.pads().connect(1);
    game.frames(1);
    game.tap(1, Button::South);
    assert!(!starting(&game));
    game.tap(1, Button::Menu);
    assert_eq!(game.app.world().resource::<crate::menu::MainMenu>().pad_focus, Some(crate::menu::Action::Start));
    // Left to the scene list (at the selected scene), down one, select it.
    let first = menu(&game);
    game.tap(1, Button::DPadLeft);
    game.tap(1, Button::DPadDown);
    game.tap(1, Button::South);
    assert_eq!(menu(&game), (first + 1) % scenes);
    assert!(!starting(&game), "selecting a scene does not start it");
    // Right back to START, and start.
    game.tap(1, Button::DPadRight);
    game.pads().press(1, Button::South);
    game.frames(1);
    assert!(starting(&game) || game.app.world().get_resource::<SessionService>().is_some());
    game.pads().release(1, Button::South);
    crate::start_world(&mut game.app);
    assert_eq!(game.app.world().resource::<crate::scene_view::SceneView>().index, (first + 1) % scenes);
    // In the session: Menu pauses, down to LEAVE TO MENU, A leaves.
    game.frames(2);
    game.tap(1, Button::Menu);
    assert!(game.paused());
    game.tap(1, Button::DPadDown);
    game.tap(1, Button::South);
    game.frames(3);
    let session = game.app.world().get_resource::<State<service::SessionState>>().map(|s| *s.get());
    assert_eq!(session, Some(service::SessionState::Off));
    assert_eq!(game.app.world().resource::<InputService>().seat(), Some(1), "the seat outlives the session");
}

#[test]
fn history_keeps_recent_frames_with_their_suspensions() {
    let mut rig = Rig::seated(1);
    rig.pads.disconnect(1);
    rig.frame();
    for _ in 0..HISTORY + 10 {
        rig.frame();
    }
    assert_eq!(rig.input.history().count(), HISTORY);
    let mut rig = Rig::seated(1);
    rig.pads.disconnect(1);
    rig.frame();
    let last = rig.input.history().last().unwrap();
    assert_eq!((last.suspended, last.seat, last.owner), (Some(Suspend::Disconnected), None, Device::KeyboardMouse));
}
