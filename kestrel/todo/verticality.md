# Verticality

Tunnels and levels: two or more floors over one x–y spot. The design is
[`VERTICALITY.md`](../VERTICALITY.md) (decisions V1–V11). Tasks run in
order, one agent each.

## Line of sight follow-ups

- **Status:** open
- **Why:** the line of sight prototype (Done, below) only fades the
  test walker and only shades the volume ground.
- **What to do:** fade or dim other things out of sight while inside
  (the collector and other scene objects, piles and their items, a
  scenario's sprites, ghouls once they have a height); decide whether
  the eye should follow the body within its tile (today the grid is from
  the tile's middle, so the wedge steps a tile at a time); look at ground
  on other levels in sight (a stair down into a pit: only the eye layer
  blocks, and floors below the player are lit as if level); consider a
  `view_mode` control op so demos can show both modes; tune the near-wall
  box (`NEAR_TILES`, `NEAR_WALL`) for a turned camera and the brightness
  by day and night (see "Lighting underground").
- **Where:** `app/src/cutaway.rs`, `app/src/sight.rs`,
  `app/shaders/cutaway.wesl`, `app/src/piles/`, `app/src/legacy.rs`.
- **Found in:** the line of sight prototype.

## Collapse and support

- **Status:** open
- **Why:** ground holds wherever it is (V6): a bridge can be dug to a
  one-cell thread, and a down dig may open into a cave below.
- **What to do:** decide a support rule (e.g. cells with nothing under
  them within N tiles fall), how falling cells and bodies on them behave,
  and whether a dig is refused instead.
- **Where:** `sim/src/volume.rs`, `Settlement::dig_housekeeping`.
- **Found in:** verticality design (V6).

## Pile reach in 3D

- **Status:** open
- **Why:** pile reach checks use the 2D distance, so a pile on the bridge
  is in reach from the cave under it.
- **What to do:** compare the floors too (the pile's `height` and the
  body's feet, within a layer or so) in the reach and interact checks.
- **Where:** `sim/src/settlement/piles.rs`, `interact.rs`.
- **Found in:** V.7's note on V.8.

## The jetpack into a 2 m tunnel

- **Status:** open
- **Why:** a jetpack hovering outside can't enter a dug tunnel exactly
  2 m high: its head at hover height is over the mouth's ceiling, so
  `flyable_at` refuses. Walking in, then holding it, works.
- **What to do:** while the jetpack is held and the way ahead is refused
  only by the ceiling, sink to the height that fits under it (feet at the
  ceiling less `BODY_HEIGHT`, not under the floor) instead of stopping.
- **Where:** `sim/src/settlement/avatar.rs` (`fly`, `step`),
  `Terrain::flyable_at`.
- **Found in:** V.4's note on V.8.

## Lighting underground

- **Status:** open
- **Why:** V.8 made underground readable with a lighter section colour
  and a fill light over the head under a ceiling, but the light is a
  plain point light without shadows (it lights through thin walls) and
  is the same by day and night.
- **What to do:** tune it with the day cycle (stronger at night or tied
  to a lantern the player carries), consider a shadowed light or an
  ambient term for covered faces in the cutaway shader, and darken far
  tunnel interiors for depth.
- **Where:** `app/src/cutaway.rs`, `app/shaders/cutaway.wesl`,
  `app/src/daylight.rs`.
- **Found in:** V.8.

## Done

### The Sacred Tree on a volume

- **Status:** done (branch `kestrel-sacred-tree-depths`)
- **Why:** the MVP's floors are only in the Tunnel Lab (V11); the town's
  map is still a heightfield, so its mine can't have tunnels.
- **What to do:** build the Sacred Tree's map (its hill, mine terraces
  and pits) as a volume, with `ground(p)` callers that matter there moved
  to `floor(p, z)` / `footprint_at`; check the golden and the soaks.
- **Where:** `sim/src/settlement/hill_tests.rs` (the map),
  `app/assets/levels/sacred_tree_supply.ron`, `sim/src/terrain.rs`,
  `app/src/digging.rs`.
- **Found in:** verticality design (V11), V.8.
- **Done:** as M8.3 of the depths design; see "M8.3 The town as a
  volume" in [`sacred-tree.md`](sacred-tree.md).

### Ghouls and haulers with a height

- **Status:** done (branch `kestrel-sacred-tree-depths`, M8.2 in `sacred-tree.md`)
- **Why:** in volume scenes only the avatar, sprites (the walker) and
  piles have a feet height (V.7); haulers, threats and chasers stand on
  the top floor, so a ghoul over a tunnel would walk on the mountain.
- **What to do:** give haulers and threats a feet height (hashed, a save
  format bump), path them with `nav::path_at`, and move `chase.rs` and
  `Threat::position` to it. Also still on the top floor: the jetpack's
  `GoTo` (a 2D route), sprite respawn (`sprite_housekeeping` puts a sprite
  home with its old height), piles made by a pile haul's drop (`PileLab`
  walk, height 0) and by `spill`, and `TossEnd::Pit` for a sprite's or
  avatar's pit dig. The walker falls instantly if a step drops it (paths
  never do) and steers round no one.
- **Where:** `sim/src/settlement/haulers.rs`, `chase.rs`, `threats`,
  `sprites.rs`, `piles.rs`.
- **Found in:** V.7's note on V.8.
- **Done:** in M8.2 ("M8.2 Every body on a floor" in `sacred-tree.md`):
  haulers and threats keep a feet height (save format 43), walk on foot by
  `walk_at`, plan with `nav::path_at` / `path_toward_at` (chases), back
  their replans off after a failed plan, and reach only bodies on their
  floor; the jetpack's `GoTo` flies over a 3D path, a respawned sprite
  stands on home's floor, pile-haul drops and spills lie on the dropper's
  floor and `TossEnd::Pit` carries its floor. Left: the walker still falls
  instantly and steers round no one (it is a test body); buildings and
  sites have no floor (notes on M8.3, M8.6).


### 3D paths to an unreachable floor

- **Status:** done (branch `kestrel-sacred-tree-depths`, M8.1 in `sacred-tree.md`)
- **Why:** `nav::path_at` to a goal it can't reach searches the whole
  window: in the soak about 1.2 s once, when the player stood in a pit the
  walker couldn't enter, and 20–100 ms a plan across the Tunnel Lab. The
  walker plans every half second.
- **What to do:** bound the search (a node budget, or a reachability
  check of the goal's floor first), or back the walker's replans off
  after a failed plan.
- **Where:** `sim/src/nav.rs`, `sim/src/settlement/follow.rs`.
- **Found in:** V.8 (the soak).
- **Done:** in M8.1 ("M8.1 Bounded 3D paths" in `sacred-tree.md`): a
  3D plan expands at most `nav::PATH_BUDGET` nodes, a flood back from the
  goal settles a shut-in goal (a pit, a mountain's top) in about twice its
  floor's size, `nav::path_toward_at` gives a partial route to the reached
  node nearest the goal, and the walker waits 2 s after a plan that didn't
  reach.

### Seeing inside by line of sight (prototype)

- **Status:** done (branch `kestrel-verticality-sight`, no PR yet)
- **Why:** V.6's cutaway cuts a fixed shape whatever the player can see,
  and nibbles at the mountain outside. The idea from a devlog the user
  shared ("Seamless Indoors-Outdoors Transition"): you only see the inside
  when you're inside.
- **What to do:** a prototype, the user's chosen options:
  - outside (no ceiling over the head) nothing is cut;
  - inside, everything above the current level's ceiling is hidden over
    the whole view (the mountain goes), fading in and out over about
    0.3 s as you cross a mouth;
  - a flat line-of-sight pass on the player's level from head height:
    what is in sight is lit, falling off with distance; what isn't is
    dimmed, not black; the view out of a mouth has soft edges;
  - other bodies out of sight fade out while inside;
  - no fog of war (seen places don't stay revealed);
  - view only: the sim, saves and the net protocol don't change;
  - a key toggles back to V.6's cutaway to compare.
- **Where:** `app/src/cutaway.rs`, its shaders, a new visibility module.
- **Found in:** verticality follow-up, the user's devlog reference.
- **Done:** the default view in volume scenes (`cutaway::ViewMode::Sight`);
  **F7** (`cutaway::TOGGLE`; F3 and F4 are taken, F7 is free in the app and
  the control protocol) switches to V.6's cylinder (`ViewMode::Cutaway`)
  and back, with a corner label ("VIEW: LINE OF SIGHT (F7)" / "VIEW:
  CUTAWAY (F7)", bottom left, 2.5 s); V.6's path is unchanged.
  Inside test (`cutaway::follow`): `Terrain::ceiling_at` over the drawn
  feet with a 0.7 m reach (`INSIDE_REACH`, wider than the body's 0.3 m,
  so the roof starts going as the body goes under it rather than after
  it is hidden); the factor moves at a constant rate, 0 to 1 in 0.3 s
  (`sight::ease`, `sight::FADE`), drawn smoothstepped (`sight::shown`).
  Hiding (`hidden` in both shaders, so the depth prepass cuts alike):
  ground over the ceiling's height less 0.05 m (`HIDE_MARGIN`; the last
  ceiling is kept while fading out) is discarded over the whole view by
  the 4 × 4 ordered dither against the factor, so the mountain dissolves
  in; walls under it stand, their cut tops the section colour. A 3 m wall
  on the camera's side hid the body in the 3 m tunnel, so in a blocky box
  of tiles from the player's tile to 4 tiles toward the camera along x
  and z (`NEAR_TILES`) the plane is 1 m over the feet (`NEAR_WALL`); the
  box's sides sit just off tile edges so walls are cut whole (a smooth
  notch was tried and looked organic on voxels). Outside nothing is cut
  (the V.6 cylinder is off in this mode); the mountain at the spawn is
  whole. Visibility (`app/src/sight.rs`, pure): a 64 × 64 tile window
  (`WINDOW`, the whole map) round the eye tile; a tile blocks when its
  cell at layer floor + 3 (1.5–2 m over the feet, `EYE`) is solid
  (`sight::blocks`); from five points in the eye tile to five in each
  target tile, 25 rays, a grid walk (Amanatides–Woo) that must cross
  only clear tiles before the target; the value is the share that
  arrive (soft wedge sides); solid tiles take the best of their four
  clear neighbours (their faces); then every tile at least 0.5 of its
  brightest neighbour (`SPREAD`, a one-tile outward glow); recomputed only
  when the eye tile, floor layer or `Terrain::revision` changes
  (`sight::Key`). It goes to an R8 texture (linear sampling, world x–z
  to uv) bound to `CutawayMaterial` (bindings 101/102, new uniform
  fields `sight`, `window`, `eye`, `light`, `inside`). Shading
  (`shade` in `cutaway.wesl`, blended by the factor): in sight,
  (lit colour + 0.35 × albedo, `INSIDE_LIGHT`, since the mountain's
  shadow still lies on the floors) × a falloff, full within 6 m (`NEAR`)
  to 40% by 20 m (`FAR`, `FAR_LIGHT`); out of sight 25% (`DIM`), 35% of
  the way to grey (`GREY`); the section out of sight 12% (`SECTION_DIM`),
  so floors in sight read against the cut tops; a back face is shaded
  where its view ray crosses the cut (`cap`), not deep down where it lies.
  The fill light and shadows are as they were. Bodies: the test walker's
  body has its own materials and fades by `SightView::body_alpha` (its
  tile's visibility, 0 over the hidden height), hidden under 3%; the
  player always shows. Tests in `sight::tests`:
  `a_straight_tunnel_is_visible_end_to_end`,
  `a_room_round_a_corner_is_not_visible`,
  `the_view_out_of_a_mouth_widens_with_distance_and_has_a_soft_edge`,
  `solid_walls_bordering_visible_floor_are_visible`,
  `a_grid_is_recomputed_only_when_its_key_changes`,
  `the_eye_band_is_the_head_not_the_feet`,
  `in_the_tunnel_lab_the_tunnel_and_its_mouths_are_in_sight` and
  `the_inside_factor_eases_over_the_fade`; `//kestrel/app:movement_test`
  286/286. `app/demos/tunnel_lab.json` now also stops just inside the
  west (x 7) and east (x 21) mouths. Screenshots (local
  `tunnel_lab_demo`): outside by the mouth (whole mountain), mid-fade at
  the west mouth, deep in the tunnel (a walled corridor, the body clear
  in the near-wall box), looking out of each mouth (the lit wedge on the
  ground outside), in the cave (its walls as boxes, the bridge gone), on
  the bridge and at the dug stair (outside: nothing cut). Left open: the
  entry "Line of sight follow-ups" above.

### V.8 Soak and demo

- **Status:** done (branch `kestrel-verticality`, no PR yet)
- **Why:** the MVP needs a stable run and a look.
- **What to do:** a soak test digging at random in the Tunnel Lab with
  `validate` every tick; a demo video walking through the tunnel, under
  the bridge, digging a stair down; the full suite; a `docs/test-time.md`
  row. (Notes from V.4, V.6 and V.7 on the jetpack into a 2 m tunnel, the
  dark underground and what still stands on the top floor are now open
  entries above.)
- **Where:** sim tests, the demo scripts.
- **Found in:** verticality design.
- **Tests:** the soak; the full suite.
- **Done:** the soak, `the_tunnel_lab_soaks_walking_and_digging_at_random`
  in `sim/src/settlement/soak_tests.rs`: two minutes (7200 ticks) of the
  Tunnel Lab with its walker, from a fixed seed (a small xorshift). When
  free, the avatar either digs (to its tile's middle by a `GoTo`, a
  two-tick step one of sixteen headings, mostly toward a wall beside the
  tile, trying up to eight other headings while only a down dig would go;
  then `DigFront` level, down or up, mostly one `dig_volume_of` accepts),
  or walks to a random point by its 3D route (anywhere, or by the west
  cut's and tunnel's walls, the cave, the bridge's stairs or the
  mountain's foot; it digs on arrival), or walks a random heading for
  0.5–2.5 s. Every tick: `validate`, and both the avatar and the walker
  stand free (`standing_free`: `Terrain::footprint_at` has a floor at or
  under the feet, `ceiling_at` is at least `BODY_HEIGHT` over them, and
  the middle tile's cells from a layer over the feet to the head are
  empty); then a second run must hash alike every tick. It asserts at
  least two digs of each direction and ten in all (the seed digs 2 level,
  10 down and 3 up, with 7 refusals, all `Clear`) and five walks; about
  3.3 s for both runs. The soak found the walker stuck: a path's segments
  are checked every 0.1 m (`walk_at`), so a step along one can clip a
  wall's corner the samples skipped; the walker then dropped its route
  and replanned every tick, standing there for good (about 20 ms a
  tick). `follow::self_update` now slides along the wall (the step's x or
  y alone) before giving up. Polish: in volume scenes the cutaway's
  section colour is lighter (sRGB 0.3, 0.25, 0.21 from 0.11, 0.085,
  0.075), and a fill light (`cutaway::FillLight`, a warm point light
  `FILL_HEIGHT` 1.8 m over the feet, `FILL_LUMENS` 200000, range 8 m, no
  shadow map, so the sun's shadows stay) eases in under a ceiling and out
  in the open (`cutaway::follow`); the pile lab's help panel in a volume
  scene reads "F: dig in front (2 s) / Ctrl+F: dig a step down / Alt+F:
  dig a step up / Ore lands on a pile nearby / ... / SPACE: jetpack"
  (`piles/view.rs`). Demo: `//kestrel/app:tunnel_lab_demo_video` (a
  `filegroup` of `tunnel_lab_demo`'s video, as `laser_pit_demo_video`),
  built locally; `demos/tunnel_lab.json` unchanged (spawn and mountain,
  the tunnel, the cave, the stairs onto the bridge with the walker, a
  stair dug down, level and up, and back up it). Left open (entries
  above): the Sacred Tree on a volume, ghouls and haulers with a height,
  collapse, pile reach in 3D, the jetpack into a 2 m tunnel, 3D paths to
  an unreachable floor and lighting underground. Full suite 27/27
  (`bazel test --config=remote --build_tests_only //kestrel/...`).

### V.7 Paths in 3D and the test walker

- **Status:** done (branch `kestrel-verticality`, no PR yet)
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
- **Done:** 3D nav in `sim/src/nav.rs`: `nav::path_at(terrain, start, z,
  goal, goal_z, radius, extra)` returns waypoints with the feet's height at
  each. Nodes: the half-tile lattice's points (the same window as 2D nav),
  one per floor there a body fits on, at `Terrain::footprint_at` of the
  floor's height, made lazily as the search reaches a point (`Floors`).
  Edges: the 8 neighbours where `Terrain::walk_at` (new: `walkable_at`'s
  rule every 0.1 m on the five footprint points, but strict, returning
  the end feet height: a step of at most a layer up or down, headroom all
  the way, no drops of more than a layer; drops are not edges) reaches a
  node of the neighbour within a layer, plus the terrain's obstacle discs.
  A* with the 2D octile costs and heuristic, ties broken by node index
  (nodes are numbered in creation order, itself deterministic). The start
  attaches to the nearest node it walks to; the goal to every node near
  it that walks onto the wanted floor (`goal_z`: the floor
  `Terrain::footprint_near` picks, within a layer; `None`: any floor, the
  cheapest reached first). String pulling goes forward, stopping at the
  first node a straight `walk_at` misses, at most `MAX_SEGMENT` (12) nodes
  a segment. Without a volume `path_at` is `path_avoiding`, and no caller
  uses it there: every 2D path is unchanged. `Terrain::footprint_near(p,
  z, radius)` (new): the floor of `p`'s column nearest `z` with room for
  the body (the top floor's `footprint` if none). Agents: sprites keep a
  feet height, `SpriteState::height` (zero outside volume scenes), set at
  setup (`follow::start_height`, the floor nearest 0 m) and by the walker;
  haulers and threats still stand on `ground` (see V.8). Piles:
  `Pile::height` (zero outside volume scenes); a volume dig's ore goes
  through `PileLab::mined_at` with the dig's first cleared layer, which
  picks a tile with a floor within a layer of it (instead of the top
  floor's pit/steepness checks) and lies on that floor, so tunnel ore lands
  in the tunnel; a bundle dropped in a volume lies on the dropper's floor;
  the view draws piles (`app/src/piles/mod.rs`) and tosses (`piles/transfer.rs`) at
  it. Save format 42 (`SpriteState::height`, `Pile::height`,
  `SpriteJob::Follow` appended); the behaviour digest adds sprites' heights
  only when one is nonzero, so golden lines (and the forest and arena
  totals, which have no sprites or piles) are unchanged; the golden test
  passes as is. The avatar's `GoTo` (`avatar::set_intent`, so also the
  control/demo `walk` op, which sends a `GoTo`) plans with `path_at` (goal
  floor `None`) in volume scenes on foot; with the jetpack it keeps the 2D
  route. Feet placement: the scene spawn (`from_scene`: the floor nearest
  the spawn's height, or the spawn's height if over it), respawn, joining
  (`players::join`), leaving a building (`avatar`, `buildings`, `vehicle`'s
  seat housekeeping) and the vehicle exit spot use `footprint_near`
  (identical to `footprint` without a volume). Walker: `SpriteJob::Follow`
  (appended), a sprite brain of its own (`sim/src/settlement/follow.rs`):
  every `REPLAN_TICKS` (30, half a second) a `path_at` to the player's
  floor (the player's position and feet come in through `Ctx::leader`),
  then a walk of its character's walk speed per tick, each step a
  `walk_at` (its feet follow; a refused step drops the route and replans
  next tick), stopping within `KEEP` (2 m) of the player on the same floor
  (1 m). It steers round no one. The scene file's new `walkers: [(x, y,
  z)]` (assetc: up to 8, inside the volume, needs one) becomes
  `SceneSetup::walkers`, sprites of the `sprite` character with
  `scenario::Sprite::follow` (new, defaults false) appended to the scene's
  scenario (`Settlement::with_walkers`); `tunnel_lab.ron` has one at (-2,
  3). A scenario plus a style for the walker's skin was tried first and
  dropped: a style and a scenario change the Tunnel Lab's HUD and the
  player's rig. So the view draws it with `app/src/walker_view.rs`: a
  small blocky body (no animation), between ticks, at its feet height. The
  settlement view also draws a scenario's sprites at their feet in volume
  scenes (`legacy::AgentState::feet`), and a sprite's dig bar sits at its
  feet (`digging::shown_digs`), for when the Sacred Tree moves over. Tests
  in `sim/src/settlement/path_tests.rs`: `a_path_runs_through_the_tunnel`
  (in at the west mouth, out east, 0 m all the way),
  `a_path_climbs_from_under_the_bridge_onto_it` (up the stairs, ends at
  5 m over (-11, 13); and back down),
  `no_path_through_a_passage_without_headroom`,
  `a_path_goes_down_a_dug_stair` (and none down a sheer 2 m),
  `paths_in_3d_are_deterministic`,
  `the_walker_follows_the_player_into_the_tunnel_and_onto_the_bridge`
  (the player's `GoTo`s ending on the bridge too) and
  `the_walker_replays_as_the_simulation_does`; in `volume_dig_tests`,
  `ore_cells_drop_their_yield_onto_piles` now checks the piles lie on the
  tunnel's floor; in `save`, `a_volume_scene_keeps_a_sprites_height`.
  `app/demos/tunnel_lab.json` waits in the tunnel and on the bridge for
  the walker to catch up (it walks 1.6 m/s, the player 3.8).

### V.6 The cutaway

- **Status:** done (branch `kestrel-verticality`, no PR yet)
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
- **Done:** `app/src/cutaway.rs`: a volume scene's ground chunks use
  `CutawayMaterial` = `ExtendedMaterial<StandardMaterial, CutawayExt>`
  (the same vertex-coloured base, drawn with `cull_mode: None`), with
  shaders `app/shaders/cutaway.wesl` (main pass) and
  `cutaway_prepass.wesl` (the camera's depth/normal/motion prepass, which
  SSAO and TAA turn on, so it must cut the same hole); other scenes keep
  `GroundMaterial`. The cut: a cylinder round the ray from the middle of
  the body (feet + 1 m) toward the camera (the orthographic camera's
  backward axis, set each frame by `cutaway::follow` with the drawn
  player's position), capped by a half sphere round the body. Inside it
  the ground over the head plane (feet + `BODY_HEIGHT` + 0.25 m) goes:
  ceilings and the hill between camera and player, while far walls stand;
  on the camera's side of the body a narrower cylinder (`LOW_RADIUS` 2.25
  m) also cuts down to 0.5 m over the feet, so a tunnel's near wall or a
  dug stair's side hiding the body goes down to the shins. Floors at the
  feet are never cut (a bridge underfoot stays). The radius over the head
  is 4.5 m (`RADIUS`) while the player is under a ceiling
  (`Terrain::ceiling_at`), easing to 2.25 m in the open, so outside the
  terraces beside the player keep their shape (a tried always-4.5 m hole
  bit a dark crater out of the mountain at the spawn). Soft edge: the
  last 1 m of the radius is an ordered 4 × 4 dithered discard, so the
  ground stays opaque. Section: back faces (the inside of solid ground
  seen through the hole) are drawn flat `SECTION` (sRGB 0.11, 0.085,
  0.075), unlit. Shadows are the uncut ground: opaque shadow passes run no
  fragment shader, and a directional light's emulated unclipped depth
  (`UNCLIPPED_DEPTH_ORTHO_EMULATION`) skips the cut, so the mountain still
  shades the tunnel. The `UV_0.x` cut flag is unused. The Rust mirror of
  the shaders' test is `cutaway::Cut::amount`; tests in
  `cutaway::tests`: `ground_over_the_head_or_toward_the_camera_is_cut`,
  `floors_and_far_walls_are_not_cut`,
  `under_the_head_only_a_narrow_hole_is_cut`,
  `ground_outside_the_radius_is_not_cut` and
  `the_edge_fades_across_its_band`. `app/demos/tunnel_lab.json` now also
  climbs the bridge's west stairs and stands on it over the cave (-11, 13)
  at 5 m. Screenshots (local `tunnel_lab_demo`): in the tunnel and in the
  cave under the bridge (the avatar in a dark-sectioned hole), on the
  bridge (nothing cut), outside at the spawn and beside the bridge (a
  faint nibble at most) and in the dug stair (the trench's near side cut,
  the body visible). The dug stair was not brightened: the cave and tunnel
  floors are in the mountain's shadow and read close to the section
  colour (see the note on V.8).

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
