# Game architecture

This is the architecture for rewriting the game (today `game/core` and
`game/app`) as a deterministic lockstep simulation with Bevy
as a view. The goal is the same gameplay as today, rebuilt to scale to about
100k entities and 2–8 co-op players over the internet (more on LAN).

The approach was validated in an earlier prototype:

- Bit-identical simulation on Windows and Linux, Intel and AMD, including a Steam Deck.
- 1M simple tower-defense entities (towers, miners, enemies) in under 6 ms per tick.
- Fixed-point arithmetic.
- Lockstep networking at 100 ms latency with jitter.
- The message-passing simulation design and Bevy as a renderer.

Interactive explainers for each decision live in [`design/`](design/). Open them
in a browser.

## Principles

- **Deterministic lockstep.** Every peer simulates the whole world from the same
  inputs. Only inputs go over the network. There is no rollback.
- **The simulation is pure Rust with no Bevy dependency.** Bevy only reads a
  render extract and sends commands. Nothing flows from Bevy back into the
  simulation except commands.
- **No floating point in the simulation.** All simulation math uses `Fx`, a
  fixed-point type. f32 exists only in the view and in raw input before it is
  quantized into a command.
- **30 Hz simulation.** Rendering interpolates. Content is written in seconds
  and per-second rates, converted to ticks at load, so the tick rate is one
  constant.
- **Budget:** at most 10 ms per tick at 100k entities on a dev PC.

## Crates

All under `game/`, built by Bazel with `rules_rs`. The prototype keeps
`game/app` and `game/core` until the port reaches parity, so the new game
binary is `game/play` until then.

| Crate | Deterministic | Purpose |
|---|---|---|
| `fx` | yes | `Fx` (i64, 32.32), vectors, integer sqrt, table-based sin/cos/atan2, decimal parsing |
| `sim` | yes | Entity tables, chunks, the tick pipeline, message queues, RNG, timers, hashing |
| `content` | yes | Definition files, sim/view split, name tables, rules hash |
| `save` | yes | Sectioned save format, remapping on load, RON dumps, diff tool |
| `net` | yes | Turns, input delay, time sync, `Transport` and `Sessions` traits, loopback simulator, UDP |
| `headless` | tool | Runs a save plus an input log, writes per-tick hashes, benchmarks |
| `view` | no (Bevy) | Consumes the render extract: interpolation, prediction, overlays |
| `play` | no (Bevy) | The game binary: single-player, host, join |

Bazel visibility keeps `@crates//:bevy` out of the deterministic crates. `net`
counts as deterministic because its decisions depend only on its inputs and a
clock it is given, which lets tests run it on a virtual clock.

## Simulation

Details: [message system](design/message-system.html),
[big entities](design/big-entities.html).

### Fixed point

`Fx` is our own newtype over `i64` in 32.32 format. Overflow checks are on in
debug builds. Sqrt is an exact integer square root. Sin, cos and atan2
interpolate tables that are computed at compile time with integer math, so no
platform's float library is involved and results are bit-exact everywhere.
Content files contain decimals (`speed: 1.5`), parsed into `Fx` once at load
with integer arithmetic. `game/fx/testdata/golden.txt` hashes every operation
over fixed inputs; `bazel run //game/fx:update_golden` regenerates it.

### World and chunks

- Entities live in **per-type SoA tables** with **generational IDs**. IDs are
  allocated only in the apply phase.
- The world is split into **chunks**: 32 × 32 tile squares covering the full
  height. The MVP world (512 × 512 tiles, 32 voxels deep) has 256 of them.
- Every entity belongs to exactly one chunk, the one containing its position
  (or its center, or for buildings, its anchor tile). Chunk membership changes
  only during housekeeping.
- The chunk is the unit for parallel work, message ordering, sleeping, spatial
  queries, hashing, and terrain and navigation rebuilds.
- Each chunk has a **spatial index** of 8 � 8 cells (4 � 4 tiles each), rebuilt
  in housekeeping for chunks that changed. During a tick it holds last tick's
  positions, which is exactly the snapshot self update reads. Read-phase queries
  widen their radius by the maximum speed and test current positions.
- Only chunks that changed (**dirty** chunks) are reindexed and rehashed, so
  sleeping chunks cost nothing per tick.

### The tick

```
1. Apply inputs    single thread   the turn's commands, validated, in player order
2. Self update     parallel        each entity changes only its own state
3. Read phase      parallel        read the whole world, write messages to the chunk's outbox
4. Merge           single thread   concatenate outboxes in chunk order
5. Apply phase     single thread   validate each message, then apply or reject it
6. Housekeeping    single thread   chunk moves, spawn/despawn, sleep/wake, nav rebuild, hash
```

**Core rule:** an entity may change itself directly in a parallel loop. Anything
that touches another entity or a shared resource (depot, tile, terrain) is a
message.

- If self update needs another entity's data (for example separation steering),
  it reads a **snapshot of last tick's positions**, copied before stage 2.
- Messages are stored in **one queue per message type**. Types are applied in an
  explicit, fixed order, and messages within a type are applied in chunk order.
- **Conflicts:** the earlier message in the order wins. The rejected sender gets
  a failure message and re-plans next tick. Chunk order is a **shuffle seeded by
  the tick number**, so every pair of chunks has even odds over time. Player
  commands apply in stage 1, before any AI messages.
- Messages created during apply are handled next tick. There are no extra passes.
- Randomness comes from a counter-based RNG keyed on seed, tick, entity and
  stream, so parallel use is deterministic.
- Hot paths (movement, projectile flight, timers) are tight self-update loops.
  Messages are for rare interactions (damage, reservations, building, digging).

### Sleeping

Skipping work is the main performance lever.

- A chunk with no awake entities is skipped by every stage. Its entities can
  still be read by queries from other chunks.
- An entity sleeps until a message wakes it, or until its slot in the **timer
  wheel** comes up.
- Sleep state is part of the save and the hash.

### Movement, navigation and big entities

- Positions are continuous 3D `Fx`. Navigation runs on **walkable spans**, which
  allow several floors per column, so tunnels can be supported later.
- Size classes 1, 2 and 4. Each chunk has a **clearance map**, rebuilt when its
  terrain changes.
- Enemies use **flow fields**, one per target per size class. Haulers use
  **hierarchical A*** with per-tick work budgets and path caching.
- A big entity is owned by the chunk containing its center. Queries widen their
  chunk search by the largest body radius (capped at about 8 tiles), then do an
  exact footprint test. Overlap registration is reserved for future bosses above
  the cap.
- Units use **soft, mass-weighted separation**, read from last tick's positions.
  Per-type collision opt-outs (for example a spider walking over small units)
  can be added later.

### Terrain (Milestone 1)

Today's scenes are arenas: flat ground, raised walkable boxes, and disc-shaped
obstacles (rocks, trees, machines). `game/sim::terrain` holds exactly that as
simulation state, in world tiles (1 tile = 1 m), plus diggable rock columns.
Digging is a command (later also a message) that changes the terrain and its
hash. `game/sim::nav` is the original clearance-aware A* (half-tile grid,
string pulling, a turning-circle planner for carts) in fixed point.

Not built yet, because no current scene needs them: voxel chunks with
multi-level walkable spans, flow fields and hierarchical A*. Each wave's
enemies share one planned route, which is what a flow field per target would
give at this scale.

### Logistics

Items move only by haulers (no belts). Reservations use incremental counters on
each depot, updated in the apply phase. Today's stock conservation ledger is
kept as a debug and test invariant.

As built in `game/sim::settlement` (Milestone 1):

- Scenario files keep the original format (`game/content::scenario`), parsed
  into `Fx`. A host turns a scenario plus the scene's obstacle discs into the
  world at tick 0. View-only fields (labels, skins, cargo layouts) stay `f32`
  and never reach the simulation.
- Each depot counts `out` (promised stock) and `incoming` (units on their way)
  per item. `Settlement::validate` recomputes both from scratch every tick in
  tests, along with the conservation ledger.
- Haulers move themselves in the parallel self update, avoiding last tick's
  bodies. Taking a job (`Reserve`), loading, unloading, fetching and attaching
  equipment are messages; a job that no longer fits when applied shrinks or is
  rejected, and the hauler retries later.
- Docks are claimed by message every tick and resolved per depot, independent
  of message order: an agent using or leaving the dock keeps it, then the
  previous owner if still approaching, then the nearest approaching agent. An
  agent may only start loading once it owns the dock.
- Settlement agents live in world-wide tables rather than per-chunk ones, since
  scenes have at most 32 of them; their messages are still filed under the
  chunk they stand in, so they join the shuffled chunk order.
- Differences from the original game, on purpose: a newly attached trailer is
  placed in the nearest clear direction behind the animal, and a cart that
  can't find a turning circle takes the direct route instead of waiting
  forever. The original could deadlock on both, depending on the arrival angle.

### Combat and the player (Milestone 1)

`game/sim::settlement::{combat, avatar}` port the original rules: waves,
defenders, arrows and ballista bolts (deflect, pierce, impale), sword
criticals, spear punctures, arc-coil chains, stagger and knockback.

- Threats move themselves in parallel. The read phase decides projectile
  collisions, breaches, defender shots and which threats a strike reaches;
  the apply phase computes damage against current armor and health.
- The player's avatar is a simulated body now, not a Bevy transform. Commands
  set an intent (walk a direction, go to a point, stop); the original
  controller's acceleration, braking, run speed, click routes and sliding
  collision run each tick. Attacks are commands: contact resolves on the
  windup tick, and movement stays locked for the swing or the rebound after a
  hit. One follow-up click near the end of a swing is buffered.
- Manual transfers are commands and use the same reservation counters as
  haulers. Cancelling releases unlaunched units; units in the air still land.
- Tick-based tuning in scenario files (60 Hz) is converted to 30 Hz.

Compared with the original over the same two minutes (`parity_probe`), the
forest outpost delivers 99 units versus 98 and kills 21 ghouls versus 23. The
combat arena's ballista lands all 18 bolts in both; the pierce rolls fall
differently (same formula, different tick numbers and IDs at 30 Hz), so 8
ghouls die instead of 13.

## Networking

Details: [desyncs and host loss](design/desync-host-loss.html).

- **Topology:** the host's game is the server and simulates like any client.
  There is no dedicated server.
- **Turns:** the host collects inputs and broadcasts the official input list for
  each tick. A client simulates tick T only after receiving turn T. Each packet
  repeats every input the receiver hasn't acknowledged, so lost packets need no
  retransmission timers.
- **Input delay:** an input made on tick t is scheduled for tick t + D. The host
  never waits for a slow client; a late input moves to the next open turn.
  Clients stay a small buffer behind the host and catch up by running extra
  ticks. D is fixed at first. An adaptive per-client policy comes later, behind
  a `--input-delay` override.
- **Desyncs:** every client sends a hash per tick, split by subsystem and chunk.
  On a mismatch, every side writes a state dump. Dev builds halt all peers.
  Release builds resync the client through the join path.
- **Joining:** the joiner receives a save plus the inputs since that save.
  Chunks whose hash already matches can be skipped.
- **Host leaving:** the session ends. All peers autosave on the same ticks and
  also save when the host is lost, so anyone can re-host manually. There is no
  automatic migration yet, but the protocol treats the host as a role (authority
  ID, session epoch), and turn authority is its own module.
- **Player identity:** a stable player ID (a local profile ID now, Steam or EOS
  ID later) maps returning players back to their avatars.
- **Layers:** the lockstep protocol sits on a `Transport` trait (send and
  receive unreliable datagrams; the protocol repeats whatever isn't
  acknowledged, so it needs nothing more) and, later, a `Sessions` trait (find,
  create, join) for Steam and EOS lobbies. Direct IP needs no session layer.
  Implementations come in this order:
  1. Loopback simulator for tests
  2. UDP with direct IP, for LAN and Tailscale
  3. Steam
  4. EOS
- **Why not gRPC or TCP:** see [why not gRPC](design/why-not-grpc.html).
  - TCP's in-order delivery holds back every packet after a lost one until the
    retransmit arrives (head-of-line blocking), which stalls lockstep.
  - Steam and EOS relays are message-based, not TCP streams.
  - The UDP transport is therefore plain UDP with our own small framing: acks,
    repeated unacknowledged turns, a handshake, and a chunked save transfer for
    joins. It needs no async runtime and runs on the simulator's virtual clock.
  - gRPC or HTTP remain the choice for services around the game (lobbies,
    accounts, uploading desync dumps).
- **Tooling:**
  - A loopback simulator with a virtual clock and seeded latency, jitter, loss
    and spikes, so network tests are fast and reproducible.
  - An in-game net graph overlay.
  - Packet traces that can be replayed through the simulator.

## View (Bevy)

Details: [view layer](design/view-layer.html).

- **Data flow:**
  1. Bevy input becomes a command, quantized and tagged with tick t + D.
  2. The simulation runs.
  3. It produces a **render extract**: positions, headings, animation state and
     health for visible chunks, converted to f32, plus tick-tagged events.
  4. Bevy draws from the extract.

  Player movement is a command. Today's `set_controlled_pose` goes away.
- The view keeps a map from simulation ID to Bevy entity. Spawns and despawns
  arrive as events. Positions are copied from the extract each frame.
- **Interpolation:** the view blends the previous and current extract, drawing
  one tick behind. Spawns and teleports are flagged so they don't slide in.
- **Prediction (view only):**
  - Casts, ghost buildings and UI respond immediately.
  - Your own avatar runs the real `Fx` movement code on a private copy (last
    confirmed state plus your unconfirmed inputs), and eases toward the
    confirmed position when they differ.
  - Damage and deaths wait for the confirmed tick.
- **Events** (fired, hit, died, dug, delivered) play when interpolated render
  time reaches their tick. The view may drop them.
- The view may use f32, `HashMap`, random particles and wall-clock time.
- The simulation runs inside the Bevy frame at first. Bevy only reads the
  extract, so the simulation can move to its own thread without changing view
  code.

## Data

Details: [saves, content and IDs](design/saves-content.html).

- **Four kinds of data:** content (definitions), scenarios (starting setups),
  saves (state at a tick) and input logs (turns since a save).
- **A new game is a save at tick 0**, generated on the host from a scenario.
  Starting, loading, joining, resyncing and re-hosting share one load path.
  World generation runs only on the host, but should still be deterministic for
  tests and replays.
- **Content:** one file per definition, with `sim` and `view` sections. Only
  `sim` reaches the simulation crate and feeds the **rules hash**. Peers with
  different rules hashes can't play together. Visual-only changes stay
  compatible.
- **IDs:** content is named by strings in files and by dense indices at runtime.
  Each save carries its index-to-name tables and is remapped on load. Entity IDs
  are generational, and player IDs are stable across sessions.
- **Saves:** a header (format and game version, rules hash, name tables, tick,
  RNG state) followed by sections per subsystem and per terrain chunk, encoded
  with serde, postcard and zstd. The same types print as RON for readable dumps
  and diffs. Navigation data is rebuilt on load rather than saved. Estimated size
  is about 3 MB compressed for the MVP world at 100k entities.
- **Compatibility:** a save from a different format version is refused with a
  clear error. Migrations can be added per section once people keep worlds they
  care about.

## Testing

The headless runner turns a save plus an input log into one hash per tick. Each
determinism test compares two hash files that must be identical.

| Test | Compares |
|---|---|
| Golden hashes | Fixed scenarios against hash files in `game/testdata/` |
| Thread count | 1 thread vs 16 threads |
| Save and continue | Save at tick T, load and continue, vs an uninterrupted run |
| Replay | The same save and input log run twice |
| Content remap | An old save loaded after adding content |
| Net scenarios | Loopback runs with latency, jitter, loss and spikes |
| Benchmark | ms per tick at 100k entities |

Linux CI (BuildBuddy) runs all of these. Windows and Steam Deck machines check
themselves against the same golden files by running `bazel test` locally. A
single command regenerates the golden files after an intended gameplay change,
and the change shows up in review.

## Roadmap

Details: [milestone plan](design/milestone-plan.html).

**Milestone 0, infrastructure** (throwaway test entities only):

| Step | Delivers | Done when |
|---|---|---|
| 0.0 | Bevy 0.18.1 → 0.20 for the whole monorepo, old game included. Runs alongside 0.1–0.4. | The old game and its demos pass on Linux and Windows |
| 0.1 | `game/fx` | Accuracy tests against f64; golden results identical on Linux and Windows |
| 0.2 | `game/sim` core | Thread-count test passes |
| 0.3 | `game/content`, `game/save` | Save-and-continue test passes |
| 0.4 | `game/headless` | Determinism suite runs in CI; benchmark under 10 ms per tick |
| 0.5 | Minimal `game/view` and `game/play` | Smooth 144 Hz view of the 30 Hz simulation, single-player through the turn path |
| 0.6 | `game/net` | 30 minutes over Tailscale with no desync, plus a working mid-game join |

**Milestone 1, gameplay port:**

1. Terrain and navigation
2. Logistics
3. Combat and player actions
4. `game_demo` rebuilt on input logs, every scene reproduced, then
   `game/core` retired and the prototype implementation in `game/app` replaced

## Deferred

These are designed to be added without restructuring:

- Adaptive per-client input delay
- Automatic host migration
- Steam and EOS backends
- A Windows CI runner
- Overlap registration for huge bosses
- Per-type collision opt-outs
- Save migrations
- Running the simulation on its own thread
