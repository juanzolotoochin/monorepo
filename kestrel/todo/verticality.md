# Verticality

Tunnels and levels: two or more floors over one x–y spot. The design is
[`VERTICALITY.md`](../VERTICALITY.md) (decisions V1–V11). Tasks run in
order, one agent each.

## V.1 The volume

- **Status:** open
- **Why:** a heightfield has one height per x–y, so nothing can stand
  under anything (V1, V2, V4, V11).
- **What to do:** a `Volume` in the sim: per 1 m column, spans of solid
  0.5 m cells, with ore per cell. Derived floors (top of a span with 4
  empty layers above), ceilings and headroom. `floor(p, z)`: the highest
  floor at or below `z` plus one step, sloped per floor with M6's corner
  rule (neighbours from the floor within one layer in the next column).
  `Terrain::volume: Option<Volume>`, opted into per scene; with one,
  `ground(p)` is the top floor. Hashed and saved (save format up).
- **Where:** new `sim/src/volume.rs`, `sim/src/terrain.rs`, `save/`.
- **Found in:** verticality design.
- **Tests:** spans split and merge on clear and fill; floors and ceilings;
  `floor(p, z)` on two floors over one spot; sloped floors; hashing and a
  save round trip; scenes without a volume unchanged.

## V.2 The Tunnel Lab

- **Status:** open
- **Why:** the MVP needs its own map (V10).
- **What to do:** solid and empty boxes in the scene format (with stone or
  ore), checked by assetc, building the volume at load. A 64 m map: a
  mountain about 8 m high with a ready tunnel through it, a cave under a
  natural bridge (two floors over one spot), stone and ore inside. Listed
  in the launch menu. Drawing can be a placeholder until V.5.
- **Where:** `app/assets/levels/tunnel_lab.ron`, `app/assets/BUILD.bazel`,
  `tools/assetc/core`, `app/src/assets.rs`, scene setup in the sim.
- **Found in:** verticality design.
- **Tests:** the scene loads and validates; the volume has two floors over
  the bridge spot; a malformed box is refused.

## V.3 Bodies with a height

- **Status:** open
- **Why:** a body must know which floor it is on (V3).
- **What to do:** the avatar (and agents) store their feet's height; in a
  volume scene they stand on `floor(p, z)`, step up at most a layer, fall
  when the floor drops away, stop at walls and never enter a spot without
  headroom; jump and jetpack hit ceilings. Walkability uses the body's
  floor. Other scenes keep today's behaviour.
- **Where:** `sim/src/settlement/avatar.rs`, `sim/src/terrain.rs`
  (`walkable`), the view's body height.
- **Found in:** verticality design.
- **Tests:** walk through the tunnel; walk under the bridge and over it;
  blocked by low headroom; fall off a ledge; golden unchanged.

## V.4 Digging in 3D

- **Status:** open
- **Why:** you make tunnels by digging (V5).
- **What to do:** dig in front, level, down or up (chosen by where the
  player looks or by a modifier), clearing the 4 cells of headroom in
  front at that height; never under yourself. Ore lands in piles as
  today. Refusals as today (reach, own tile, undiggable).
- **Where:** `sim/src/settlement/avatar.rs`, `sim/src/settlement/action.rs`,
  `sim/src/volume.rs`, the input and the command if it needs the
  direction.
- **Found in:** verticality design.
- **Tests:** each direction from each side; never under yourself; a
  stair down and back up; ore yields.

## V.5 Drawing the volume

- **Status:** open
- **Why:** the volume must be seen (V9).
- **What to do:** a mesher for volume chunks (16 × 16 columns): floors
  sloped as today's ground meshes, walls and ceilings as blocky faces;
  re-mesh the chunks near a dig.
- **Where:** `app/src/digging.rs` or a new `app/src/volume_mesh.rs`.
- **Found in:** verticality design.
- **Tests:** mesh unit tests (faces per cell, no faces between solids);
  screenshots of the mountain, the tunnel and the bridge.

## V.6 The cutaway

- **Status:** open
- **Why:** underground, the ground above hides the player (V7).
- **What to do:** ground above the player's head is not drawn within a
  radius (a soft-edged hole); cut faces are dark.
- **Where:** the volume material or shader, `app/src/rig.rs`.
- **Found in:** verticality design.
- **Tests:** screenshots in the tunnel, under the bridge and outside.

## V.7 Paths in 3D and the test walker

- **Status:** open
- **Why:** sprites and ghouls will need to path through tunnels (V8).
- **What to do:** nav nodes per floor (x, y, floor height), edges where
  walkable with headroom; A* over them. One test walker in the Tunnel Lab
  follows the player.
- **Where:** `sim/src/nav.rs`, a walker in the settlement.
- **Found in:** verticality design.
- **Tests:** paths through the tunnel and from under the bridge to over
  it; the walker follows the player; no path where there is no headroom.

## V.8 Soak and demo

- **Status:** open
- **Why:** the MVP needs a stable run and a look.
- **What to do:** a soak test digging at random in the Tunnel Lab with
  `validate` every tick; a demo video walking through the tunnel, under
  the bridge, digging a stair down; the full suite; a `docs/test-time.md`
  row.
- **Where:** sim tests, the demo scripts.
- **Found in:** verticality design.
- **Tests:** the soak; the full suite.

## Done
