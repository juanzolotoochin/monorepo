# Verticality

Tunnels and levels: two or more floors over one x–y spot. The design is
[`VERTICALITY.md`](../VERTICALITY.md) (decisions V1–V11). Tasks run in
order, one agent each.

## V.6 The cutaway

- **Status:** open
- **Why:** underground, the ground above hides the player (V7).
- **What to do:** ground above the player's head is not drawn within a
  radius (a soft-edged hole); cut faces are dark.
  Note (from V.3): `tunnel_lab_demo` (`app/demos/tunnel_lab.json`) walks
  into the tunnel; today the mountain hides the avatar there.
  Note (from V.5): volume chunks are `GroundChunk`s with the shared
  `GroundMaterial` (a plain `StandardMaterial`, vertex colours); cut faces
  (walls, ceilings) have `UV_0.x = 1`, tops 0, so a cutaway shader or
  extended material can darken cut faces and discard the ground over the
  player. The camera looks toward -x -z, so the west tunnel mouth and the
  bridge's north side face away from it; a 1 m dug stair is mostly in
  shadow.
- **Where:** the volume material or shader, `app/src/rig.rs`.
- **Found in:** verticality design.
- **Tests:** screenshots in the tunnel, under the bridge and outside.

## V.7 Paths in 3D and the test walker

- **Status:** open
- **Why:** sprites and ghouls will need to path through tunnels (V8).
- **What to do:** nav nodes per floor (x, y, floor height), edges where
  walkable with headroom; A* over them. One test walker in the Tunnel Lab
  follows the player.
  Note (from V.3): agents (sprites, ghouls, haulers) still stand on
  `ground(p)`, the top floor, and store no height: give them a feet height
  (hashed, so save format 42 — V.4 took 41 — and a golden check) and move them to
  `Terrain::footprint_at` / `walkable_at`. The avatar's own `GoTo` routes
  (`avatar::set_intent`, and the demo scripts' `walk`) still plan with
  `Terrain::walkable` on the top floor, whose sloped branch ignores floor
  jumps, so a route into the tunnel is a straight line that only works
  because `walkable_at` then walks it; plan them with the 3D nav. Spawn,
  respawn, joining players, leaving a building and climbing out of a
  vehicle still put the feet on the top floor (`footprint`).
  Note (from V.5): a sprite's dig bar (`digging::shown_digs`) stands on
  `Terrain::height`, the top floor; give it the sprite's feet height.
  Note (from V.4): piles store no height either: ore a volume dig drops
  lands on a pile at a tile within two of the dug one
  (`PileLab::mined`, which checks pits and the top floor's steepness, not
  the volume), so in a tunnel it may sit over a wall or on the mountain's
  top floor as drawn; give piles a floor and pick the spot by the volume.
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
  row. Note (from V.4): the control protocol has `{"op": "dig_front",
  "direction": "Down"}` (`"Level"`, `"Down"`, `"Up"`) for the stair demo;
  the `tunnel_lab_demo` was not built for V.4 (the placeholder mesher
  doesn't re-mesh yet). A jetpack hovering outside can't enter a tunnel
  exactly 2 m high (its head at hover height is over the mouth's
  ceiling): walk in, then hold it.
- **Where:** sim tests, the demo scripts.
- **Found in:** verticality design.
- **Tests:** the soak; the full suite.

## Done

### V.5 Drawing the volume

- **Status:** done (branch `kestrel-verticality`, no PR yet)
- **Why:** the volume must be seen (V9).
- **What to do:** a mesher for volume chunks (16 × 16 columns): floors
  sloped as today's ground meshes, walls and ceilings as blocky faces;
  re-mesh the chunks near a dig. Note (from V.2): replace
  `digging::volume_placeholder_mesh` (flat blocks, no slopes); chunks are
  rebuilt only when pits change (`digging::update`), so volume edits need
  their own trigger (`Terrain::revision`); and `digging::ground_hit`
  searches down from 6 m, so the cursor can't hit the mountain's top (8 m).
  Note (from V.4): a volume dig clears up to `HEADROOM` cells of one
  column (`avatar::Dig { tile, layer: Some(bottom) }`, finished in
  `Settlement::dig_housekeeping` through `Terrain::clear_column`), bumping
  `Terrain::revision` once per cleared cell; re-mesh the dug tile's chunk
  and its neighbours' (the corner rule slopes the tiles round it). The
  dig's crack/progress visuals (`digging::shown_digs`) still draw at the
  dig's tile on the top floor (`ground`): draw them at the dug cells.
- **Where:** `app/src/digging.rs` or a new `app/src/volume_mesh.rs`.
- **Found in:** verticality design.
- **Tests:** mesh unit tests (faces per cell, no faces between solids);
  screenshots of the mountain, the tunnel and the bridge.
- **Done:** `app/src/volume_mesh.rs` replaces
  `digging::volume_placeholder_mesh`; `digging::ground_mesh` calls
  `volume_mesh::chunk_mesh` for a volume scene's 16 × 16 chunks, with the
  ground's one vertex-coloured `StandardMaterial` (`GroundMaterial`), so
  chunks cast and receive shadows as the ground does. Faces: every span's
  top, sloped by the volume's corner rule (`volume_mesh::top_corners`:
  each corner the lowest of the four tiles touching it, a neighbour by
  its `Volume::floor_near` the span's top, as `Volume::patch` has it),
  split on the diagonal as the sloped ground is; every span's underside
  (ceilings, and the volume's bottom) flat; walls on each tile side as
  the part of the column's profile (each span from its flat bottom up to
  its sloped top along that side) the neighbour's profile doesn't cover,
  cut by exact half-plane clips, so no face lies between solids and a
  wall reaches exactly the sloped top's edge (no fins over ramps, no
  cracks under them). Colours: open-sky tops by height as terraces are
  (`digging::surface`), tops under a ceiling and walls the stone of their
  layer (rock deep, warmer up), walls banded by 0.5 m layer, ore cells
  tinted by mineral (`piles::mineral_color`); walls and ceilings darker
  (`CUT`). For V.6: cut faces carry `UV_0.x = 1` (tops 0;
  `MeshBuilder::cut`), and tops face up (normal y > 0), walls sideways,
  ceilings down. Re-mesh: `Digging` keeps the volume it drew and the
  `Terrain::revision` it read; on a new revision `digging::update`
  diffs the volumes (`volume_mesh::changed_tiles`: spans or ore per
  arena tile) and rebuilds the chunks within two tiles of each change
  (`dirty_chunks(.., sloped: true)`), and `feedback` bursts dust (and
  the stone cue) from each dug tile's lowest cleared cell
  (`lowest_cleared`). Cursor: `digging::ground_hit` in a volume scene
  marches the ray from over the highest column to under the lowest
  (`volume_mesh::ray_hit`), hitting the first point inside a solid cell
  under its span's drawn top, and returns that top. `shown_digs` already
  drew an avatar's bar and chips at its feet (`AvatarState::height`, its
  floor), so it is unchanged (sprites still read `ground`, see V.7).
  Tests in `volume_mesh::tests`: `a_single_cell_has_six_faces`,
  `two_solids_side_by_side_share_no_face`,
  `a_tunnel_has_floor_walls_and_ceiling`,
  `sloped_floors_are_the_simulations` (every drawn top corner of a ramp,
  a tunnel and a cave is `Volume::floor` there),
  `the_surface_is_closed` (face area vectors sum to zero over ramps
  beside walls, a tunnel, a cave and a dug stair),
  `a_dig_remeshes_only_the_dirty_chunks` (clean chunks' meshes are
  unchanged) and `the_ray_hits_the_volumes_ground` (a hill's 8 m top).
  `app/demos/tunnel_lab.json` now also walks out of the east mouth, to
  the bridge's south side, and digs a stair (three `Down`, a `Level`,
  an `Up`; a dig needs about 80 frames at 30 fps and walking cancels
  it). Screenshots (local `tunnel_lab_demo`): the mountain from the
  spawn, the east cut and tunnel mouth, the bridge with the cave mouth,
  and the dug stair (its ramped rim follows the corner rule).

### V.4 Digging in 3D

- **Status:** done (branch `kestrel-verticality`, no PR yet)
- **Why:** you make tunnels by digging (V5).
- **What to do:** dig in front, level, down or up (chosen by where the
  player looks or by a modifier), clearing the 4 cells of headroom in
  front at that height; never under yourself. Ore lands in piles as
  today; decide a volume cell's yield. Refusals as today (reach, own
  tile, undiggable).
- **Where:** `sim/src/settlement/avatar.rs`, `sim/src/settlement/action.rs`,
  `sim/src/volume.rs`, the input and the command.
- **Found in:** verticality design.
- **Tests:** each direction from each side; never under yourself; a
  stair down and back up; ore yields.
- **Done:** a modifier, not the camera (the rig is fixed): in a volume
  scene F digs level, Ctrl+F (either Ctrl) a step down, Alt+F (either
  Alt) a step up; the controller picks DIG, DIG DOWN or DIG UP from the
  utility panel (`input::Utility::DigDown`/`DigUp`, offered only when
  the terrain has a volume, so other scenes' Ctrl+F and Alt+F stay a
  plain dig). The HUD line reads `F DIG / CTRL+F DIG DOWN / ALT+F DIG UP`;
  a refusal shows as the dispatcher's notice (`digging::refusal_text`).
  The direction travels in a new command, `CommandKind::DigFront { agent,
  direction: volume::DigDirection }` (`Level`, the default, `Down`, `Up`),
  appended after `Burst`: input format 11 (an older log that claims it is
  refused), net protocol 13. `DigDown` keeps its encoding and in a volume
  scene digs level; elsewhere `DigFront` digs as `DigDown`. The control
  protocol gained `dig_front`. `avatar::Dig` gained `layer: Option<i32>`
  (the first cleared layer of a volume dig, none for a pit): save format
  41; golden lines unchanged (no golden scene digs). The rule
  (`Terrain::volume_dig`, via `View::dig_volume` and
  `Settlement::dig_volume_of` for the app): the tile in front
  (`front_tile`), cells `F + offset .. F + offset + HEADROOM` where `F` is
  the digger's floor layer (`Volume::floor_at(position, avatar.height)`)
  and the offset 0, -1 or +1, so the tile's floor ends at the digger's
  floor, a layer under or a layer over it, with 2 m above. Refused:
  `Undiggable` (no volume, not diggable, outside the arena, an undiggable
  range, no floor under the digger), `OwnTile` (any of the footprint's
  five points in the tile: never under yourself), `Clear` (new: every
  cell already empty), `Bottom` (nothing solid would be left under the
  cleared cells), and `Blocked`/`Pile` as today (`dig_blocked`). A down
  dig may open onto a cave below: allowed, whoever walks in falls (no
  collapse, V6); no stranding check (you can always dig a step back up).
  Housekeeping clears the cells (`Terrain::clear_column`, a revision bump
  per cell). Each cleared solid cell yields `mine::cell_yield(tile,
  layer)` = 3 to 6 (the mine's hash with seed 0): an ore cell its
  mineral onto a pile beside the tile (`PileLab::mined`, tossed as
  today's ore), a stone cell the scene's stone item if it has one (the
  Tunnel Lab has none). Volume scenes' pile labs now pile anywhere in the
  map (as a mine's do), not only within 6.5 m of the center. The
  jetpack-cap stall (V.3's note) was real on a dug stair up: a body held
  on its floor by the cap now walks (`walkable_at`) where `flyable_at`
  refuses (`avatar::step`). Tests in
  `sim/src/settlement/volume_dig_tests.rs`:
  `each_direction_from_each_side_clears_the_four_cells_in_front`,
  `a_dig_is_never_under_the_digger`,
  `volume_digs_are_refused_as_the_ground_says`,
  `three_down_digs_make_a_stair_to_walk_down_and_back_up`,
  `three_up_digs_make_a_stair_up_through_a_hill` (also the held jetpack
  climbing it), `ore_cells_drop_their_yield_onto_piles` and
  `a_replayed_avatar_digs_as_the_simulation_does`; in `save`,
  `a_version_ten_log_with_a_dig_front_is_refused` and
  `a_volume_dig_in_progress_keeps_its_layer`; in `net`,
  `a_dig_front_needs_protocol_thirteen`; in the app's input tests,
  `ctrl_and_alt_f_dig_down_and_up_in_a_volume`.

### V.3 Bodies with a height

- **Status:** done (branch `kestrel-verticality`, no PR yet)
- **Why:** a body must know which floor it is on (V3).
- **What to do:** the avatar (and agents) store their feet's height; in a
  volume scene they stand on `floor(p, z)`, step up at most a layer, fall
  when the floor drops away, stop at walls and never enter a spot without
  headroom; jump and jetpack hit ceilings. Walkability uses the body's
  floor. Other scenes keep today's behaviour. Walks in a volume must also
  refuse a floor change of more than a layer (from V.1).
- **Where:** `sim/src/settlement/avatar.rs`, `sim/src/terrain.rs`
  (`walkable`), the view's body height.
- **Found in:** verticality design.
- **Tests:** walk through the tunnel; walk under the bridge and over it;
  blocked by low headroom; fall off a ledge; golden unchanged.
- **Done:** the avatar's feet height is the existing
  `AvatarState::height` (already hashed and saved), so the saved state and
  the save format (40) are unchanged; agents keep `ground(p)` (see V.7).
  `Volume::floor_at(p, z) -> Option<(layer, height)>` replaces V.1's
  `floor_layer`: the highest floor of `p`'s tile whose *sloped* height at
  `p` is at most `z + STEP_UP` layers (comparing the tile's layer instead
  stalled walks up ramps, whose sloped height is under the tile's floor);
  `floor(p, z)` is its height, `ceiling_over(p, z)` the ceiling over it.
  `volume::BODY_HEIGHT` = 2 m (= `height(HEADROOM)`). New `Terrain` methods,
  each the old one outside volume scenes: `footprint_at(p, z, radius)` (the
  highest of the five footprint points' floors, nothing if any has none: a
  wall, or a passage under `HEADROOM`, whose bottom is no floor),
  `ceiling_at(p, z, radius)` (lowest ceiling over the five points; always
  nothing without a volume), `walkable_at(a, b, z, radius)` (every 0.1 m
  each point needs a floor at most a layer over the feet, so a 1 m step is
  a wall; the feet follow ramps and stay up past a drop of more than a
  layer) and `flyable_at(a, b, z, radius)` (each point's floor at or under
  the feet, the head under its ceiling). The avatar's `step`, `jump_step`
  and `fly` use them with its height: on foot past a drop the feet stay up
  and the body falls by the existing rules (a grounded jumper over a drop
  turns `Airborne` with velocity 0; the jetpack sinks at `sink`), jump and
  jetpack heights are capped at the ceiling less `BODY_HEIGHT` (a capped
  jump starts falling), and the jetpack's look-ahead reads floors within a
  step of the feet, not the top floor. A threat's swing height check uses
  `footprint_at` too. Every new rule is gated on `terrain.volume`, so
  golden lines and hashes are unchanged. Prediction runs the same
  `avatar::self_update`, so it follows. View: the avatar was already drawn
  at `AvatarState::height`; `SessionService::ground_height` (foot IK,
  targets) now reads `Terrain::floor(p, p.y)` in volume scenes so feet plant
  on the tunnel floor. API for V.4 and V.7: a body's feet are
  `avatar.height`; its floor is `terrain.footprint_at(position, height,
  radius)` (or `volume.floor_at(p, height)` for the layer). A
  `tunnel_lab_demo` game demo (`app/demos/tunnel_lab.json`) walks from the
  spawn into the tunnel to (12, 0) at 0 m; inside, the placeholder mountain
  hides the avatar until V.6's cutaway. Tests in
  `sim/src/settlement/tunnel_lab_tests.rs`:
  `the_avatar_walks_through_the_tunnel_on_its_floor`,
  `the_avatar_walks_under_the_bridge_and_climbs_onto_it` (0 m in the cave,
  5 m on the bridge over the same tile (-11, 13)),
  `low_headroom_stops_a_walk`, `a_step_two_layers_up_is_a_wall`,
  `walking_off_a_ledge_falls_to_the_floor_below` (jetpack and jumper),
  `a_jump_is_stopped_by_the_ceiling`,
  `the_jetpack_is_stopped_by_the_tunnel_roof` and
  `a_replayed_avatar_walks_into_the_tunnel_as_the_simulation_does`; V.1's
  `volume_tests` pass unchanged under the new `floor`.

### V.2 The Tunnel Lab

- **Status:** done (branch `kestrel-verticality`, no PR yet)
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
- **Done:** the scene format's optional `volume:` section
  (`sim::volume::VolumeLayout`, embedded as `Scene::volume` in
  `tools/assetc/core`): `(extent: 32, boxes: [...])`, each box
  `Solid(min: (x, z), max: (x, z), bottom: k, top: k, material: ...)` or
  `Empty(min, max, bottom, top)`, in cells: tiles (tile `(x, z)` centered
  on scene meters `x, z`, `min`/`max` both included) and 0.5 m layers
  (`bottom` up to, not including, `top`; `top: 0` is a floor at 0 m).
  `material` is `Stone` (the default), `AmberOre`, `BlueCrystal` or
  `GreenMalachite` (minerals 0, 1, 2 as `mine` numbers them). Boxes apply
  in order: a solid box fills and sets its cells' ore (stone clears it), an
  empty box clears cells and ore. `VolumeLayout::validate` refuses an
  extent outside 8..128 tiles, no boxes or more than 256, a box with min
  past max, leaving the map, `bottom >= top`, layers outside -32..64, or
  more than 4M cells in all, naming the box (`volume box 3: ...`); unknown
  materials or fields don't parse. assetc also wants `SlopedGround`, no
  mine or terrain, and the spawn over ground. `VolumeLayout::build` makes
  the `Volume`; `lockstep::scene_setup` passes it as `SceneSetup::volume`
  and `assets::extent` takes the volume's extent. The map,
  `app/assets/levels/tunnel_lab.ron` ("tunnel-lab", "TUNNEL LAB / CAVES",
  exosuit, Digging and SlopedGround, last in the launch menu), in tiles
  from the origin (x east, z south): ground layers -8..0 everywhere (floor
  0 m); a mountain of eight 1 m tiers, each a tile in, from x 3..25, z
  -12..12 to x 10..18, z -5..5 at 8 m (layer 16); a tunnel along x through
  z -1..1 (3 m wide), floor 0 m, layers 0..6 clear (3 m), roofed from x 6
  to x 22 (west mouth at x 6, east at x 22, open cuts from x 3 and to x
  25); a natural bridge, block x -18..-4, z 10..16 topped at 5 m (layer
  10), the cave under it x -13..-9 through z 10..16, floor 0 m, 3 m high,
  so over `(-11, 13)` there are floors at layers 0 and 10 (0 m and 5 m);
  stairs a layer a tile up its west side, x -27 (0.5 m) to x -19 (4.5 m),
  z 11..15; ore: amber x 11..13, z 5..7, layers 9..11, blue in the
  tunnel's south wall x 13..15, z -3..-2, layers 1..4, green x 17..18, z
  2..3, layers -4..-2. Spawn at the origin (open ground, 6 m west of the
  tunnel's roof), a collector at (-3, -4). Drawing:
  `digging::volume_placeholder_mesh`, marked PLACEHOLDER, draws each
  column's spans as flat blocks (tops, cave and tunnel ceilings, walls
  against empty cells, ore-tinted). Tests: in `tools/assetc`,
  `the_tunnel_lab_loads_and_validates` and
  `a_malformed_volume_box_is_refused`; in
  `sim/src/settlement/tunnel_lab_tests.rs` (reading the scene's `volume:`
  section with `ron`, a `sim_test`-only dep),
  `the_tunnel_lab_volume_is_valid`,
  `the_bridge_has_two_floors_over_the_cave`,
  `the_tunnel_runs_through_the_mountain_with_headroom` and
  `digging_in_the_tunnel_lab_does_not_panic`; the app's scene list and
  obstacle counts updated. Golden unchanged.

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
