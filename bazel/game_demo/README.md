# Controlled game demos

`game_demo` replays structured commands through the game's command dispatcher,
records the real scene offscreen, and encodes an MP4 with the game's procedural footsteps, dispenser cues, tosses, and landing sounds. Scripts and live clients
use the same commands and world snapshots as the renderer-free integration tests.

```starlark
load("//bazel/game_demo:defs.bzl", "game_demo")

game_demo(
    name = "haul_four_demo",
    game = ":demo",
    script = "demos/haul_four.json",
    tags = ["manual"],
    target_compatible_with = ["@platforms//os:linux"],
)
```

```sh
bazel build --config=full_local //examples/rust/bevy_isometric:haul_four_demo
```

Outputs in `bazel-bin/examples/rust/bevy_isometric/`:

- `haul_four_demo.mp4`: H.264 video with synchronized AAC game audio, with the script's resolution and FPS.
- `haul_four_demo.state.json`: action receipts, final world state, assertions,
  frame count, and encoder metadata.
- `haul_four_demo.wav`: deterministic 48 kHz stereo game-audio mix.
- `haul_four_demo.frames.jsonl`: one semantic world snapshot per video frame,
  with its zero-based frame number and video timestamp.

The action executes locally on Linux using a Vulkan-capable GPU/driver (including
compatible software Vulkan) and a local FFmpeg with `libx264`. It renders without
X11 or Wayland. Encoder discovery uses `PATH`, or an explicit
`--repo_env=GAME_DEMO_FFMPEG=/absolute/path`. The encoder version is recorded in
the report. Hardware-dependent GPU renders stay local and out of remote caches.
Bazel reuses local outputs when inputs are unchanged. The game executable is built for the execution platform.

## Scripts

See `examples/rust/bevy_isometric/demos/haul_four.json`. Script version 1 declares
`scene`, `fps`, `width`, `height`, `max_frames`, and sequential `steps`:

```json
{
  "op": "grab",
  "pile": 0,
  "units": 4,
  "material": "AmberOre",
  "seconds_per_unit": 0.7,
  "approach": [-2.3, -2.8]
}
```

Scripts use 10–60 FPS. The example adds 8 units, approaches the source from the side, tosses exactly 4
units into the backpack one at a time, walks elsewhere,
drops all 4, and checks both pile counts and conservation. Commands complete
before the next step starts. Pickup/deposit commands include travel to a valid
interaction distance. A failed command or assertion stops the script and fails
the Bazel action. Fixed simulation timesteps, bounded command durations, and
explicit waits make demonstrations repeatable. `seconds_per_unit` paces pickup
and deposit; `approach` selects a reachable standing point within safe interaction
range, which makes camera composition controllable. Airborne units follow a spinning
arc into the backpack or onto the floor. They replace the corresponding resting
chunk until landing, preserving one visible chunk per conserved unit. The renderer warms up before
recording and holds the final state for one second. Pixels follow the local
GPU/driver's rendering behavior. Audio is synthesized offline from frame-stamped
game cues using the same procedural bank as interactive playback, so it requires
no microphone or audio device. The state report includes the cue timeline.

Run the same script quickly with no GPU or audio device:

```sh
bazel run //examples/rust/bevy_isometric:demo -- \
  --simulation \
  --demo-script "$PWD/examples/rust/bevy_isometric/demos/haul_four.json" \
  --report /tmp/haul-state.json
```

## Live commands and state inspection

Enable the local control endpoint in a normal game, or add `--simulation` for
renderer-free automation:

```sh
bazel run //examples/rust/bevy_isometric:demo -- \
  --scene pile-lab --control-port 7017
```

```sh
bazel run //tools/game_demo:client -- --address 127.0.0.1:7017 \
  '{"id":"status","command":{"op":"inspect"}}'
bazel run //tools/game_demo:client -- --address 127.0.0.1:7017 \
  '{"id":"add-ore","command":{"op":"add","dispenser":0,"units":8}}'
bazel run //tools/game_demo:client -- --address 127.0.0.1:7017 \
  '{"id":"grab-four","command":{"op":"grab","pile":0,"units":4}}'
```

The protocol uses newline-delimited JSON over TCP on `127.0.0.1`. Each request
has an `id` and `command`. Each reply includes the matching ID, a `completed` or
`failed` status, an error when applicable, and the world state after completion.
A connection handles requests sequentially; another connection can inspect while
a movement/transfer command is still running. `--control-port 0` chooses a free
port; `--control-address-file /tmp/game-address` publishes the selected address.
The client accepts `--address-file` as well.

| Operation | Fields | Behavior |
| --- | --- | --- |
| `inspect` | none | Read state immediately, including during another action. |
| `scene` | `scene` | Load `mining-deck`, `boulder-lab`, or `pile-lab`. |
| `walk` | `position: [x, z]` | Navigate to a reachable ground location. |
| `add` | `dispenser`, `units` | Add an exact quantity to a dispenser's output. |
| `grab` | `pile`, `units`, optional `material`, `seconds_per_unit`, `approach` | Collect an exact quantity within capacity. |
| `drop_all` | `position: [x, z]`, optional `seconds_per_unit`, `approach` | Walk to a clear location and create a pile from all cargo. |
| `deposit_all` | `pile`, optional `seconds_per_unit`, `approach` | Walk over and deposit all cargo into a compatible pile. |
| `strike` | `target: [x, z]` | Perform a sword strike and wait for recovery. |
| `wait` | `frames` | Advance simulation for the specified number of frames. |
| `assert` | optional `cargo_units`, `total_units`, `piles: [{id, units}]` | Check quantities and conservation. |
| `quit` | none | Shut down a live session. |

World snapshots include the player position, movement route/velocity, combat
state, cargo type/count/capacity, stable pile IDs and quantities, source flags,
created/ground/carried totals, visible mineral chunk count, airborne-unit count and positions/destinations, collision obstacles,
impact targets and their material responses, and the active command. IDs 0 and
1 are the authored dispenser outputs in this scene; IDs 2–4 are starter piles.
New pile IDs increase monotonically within the scene. Scene changes reset the
pile demonstration, as they do when controlled with the keyboard.

Protocol types live in `src/control/protocol.rs`; transport, dispatch/state,
and offscreen capture are separate modules. Tests exercise the shared engine
and the actual TCP endpoint, including inspection during movement and rejected
exact-quantity requests.

Interactive pickup/deposit also uses the toss animation, paced at 0.18 seconds
per unit. Scripts can choose a slower interval for presentation or zero for
instant transfers in fast tests. Quantities commit when a toss starts; an
airborne unit is a reserved part of its cargo/destination total. The `airborne`
state describes that visual subset, and completion waits for every unit to land.

### Flight and elevated scenes

`jetpack-lab` is scene 4. `{"op":"jetpack","enabled":true}` enables the same
terrain-following controller used by Space. `walk` then navigates across raised
platforms and chasms. Setting `enabled` to false requests landing; over a void,
the controller keeps hovering until it reaches supported ground. Commands are
sequential, so a flight request completes immediately; follow it with `wait`
when a demo should show takeoff or landing before the next movement.

`{"op":"assert_flight","airborne":true,"altitude":3.8,"tolerance":0.05}`
checks actual flight state and altitude. Tolerance defaults to 0.05 meters.
Snapshots expose `terrain.platforms` and `player.jetpack` (`requested`,
`airborne`, `support_height`, `hover_height`, `clearance`). Clearance is relative
to the current/last supported top; during anticipatory climbs it can exceed the
nominal hover height. The jetpack demo exercises and asserts climb, gap crossing,
release over a chasm, and landing, and records turbine audio:

```sh
bazel build --config=full_local //examples/rust/bevy_isometric:jetpack_demo
```

### Field trial 05

The `surprise-lab` scene and `surprise_demo` target provide another complete
programmatically controlled demonstration, with in-scene instructions.

<details>
<summary>Protocol additions (mechanic spoilers)</summary>

- `memory_record`: start a fresh movement recording, replacing the previous
  projection. Rejects a second start while already recording.
- `memory_deploy`: finish the recording, return the player to its initial pose,
  and project a replay that holds its final pose. Requires 0.15 seconds of tape.
- `memory_clear`: cancel recording and dismiss the projection.
- `assert_trial`: check `unlocked` and `collected` boolean fields.

Recordings are limited to eight seconds and automatically deploy at the limit.
Replay interpolates poses in simulation seconds. Inventory and world state
remain live. The shared dispatcher invokes the same functions used by E and Q.

Snapshots include `memory`: `recording`, `recorded_seconds`, `replay_seconds`,
`sample_count`, `ghost_position`, `plates`, `charge`, `unlocked`, and `collected`.
The trial script asserts that the objective starts locked, unlocks through the
new interaction, and is ultimately collected by the player. State traces let
clients inspect the replay while other commands run.

</details>
