# Verticality

Tunnels and levels: two or more floors over one x–y spot. The design is
[`VERTICALITY.md`](../VERTICALITY.md) (decisions V1–V11). Tasks run in
order, one agent each.

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
  floor. Other scenes keep today's behaviour. Note (from V.1): a neighbour
  floor two or more layers off is a wall edge to the corner rule, so the
  patches on both sides stay level and `Terrain::walkable`'s sloped branch
  (which checks steepness, not jumps) would let a walk climb it: walks in
  a volume must also refuse a floor change of more than a layer.
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
  today. The volume stores only a cell's mineral (V.1): decide its
  yield (the mine hashes 3 to 6 from its seed; a volume has no seed).
  `Terrain::dig_refusal`, `deepen` and `walks_out` still work on pits and
  ignore the volume. Refusals as today (reach, own tile, undiggable).
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

### V.1 The volume

- **Status:** done (branch `kestrel-verticality`, no PR yet)
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
- **Done:** `sim/src/volume.rs`: a `Volume` of `Span`s (solid layers
  `bottom..top`) per tile column, sorted, merged and without empty
  columns, and ore per cell as a sorted sparse list of `((tile, layer),
  mineral)` (the mineral numbered as `mine` numbers them; no yield yet).
  Edits: `fill`, `clear` (drops the cell's ore), `fill_box`, `clear_box`,
  `set_ore`, `shifted`. Queries: `spans`, `columns`, `is_solid`, `ore`,
  `floors` (tops of spans with `HEADROOM` = 4 empty layers above, and the
  top span's), `top`, `ceiling`, `headroom`, `floor_near` (the floor
  within a layer), `floor_layer` and `floor(p, z)`: the highest floor at or
  below the feet plus `STEP_UP` = one layer (the most a sloped walk climbs
  per tile; floors in a column are at least five layers apart, so it never
  reaches the next), sloped by M6's corner rule with each neighbour from
  its `floor_near`, none counting as a wall edge (level). `height(layer)`
  and `layer_at(z)` convert. The corner rule is factored into
  `terrain::Patch::over`, shared by `Terrain::patch` and `Volume::patch`.
  `Terrain::volume: Option<Volume>` (from `SceneSetup::volume`, in tiles
  from the arena's center tile): with one it is the ground, `ground`,
  `tile_height`, `patch` (so `walkable` and `steep`) read the top floor,
  and surfaces, mines and pits don't count. `Terrain::floor(p, z)` is the
  floor underfoot (`ground` without a volume); `Terrain::fill_cell` and
  `clear_cell` edit it and bump `revision`. Hashed only when present, so
  other scenes' hashes and golden lines are unchanged; save format 40.
  Tests (`sim/src/volume_tests.rs`): `spans_split_on_clear_and_merge_on_fill`,
  `floors_need_headroom_and_have_ceilings`,
  `a_bridge_has_two_floors_over_one_spot`,
  `tunnel_floors_slope_by_the_corner_rule`,
  `edits_bump_the_revision_and_change_the_hash`,
  `scenes_opt_into_a_volume`; in `save/src/tests.rs`,
  `a_volume_scene_keeps_its_cells_and_ore`.
