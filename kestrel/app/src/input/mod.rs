//! Device input becomes actions (App layer). See
//! kestrel/docs/controller-support.md for the agreed design.
//!
//! Once per frame, at the start of the Input phase, [`InputService`]
//! samples the keyboard and mouse (through `debug_ui::GameInput`) and every
//! controller (a [`pad::GamepadSnapshot`] each, from the native reader or
//! the synthetic source), and resolves one [`ActionFrame`]:
//!
//! - **Seat.** Menu/Start claims the single local controller seat; the
//!   claim press does nothing else. Other controllers are ignored until the
//!   seat is free again. A seated controller that disconnects frees it.
//! - **Ownership.** One device group drives gameplay: keyboard/mouse or the
//!   seated controller. The last deliberate input takes over (a fresh press,
//!   a stick leaving its dead zone, mouse movement past a threshold). If
//!   both are deliberate in one frame the current owner keeps it. The
//!   outgoing device's movement and thrust are released by the gameplay
//!   dispatcher (`gameplay::dispatch`) before the incoming actions apply.
//! - **Contexts.** The utility panel and item choosers are resolved here,
//!   before the Session's pause overlay and the World's gameplay, and take
//!   the controller's UI actions. UI handlers take actions with the
//!   `take_*` methods; a press taken by UI never also reaches gameplay.
//! - **Suspension.** Pause, focus loss, a seated controller disconnecting,
//!   and opening a panel suspend direct control: the frame says so once
//!   (`suspended`), the dispatcher releases the avatar, and the controller
//!   counts again only once every button is up and its sticks are centered.
//!   Focus loss and disconnects also ask the pause overlay to open; nothing
//!   resumes by itself.
//!
//! Device data and floating point stay in the front end: the World
//! dispatcher turns the frame into the same semantic, fixed-point commands
//! keyboard and mouse send.

pub mod gameplay;
mod hud;
mod native;
pub mod pad;
pub mod synthetic;
pub mod targets;
#[cfg(test)]
mod tests;
pub mod tuning;

use std::collections::{BTreeMap, VecDeque};

use bevy::prelude::*;
use content::scenario::Weapon;
use debug_ui::GameInput;
use service::{AppLayer, AppServices, Phase, Service};

use pad::{Button, Control, DeviceId, DirectionCoalescer, GamepadSnapshot, PadFrame, PadTracker};

/// Which device group drives gameplay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Device {
    #[default]
    KeyboardMouse,
    Gamepad,
}

/// A step of focus navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
}

/// Why direct control was suspended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suspend {
    Pause,
    FocusLost,
    Disconnected,
    /// The utility panel or a chooser opened.
    Panel,
}

/// What the utility panel offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Utility {
    Dig,
    /// Record, or deploy a recording in progress.
    Record,
    ClearRecording,
    Respawn,
    CancelLogistics,
}

impl Utility {
    pub fn label(self, recording: bool) -> &'static str {
        match self {
            Utility::Dig => "DIG",
            Utility::Record if recording => "DEPLOY RECORDING",
            Utility::Record => "RECORD",
            Utility::ClearRecording => "CLEAR RECORDING",
            Utility::Respawn => "RESPAWN",
            Utility::CancelLogistics => "CANCEL LOGISTICS",
        }
    }
}

/// A one-shot gameplay action, in the order it happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Attack,
    /// Jump in Jump scenes; otherwise the jetpack goes on (held state).
    Lift,
    /// The jetpack goes off.
    LiftRelease,
    /// The selected nearby target's primary action.
    Interact,
    /// Collect more compatible material while carrying.
    Collect,
    /// Propose a cargo drop just ahead.
    Drop,
    NextWeapon,
    SelectWeapon(Weapon),
    TargetPrevious,
    TargetNext,
    /// Keyboard E in scenes with a supply chain: the nearest machine.
    MachineToggle,
    Utility(Utility),
    /// An item chooser's pick.
    Choose(usize),
}

/// Navigation, confirm, back, and the menu buttons, from the seated
/// controller.
#[derive(Clone, Debug, Default)]
pub struct UiActions {
    nav: Option<Nav>,
    confirm: bool,
    back: bool,
    menu: bool,
    /// Keyboard Escape, taken by a panel.
    escape_taken: bool,
}

/// Held state and one-shot actions for gameplay, from the owning device.
#[derive(Clone, Debug, Default)]
pub struct Gameplay {
    /// Input-space direction (x right, y up): unit length or zero.
    pub movement: Vec2,
    pub run: bool,
    /// The lift (Space / RB) is held.
    pub lift_held: bool,
    /// An explicit input-space aim direction from the aim stick.
    pub aim: Option<Vec2>,
    pub edges: Vec<Edge>,
    /// View zoom change (positive zooms out).
    pub zoom: f32,
}

/// This frame's resolved input.
#[derive(Clone, Debug, Default)]
pub struct ActionFrame {
    pub owner: Device,
    /// Ownership changed this frame: release the outgoing device first.
    pub takeover: bool,
    pub gameplay: Gameplay,
    ui: UiActions,
    /// A UI context has gameplay this frame (pause, a panel).
    pub blocked: bool,
    /// Direct control was suspended this frame.
    pub suspended: Option<Suspend>,
    /// Focus loss or a disconnect: the pause overlay should open.
    pub pause_requested: bool,
}

/// An open panel: the utility panel, or an item chooser.
#[derive(Clone, Debug, PartialEq)]
pub enum Panel {
    Utility { items: Vec<Utility>, focus: usize },
    Chooser { title: String, options: Vec<String>, focus: usize },
}

/// What the World tells the input layer about the current scene.
#[derive(Clone, Debug, Default)]
pub struct SceneContext {
    /// A supply chain: keyboard E is the machine toggle, not memory.
    pub scenario: bool,
    pub utility: Vec<Utility>,
    pub recording: bool,
}

/// One resolved frame, kept for inspection.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub owner: Device,
    pub seat: Option<DeviceId>,
    pub edges: Vec<Edge>,
    pub movement: Vec2,
    pub blocked: bool,
    pub suspended: Option<Suspend>,
}

const HISTORY: usize = 240;

/// The latest snapshot of every controller, written once per frame by the
/// native reader or, when present, the synthetic source.
#[derive(Resource, Default, Clone, Debug)]
pub struct GamepadSnapshots(pub Vec<GamepadSnapshot>);

/// Keys that drive gameplay: only these take ownership from a controller.
const GAMEPLAY_KEYS: [KeyCode; 20] = [
    KeyCode::KeyW,
    KeyCode::KeyA,
    KeyCode::KeyS,
    KeyCode::KeyD,
    KeyCode::ArrowUp,
    KeyCode::ArrowDown,
    KeyCode::ArrowLeft,
    KeyCode::ArrowRight,
    KeyCode::ShiftLeft,
    KeyCode::ShiftRight,
    KeyCode::Space,
    KeyCode::KeyF,
    KeyCode::KeyR,
    KeyCode::KeyE,
    KeyCode::KeyQ,
    KeyCode::KeyZ,
    KeyCode::KeyX,
    KeyCode::KeyC,
    KeyCode::KeyV,
    KeyCode::ControlLeft,
];

#[derive(Clone, Copy, Debug, Default)]
struct NavRepeat {
    held: Option<Nav>,
    until: f32,
}

#[derive(Resource, Default)]
pub struct InputService {
    pads: BTreeMap<DeviceId, PadTracker>,
    seat: Option<DeviceId>,
    owner: Device,
    frame: ActionFrame,
    panel: Option<Panel>,
    /// A chooser the World asked for, opened at the next sample.
    requested_chooser: Option<Panel>,
    scene: SceneContext,
    movement: DirectionCoalescer,
    repeat: NavRepeat,
    history: VecDeque<HistoryEntry>,
    /// A suspension asked for after this frame's sample (by pause).
    pending_suspend: Option<Suspend>,
}

impl Service for InputService {
    type Layer = AppLayer;
    type Deps = ();

    fn init(_world: &mut World) -> Self {
        InputService::default()
    }

    /// Registered first among App services, so every reader of actions
    /// runs after the sample.
    fn systems(app: &mut App) {
        app.init_resource::<GamepadSnapshots>()
            .add_service_systems::<Self, _>(
                Phase::Input,
                (
                    synthetic::play,
                    native::read.run_if(not(resource_exists::<synthetic::SyntheticGamepads>)),
                    synthetic::publish.run_if(resource_exists::<synthetic::SyntheticGamepads>),
                    sample,
                )
                    .chain(),
            )
            .add_service_systems::<Self, _>(Phase::Present, hud::show.run_if(crate::control::rendered));
    }
}

fn sample(time: Res<Time>, snapshots: Res<GamepadSnapshots>, mut game: ResMut<GameInput>, mut input: ResMut<InputService>) {
    input.sample(&game, &snapshots.0, time.delta_secs());
    if input.panel.is_some() {
        // A panel has gameplay: held keys end once, like under pause.
        game.block();
    }
}

impl InputService {
    pub fn frame(&self) -> &ActionFrame {
        &self.frame
    }

    pub fn seat(&self) -> Option<DeviceId> {
        self.seat
    }

    #[cfg(test)]
    pub fn owner(&self) -> Device {
        self.owner
    }

    pub fn panel(&self) -> Option<&Panel> {
        self.panel.as_ref()
    }

    /// The last few seconds of resolved frames, oldest first.
    #[cfg(test)]
    pub fn history(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.history.iter()
    }

    pub fn recording(&self) -> bool {
        self.scene.recording
    }

    /// The World's view of the scene, for bindings and the utility panel.
    pub fn set_scene(&mut self, scene: SceneContext) {
        self.scene = scene;
    }

    /// Opens an item chooser at the next frame. Its pick arrives as
    /// [`Edge::Choose`].
    pub fn open_chooser(&mut self, title: impl Into<String>, options: Vec<String>) {
        self.requested_chooser = Some(Panel::Chooser { title: title.into(), options, focus: 0 });
    }

    /// The UI navigation step this frame, if any; taking it consumes it.
    pub fn take_nav(&mut self) -> Option<Nav> {
        let nav = self.frame.ui.nav.take();
        if nav.is_some() {
            self.consumed();
        }
        nav
    }

    pub fn take_confirm(&mut self) -> bool {
        std::mem::take(&mut self.frame.ui.confirm) && self.consumed()
    }

    pub fn take_back(&mut self) -> bool {
        std::mem::take(&mut self.frame.ui.back) && self.consumed()
    }

    /// Menu/Start on the seated controller (not a claim).
    pub fn take_menu(&mut self) -> bool {
        std::mem::take(&mut self.frame.ui.menu) && self.consumed()
    }

    /// Whether a panel took keyboard Escape this frame (so it doesn't also
    /// pause).
    pub fn escape_taken(&self) -> bool {
        self.frame.ui.escape_taken
    }

    /// A UI handler took a controller press: the same press is not gameplay.
    fn consumed(&mut self) -> bool {
        if self.owner == Device::Gamepad {
            self.frame.gameplay.edges.clear();
        }
        true
    }

    /// Gameplay is the UI's for the rest of this frame (pause is open).
    pub fn block(&mut self) {
        self.frame.blocked = true;
    }

    /// Suspends direct control now (pause opening): the dispatcher releases
    /// the avatar this frame and the controller re-arms.
    pub fn suspend(&mut self, reason: Suspend) {
        self.frame.suspended.get_or_insert(reason);
        self.suspend_pads();
        self.pending_suspend = None;
    }

    fn suspend_pads(&mut self) {
        if let Some(pad) = self.seat.and_then(|seat| self.pads.get_mut(&seat)) {
            pad.suspend();
        }
        self.movement.reset();
        self.repeat = NavRepeat::default();
    }

    /// Resolves one frame from the routed keyboard/mouse and every
    /// controller's snapshot. `dt` is the frame's duration, for repeats.
    pub fn sample(&mut self, game: &GameInput, snapshots: &[GamepadSnapshot], dt: f32) {
        let mut frame = ActionFrame { owner: self.owner, ..default() };

        // Connections: forget devices that went away; a seated one frees
        // the seat and suspends.
        let connected: BTreeMap<DeviceId, &GamepadSnapshot> = snapshots.iter().filter(|s| s.connected).map(|s| (s.device, s)).collect();
        self.pads.retain(|device, _| connected.contains_key(device));
        if self.seat.is_some_and(|seat| !connected.contains_key(&seat)) {
            self.seat = None;
            frame.suspended = Some(Suspend::Disconnected);
            frame.pause_requested = true;
            if self.owner == Device::Gamepad {
                self.owner = Device::KeyboardMouse;
                frame.owner = Device::KeyboardMouse;
            }
            self.movement.reset();
        }
        let mut frames: BTreeMap<DeviceId, PadFrame> = BTreeMap::new();
        for (&device, snapshot) in &connected {
            frames.insert(device, self.pads.entry(device).or_default().update(snapshot).clone());
        }

        // Claiming: Menu on a free controller, lowest identity first.
        let mut claimed = false;
        if self.seat.is_none() {
            if let Some((&device, _)) = frames.iter().find(|(_, f)| f.pressed(Button::Menu)) {
                self.seat = Some(device);
                claimed = true;
            }
        }
        let empty = PadFrame::default();
        let pad = self.seat.and_then(|seat| frames.get(&seat)).unwrap_or(&empty);

        if game.focus_lost() {
            frame.suspended.get_or_insert(Suspend::FocusLost);
            frame.pause_requested = true;
        }

        // Panels: a chooser the World asked for opens now.
        if let Some(chooser) = self.requested_chooser.take() {
            self.panel = Some(chooser);
            frame.suspended.get_or_insert(Suspend::Panel);
        }

        // UI actions from the seated controller.
        let nav = self.navigation(pad, dt);
        let mut ui = UiActions {
            nav,
            confirm: pad.pressed(Button::South),
            back: pad.pressed(Button::East),
            menu: pad.pressed(Button::Menu) && !claimed,
            escape_taken: false,
        };
        let view = pad.pressed(Button::View);
        let escape = game.keys().just_pressed(KeyCode::Escape);
        let mut panel_edges = Vec::new();
        let mut panel_took = false;
        if let Some(panel) = &mut self.panel {
            panel_took = true;
            let (count, focus) = match panel {
                Panel::Utility { items, focus } => (items.len(), focus),
                Panel::Chooser { options, focus, .. } => (options.len(), focus),
            };
            match ui.nav.take() {
                Some(Nav::Up) if count > 0 => *focus = (*focus + count - 1) % count,
                Some(Nav::Down) if count > 0 => *focus = (*focus + 1) % count,
                _ => {}
            }
            let mut close = std::mem::take(&mut ui.back) || std::mem::take(&mut ui.menu) || view;
            if escape {
                ui.escape_taken = true;
                close = true;
            }
            if std::mem::take(&mut ui.confirm) && count > 0 {
                panel_edges.push(match panel {
                    Panel::Utility { items, focus } => Edge::Utility(items[*focus]),
                    Panel::Chooser { focus, .. } => Edge::Choose(*focus),
                });
                close = true;
            }
            if close {
                self.panel = None;
            }
        } else if view && !self.scene.utility.is_empty() {
            self.panel = Some(Panel::Utility { items: self.scene.utility.clone(), focus: 0 });
            frame.suspended.get_or_insert(Suspend::Panel);
            panel_took = true;
        }
        frame.ui = ui;

        // Ownership: the last deliberate input takes over; a tie keeps the
        // current owner.
        let keys = game.keys();
        let keyboard_deliberate = GAMEPLAY_KEYS.iter().any(|&k| keys.just_pressed(k))
            || game.mouse().get_just_pressed().next().is_some()
            || game.motion().length() >= tuning::MOUSE_TAKEOVER_PIXELS;
        let pad_deliberate = (self.seat.is_some() && pad.deliberate()) || claimed;
        let incoming = match (keyboard_deliberate, pad_deliberate) {
            (true, false) => Device::KeyboardMouse,
            (false, true) => Device::Gamepad,
            _ => self.owner,
        };
        if incoming != self.owner && !panel_took {
            frame.takeover = true;
            self.owner = incoming;
            frame.owner = incoming;
            self.movement.reset();
        }

        // Gameplay from the owner only; neutral input from the other device
        // contributes nothing.
        frame.gameplay = match self.owner {
            Device::KeyboardMouse => self.keyboard_gameplay(game),
            Device::Gamepad => self.pad_gameplay(pad),
        };
        if claimed || panel_took {
            frame.gameplay.edges.clear();
        }
        frame.gameplay.edges.extend(panel_edges);
        if self.panel.is_some() {
            frame.blocked = true;
        }
        if let Some(reason) = self.pending_suspend.take() {
            frame.suspended.get_or_insert(reason);
        }
        if frame.suspended.is_some() {
            self.suspend_pads();
        }
        self.frame = frame;
        self.remember();
    }

    fn remember(&mut self) {
        let f = &self.frame;
        self.history.push_back(HistoryEntry {
            owner: f.owner,
            seat: self.seat,
            edges: f.gameplay.edges.clone(),
            movement: f.gameplay.movement,
            blocked: f.blocked,
            suspended: f.suspended,
        });
        while self.history.len() > HISTORY {
            self.history.pop_front();
        }
    }

    /// Focus navigation from the D-pad or the left stick, repeating while
    /// held.
    fn navigation(&mut self, pad: &PadFrame, dt: f32) -> Option<Nav> {
        let dpad = [(Button::DPadUp, Nav::Up), (Button::DPadDown, Nav::Down), (Button::DPadLeft, Nav::Left), (Button::DPadRight, Nav::Right)]
            .into_iter()
            .find(|&(b, _)| pad.held(b))
            .map(|(_, nav)| nav);
        let stick = pad.movement.filter(|d| d.x.abs().max(d.y.abs()) >= tuning::NAV_STICK).map(|d| {
            if d.y.abs() >= d.x.abs() {
                if d.y > 0.0 { Nav::Up } else { Nav::Down }
            } else if d.x > 0.0 {
                Nav::Right
            } else {
                Nav::Left
            }
        });
        let held = dpad.or(stick);
        let repeat = &mut self.repeat;
        match held {
            None => {
                *repeat = NavRepeat::default();
                None
            }
            Some(nav) if repeat.held != Some(nav) => {
                *repeat = NavRepeat { held: Some(nav), until: tuning::NAV_REPEAT_DELAY };
                Some(nav)
            }
            Some(nav) => {
                repeat.until -= dt;
                if repeat.until <= 0.0 {
                    repeat.until += tuning::NAV_REPEAT_INTERVAL;
                    Some(nav)
                } else {
                    None
                }
            }
        }
    }

    fn keyboard_gameplay(&self, game: &GameInput) -> Gameplay {
        let keys = game.keys();
        let pressed = |a, b| if keys.pressed(a) || keys.pressed(b) { 1.0 } else { 0.0 };
        let movement = Vec2::new(
            pressed(KeyCode::KeyD, KeyCode::ArrowRight) - pressed(KeyCode::KeyA, KeyCode::ArrowLeft),
            pressed(KeyCode::KeyW, KeyCode::ArrowUp) - pressed(KeyCode::KeyS, KeyCode::ArrowDown),
        )
        .normalize_or_zero();
        let mut edges = Vec::new();
        for (key, weapon) in [(KeyCode::KeyZ, Weapon::Sword), (KeyCode::KeyX, Weapon::Spear), (KeyCode::KeyC, Weapon::Bow), (KeyCode::KeyV, Weapon::ArcCoil)] {
            if keys.just_pressed(key) {
                edges.push(Edge::SelectWeapon(weapon));
            }
        }
        if keys.just_pressed(KeyCode::KeyR) {
            edges.push(Edge::Utility(Utility::Respawn));
        }
        if keys.just_pressed(KeyCode::Space) {
            edges.push(Edge::Lift);
        }
        if keys.just_released(KeyCode::Space) {
            edges.push(Edge::LiftRelease);
        }
        if keys.just_pressed(KeyCode::KeyF) {
            edges.push(Edge::Utility(Utility::Dig));
        }
        // E toggles a machine where there is a supply chain, and drives the
        // memory recording elsewhere; Q clears it.
        if self.scene.scenario {
            if keys.just_pressed(KeyCode::KeyE) {
                edges.push(Edge::MachineToggle);
            }
        } else {
            if keys.just_pressed(KeyCode::KeyQ) {
                edges.push(Edge::Utility(Utility::ClearRecording));
            }
            if keys.just_pressed(KeyCode::KeyE) {
                edges.push(Edge::Utility(Utility::Record));
            }
        }
        Gameplay {
            movement,
            run: keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
            lift_held: keys.pressed(KeyCode::Space),
            aim: None,
            edges,
            zoom: 0.0,
        }
    }

    fn pad_gameplay(&mut self, pad: &PadFrame) -> Gameplay {
        let movement = self.movement.update(pad.movement, tuning::MOVE_DIRECTION_STEP).unwrap_or(Vec2::ZERO);
        let mut edges = Vec::new();
        let mut press = |control: Control, edge: Edge| {
            if pad.pressed.contains(&control) {
                edges.push(edge);
            }
        };
        press(Control::RightTrigger, Edge::Attack);
        press(Control::Button(Button::RightBumper), Edge::Lift);
        press(Control::Button(Button::South), Edge::Interact);
        press(Control::Button(Button::West), Edge::Collect);
        press(Control::Button(Button::East), Edge::Drop);
        press(Control::Button(Button::North), Edge::NextWeapon);
        press(Control::Button(Button::DPadLeft), Edge::TargetPrevious);
        press(Control::Button(Button::DPadRight), Edge::TargetNext);
        if pad.released(Button::RightBumper) {
            edges.push(Edge::LiftRelease);
        }
        let zoom = if pad.pressed(Button::DPadUp) {
            -tuning::ZOOM_STEP
        } else if pad.pressed(Button::DPadDown) {
            tuning::ZOOM_STEP
        } else {
            0.0
        };
        Gameplay {
            movement,
            run: pad.held(Button::LeftBumper),
            lift_held: pad.held(Button::RightBumper),
            aim: pad.aim,
            edges,
            zoom,
        }
    }
}
