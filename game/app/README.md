# Kestrel // 07

A voxel sci-fi action prototype in Bevy 0.18.1, built with Bazel. An armored
salvager carries an industrial cleaver across a suspended mining deck. Cuboid
geometry, materials, effects and sound are generated locally; no art downloads
or runtime asset paths are required.

Asset provenance: models and effects are authored in this example's Rust code
and RON recipes;
the swing, clang and footsteps are original procedural synthesis in `sound.c`
(noise, low tones and inharmonic ringing modes). Miniaudio provides playback,
not sound samples. The HUD uses Bevy's bundled default font.

```sh
bazel run //game/app:demo
bazel test //game/app:movement_test
```

Run without flags, the game opens on its main menu: type a name, then Play
or Host on the scene shown (`<` and `>` pick another), or Join an address.
Enter in the port or address field hosts or joins. `--host`, `--join`,
`--scene` and `--replay-input` skip the menu and start that session at once;
runs driven by a demo script or the control port never show it.

To cross-compile a Windows x64 release from Linux or macOS, use the MSVC
target platform. The compiler and build scripts run on the host; Rust and the
native audio code target Windows through hermetic Clang/LLD, the Microsoft
runtime and Windows SDK. No host Visual Studio installation is needed.

The upstream toolchain requires explicit acceptance of both Microsoft licenses.
After accepting the [runtime and SDK license terms](https://github.com/hermeticbuild/hermetic-llvm/tree/v0.8.24#windows), run:

```sh
bazel build -c opt \
  --platforms=//platforms:rust_windows_x86_64 \
  --repo_env=BAZEL_MSVC_RUNTIME_VISUAL_STUDIO_EULA=1 \
  --repo_env=BAZEL_WINDOWS_SDK_EULA=1 \
  //game/app:demo
```

Copy `bazel-bin/game/app/demo.exe` to Windows. All scene
assets and sound synthesis are embedded in the executable. The default MSVC
configuration links the runtime dynamically: install the current
[Visual C++ v14 x64 Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist)
if `VCRUNTIME140.dll` is unavailable. A compatible GPU driver is also needed.
Cross-compilation verifies the build; rendering, input and audio still need a
smoke test on Windows. Use `bazel build`, rather than `bazel run` or `bazel test`,
for Windows targets on a Linux/macOS host. To compile the headless test executable,
use the same flags with `//game/app:movement_test`.

Windows integration notes: BLAKE3 uses intrinsics to avoid the host-only MASM
assembler; Bazel drives the MSVC link action and supplies the windowing/pipe
SDK libraries. `Cargo.lock` intentionally resolves `gpu-allocator`'s broad
Windows API dependency to `windows 0.58.0`, matching `wgpu-hal`'s DirectX types.
Other dependencies still use `windows 0.57.0`; preserve that separate resolution
when updating the lockfile.

Audio initialization and shutdown run on a dedicated thread. On Windows this
keeps miniaudio's multithreaded COM apartment separate from winit's window-thread
OLE apartment. The Windows test executable includes a regression for this
startup sequence; run it on Windows with:

```powershell
.\movement_test.exe sound::tests::audio_startup_leaves_the_window_thread_compatible_with_ole --exact --nocapture
```

| Control | Action |
| --- | --- |
| 1 / 2 | Load the mining deck / boulder lab |
| Right click / hold | Move / steer toward the cursor; find a route around obstacles |
| WASD / arrows | Camera-relative movement; cancel the click route |
| Shift | Run |
| Shift + left click | Face the cursor and commit to one heavy sword strike |
| Mouse wheel | Smooth camera zoom |
| R | Reset character position and actions |
| M | Mute / unmute |
| Esc | Pause overlay: Resume, Leave to menu, Quit (the world keeps running). Quits in scripted runs |

Move within about two world units of a rock and strike toward it. The blade
has a deliberate wind-up, fast cut and long recovery. Contact stops the blade,
briefly freezes the character, then kicks the sword back. Rock strikes create
hot voxel sparks, bouncing chips, a brief light, a small camera kick and a
procedural metallic clang. One cut produces at most one contact. Click again
near the end of recovery to buffer the next strike. The HUD shows the actual
weapon state and contact count. Rocks and props are solid, indestructible
surfaces; there are no enemies, health or loot yet.

Movement uses acceleration, braking, smoothed facing, animated legs and
footsteps. Click routes use a half-unit A* grid with body clearance, diagonal
collision checks and line-of-sight smoothing. Local collision uses substeps
and sliding. Attack collision sweeps the blade through angle space, so a fast
cut cannot skip a rock between rendered frames. Impact particles and lights
expire automatically.

The lighting combines warm shadows, cool fill, ambient occlusion, temporal
antialiasing and restrained bloom. Run on a desktop with a working Vulkan
(Linux) or Metal (macOS) driver. Linux uses X11 / XWayland and a PulseAudio or
PipeWire audio service; audio is optional if no output device is available.
Audio uses the repository's pinned miniaudio, without host development headers.

For a repeatable approach-and-strike preview with rendered screenshots:

```sh
bazel run //game/app:demo -- \
  --showcase --capture-dir /tmp/kestrel
```

This writes `overview.png` and `impact.png`, then exits. `--showcase` alone
plays the short sequence and leaves the window open for further interaction.

Code is split into `assets.rs` (compiled scenes and prefab caching),
`scene.rs` (shared deck, character art and HUD), `gameplay.rs` (controls and
combat), `navigation.rs` (paths and clearance), `effects.rs` (impact feedback),
`sound.rs` / `sound.c` (procedural foley), and `preview.rs` (visual QA).
Headless tests cover obstacle routing, slide collision, blade reach and sweep
consistency, attack timing, one-contact recoil, hit-stop and effect cleanup.

Third-party versions come from the root `Cargo.toml` / `Cargo.lock`.
The targets expose that manifest as a compile-time input because Bevy's
derive macros read it to find the `bevy` crate path. First-party code builds
with Bazel, following the other Rust examples in the repository.


Boulders now use the [Bazel voxel asset pipeline](../build/voxel_assets/README.md).
Edit recipes in `assets/prefabs/`, material values in `assets/palette.ron`, and
placements in `assets/levels/`. `assets/BUILD.bazel` declares reusable prefab
and scene targets. The generator loops live in `//game/tools/assetc/core`; changing
a recipe rebuilds its mesh during the Bazel build. The deck's other props and
character parts are still authored in `scene.rs`.

Press **1** or **2** to load the mining deck or boulder lab. Both use the same
deck stage; the lab hides the industrial dressing. Baked assets are shared
between the two layouts. The lab also shows runtime seed/size overrides and
repeated instances that reuse one generated mesh. The HUD's `DYNAMIC BUILDS`
count stays unchanged on repeat visits because geometry is cached. Scene
changes reset movement, attacks and contact feedback.

```sh
bazel run //game/app:demo -- --scene boulder-lab
bazel build //game/app/assets:ore_small
```

For visual QA of either scene, add `--scene mining-deck` or `--scene boulder-lab`
to the existing `--showcase --capture-dir ...` command. All packs are embedded,
including in the Windows `.exe`.

The lab includes labeled **metal**, **crystal**, **polymer**, and **mixed** samples.
Material palettes (`assets/{metal,crystal,polymer,mixed}.ron`) assign an `impact`
response independently of their visual color, emission, and roughness. Stone
chips, metal throws hot sparks, crystal pulses blue and chimes, and polymer
absorbs the blow with a muffled knock and greater displacement.

The mixed sample uses four material quadrants: strike different sides to try
all four reactions on one asset. The HUD reports the last response under `HIT`.
Existing ore boulders also mix stone and metal
reactions. Exact sword-segment/mesh contact selects the struck material after
broad-phase collision checks. Only that material slot on that instance flashes;
authored emission is restored afterward. Meshes and CPU contact triangles are
cached; reactive material handles belong to each instance and are removed when
switching scenes. The contact query currently scans nearby targets' triangles;
it has no spatial acceleration structure for very large models.

Press **3** or use `--scene pile-lab` for mineral logistics. Click either
world dispenser or its button to create 5 units in its output pile. Left click
a pile to walk over and collect up to **12 units**. While carrying, left click
open floor to walk there and create a pile, or a matching pile to deposit into
it. **Ctrl + left click** collects more into a partial load. Different minerals
stay separate; full destinations leave excess cargo on the player. Right click,
WASD, or a sword strike cancels a hauling job without losing carried material.

Every mineral chunk represents **one unit**, both on the ground and in the
player's backpack. Labels give exact pile counts, and the logistics HUD shows
cargo, 12 capacity indicators, ground stock, and total created units. The scene
starts with 8, 24, and 64-unit samples. Piles cap at 96 units; the scene supports
32 piles. Transfers conserve quantities; only dispensers create new stock.
Returning to this demonstration scene resets its stock and fixtures.

Fixtures and starting quantities are authored in `assets/levels/pile_lab.ron`
and validated during the Bazel scene-pack build. `pile_model.rs` owns pure
quantity bookkeeping; `piles/mod.rs` owns interaction and movement jobs;
`piles/view.rs` owns shared visual assets and quantity displays. Pile and cargo
views rebuild when quantities or airborne slots change. Shader assets and chunk meshes
are shared rather than generated per unit.

```sh
bazel run //game/app:demo -- --scene pile-lab
# Scripted real navigation + pickup + drop, with captures and quantity assertions:
bazel run //game/app:demo -- \
  --scene pile-lab --haul-showcase --capture-dir /tmp/kestrel-piles
```

The [controlled-demo rule and command protocol](../build/game_demo/README.md)
provide MP4 generation, scripted assertions, renderer-free tests, and live JSON
world inspection. Generate the four-unit hauling demonstration with:

```sh
bazel build --config=full_local //game/app:haul_four_demo
```

Any run can be recorded and replayed exactly. `--record-input run.glog` writes
the input log on exit (and the starting save as `run.glog.gsav`);
`--replay-input run.glog` plays it back in the same scene:

```sh
bazel run //game/app:demo -- --scene forest-outpost --record-input /tmp/run.glog
bazel run //game/app:demo -- --scene forest-outpost --replay-input /tmp/run.glog
```

The output video, final-state report, and per-frame state trace are alongside
`demo` in `bazel-bin/game/app/`.

Controlled transfers can specify `seconds_per_unit` and an `approach` standing
point. The example video uses a side approach and 0.7 seconds per unit; chunks
spin through a high arc into the backpack and back onto the floor. Airborne
chunks replace their resting representation until landing, so the visual count
continues to match conserved stock. The MP4 includes procedural footsteps,
dispenser beeps, and synchronized toss/landing foley; its `.wav` sidecar uses the
same sound bank as interactive playback.

## Jetpack and elevated terrain

Press **4** for the jetpack lab, or launch it directly:

```sh
bazel run --config=full_local //game/app:demo -- --scene jetpack-lab
```

Hold **Space** to hover 1.8 meters above the terrain; steer with WASD or right
click a destination. The controller looks ahead and climbs toward taller
platforms before reaching their walls. Swept body clearance stops horizontal
movement if a wall still needs more altitude. Ground navigation rejects steep
steps and gaps. Release Space to descend onto a platform. Above a chasm, the
jetpack keeps the last supported hover altitude until you reach a safe landing
surface. There is currently no fuel limit. **R** resets to the current scene's
spawn, and switching scenes resets flight.

The lab has four islands at 0, 1, 2, and 3.5 meters, including a contiguous
vertical step and an open chasm. Its optional `terrain` layout is authored in
`assets/levels/jetpack_lab.ron` and validated by the existing scene-pack Bazel
rule. Rectangular platforms define their bounds and top height; overlapping
platforms use the highest top, and uncovered locations are void. Other scenes
retain their flat deck. Terrain collision and navigation use the same authored
surfaces as rendering. Existing prop obstacles still use their planar clearance.

Generate the audiovisual ascent, gap-crossing, safety-hover, and landing demo:

```sh
bazel build --config=full_local //game/app:jetpack_demo
```

Outputs are `jetpack_demo.mp4`, `.state.json`, `.frames.jsonl`, and `.wav` in
`bazel-bin/game/app/`. The script uses the same flight
controller as Space, and asserts both hover and landed altitudes. Thruster
plumes, altitude telemetry, and procedural turbine audio are included.

Live/script commands `{"op":"jetpack","enabled":true}` and
`{"op":"jetpack","enabled":false}` request flight and landing. The existing
`walk` command plans a flight route while airborne. `assert_flight` checks
`airborne` and `altitude`, with an optional `tolerance` (default 0.05 meters).
World inspection includes authored `terrain` and `player.jetpack` fields for
requested flight, airborne status, support height, hover height, and clearance.

## Dig lab

Press **9** for the dig lab, or launch it directly:

```sh
bazel run --config=full_local //game/app:demo -- --scene dig-lab
```

The lab is an open-pit mine: 256 × 256 m of generated ground (`mine` in
`assets/levels/dig_lab.ron`: its size and seed). Terraces rise a walkable
quarter meter at a time, mesas a meter at once (fly up), and a round landing
around the spawn is flat. The simulation generates the same ground on every
machine from the seed (`game/sim/src/mine.rs`); only the seed and the pits are
state. The camera follows you over it.

Press **F** to dig out the tile you stand on (**X** picks the spear in combat
scenes). A dig takes 2 seconds standing still, stepping to the middle of the
tile, crouched and chopping into the ground with your tool (a strike every
half second, with a puff of dust), with a bar over your head; walking off,
attacking or the jetpack stops it. Each dig takes out one 0.5 m layer, down to 3 layers (1.5 m). Pit walls are
too steep to walk into or out of, and routes go around pits: hold **Space** to
fly out.

Each layer may hold ore: **amber ore** and **blue crystal**, richer deeper
down, and rarer **green malachite** below the top layer. Ore shows as a tint on
the ground and in pit walls. Digging out an ore layer throws 3 to 6 units onto a
pile beside the pit (or onto a pile of the same ore already there). Collect it
as in the pile lab (left click a pile; you can reach it from the pit), fly out,
and left click the **collector** by the landing to deliver; it takes every
mineral and counts each. Up to 512 piles; no pile or machine can be dug under.

The pits are part of the simulation, so in a networked game everyone digs the
same ground (`--scene dig-lab` on every machine). Every player has their own
carrier and haul (in the pile lab too): two players can collect and deliver at
once, each sees the other's backpack fill, and tosses fly to and from whoever
made them. The logistics panel shows your own load.

The control port's `{"op":"dig"}` digs and `{"op":"deliver"}` delivers the
cargo. Inspection's `digging` lists the pits by arena tile, whether the player
is digging, the collector's counts by mineral, and the nearest ore within 8 m.

## Field trial 05

A new mechanic has a dedicated scene with instructions inside the game. Press
**5** to visit it, or start directly:

```sh
bazel run --config=full_local //game/app:demo -- --scene surprise-lab
```

The scene explains its own controls and objective. **R** restarts the trial.
To generate its scripted audiovisual playthrough and state trace:

```sh
bazel build --config=full_local //game/app:surprise_demo
```

The outputs are `surprise_demo.mp4`, `.state.json`, `.frames.jsonl`, and `.wav`
in `bazel-bin/game/app/`.

<details>
<summary>Mechanic and implementation notes (spoilers)</summary>

**Echo Anchor:** press E to record up to eight seconds of movement. Press E
again to return to the recording's anchor and project a holographic partner
that repeats the recorded route and holds its final pose. Q dismisses it.
Reaching eight seconds automatically deploys the echo. A new recording replaces
the previous one. Recording and playback work in every scene; the trial adds
receivers that can be operated by either the player or the projection.

The trial asks you to power two distant plates together for 1.2 seconds. An
echo can hold one plate while you stand on the other. The lock then latches
open, allowing you to recover the core. The echo has no cargo, collectible,
combat, or collision components: only spatial motion is recorded, and the
world and its inventory are never rewound or duplicated. Recorded height is
preserved, so a projection flying above a plate does not activate it.

`src/palimpsest/mod.rs` owns recording, interpolation, replay, plate logic, and
state inspection; `view.rs` owns scene geometry, projection visuals, UI, and
lock animation. Shared handles survive scene changes. Scene changes and R clear
the projection and trial state. Existing live/script controls gain
`memory_record`, `memory_deploy`, `memory_clear`, and `assert_trial`.

</details>

## Styles and the Tidal Aetherworks

Press **6** for a dark Atlantis / magic / steampunk scene, or launch it directly:

```sh
bazel run --config=full_local //game/app:demo -- --scene atlantis
bazel build --config=full_local //game/app:atlantis_demo
```

The scene uses deep teal stone, aged bronze, water channels, a tide gate,
clockwork machinery, irregular pavement wear, broken inlays, sea plants, coral,
and workshop salvage. A detailed diving suit and aether trident replace the
industrial character meshes. A visible helmet lamp illuminates the nearby path
and follows the character. Other lamps and crystals have real local lights;
there is no invisible sun casting this scene's shadows.

Styles are data in `assets/styles/atlantis.ron`. A profile supplies named
materials, physical voxel size, optional articulated actor/weapon recipes,
lighting, camera exposure, fog, bloom, and shadow/contact-shading settings.
Atlantis uses **10 voxels/m** for the environment and **20 voxels/m** for the
suit and trident; the original style's nominal density is about **3.3 voxels/m**.
Voxel density controls geometry detail in world units, independently of screen
resolution. Existing hand-authored meshes retain their topology unless the
profile supplies replacements.

Both architectural and character geometry bake during Bazel asset compilation.
Hidden faces are removed and coplanar faces merge into rectangles. Character
parts attach to existing animation pivots, preserving movement and combat.
Shared meshes/materials are cached across visits; switching back restores the
original character, palette, lighting, and shadow settings. Styled boulders use
the same profile's grid and material roles and retain per-material hit reactions.

To add another setting, author a style RON file, optionally an architectural
layout, and pass `style = "styles/your_style.ron"` to `scene_assets` and opted-in
`boulder_prefab` targets. No new renderer branch for the style is necessary.
See the [asset rule documentation](../build/voxel_assets/README.md).
The controlled-demo report and frame trace expose style density, actor parts,
weapon identity, baked statistics, fixture lights, and moving actor lights.

## Brasswood: a living supply chain

Press **7**, or launch the outdoor medieval / forest / steampunk style directly:

```sh
bazel run --config=full_local //game/app:demo -- --scene forest-outpost
```

Smaller actors and a wider camera show several workers operating at once.
Foresters deliver lumber, the ironmonger supplies metal, and the fletching mill
turns one lumber plus one iron into four arrows. A runner resupplies the
ballista, which spends actual arrows protecting the camp against ghoul waves.
The cartworks consumes two lumber and one iron to build a cart. Bramble the ox
collects and attaches it, increasing carrying capacity from three to nine units.
Every ground item, carried item, and airborne transfer represents one real unit.

The player remains active in this scene. **RMB / WASD** moves, **LMB** on stock
queues collection of up to four units, and **Ctrl+LMB** on a depot deposits the
first carried material. **E** toggles a nearby machine, **P** pauses automation,
and **Shift+LMB** attacks ghouls with the longsword. Manual movement cancels pending
player transfers; already airborne items still arrive. The player shares stock,
capacity reservations, and production inputs with the workers, so diverting
supplies changes how the settlement runs.

The architecture is split into independent layers:

- `//game/sim` owns the deterministic 60 Hz decisions, quantities,
  reservations, processing, equipment, the player's movement, threats and
  defense. The app runs it through `//game/session` and only sends commands;
  see [the architecture](../docs/architecture.md).
- `assets/levels/forest_supply.ron` defines the supply chain and agent abilities.
  `scene_assets(scenario = ...)` validates and embeds it at build time.
- `assets/styles/forest.ron` defines the palette, smaller actor scale, articulated
  workers, animals, carts, machines, turret and item skins. Generic biped,
  quadruped, wheel, rotor and aiming animations come from skin metadata.
- `src/settlement.rs` projects model state into cached rigs, quantity displays,
  airborne arcs, captions and audio. It does not decide who hauls what.

Generate an audiovisual tour that also demonstrates the player helping the mill:

```sh
bazel build --config=full_local //game/app:forest_outpost_demo
```

Outputs are `forest_outpost_demo.mp4`, `.state.json`, `.frames.jsonl`, and `.wav`
in `bazel-bin/game/app/`. The script asserts delivery,
production, cart attachment, defense, and material conservation.

The existing JSON/TCP interface also exposes `settlement_transfer` (`depot`,
`item`, `units`, `deposit`), `settlement_cancel`, `settlement_pause` (`paused`),
`settlement_rate` (`rate`, 0.25–4), `settlement_link`, `settlement_machine`, and
`settlement_defender` (each `id`, `enabled`), and `settlement_stock` (`depot`,
`item`, `units`). Transfers walk the player to the dock and wait for the precise
quantity to arrive. `assert_settlement` checks minimum `delivered`, `cycles`,
`equipped`, and `kills` counts plus all accounting invariants. `inspect` includes
full `settlement.simulation`, scenario data, and per-material `balances`.

## Red Clearing combat arena

Press **8** or start the focused forest combat scene directly:

```sh
bazel run --config=full_local //game/app:demo -- --scene combat-arena
```

Right click to position the player, then Shift + left click toward an approaching
ghoul. The faster longsword stops on contact. A short hit pause, camera jolt,
growl, and voxel blood burst mark the hit; a surviving ghoul recoils before
limping forward. The same 60 Hz simulation owns health, knockback, kills, and
the melee metrics in the world-state dump.

```sh
bazel test --config=full_local //game/app:combat_arena_test
bazel build --config=full_local //game/app:combat_arena_demo
```

The demo produces an MP4 with audio, a WAV, a final state report, and a per-frame
trace. Its assertions check hits, a kill, and actual knockback displacement.

## Multiplayer

One machine hosts and others join over UDP. Each player who joins gets an
avatar of their own beside the spawn point and drives it with the usual
controls. Other players wear the same exosuit, dressed by the scene's style,
and walk, idle, fly and swing with the same animation, holding the weapon
they have picked, with their footsteps and swings audible.

```sh
# The host listens on a UDP port:
bazel run --config=full_local //game/app:demo -- --host 7777
# Others join by address or machine name (a Tailscale name works):
bazel run --config=full_local //game/app:demo -- --join my-desktop:7777
```

A name tag floats over every player (yours in cyan): `--name Ada` sets
yours, otherwise it's who is logged in to the computer. Names travel
through the simulation, so everyone sees the same ones.

Networked games start on the flat `field` scene (also on key **0** when
playing alone). `--scene` picks another one; every player must pass the same
scene. Scene keys are disabled while networked. The HUD's top right shows the
connection. Leaving and rejoining gives a new avatar; the old one stays put.
If the host leaves (or crashes: after 5 seconds without hearing from it), or
turns a player away, or falls out of sync with them, that player goes back to
the main menu, which says why; there's no host migration. Runs driven by a
script or the control port show why and close 4 seconds later instead. The
menu's Host and Join do the same as `--host` and `--join`.

To defend the Red Clearing together, both pass `--scene combat-arena`:

```sh
bazel run --config=full_local //game/app:demo -- --host 7777 --scene combat-arena
bazel run --config=full_local //game/app:demo -- --join my-desktop:7777 --scene combat-arena
```

Everything in the single-player arena works for both players: each picks
their own weapon (Z/X/C/V) and fights the three ghoul lanes; blood, sounds,
deflections, decapitations, bolts and arc chains show for everyone's hits,
while the hit pause, rebound and camera shake are only for your own. Either
player can carry bolts from the rack, switch the ballista off and on (shared,
like the base's health and the kill count), and respawn with **R**. Networked
games can't pause or change speed.

The host's world is the only real one: clients load it from the host and then
run the same 60 Hz simulation on the same turns, so commands apply on every
machine after the input delay (4 ticks). `//game/session:session_test` runs
two players over localhost UDP, on the field and fighting the arena's ghouls,
and checks that both machines simulate the same world.

## Performance telemetry

The top right of the window shows frames per second. **F3** adds the
breakdown: CPU time in Update against rendering and waiting, the session's
update per frame and how many ticks it ran, and each world tick split into its
stages (`commands`, `self update`, `read`, `merge`, `apply`, `housekeeping`,
`hash`, from `sim::World::stage_times`) plus copying the settlement for the
view. Each line is the mean / p95 in milliseconds over the last 240 samples.

Live inspection, script receipts,
and demo frame traces include a versioned `telemetry` object. Timing histories
are bounded to 240 samples; counters and all-time maxima accumulate for the run.
Timings expose last, mean, p50, p95, p99 and maximum milliseconds. CPU wall time
is separate from game time: recorded frame pacing and GPU readback are included
in `frame_wall`; `session_update` and the `tick_*` samples isolate the
simulation, and `settlement_projection` and `settlement_sync` presentation
preparation and ECS work.

Gauges include entity/mesh/material counts, asset-cache entries, active and
pooled rigs, and fixed steps per frame. Counters expose rig creation, reuse,
pooling, destruction, transform writes and avoided writes, label changes,
clamped simulation frames, and frames over 16.7 / 33.3 ms. Render diagnostics
include CPU/GPU pass timings and pipeline statistics where the backend supports
them, plus mesh-allocator slab sizes and allocations. These are observations,
not portable pass/fail budgets; compare equivalent resolution, quality,
backend, optimization mode and warm-up periods.

The simulation also counts navigation requests/failures/waypoints,
assignment attempts and retries, dock waiting, loaded versus empty travel,
per-agent phase ticks, and per-producer processing/starvation/output-blocking/
disabled/completed ticks. These explain simulation waste separately from CPU
cost. Capture telemetry measures inspection snapshots, trace writes, pending
screenshots, and PNG conversion/write time so recording overhead is visible.

Brasswood uses continuous forest ground and separated work sites rather than a
floating platform. Ghouls emerge in three staggered lanes from the ruined burial
gate; hits recoil and kills briefly collapse before disappearing. Routine bulk
transfer audio is silent and other activity cues are coalesced. Depot counts
remain small; hovering a count expands its inventory and interaction hints.
Raised roads declare baked `walkable` support so player feet, workers and cart
wheels follow the rendered surface height.

The enlarged Brasswood map uses a 12-meter navigation half-width. Landed arrows
attach to a ghoul's rig and follow its movement and collapse; the corpse and its
arrows fade over three seconds. Temporary fade materials are removed before
rigs are reused, and pooled rig visibility propagates through every bone.

Brasswood uses slate and warm timber workshops, mixed evergreen/broadleaf groves,
flower clusters and connected dirt tracks. Foresters gather locally into the
timber yard; Bramble carries lumber across the camp to the mill. Backpacks use
compact upright bundles, while the cart keeps full-sized horizontal loads.
Carrier packing and trailer clearance are scenario settings, not skin-name checks.

Hover a workshop for its description, recipe, cycle time and live production
status. Hover the ballista for ammunition, damage, firing interval and range.
Building cards stay hidden otherwise. Controlled demos can pin the same card
with `{"op":"inspect_building","building":"sawmill"}` (or `cartworks` /
`watchtower`); pass `"building":null` to restore normal mouse hover. The forest
video demonstrates both workshop cards after the player clears the unloading dock.
