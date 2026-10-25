# debug_ui

A debug overlay for the game, drawn with [egui](https://github.com/emilk/egui)
through [bevy_egui](https://github.com/vladbat00/bevy_egui) 0.43.1 (features
`render`, `bevy_ui`, `default_fonts`). The names `ImguiManager` and
`ImguiMenu` predate the choice of egui.

The overlay starts hidden. `/` shows or hides the menu bar and every open
window together; hiding keeps which windows were open. Each category of the
menu bar is a dropdown of entries, and each entry opens its own window. A
debug text field that has the keyboard types `/` instead, and Escape there
closes the overlay (without also pausing or quitting). So does a focused
text field of the game's own menus (`TextFocus`).

The backend is installed only in windowed runs a player drives
(`DebugUiPlugin { interactive: true }`); simulation and offscreen capture
runs get input routing only, with nothing captured.

## Adding windows

A provider is any resource implementing `ImguiMenu`. It declares its
entries (stable ID, category, name) and draws each open one, on every UI
pass, including while the window is collapsed:

```rust
use debug_ui::{egui, ImguiManager, ImguiMenu, MenuHandle, MenuId, MenuSpec, UiOutbox};

const MENUS: &[MenuSpec] = &[
    MenuSpec::new("routes", "World", "Routes"),
    MenuSpec::new("pathing", "World", "Pathing costs"),
];

impl ImguiMenu for Navigation {
    fn menus() -> &'static [MenuSpec] {
        MENUS
    }

    fn show_menu(&mut self, id: MenuId, root: &mut egui::Ui, open: &mut bool, world: &mut World) {
        // The window's identity comes from the provider and entry ID; its
        // close button is bound to `open`.
        id.window().open(open).show(root.ctx(), |ui| match id.id() {
            "routes" => {
                if ui.button("Clear routes").clicked() {
                    world.resource_mut::<UiOutbox>().push(|world| {
                        // Applied once, after the pass and before gameplay
                        // input, through the normal command path.
                        world.resource_mut::<SessionService>().command(/* ... */);
                    });
                }
            }
            _ => { ui.label("..."); }
        });
    }
}
```

Rules for `show_menu`:

- It creates its own window and binds the close control to `open`.
- It may run more than once a frame (egui can rerun a pass), so it must not
  act directly: changes go to the `UiOutbox`, whose actions apply once after
  the pass. Never step the simulation or change deterministic state here.
- While it runs, the provider is `self` and is not in `world`.
- It reads the latest completed frame's state (a window drawn before the
  Step phase shows the previous frame's numbers).

### Registration follows its owner's lifetime

A service registers itself in `init` and unregisters in `shutdown`. List
`ImguiManager` in its `Deps`; the manager is an App service registered
before every provider, so services of any layer may use it:

```rust
impl Service for Navigation {
    type Layer = WorldLayer;
    type Deps = (ImguiManager, SessionService);

    fn init(world: &mut World) -> Self {
        let menu = ImguiManager::register::<Navigation>(world).map_err(|e| warn!("{e}")).ok();
        Navigation { menu, ..default() }
    }

    fn shutdown(self, world: &mut World) {
        if let Some(menu) = self.menu {
            ImguiManager::unregister(world, menu);
        }
    }
}
```

An ordinary resource that isn't a service is registered the same way by
whatever owns its lifetime: the game's `TelemetryPanels` App service
registers the `Telemetry` resource for **Diagnostics → Frame timing** and
**Diagnostics → Counters and gauges**.

`register` rejects a provider registered twice, an entry ID declared twice
by one provider, two entries with the same category and name, and empty
labels. Unregistering forgets which windows were open and drops empty
categories; a handle from an earlier registration removes nothing. A
provider that leaves the world without unregistering is never called: its
entries are dropped the next time the overlay draws or anything registers.

## Input

Gameplay, global shortcuts and the game's own menus read `GameInput`, not
`ButtonInput`, keyboard messages or the wheel:

```rust
fn jump(input: Res<GameInput>) {
    if input.keys().just_pressed(KeyCode::Space) { /* ... */ }
}
```

While visible the overlay captures keyboard and pointer independently:
keyboard when a debug widget has keyboard focus, pointer when it is over a
debug window or menu, using it, or has a popup open. Showing the bar alone
captures nothing.

Every button is owned from press to release, by gameplay or by the UI. A UI
press never becomes a gameplay press or hold, even after the capture ends
or the overlay hides. When something interrupts a gameplay hold (keyboard
capture, the UI taking the pointer mid-drag, the pause overlay through
`GameInput::block`, the window losing focus), gameplay sees one release, so
its normal path ends the action (walking stops, the jetpack switches off),
and the button stays off until it is released and pressed again. Hovering a
window during a gameplay drag takes nothing over. Destinations, attacks and
script or control-port commands already issued are not touched.
`GameInput::focus_lost` says the window lost focus this frame; the game's
input service uses it to pause and release the avatar (see
[controller support](../docs/controller-support.md)).

Clicks and hovers over the overlay don't reach Bevy UI buttons beneath it.

## Frame

```text
First       telemetry timing, scripts and the control port
PreUpdate   Bevy input -> bevy_egui input translation -> `/`, Escape, hidden-input cleanup
Update      DebugUiSystems::Pass      the one egui pass (menu bar and open windows)
            DebugUiSystems::Route     GameInput, Bevy UI click-through guard
            DebugUiSystems::Apply     the UI outbox
            DebugUiSystems::Shortcuts global shortcuts (F3)
            Phase::Input -> Step -> Mirror -> Present
PostUpdate  bevy_egui output processing and rendering
```

The context runs manually (`EguiContextSettings::run_manually`), so
bevy_egui's own PostUpdate pass skips it; `DebugUi::extra_passes` counts any
other pass. bevy_egui's input absorber stays off: it clears Bevy's input
resources, which routing leaves untouched. While hidden, the pass is empty
(no providers, no capture, no paint), the context only learns of releases,
and bevy_egui stops setting the cursor icon and IME after restoring them.

## Tests

`bazel test //kestrel/debug_ui:debug_ui_test` covers ownership, interruption
and registry rules; `//kestrel/app:movement_test` checks held walking and
jetpack interruption and scripts against the game's command path.
