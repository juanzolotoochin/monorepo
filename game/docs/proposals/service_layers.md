# Service layers

Status: design agreed on 2026-10-24; migration steps 1 to 5 and the main
menu (step 6a) are done, so `game/app` runs on services (see
[As built](#as-built)). Still to come: the lobby (step 6b), `Bindings`,
`ScriptDriver` and `Profile`, and `game/play`.

The game's front end needs one answer to "where does this flow or system
live, and for how long?". This proposal groups front-end code into
**services** that belong to one of three **layers**: App, Session and World.
Each layer has a lifetime, and lower layers always start before and stop after
higher ones, so a service can always rely on the layers below it.

The idea comes from Unreal's subsystems (engine, game instance, world, local
player), adapted to lockstep and to Bevy.

## Why

`game/app` has no notion of lifetime today, so each feature works it out
alone:

- `lockstep::Game` holds state with four different lifetimes in one resource,
  and `build_world` resets eight of its fields by hand on each scene change.
- Four features detect a scene change in their own way: `game.scene` in
  `lockstep`, `lab.scene` in `piles/view`, `flight.current` in `jetpack`, and
  `drawn.world` in `digging`. `assets::switch_scene` despawns boulders and
  captions one by one.
- `build_world` builds the simulation's world from `Obstacle` components that
  `scene::setup` spawned in Bevy, so the simulation's setup depends on the
  view's entities.
- The frame is one long `.chain()` of about 30 systems in `main.rs`, and every
  new feature edits it.

## Scope

Service layers are a front-end concept, for `game/app` and `game/play`.
`game/sim` keeps its own structure (chunks, the six tick stages, settlement
subsystems), stays pure Rust and never sees a service. The deterministic
crates (`fx`, `sim`, `content`, `save`, `net`, `session`) don't change shape;
`session` gains a world slot (see below).

## Layers

| Layer | Lives | Holds | Unreal analogue |
|---|---|---|---|
| App | The whole process | Window, sound, the control port, telemetry, content, the scene library, the player's profile and key bindings, the front-end flow (main menu) | Engine and game instance subsystems |
| Session | From HOST, JOIN or PLAY until leaving | The connection, turns, the clock, the input log, autosaves, the lobby, and the slot that holds the simulation's world | The online session and net driver, but outliving maps |
| World | While the session's world slot is filled | Everything that shows or drives one loaded world: mirrors, views, the camera, turning input into commands, effects, the HUD | World subsystems |

Decisions behind this stack:

- **Program and Game are one layer.** One process runs one game, so they
  would start and stop together. Which code is game-agnostic is decided by
  crates (`play` reuses the lower crates, `app` is the game), not by layers.
- **There is no Local player layer.** There is one player per machine and no
  split-screen. The profile and key bindings outlive worlds and live in App;
  the camera, input and prediction belong to one world and live in World.
- **Session is a layer of its own.** A client's session exists before its
  world (connecting, downloading the save), and it outlives a world: a resync
  reloads the world, and a session can go lobby → world → lobby.

### Flow

```
App      Main menu ──HOST / JOIN / PLAY──┐                ┌── leave / failed
                                         ▼                │
Session            Loading (bind, connect, handshake) ──▶ Lobby ◀──────────┐
                                         │ PLAY            │ start (command) │ return to lobby
                                         ▼                 ▼                 │
World                                  Loading (save → sim world, views) ──▶ Active
```

- The main menu is App flow. Its buttons start a Session in Loading.
- The lobby is a Session state with no World: a screen with the players,
  ready flags and map choice. A walkable hub can come later as just another
  world.
- PLAY (single-player) is a local session that goes from Loading straight
  into a world.
- A resync or a map change makes World exit and enter again; the Session
  stays.
- Today's `--host` and `--join` flags become "start a session on launch".
  The dev keys that change scenes change the world, not the session.

### The session and the simulation's world

- `session::Session` owns a slot for the simulation's world, which may be
  empty. The host has to read the world to upload saves to joiners and to
  compare hashes, so the session holds it. The World layer is active while
  the slot is filled. World services read the world through the session and
  send commands back to it.
- Every world starts from a save: a new map (the host writes a tick-0 save
  and sends it), a join and a resync all use one load path, as
  [`architecture.md`](../architecture.md) already says.
- Ticks restart at 0 for each world. Turns carry a world number, so a late
  packet from the previous world is dropped.
- Lobby actions (ready, pick map, start) are commands on the normal turn
  path, applied to a small deterministic lobby state in the session. Start is
  a command for tick T, so every peer begins loading at the same point. This
  needs no new protocol.

## Services

A service is a Bevy resource, so there is exactly one of each. It belongs to
one layer and declares the services it depends on.

```rust
pub trait Service: Resource + Sized {
    /// App, Session or World.
    type Layer: Layer;
    /// Services this one uses: its own layer or lower, registered earlier.
    type Deps: Deps<Self::Layer>;

    /// Runs when the layer starts, in registration order. Does no slow work.
    fn init(world: &mut World) -> Self;
    /// Runs when the layer stops, in reverse registration order.
    fn shutdown(self, world: &mut World) {}
    /// Polled while the layer is Loading; the layer becomes Active once every
    /// service in it is ready.
    fn ready(&self, world: &World) -> bool { true }
    /// Adds the service's systems, each in a phase.
    fn systems(app: &mut App) {
        app.add_service_systems::<Self, _>(Phase::Mirror, mirror_piles);
    }
}

app.add_plugins(DefaultPlugins)          // before ServicesPlugin, which adds
    .add_service::<SettlementMirror>()  // Bevy's StatesPlugin only if missing
    .add_service::<PilesView>();        // World #2: starts after, ticks after, stops before
```

- **One state per layer.** `AppState` (`Loading`, `Active`) runs for the
  whole process. `SessionState` and `WorldState` (`Off`, `Loading`,
  `Active`) are sub-states: `SessionState` exists once the App is Active,
  `WorldState` once the Session is Active, and `WorldState::Off` is the
  lobby. A layer runs while its state is `Loading` or `Active`; front ends
  change layers with `NextState`, and a layer moves from `Loading` to
  `Active` by itself. A layer starts its services just before its own
  `OnEnter` schedules and stops them just after its `OnExit` schedules, so a
  service exists during its layer's enter and exit systems. Services' systems
  run only while their layer runs. Entities that belong to a world carry
  `WorldScoped` (or `SessionScoped`): the framework despawns them when the
  layer stops, after its services, whether it was Loading or Active. A
  marker that tags a feature's entities can require it
  (`#[require(WorldScoped)]`).
- **`SessionService` moves the World layer.** After the Step phase it sets
  World Off for a frame when the session's world is replaced, and Loading
  once a new one is in the slot; World services decide when it is Active.
  `Flow` (App) starts and leaves the session. Nothing else sets a layer's
  state.
- **Registration order is the order.** Within a layer, services start, tick
  and (in reverse) stop in the order they are registered. A dependency must be
  registered earlier, so there is nothing to sort and no cycle is possible.
- **A service never sees a dependency that hasn't started.** Lower layers
  start first; within a layer, every `init` runs during one state transition,
  before any of the layer's systems tick. Readiness is separate: slow work
  (waiting for the save, building meshes) happens while ticking in Loading,
  and systems that need their dependencies ready run only in Active.

### Checks

- **Compile time:** the layer rule. `Deps<L>` holds only if every listed
  service's layer is `L` or lower, so a Session service naming a World service
  doesn't compile.
- **Startup:** before the first frame, `add_service` checks that each
  dependency was registered earlier, in the same layer or a lower one, and
  panics naming both services otherwise.
- **CI:** a test builds the app headless (the control port's simulation mode
  already runs it that way), so a broken graph fails `bazel test` before it
  merges.

## The frame

Each frame has four phases, chained in `Update`:

```
Input → Step → Mirror → Present
```

Inside each phase, App services run before Session services, which run before
World services, and services in one layer run in registration order. Each
service gets its own system set per phase, and the sets are chained, so
services never run in parallel with one another. Systems inside one service
may still run in parallel. The front end is light; the parallel work is inside
the simulation.

| Phase | Services and work |
|---|---|
| Input | App `Bindings` turns keys and the mouse into actions; App `ControlPort` queues script requests. World `ScriptDriver` pulls those requests, and World `PlayerControl` turns actions into commands (see [Input](#input)). |
| Step | Session `SessionService` runs the network and every tick due, and the lobby. |
| Mirror | World services copy the new ticks into view state: the settlement mirror, piles, digging. |
| Present | World services draw: rigs and animation, effects, the camera, nametags, the HUD. |

Bevy's `First` and `Last` schedules keep the work that must wrap the whole
frame (telemetry, the control port's frame boundaries, frame capture).

Input comes before Step because `Host::queue` stamps a command with
`world.tick() + input_delay` when it's queued, and single-player uses an input
delay of 0. A command made after the step misses any tick due that frame and,
on a client, is sent a frame later.

There is no separate animation phase: within Present, registering the
animation service before the camera and HUD services gives the same order.

Setup work leaves the frame: `assets::switch_scene`, `piles::setup_scene`,
`settlement::setup`, `lockstep::build_world` and the scene-change checks become
World service `init`, which runs during Bevy's state transition between
`PreUpdate` and `Update`.

## Input

Four things travel the input path:

| Thing | Made by | What it is | Example |
|---|---|---|---|
| Device input | Bevy | Raw keyboard and mouse state this frame | F just pressed; right mouse held, cursor at pixel (812, 430) |
| Action | App `Bindings` | What the player means after key bindings, still in screen terms: it can say "at the cursor", not where that is | `Dig`, `MoveToCursor { screen, run }`, `Strike { screen }` |
| Control-port request | App `ControlPort` | A JSON line from a test, a demo script (`app/demos/*.json`) or a tool on a socket | `{"op":"walk","position":[-1.7,-3.4]}`, `{"op":"assert","cargo_units":4}` |
| Command | World `PlayerControl` | A `sim::CommandKind`: the only thing that enters the simulation or goes over the network. Fixed point, world tiles, one avatar | `DigDown { agent }`, `MoveAvatar { agent, intent: GoTo { target, run } }`, `Attack { agent, weapon, aim, muzzle }` |

```
Input phase                                                        Step phase

App      keys, mouse ─▶ Bindings          ControlPort
                           │ actions          │ requests
                           ▼                  ▼
World    PlayerControl·aim                ScriptDriver ──▶ (asserts read mirrors)
         (camera from last frame)             │
                           │ world point      │ world actions
                           ▼                  │
         PlayerControl·act ◀──────────────────┘
         (local feedback now)
                           │ commands
                           ▼
Session  command queue (stamps tick + input delay) ───────────────▶ step
```

- **Bindings** (App) reads devices and the profile's key bindings and
  publishes this frame's actions. Rebinding keys touches only this service.
  It needs nothing from the world.
- **PlayerControl** (World) resolves actions into commands in two steps:
  - *aim* turns a cursor pixel into a world point: a ray from the camera (as
    Present left it last frame) onto the scene's ground (`cursor_surface`).
    Actions that don't point, like `Dig`, pass through.
  - *act* takes world actions (`GoTo(point)`, `Strike(point)`, `Dig`,
    `Equip(weapon)`), finds this player's avatar, checks what the front end
    can check (diggable terrain, the weapon is in the scenario, re-plan
    throttling), converts to fixed point with `to_sim`, and queues the
    command with the session. It also starts local feedback right away, such
    as the swing animation of a strike.
  - Making commands needs the camera, the avatar, the arena center and the
    scene's style, so it's a World service and not App or Session.
- **ControlPort** (App) owns the socket and `--demo-script`, parses requests
  and queues them. App can't call World services, so it only publishes.
- **ScriptDriver** (World) pulls requests every Input phase:
  - Player-input ops (`walk`, `strike`, `grab`, `dig`, `jetpack`, `equip`)
    become world actions for *act*. Scripts get the same checks and
    feedback as a player, skip *aim* (they already give world points), and
    so work under `--simulation`, which has no window or camera.
  - Asserts and inspection read the mirrors and answer through the control
    port. Nothing reaches the simulation.
  - Session ops (`pause`, `rate`) go to `SessionService`.
  - It answers a request once it is done, for example when the avatar
    arrives (today's `Pending::Walk`).

Input runs before Step because a command is stamped with its tick when it's
queued. Run after the step, a command misses any tick due that frame and lands
one tick (16.7 ms) later, which happens on about 4 frames in 10 at 144 Hz.

## As built

Services in `game/app`, in registration order (which is also their tick
order within a layer):

| Layer | Service | File | What it owns |
|---|---|---|---|
| App | `Flow` | `flow.rs` | Opens and leaves sessions (from the command line or the menu) and keeps why the last one ended. Escape (without menus) and mute keys |
| App | `MainMenu` | `menu.rs` | The main menu, while Session is Off: name, Play or Host on a scene, Join, Quit. Players' runs only |
| Session | `Pause` | `pause.rs` | The pause overlay (Escape): Resume, Leave to menu, Quit. Takes the session's input while open. Players' runs only |
| Session | `SessionService` | `lockstep.rs` | The session, clock, scene requests (digit keys, control port), recordings, the ending banner; moves the World layer |
| World | `WorldMirror` | `lockstep.rs` | The last two ticks for interpolation, and the world's scene from the session's label |
| World | `SceneView` | `scene_view.rs` | The scene shown: player body, industrial props, boulders, style, the deck's visibility |
| World | `Settlement` | `settlement.rs` | The settlement's rigs, captions and HUD |
| World | `PileLab` | `piles/` | The pile lab's dispensers, piles, cargo and panel |
| World | `Avatar` | `gameplay.rs` | The player's body: following the avatar, animation, swings and hits |
| World | `Peers` | `peers.rs` | Other players' avatars and name tags |
| World | `Digging` | `digging.rs` | The dug ground and the follow camera |
| World | `Flight` | `jetpack.rs` | Raised platforms and the jetpack's view |
| World | `Memory` | `palimpsest/` | The memory trial's view |
| World | `Effects` | `effects.rs` | Particles, flashes, camera shake |
| World | `Hud` | `scene.rs` | The HUD's live readouts |
| World | `PlayerControl` | `lockstep.rs` | Movement, attack and keyboard input into commands |

**Players' runs and scripted runs.** `Options::menus` is true for a run with a
window and no demo script, frame recording or control port. Those runs get
`MainMenu` and `Pause`: with no session flags (`--host`, `--join`, `--scene`,
`--replay-input`) the game opens on the menu, and a session that ends or
can't start returns to it with the reason. Every other run starts its
session from the flags, keeps the closing banner, and exits on a failed
start, so demos, replays and render checks are unchanged.

Still plain App resources: `SceneLibrary` (the packs), `ModelCache`,
`SceneArt` (shared meshes and materials), `Telemetry`, `Sound`, the control
port's `Hub`, and `Controls` and `Feedback`, which the player's input,
combat and effects share and `SceneView` resets when a world starts.
Telemetry and the control port wrap the frame in `First` and `Last`.

The control port starts commands, answers inspections and writes its trace
only while a world is up, and holds the session's clock until the first
world, so a rendered replay of a headless recording still matches it
command for command. Every scene renders as before.

## Migration

Each step keeps the game and its demos working, and the demo replays keep
passing.

1. Add the framework: layers, states, `Service`, `add_service`, phases and
   the checks, in a new Bevy crate (`game/service`) that `app` and `play` both
   use.
2. Put today's systems into the four phases, without changing their order.
3. Build the world from scene data instead of from `Obstacle` entities, so a
   World can start before its views exist. Done: `app/src/obstacles.rs` lists
   each scene's discs in a fixed order (architecture, boulders, industrial
   props, pile fixtures, memory wall) from the pack and constants the views
   share. Boulder radii come from the generated models in `ModelCache`, so
   the scene's models must be loaded first. The terrain hash changed once
   (order only); the discs matched today's in all 10 scenes.
4. Give `session::Session` its world slot and world number, and change scenes
   by reloading the slot instead of starting a new session. Done:
   `Session::load_world` (host) sends every client a save of the new world,
   like a join, with the app's label (the scene id) in `Welcome`; the host
   holds tick 0 until every client has loaded it. Packets carry the world
   number next to the epoch (protocol version 2). The app starts its session
   once, on the first scene, and loads later scenes into it; the host may
   switch scenes in networked games and clients follow the label. Each
   world gets its own recording: `<path>` for the first, `<path>.<n>` after.
   The host's slot is never empty yet: unloading comes with the lobby.
5. Split `lockstep::Game`, then move the features into World services one at
   a time, deleting each hand-made scene-change check. Done: see
   [As built](#as-built). Seven checks went (`game.scene`, `Settlement`,
   `ActiveStyle`, `Flight`, `Memory`, `PileLab` and digging's `Local`), and
   so did `SceneLibrary.current` and `switch_scene`.
6. Build the main menu and the lobby on top.
   - 6a, the main menu. Done: `MainMenu`, `Pause`, and `Flow` opening
     sessions itself (`lockstep::open`), so a failed bind is a message, not
     a panic. Host and Join go straight into a world; the menu is bevy_ui
     with a small hand-written text field (Bevy's `EditableText` needs the
     `bevy_ui_widgets` feature, which the build leaves out). The menu's
     default port is 7000. Nothing is remembered between runs until
     `Profile`.
   - 6b, the lobby: Host and Join land in it, the host's slot starts empty.

## Open questions

- When `game/play` moves over: with step 1, or once `app` has proved the
  framework.
