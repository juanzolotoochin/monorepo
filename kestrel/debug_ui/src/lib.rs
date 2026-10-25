//! A debug overlay for the game: a menu bar of categories whose entries open
//! windows, drawn with egui (through bevy_egui). It starts hidden; `/`
//! shows or hides it. The names `ImguiManager` and `ImguiMenu` predate the
//! choice of egui.
//!
//! # Providers
//!
//! A provider is a resource implementing [`ImguiMenu`]. It declares entries
//! (stable ID, category, name) and draws each open one:
//!
//! ```ignore
//! const MENUS: &[MenuSpec] = &[
//!     MenuSpec::new("timing", "Diagnostics", "Frame timing"),
//!     MenuSpec::new("counters", "Diagnostics", "Counters"),
//! ];
//!
//! impl ImguiMenu for MyService {
//!     fn menus() -> &'static [MenuSpec] { MENUS }
//!     fn show_menu(&mut self, id: MenuId, root: &mut egui::Ui, open: &mut bool, world: &mut World) {
//!         id.window().open(open).show(root.ctx(), |ui| match id.id() {
//!             "timing" => { ui.label("..."); }
//!             _ => { ui.label("..."); }
//!         });
//!     }
//! }
//! ```
//!
//! A service registers itself in `init` and unregisters in `shutdown`, and
//! lists [`ImguiManager`] in its `Deps` (the manager is an App service, so
//! services of any layer may):
//!
//! ```ignore
//! impl Service for MyService {
//!     type Layer = WorldLayer;
//!     type Deps = (ImguiManager,);
//!     fn init(world: &mut World) -> Self {
//!         let menu = ImguiManager::register::<MyService>(world).map_err(|e| warn!("{e}")).ok();
//!         MyService { menu, ..default() }
//!     }
//!     fn shutdown(self, world: &mut World) {
//!         if let Some(menu) = self.menu { ImguiManager::unregister(world, menu); }
//!     }
//! }
//! ```
//!
//! An ordinary resource is registered the same way by whoever owns its
//! lifetime (the game's `TelemetryPanels` does this for `Telemetry`).
//!
//! `show_menu` must not change the simulation: push actions to the
//! [`UiOutbox`] (`world.resource_mut::<UiOutbox>().push(|world| ...)`), and
//! they apply once after the pass, before gameplay input, through whatever
//! command path they call (for example `SessionService::command`).
//!
//! # Input
//!
//! Gameplay, global shortcuts and the game's own menus read [`GameInput`]
//! instead of `ButtonInput`, keyboard messages or the wheel. See
//! [`input`] for how presses are owned and interrupted.
//!
//! # Frame
//!
//! In `Update`, before [`service::Phase::Input`]: [`DebugUiSystems::Pass`]
//! (the egui pass, interactive runs only), [`DebugUiSystems::Route`]
//! (`GameInput`), [`DebugUiSystems::Apply`] (the outbox) and
//! [`DebugUiSystems::Shortcuts`] (global shortcuts that read `GameInput`).

mod backend;
pub mod input;
pub mod menu;

use bevy::prelude::*;
pub use bevy_egui::egui;
use service::Phase;

pub use input::{Capture, GameInput};
pub use menu::{Category, ImguiManager, ImguiMenu, MenuHandle, MenuId, MenuSpec, RegisterError, UiOutbox};

/// The debug UI's steps in `Update`, in this order, before gameplay input.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DebugUiSystems {
    /// The egui pass: the menu bar and every open window.
    Pass,
    /// This frame's `GameInput`.
    Route,
    /// The UI's queued actions.
    Apply,
    /// Global shortcuts (they read `GameInput`).
    Shortcuts,
}

/// The overlay's state.
#[derive(Resource, Default, Debug)]
pub struct DebugUi {
    visible: bool,
    backend: bool,
    /// Passes run so far, and passes in a row while hidden.
    passes: u64,
    hidden_passes: u32,
    /// The context's pass number after our last pass.
    last_pass: Option<u64>,
    extra_passes: u64,
}

impl DebugUi {
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Shows or hides the overlay from code (scripts, tests).
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Whether the interactive backend is installed.
    pub fn interactive(&self) -> bool {
        self.backend
    }

    /// UI passes run so far.
    pub fn passes(&self) -> u64 {
        self.passes
    }

    /// Frames in which something else ran a pass on the debug context.
    pub fn extra_passes(&self) -> u64 {
        self.extra_passes
    }
}

/// Something other than the debug UI has a text field focused (the game's
/// own menus): `/` then types instead of toggling. Set it every frame.
#[derive(Resource, Default, Debug)]
pub struct TextFocus {
    pub external: bool,
}

/// Adds routed input and the outbox in every run, and the egui backend in
/// interactive (windowed, player-driven) runs only. Add it after Bevy's
/// window plugins.
pub struct DebugUiPlugin {
    pub interactive: bool,
}

impl Plugin for DebugUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameInput>()
            .init_resource::<input::CaptureSnapshot>()
            .init_resource::<UiOutbox>()
            .init_resource::<TextFocus>()
            .insert_resource(DebugUi { backend: self.interactive, ..default() })
            .configure_sets(
                Update,
                (DebugUiSystems::Pass, DebugUiSystems::Route, DebugUiSystems::Apply, DebugUiSystems::Shortcuts)
                    .chain()
                    .before(Phase::Input),
            )
            .add_systems(Update, input::route.in_set(DebugUiSystems::Route))
            .add_systems(Update, menu::apply_outbox.in_set(DebugUiSystems::Apply));
        if self.interactive {
            backend::install(app);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_runs_before_gameplay_input_without_a_backend() {
        #[derive(Resource, Default)]
        struct Seen(bool);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(DebugUiPlugin { interactive: false })
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Seen>()
            // Registered before anything else, but ordered by its set.
            .add_systems(Update, (|input: Res<GameInput>, mut seen: ResMut<Seen>| seen.0 = input.keys().just_pressed(KeyCode::KeyW)).in_set(Phase::Input));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        app.update();
        assert!(app.world().resource::<Seen>().0);
        assert!(!app.world().resource::<DebugUi>().interactive());
        assert_eq!(app.world().resource::<DebugUi>().passes(), 0);
    }
}
