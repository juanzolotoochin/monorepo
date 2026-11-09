# Sacred Tree

The town MVP's design and decisions are in
[`kestrel/SACRED_TREE_HANDOFF.md`](../SACRED_TREE_HANDOFF.md). Tasks follow
its milestones, in order; one PR per milestone.

## M9.1 The fog grid from many eyes

- **Status:** open
- **Why:** fog of war ([`SACRED_TREE_FOG.md`](../SACRED_TREE_FOG.md))
  needs to know, per tile and level, what the team sees now and has seen.
- **What to do:** a view-only, pure fog grid over the whole town, per
  level: in sight, seen before, never seen (F2, F3, F9). Eyes (F1, F8):
  every player's elder or possessed sprite (about 14 m), every sprite
  (about 6 m), the tree (about 8 m), buildings (about 4 m), and lit
  lanterns (the light grid's tiles). Night halves units' ranges, not
  light's; underground is always night (F4). Ground over eye height blocks
  (F5), reusing `sight.rs`'s ray rules. Each eye recomputed only when its
  tile, level or the ground changes. Start with the town round the tree
  seen (F9).
- **Where:** `app/src/sight.rs` (or a new `app/src/fog.rs`),
  `sim/src/settlement/light.rs` (read only).
- **Found in:** fog design.
- **Tests:** ranges, ground blocking, night, lanterns, levels, memory,
  team eyes adding up; a cost test on the town with every eye.

## M9.2 Drawing the fog

- **Status:** open
- **Why:** the fog grid must show.
- **What to do:** the shader draws in sight as today, seen-before dimmed
  and desaturated, never-seen black, with soft edges (F2). The fog
  replaces the inside view's dimming; inside, the ground over you still
  fades (F10). F7 cycles fog, the inside view as it is now, and the
  cutaway; only the town has fog (F11).
- **Where:** `app/src/cutaway.rs`, `app/shaders/cutaway.wesl`,
  `app/src/sight.rs`.
- **Found in:** fog design.
- **Tests:** mode cycling; other scenes unchanged; screenshots by day, at
  night and underground.

## M9.3 Hiding things in the fog

- **Status:** open
- **Why:** the fog must hide what the team can't see.
- **What to do:** ghouls and other moving things are drawn only in sight;
  buildings and piles in remembered ground show as last seen (F2), with
  their labels. A ghoul within about 12 m of any elder, out of sight, shows
  only its glowing eyes (through rock too) and growls (F6). Widen
  `SightView::shows` from the inside view to the fog.
- **Where:** `app/src/sight.rs`, `app/src/legacy.rs`, the piles, buildings
  and ghoul views, audio.
- **Found in:** fog design.
- **Tests:** a hidden ghoul isn't drawn; eyes within 12 m only; last-seen
  buildings and piles.

## M9.4 Demo and review

- **Status:** open
- **Why:** the prototype is reviewed by video.
- **What to do:** a demo script and video: the elder walks out from the
  tree by day, the fog opening and staying remembered, the hill as a
  lookout, a lantern's pool, dusk shortening sight, ghouls' eyes in the
  fog, then in sight, and a tunnel. Screenshots; the full suite; a
  `docs/test-time.md` row for M9; the design's status and what changed.
- **Where:** `app/demos/`, `app/BUILD.bazel`, docs.
- **Found in:** fog design.
- **Tests:** the full suite.

## Depths follow-ups

- **Status:** open
- **Why:** what M8 left for later.
- **What to do:** wave routes planned before a lantern goes up aren't
  replanned for its light; a stranded miner's retreat picks the nearest
  tile by straight distance (it may be past a wall); a traffic detour
  planned from a swerve into the dark ignores the light rule; dawn still
  burns ghouls underground (decide if an unlit ghoul there outlives it);
  miners supply only lanterns' sites; a lantern's post blocks no one; the
  ground over a tunnel lantern is protected from digs at every depth; no
  reachability check when a lantern is placed; a lantern lights floors
  within a layer of its own, so a stair needs one every few steps (or let
  light follow a stair); the elder's lamp still doesn't count (L5); the test
  walker and haulers ignore light; a dig's ore is still lost once the pile
  lab is at its pile cap (the soak met 14 amber cells and got 8 amber);
  the demo's tunnel shots are dark and far (a closer camera underground);
  M7's night lanterns stay decoration; amber income: revisit with a
  playtest (one a cell, about a lantern a day).
- **Where:** `sim/src/settlement/chase.rs`, `miners.rs`, `light.rs`,
  `piles.rs`, `app/src/cutaway.rs`.
- **Found in:** M8.1–M8.8.
- **Tests:** per item.

## A possessed ballista fires faster

- **Status:** open
- **Why:** possessing a gatherer speeds it up, but possessing a ballista
  only adds aiming and the fireball (ballista design, "Settled").
- **What to do:** decide after the playtest whether a possessed ballista
  also fires faster.
- **Found in:** ballista design.

## Repairing buildings

- **Status:** open
- **Why:** a broken building is lost back to a site, and nothing repairs a
  damaged one (B8).
- **What to do:** decide after the playtest, for example delivering 1 wood
  to restore health.
- **Found in:** ballista design.

## Art for the Sacred Tree

- **Status:** open
- **Why:** the scene stands in existing art: the elder is the `exosuit`
  actor, sprites the `worker` skin, the gatherers the forest style's `mill`
  and `factory`, wood and stone its `item_lumber` and `item_iron`, and the
  tree plain architecture.
- **What to do:** a flame actor for the elder (and its flame form in a
  building, D3), small sprite skin variants, Wood and Stone Gatherer skins,
  wood and stone item skins, and a Sacred Tree that reads as the center of
  town, probably in a style of its own instead of `styles/forest.ron`.
- **Where:** `kestrel/app/actors/`, `kestrel/app/assets/styles/`,
  `kestrel/content/data/character/elder.ron` and `sprite.ron`,
  `kestrel/app/assets/levels/sacred_tree*.ron`.
- **From M1.4:** sprites are the `worker` skin, told apart only by size,
  with a glowing ball over the head as a stand-in flame
  (`settlement::town`); a flame skin can take its activity color and
  motion from the same place. The camera shows the whole 26 m clearing, so
  the tags of the four sprites standing together at the start still touch.
  Since M4.1 a tag shows the name alone until the elder is within 2 m
  (`settlement::TAG_UNFOLD`), a sprite working inside has none, and every
  other tag stands 0.4 m higher (was 1 m, which reached the tree's piles);
  a closer camera would still read better.
- **From M2.3:** a site is its kind's skin scaled down per installed part
  (`settlement::town`, `grown`) on a plot ringed in gizmos (`input::hud`),
  and the ghost is the skin with every material tinted (`rig::Fade::tint`);
  per-part skins (a foundation, then the mechanism) would read better. The
  canopy that fades is the scene's raised architecture within 4 m of its
  center (`canopy.rs`); a tree in a style of its own should keep its canopy
  findable that way, or mark it.
- **From M3.1:** a possessing elder's rig is only hidden
  (`settlement::hide_possessing`); its flame form in the building is still
  to draw. The "E  LEAVE" prompt at the slot spot overlaps the tag of a
  sprite put out beside the building.
- **From M2.4:** a ground pile is its item's skin stacked as an output's
  units are (`settlement::town`, `OUTPUT_LAYOUT`); a loose heap, like the
  pile lab's mound (`pile_model::unit_positions`), would read more like
  something dropped.
- **Found in:** M1.1.

## A scene with no weapon

- **Status:** open
- **Why:** the elder has no weapon, but a scenario with an empty `weapons`
  list still offers the sword (`combat::offers_weapon`, as the original
  game did), so the elder can swing one.
- **What to do:** let a scenario say it offers no weapon (for example an
  explicit empty list that differs from a missing one), and have the
  attack command do nothing there.
- **Where:** `kestrel/content/src/scenario.rs`,
  `kestrel/sim/src/settlement/combat.rs`, `kestrel/app/src/input/gameplay.rs`.
- **Since the view turn:** keyboard Z and C turn the town's view, so
  only X, V and controller Y could still choose a weapon there.
- **Found in:** M1.1.

## Interact feedback and the mouse

- **Status:** open
- **Why:** three rough edges M2.2 left. The job menu has no mouse: its
  ring is picked by direction keys, the stick or the D-pad only. Walking
  off cancels a pending deposit or withdrawal at a tree pile (the app's
  `movement` cancels jobs `manual_jobs` lists, depot jobs only) but not a
  pickup at an output, which waits until the elder is back in the dock's
  reach. And letting go of a walk key just after pressing E sends Stop,
  which ends the elder's step in, so the job waits the same way.
- **What to do:** hover and click the ring's options; list output jobs in
  `manual_jobs` (or cancel by the carry job itself) so walking off cancels
  both alike; keep the step in through a Stop, or cancel the job with it.
  A delivery to a site (M2.3) is a carry job on `InventoryRef::Site` and
  waits the same way, and so does a drop or a ground pile pickup (M2.4,
  `InventoryRef::Pile`): the elder walking off before its units moved
  leaves them waiting until it is back within the dock's reach. A site's
  needs readout sends nothing: pressing E shows its prompt over the elder
  as a refusal; a blueprint refused in
  stage 5 (`Event::BlueprintRefused`) names no player, so every client
  shows it.
- **Where:** `kestrel/app/src/input/town.rs`, `gameplay.rs`,
  `kestrel/app/src/legacy.rs` (`manual_jobs`), `kestrel/app/src/settlement.rs`.
- **From M3.1:** a G press while carrying shows "Drop the bundle first
  (Q)"; E offers no slot then, so nothing says why. (Since M4.1, a sprite
  working inside is no interact target, so E possesses a staffed building
  as G does; the job menu is for sprites standing outside.)
- **Found in:** M2.2.

## Moats make the tree safe

- **Status:** open
- **Why:** since M6.7 a ghoul with no walkable way to the tree walks to the
  reachable point nearest it and waits there (`Threat::stranded`). A moat
  two layers deep round both ramps' feet (about 124 tile-layers, four or
  five minutes of the elder's digging) leaves every wave waiting at the
  cliff's foot, 16 m from the tree: nothing breaches, and the defence is
  trivial. Cutting one ramp only sends its ghouls round to the other
  (`lane_tests`).
- **What to do:** decide after the playtest whether that is a fair
  strategy (it costs the calm start and more), or whether ghouls should
  answer it: dig or fill their way through, climb a cliff slowly, or the
  ramps' feet stay undiggable for a few tiles.
- **Where:** `kestrel/sim/src/settlement/chase.rs` (`route_home`,
  `retry_stranded`), `mine.rs` (`Hill::ranges`).
- **Found in:** M6.7.

## Fled sprites and ghouls that linger

- **Status:** open
- **Why:** a sprite that fled a ghoul (M6.7) goes back to work once no
  ghoul chases it or is within three detect radii (9 m). A miner whose
  stone lies past a ghoul that lingers (a stranded one, or one chasing
  someone else) walks back toward it, is threatened again and runs home:
  it can go back and forth without digging. Miners also plan without
  looking at ghouls, so one may pick stone beside the column's way.
- **What to do:** watch it in the playtest; if it matters, let a miner
  give up targets near a ghoul until the ghoul goes (as `unreachable`
  does for routes), or stay home while a wave is out.
- **Where:** `kestrel/sim/src/settlement/sprites.rs` (`Alarm`, `replan`),
  `miners.rs` (`plan`).
- **Found in:** M6.7.

## Stone yield and veins in content

- **Status:** open
- **Why:** M6.7 tuned the waves in the supply file, but a stone layer's
  3–6 (`Mine::yield_of`) and the 4 guaranteed exposed veins
  (`EXPOSED_VEINS`, two layers deep) are code, so tuning them needs a
  build, not a content edit.
- **What to do:** move them to the scene's `mine:` line (with the
  `voxel_assets` scene schema) if the playtest wants them tuned.
- **Where:** `kestrel/sim/src/mine.rs`, `kestrel/tools/assetc/core`.
- **Found in:** M6.7.

## Pale stripes on far terraces

- **Status:** open
- **Why:** seen at a low angle from the foot of the Sacred Tree's east
  ramp, the far terraces show tall, pale, one-tile-wide vertical walls
  (M6.6, checked again in M6.7's demo frames). `sloped_chunk_mesh` draws
  a wall only from a steep tile down to a lower neighbour, or down past the
  deepest layer at the arena's edge, and the town's generator makes no
  drop tiles, so the cause is still unknown.
- **What to do:** sample the sim's heights under one stripe (the demo's
  first seconds) and compare with the chunk's walls; check `Heights::read`'s
  window at chunk borders and walls facing the camera on terraces that
  rise past the plateau.
- **Where:** `kestrel/app/src/digging.rs` (`sloped_chunk_mesh`, `drawn`,
  `Heights`).
- **Found in:** M6.6, M6.7.

## The dawn's dispel effect

- **Status:** open
- **Why:** at sunrise a ghoul burns (its fire flames show) and then just
  vanishes: `Event::Dispelled` (M7.2) has no effect or sound in the app.
- **What to do:** a fall, ash or smoke effect and a soft cue on
  `legacy::Event::Dispelled`, in the style of the fire kill.
- **Where:** `kestrel/app/src/settlement.rs` (events to effects and cues),
  `app/src/effects.rs`.
- **Found in:** M7.2.

## Night lights polish

- **Status:** open
- **Why:** at the town zoom a ghoul's glowing eyes (M7.4) are a pixel or
  two and show only when it faces the camera; the tree's warm pool shows
  faint concentric bands on the grass; lanterns are drawn only, so the
  elder and sprites walk through their posts.
- **What to do:** bigger or brighter eyes, or a faint glow on the ghoul's
  head readable from behind; check the banding (tonemapping, the point
  light's range falloff or the grass's voxels); make lanterns solid (sim
  obstacles from the scene, keeping routes and the happy path) if walking
  through them reads badly in the playtest.
- **Where:** `app/assets/styles/forest.ron` (`ghoul` skin, `eyes`),
  `app/src/daylight.rs`, `app/assets/levels/sacred_tree.ron`.
- **Found in:** M7.4.

## Clock dial polish

- **Status:** open
- **Why:** the dial reads, but a few edges were left as they are.
- **What to do:** by night the dial gives no countdown to dawn (a "DAWN IN
  0:20" in the night's last seconds would help a player holding out); with
  three lines (day, warning, nights survived) the dial reaches about 140 px
  down the right edge and can sit under the F3 metrics overlay (`telemetry.rs`,
  top 135 px), so move one of them; nights survived shows from the first
  dawn, not only once the last listed night repeats (N10 allows either);
  the dome is 64 x 34 px, small on the Steam Deck: check it there.
- **Where:** `app/src/clock_hud.rs`, `app/src/telemetry.rs`.
- **Found in:** M7.5.

## Done

### M8.8 Tuning, soak and demo

- **Status:** done (branch `kestrel-sacred-tree-depths`)
- **Why:** the milestone needs a stable run and a look.
- **What to do:** tune amber, lantern and wave numbers; a soak over
  several days and nights with digging and lanterns; a demo video of a
  day and a night underground; the full suite; a `docs/test-time.md` row.
- **Where:** sim tests, the demo scripts.
- **Found in:** depths design.
- **Tests:** the soak; the full suite.
  Note (from M8.1): count the town soak's plans as the Tunnel Lab soak
  does (`Dug::plans`, `unreached`, `most_expanded`) and assert they stay
  within `nav::PATH_BUDGET`. The Tunnel Lab soak's seed never planned to an
  unreachable goal (240 plans, none unreached, at most 902 nodes); the
  town's should, with ghouls chasing the elder up where they can't go.
  Note (from M8.3): amber numbers (`mine::amber`, the mountain's boxes),
  the town save's size (1.5 MB with its volume) and the quarter-meter
  rounding of the terraces are for here.
  Note (from M8.4): miners' numbers (`miners::AMBER_WORTH` 4,
  `TILES_PER_WORTH` 2, `VOLUME_SEARCH_TILES`, `WALL_SEARCH_NODES`): today
  they mine the nearest stone walls (49 stone in 5 min for two) and go to
  the cave's amber only once there's no nearer wall. A walker
  (`haulers::move_worker`) can dither for minutes where a string-pulled 3D
  route cuts diagonally from a sloped stair step onto the ground beside it
  (a trench a layer a tile down, its first step a layer under the ground):
  the route was checked with the node's feet, the walker's are lower on the
  slope and the step up is refused, and its swerves keep it there.
  `miner_tests::tunnel` walls its trench to avoid it; dropping the route on
  a refused straight step was tried and made it worse (the replan from
  there found no route and the miner gave the wall up). Fix in nav (check
  pulled segments with a margin) or the walker.
  Note (from M8.6): the demo's Reed, carrying stone to the cave's lantern,
  dithered 20 s at the plateau's north edge (about x -11, z -16) before
  going round by the west ramp: the same walker trouble. Lantern numbers:
  range 6 m, falloff `1 - (d / range)²`, the shader's `LANTERN` 2.0 and
  `LANTERN_TINT`, the point light (`settlement::LANTERN_LUMENS` 60000, 6 m).
  Whether miners should also supply other kinds' sites, a lantern's post
  blocking bodies, and a tunnel lantern protecting the ground over it from
  digs are open.
  Note (from M8.7): light. Miners mine only walls joined to the stone pile
  by lit floors, so a deep stretch needs a chain of lanterns (one about
  every 10 m): tune the range or say so in the demo. A ghoul's lit cost
  (`chase::LIT_STEP` 30) makes its plans into a lit stretch expand more
  nodes: count them in the soak against `nav::PATH_BUDGET`. Wave routes
  (`WaveState::route`) are planned once and replanned on ground changes,
  not when a lantern's light changes. Dawn still burns every ghoul,
  underground too: decide whether an unlit ghoul underground outlives it.
  A stranded miner's retreat (`Targets::retreat`) picks the nearest tile in
  reach by straight distance, which may be past a wall (it gives it up
  after two asks, then goes home); a traffic detour planned from a swerve
  into the dark ignores the rule (rare). The elder's lamp still doesn't
  count (L5); the test walker and haulers ignore light.

- **Done:** **F on flat ground**: in volume scenes the plain dig (F,
  `DigDown`, the control `dig`) digs level, or a step down where every cell
  a level dig would clear is already empty (`View::dig_volume_plain`,
  `Settlement::dig_plain_of`, the app's `volume_dig(None)`); Ctrl+F and
  Alt+F keep their one direction. HUD: `F DIG (DOWN ON FLAT GROUND) /
  CTRL+F DIG DOWN / ALT+F DIG UP`, and the pile lab's help. Test
  `volume_dig_tests::the_plain_dig_goes_level_into_a_wall_and_down_on_flat_ground`.
  **The walker dithering (root cause)**: `haulers::move_worker` passed a
  waypoint once within 0.1 m of it, and a 3D route's next leg is checked
  from the waypoint itself (`nav::plan_with`'s string pulling); one that
  climbs sideways off a sloped stair step onto the ground beside it walks
  from the waypoint (feet -2.89) and not from 8 cm down the slope short of
  it (feet -2.93), so the straight step was refused and the swerves kept
  the walker sliding along the step for minutes. In a volume a waypoint is
  now passed only once the next leg walks from where the walker is (else it
  walks on to the waypoint itself), and walkers in a volume don't skip
  waypoints (a skip could end on another floor than the route's: a miner
  sent up a ramp's side instead of along its foot). Test
  `miner_tests::a_miner_climbs_out_of_an_open_trench_sideways` (the
  tunnel's trench open to the ground beside it: undelivered after 300 s
  before, delivered at 86 s now, never 5 s in one spot); the walled tunnel's
  amber now comes up at 88 s instead of 409 s. **The town soak**
  (`soak_tests::the_town_digs_and_lights_its_depths_through_three_nights`,
  6 s): three days and nights, two ballistas manned, Moss and Reed mining,
  the elder defending while ghouls are near the tree and otherwise digging
  a stair 6 m down from the north foot and a tunnel north (the plain dig),
  picking up its amber, carrying piles in its way out and putting up a
  lantern every 8 m the tree can pay for; validate and the balances every
  tick. Every 3D plan the town's bodies make is counted (test-only
  `nav::tally`, a thread-local read on the simulation's pool with
  `Simulation::on_pool`): 417 plans, 27 short of their goal, 6 at
  `PATH_BUDGET` (the most expanded 16384), none over. The town: tree 20 of
  20, 3 nights survived, 24 kills, no breaches; 124 tiles dug, 14 amber
  cells met, 8 amber and 1026 stone dug, 7 lantern blueprints, 4 built, 35
  floors lit; no ghoul went underground. **Tuning, from the soak**: the
  first soak's 117-tile tunnel 5 m down met no amber at all (the pockets
  were 6-tile blobs, a 40 m tunnel at 5 m met none 59% of the time); amber
  now on a 3-tile noise grid (`mine::amber`: about as much amber, 3.75% of
  the deep cells; a 40 m tunnel at 5 m meets none 41% of the time, at 6 m
  29% instead of 51%) and a town's amber cell yields one amber
  (`mine::AMBER_YIELD`, was 3 to 6), so a tunnel 5 to 6 m down pays for a
  lantern every 10 to 20 m, about a lantern a day. A volume dig's ore with
  no room round it in a tile-wide tunnel lands on the dug tile (crowding its
  piles) instead of staying buried, and in a volume a pile holds up only
  its own column's digs (a tunnel's dug tile's pile stopped the next dig).
  Lantern numbers kept (range 6 m, `LANTERN` 2.0, 60000 lm): a lantern
  every 8 m chains its light; waves kept as they were (no ghoul came
  underground in the soak). **Save size**: a volume made from the mine
  keeps its layout (`Terrain::mine_volume`, `generated_volume`) and a save
  stores the volume's edits over the generated one (`Volume::edits_over`,
  `with_edits`, `WorldSection::volume_edits`), remade at load; save format
  46. The town's save: 1.5 MB (the volume alone 1 529 960 bytes
  compressed) to 1484 bytes untouched, 1509 with 44 cells dug (the demo's
  `.glog.gsav` is 1493 bytes); `save::tests::a_towns_volume_is_saved_as_its_edits`.
  **Demo** (`app/demos/sacred_tree_depths.json`, rewritten, 3233 frames at
  30 fps, 2x and 4x between): dawn over the town; down the west ramp; a
  stair six steps down north from its foot (Ctrl+F); a lantern's
  blueprint on the fifth step; Moss and Reed set to mine bring its stone
  and amber down and build it (97 s) and mine the stair's walls in its
  light; the tunnel on six tiles with F; dusk; the elder at the stair's
  mouth by the west column's way, then a tile at a time down into the lit
  tunnel with two ghouls after it, where they kill it; dawn, the last ghoul
  burning by the ramp. Local plain build; video and screenshots in the
  session. The tunnel shots are dark and small at the town camera's
  distance, and the ghouls in the tunnel are hard to see.
  Not done, for later ("Depths follow-ups"): see that entry.

### M8.7 Sprites and ghouls read light

- **Status:** done (branch `kestrel-sacred-tree-depths`)
- **Why:** light matters to the town (L3, L4).
- **What to do:** underground, sprites only walk and work on lit tiles
  (the surface counts as lit); ghouls' paths cost more on lit tiles but
  never block.
- **Where:** `sim/src/nav.rs`, `sprites.rs`, `miners.rs`, `chase.rs`.
- **Found in:** depths design.
- **Tests:** a sprite won't mine a dark face and will once lit; a ghoul
  takes the dark route when one exists and still reaches a lit target.
  Note (from M8.2): the dawn still dispels ghouls on every floor; decide
  whether ghouls underground (unlit) survive it.
  Note (from M8.4): where miners need light: `miners::wall_search` (skip
  floors and the stand tiles that are dark, so a wall is a target only from
  a lit stand), `Step::DigWall` in `sprites.rs` (refuse to start, or stop,
  on a dark tile), the pile fetch in `miners::plan` (`stand_spot`), and the
  `GoTo` routes (`haulers::route_at`: dark tiles are walls underground for
  sprites). The search's cache is keyed by terrain revision only: key it
  by the light grid's revision too.
  Note (from M8.6): the query is `Settlement::light_at(terrain, tile,
  layer) -> Fx` (0..1, `light::LightGrid::at`) and `Settlement::lit(terrain,
  p, z)` (a body's feet), both 1 under open sky and without a volume;
  `Settlement::light.level(tile, layer)` is the grid's byte and
  `light.revision` goes up when the light changes (key caches by it). The
  grid is derived (not saved or hashed) and refreshed before and after
  every tick, so the brains read this tick's. A lantern blueprint's site
  is supplied by miners (`miners::lantern_errand`): once dark tiles stop
  sprites, a miner can't walk into a dark tunnel to build its first
  lantern; let a lantern's site and the tiles on the way count as lit for
  its errand, or let the elder's lamp-lit tiles count for that (L5 says
  the lamp doesn't count).

- **Done:** **Nav**: `nav::plan_with` (and `path_at_with`,
  `path_toward_with`) takes a `nav::Weigh`, a node's extra cost or `None`
  where the body may not go; the start's node is exempt, the goal's nodes
  and the goal's flood keep off barred nodes, and string pulling (and the
  straight walk tried first) takes a segment only where every half tile
  weighs no more than the heaviest node it replaces, so no corner is cut
  through the dark or the light. `plan_at` is `plan_with` with none:
  heightfield scenes and every old plan are unchanged (golden unchanged).
  **Sprites (L3)**: `haulers::route_at` takes the light grid, and a
  sprite's routes in a volume keep to lit floors (`LightGrid::sprite_lit`:
  open sky is lit) unless it is on a lantern errand
  (`SpriteState::on_errand`: a plan with `Fetch`, `Supply` or `Install`)
  or stranded; traffic detours too (`Walker::route_to` now takes the
  `View`, which gained `light`). `miners::wall_search` walks lit floors
  only, so a wall is a target only from a lit stand joined to the stone
  pile by lit floors; the floors it walked are the miner's reach
  (`Targets::reaches`), cached with the targets by (terrain revision, light
  revision); a pile is fetched only from a lit spot (`stand_spot`); a
  `DigWall` doesn't start on an unlit tile (it gives the spot up as before).
  **L3a (the gap, for the user's review, in `SACRED_TREE_DEPTHS.md`)**: a
  miner on a lantern errand walks dark floors for it only, never mines in
  the dark. **Stranded**: worked out every tick by the brain
  (`SpriteState::see_light`, `sprites::Lighting`, not state): a miner off
  its reach (in the dark, or in a lit pool a dark way cuts off), another
  sprite in the dark. A stranded miner not on an errand, standing still,
  drops its plan and walks back (through the dark) to the nearest tile in
  its reach on its level within `miners::RETREAT_TILES` (12, the lower
  tile on a tie, skipping tiles it gave up), or home. Light governs only
  scenes that have a lantern kind (`LightGrid::governs`): the Tunnel Lab's
  sprites walk anywhere, as before. **Ghouls (L4)**: `chase::LIT_STEP` 30
  per lantern-lit node (a half-tile step costs 10, so a lit tile costs four
  tiles' walk), via `chase::shun` in every threat plan (`chase_at`'s 3D
  chase, `route_home_at`: wave routes, replans, giving up); only lantern
  light (`LightGrid::lantern`), not open sky; never barred; none while
  nothing is lit. **Dawn**: kept, every ghoul burns at dawn, underground
  too (noted for M8.8). Haulers keep no light rule (no volume scene has
  any; their docks are on the top floor); the elder and the test walker
  ignore light. Save format unchanged (45): nothing new is saved. Tests
  (`sim/src/settlement/dark_tests.rs`): `a_sprite_wont_route_through_a_dark_tunnel`
  (round by an open-sky gap, nowhere without one, through once lit),
  `a_ghoul_takes_the_dark_way_and_still_reaches_a_lit_target` (two tunnels
  of one length, whichever is lit; a target in the lit one; the lit one
  the only way), `a_miner_wont_mine_a_dark_face_and_will_once_a_lantern_lights_it`,
  `a_miner_on_a_lantern_errand_walks_into_the_dark_and_builds_it`,
  `a_miner_stranded_in_the_dark_walks_back_to_light`,
  `a_ghoul_comes_into_a_lit_tunnel_after_a_sprite` (in the town's tunnel,
  hits it there), `light_read_miners_are_deterministic_across_threads_and_saves`;
  `light_tests::light_up` puts up a built lantern for tests;
  `LightGrid::of_lamps` (test only) a grid from lamps. Updated: the
  tunnel-wall, cave-amber and protected-column miner tests light their
  tunnel or the cave's hall with a lantern. `//kestrel/sim:sim_test`
  passes (the happy path, the soaks and golden among it); save and app
  unchanged, not run. No screenshots.
  Not done, for later (M8.8, notes there): wave routes planned before a
  lantern goes up aren't replanned for its light; a stranded miner's
  retreat picks the nearest tile by straight distance (it may be past a
  wall: it gives it up after two asks, then home); a traffic detour from
  a swerve into the dark is planned without the rule; the dawn rule
  underground; the cost of ghoul plans into lit stretches against
  `nav::PATH_BUDGET`.

### M8.6 Lanterns and the light grid

- **Status:** done (branch `kestrel-sacred-tree-depths`)
- **Why:** light is a resource you build (L1, L2, L5, L6).
- **What to do:** a lantern building (blueprint on any floor, 1 stone and
  1 amber, built by sprites; haulers deliver underground); a light grid
  in the sim, each lantern lighting its line of sight on its level within
  6 m, hashed and saved, recomputed on change; the view lights from the
  grid with soft edges; the elder's lamp stays view only. Settle M7's
  night lanterns.
- **Where:** `sim/src/settlement/buildings.rs`, a new light module,
  content, `app/src/cutaway.rs`.
- **Found in:** depths design.
- **Tests:** the grid stops at rock, at 6 m and at its level; a lantern
  blueprint underground is supplied and built; save round trip.

- **Done:** **The lantern** is a building kind (`sacred_tree_supply.ron`'s
  `lantern`: radius 0.2, one part of 1 stone and 1 amber, `output_distance`
  0, `light: Some((range: 6.0))`; content's `BuildingKind::light`, a role of
  its own in validation: no product, defence or production numbers; sim
  `BuildingKind::light`, `is_lantern`). Placed with the B menu's LANTERN
  (the same `PlaceBlueprint` command: input log and net protocol unchanged)
  on any floor: stage 1 reads the placer's elder's feet into the
  blueprint's message (`messages::PlaceBlueprint::feet`) and
  `buildings::lantern_placement` puts it on the floor under its footprint
  within a layer of them (`Terrain::footprint_at`, the walkers' rule, so a
  floor with headroom); none, or one more than a layer off (a drop), is
  the new `PlacementRefusal::NoFloor` ("NO FLOOR HERE"). A hollow column
  and a wall's edge are fine for a lantern; other kinds keep their rules
  (`Uneven` over a tunnel). It keeps off props on the top floor, other
  buildings on its floor, docks and other buildings' piles and slot spots;
  no walk from the depots is checked (the ghost asks every frame; a miner
  that finds no way gives the site up until the ground changes).
  `Settlement::placement_check_at(kind, position, feet, terrain)` returns
  a `buildings::Spot { output, height }`; `placement_check` is it with no
  feet. **Every building has a floor** (`BuildingState::height`: the
  ground at its middle, a lantern's own floor), which `on_building_floor`
  now reads (`pick_target` is unchanged: ballistas stand on the top floor
  anyway), and the interact target only offers buildings on the elder's
  floor in a volume. A lantern is no obstacle (a tunnel is a tile wide;
  bodies pass its post, as M7's lanterns), has no slot
  (`slot_position` is its middle: no possessing, no job menu entry, no
  boarding), its pile is the lantern itself, and ghouls leave it be
  (`breakable` is false), so a built one burns forever (L2); its column is
  protected from digs by its pile spot as any building's. **Miners build
  lanterns**: miners are the town's carriers, so `miners::lantern_errand`
  comes first in a miner's plan: for the nearest lantern site it hasn't
  given up, carrying an item the site wants it takes it there
  (`Step::Supply`), with the part all there it installs it
  (`Step::Install { site, part }`, a new late message queue
  `sprite_install`, `Settlement::sprite_installs`), else it fetches an item
  the site wants from the tree's pile of it (`Step::Fetch { depot, site }`,
  only as many units as the site wants) and takes it there; a need another
  miner is on (`miners::Errand`, the plans as of last tick in `Ctx`) is left
  to it, so the two miners fetch the stone and the amber at once. The
  `GoTo` before a `Supply` or `Install` plans to the site's floor
  (`haulers::route_at`'s goal floor), down the stair into a tunnel. Other
  kinds' sites stay the elder's to supply and install (D4); the elder can
  still supply and install a lantern's site too.
  **The light grid** (`sim/src/settlement/light.rs`, `LightGrid` on
  `Settlement::light`): each built lantern lights the floors of the tiles
  within its range (by tile middles) that have a floor within a layer of
  its own (`Volume::floor_near`: a ramp's step is the same level, a tunnel
  two meters under or over is not), keyed by (tile, floor layer), when the
  line from its tile's middle crosses no solid cell at its head height
  (`light::HEAD`, the cells 1 to 2 m over its floor; sampled four times a
  tile in integers, the far tile included), at `FULL` (255) times
  `1 - (d / range)²` as a byte, the brightest lantern winning. **Derived,
  not saved or hashed** (`#[serde(skip)]`): it is a function of the
  buildings and the volume, which are; `World::step` refreshes it before
  and after every tick and `World::from_state` on load
  (`Settlement::refresh_light`), recomputing only when the terrain's
  revision or the built lanterns changed; `LightGrid::revision` counts the
  recomputes that changed the light. The save stays small and a loaded
  world's grid is the one it had (a test checks). **The query for M8.7**:
  `Settlement::light_at(terrain, tile, layer) -> Fx` (0..1;
  `LightGrid::at`) and `Settlement::lit(terrain, p, z)` (a body's feet;
  `LightGrid::lit`), both 1 under open sky (the sun or the moon) and in
  scenes without a volume; `LightGrid::level(tile, layer)` is the grid's
  own byte. The elder's lamp is not in the grid (L5). Nothing reads it yet.
  **View**: `cutaway::follow` fills `Sight::set_light` from the grid for
  the player's level (each tile's floor within a layer of the player's)
  with `Key::light` the grid's revision; the shader's `LANTERN` adds it to
  roofed ground (soft edges by the texture's filtering), so no light goes
  through walls. The lantern's model is a `lantern` skin in
  `styles/forest.ron` (a stone base, a 1.1 m wooden post, a head in a new
  emissive `lanternglow` material, M7's lantern colours, and a cap: 1.44 m,
  under a tunnel's 2 m roof), drawn on its floor (legacy
  `Building::feet`); a built lantern has no card, lamp or pile caption, and
  a small warm point light under its head (`settlement::LANTERN_LUMENS`
  60000, range 6 m, no shadows, always on), for bodies and the lantern
  itself; a site shows its card ("LANTERN 0/1 STONE, 0/1 AMBER"). The
  ghost stands on the lantern's floor. Outside, lanterns look like
  lanterns (open-sky ground keeps the sun). **M7's night lanterns stay
  decoration** (view only, `daylight::spawn_night_lights`): converting them
  to built lanterns would make them buildings in the sim (prebuilt kinds,
  golden and the town's hash), for no gameplay today, since the surface is
  always lit. Save format 45; golden unchanged (no golden scene has
  buildings). Tests (`sim/src/settlement/light_tests.rs`):
  `the_grid_stops_at_its_range_and_falls_off_softly`,
  `the_grid_stops_at_rock` (a wall, round its end, over a low pillar, a
  one-tile tunnel), `the_grid_stays_on_its_level` (rooms over one another,
  a ramp's step), `ground_under_open_sky_counts_as_lit`,
  `a_lantern_blueprint_underground_gets_its_stone_and_amber_and_is_built`
  (in `miner_tests::tunnel`: placed from the tunnel's floor, `NoFloor` in
  the rock beside it and over the trench's drop, a Wood Gatherer `Uneven`
  there; the two miners bring the tree's stone and amber down and build
  it; it lights its stretch of tunnel and not the ground over it, a dig far
  off changes nothing and the end wall dug out is lit on the next tick; a
  save gives the same grid and steps the same), and
  `a_lantern_on_the_surface_lights_its_ground_and_has_no_slot`; content
  `the_sacred_tree_supply_loads_its_tunables` (the lantern, invalid
  variants); updated: `buildings_tests` (three kinds), the test-only Stone
  Gatherer is kind 3 (`construction_tests`, `soak_tests`), the control
  test's build menu, and content's `the_tree_piles_take_one_item_...` (the
  amber pile from M8.3).
  `//kestrel/sim:sim_test`, `//kestrel/save:save_test`,
  `//kestrel/app:movement_test`, `//kestrel/content:content_test` pass.
  The cave's first look was a white wash (the channel added white light
  at `LANTERN` 1.2 over the lamp's): the channel is now tinted warm amber
  in the shader (`LANTERN_TINT` (1.0, 0.55, 0.22)) at `LANTERN` 2.0, and
  the pool reads apart from the lamp. `app/demos/sacred_tree_depths.json`
  now walks into the mountain's cave first, stocks a stone and an amber at
  the tree, puts a lantern's blueprint down at the hall's back
  (x -7, z -52) from the hall's floor, sets Reed and Moss to mine (their
  first errand is the lantern), waits at 4x (`settlement_rate`) until it
  is built (128.6 s: Moss brings the amber at 78 s; Reed, with the stone,
  dithers 20 s at the plateau's north edge on the known walker trouble,
  M8.8), then sees the pool from the passage and the hall's front, and
  digs the stair and tunnel as before (dig waits 66 frames, so it fits in
  3529 frames; the stair now runs into the first night). Screenshots
  (local plain build): the site placed with its card, Reed coming in with
  the stone, the lantern built, its pool from the passage and from the
  hall's front, the tunnel at night.
  Not done, for later: miners supply only lanterns' sites (other kinds
  stay the elder's); a lantern's post blocks no one (bodies pass through
  it); its column is protected from digs at every depth, the ground over a
  tunnel lantern too; no reachability check at placement (a miner gives an
  unreachable site up until the ground changes); M7's night lanterns stay
  decoration; light numbers (range 6, falloff, `LANTERN` 2.0, the point
  light) are for M8.8.

### M8.5 Seeing, second pass

- **Status:** done (branch `kestrel-sacred-tree-depths`)
- **Why:** the line-of-sight prototype is too dark and leaves big flat
  lids (T7).
- **What to do:** rock beyond a one-tile rim round what you see goes near
  black; inside, light comes from lanterns and the elder's lamp, not the
  sun; F7 keeps the cutaway. Pick up the cheap items of "Line of sight
  follow-ups" in `verticality.md` (dim objects, piles and ghouls out of
  sight).
- **Where:** `app/src/cutaway.rs`, `app/src/sight.rs`,
  `app/shaders/cutaway.wesl`.
- **Found in:** depths design.
- **Tests:** sight unit tests; screenshots in the town's tunnels and the
  mountain cave, by day and night.
  Note (from M8.3): the town now uses the sight view; a stair dug down
  past 2 m is a roofed tunnel and turns the view inside (everything but
  the plateau goes dark), and inside the mountain's cave its whole top is
  one dark lid. Screenshots in M8.3's entry.

- **Done:** the line of sight view's second pass (T7), view only (sim,
  saves and protocol unchanged). **Void past a one-tile rim**: the sight
  grid (`app/src/sight.rs`) is now an RGBA8 texture per tile: visibility,
  **near** (the best ray share of the clear tiles in its 3 x 3, over
  `NEAR_FULL` 0.2: floor in sight and the rock bordering it, corners
  too, are 1), the lanterns' **light** and **open** sky (nothing solid at
  or over the eye layer). Inside, ground and cut tops of a tile whose near
  is 0 are drawn near black (`cutaway::VOID`, after fog, by the inside
  factor; `sight::void` and the shader's `void_at`, unfiltered so the rim
  is whole tiles; a wall face goes by the tile it faces, a cut top by
  where the view crosses the cut); the rim keeps the section colour. And
  inside, walls stand only `INSIDE_WALL` (0.75 m) over the feet over the
  whole view, not to the ceiling: rooms read as thin-walled boxes, as in
  the devlog, and a one-tile tunnel's floor isn't hidden behind its wall
  on the camera's side (a first try kept walls to the ceiling outside the
  near-wall box: the dug tunnel was a black slot). **Light**: roofed
  ground (open 0) is not lit by the sun but by `cutaway::underground`:
  `AMBIENT` 0.07 of its albedo, the elder's lamp (`LAMP` 1.6 at its foot,
  the fill light's place 1.8 m over the feet and its glow, falling off to
  `LAMP_RANGE` 9 m, half-wrapped by facing, times the tile's visibility,
  so no light through walls) and `LANTERN` 1.2 times the light channel;
  ground under open sky seen out of a mouth keeps the sun as before. Same
  by day and night. **The light hook for M8.6**: `Sight::light` (the
  texture's blue channel), all zero today; `Sight::set_light(|tile| ..)`
  fills it 0..1 per tile and `Key::light` (0 today) is the light grid's
  revision, so the grid is recomputed when it changes (see the note on
  M8.6). **The stair**: under a roof the view goes inside only when
  `Sight::deep`: at least `ROOF_AREA` (8) roofed floor tiles in sight
  within `ROOF_TILES` (2) of the eye (a room: a cave mouth or a wide
  tunnel goes inside at once), or no open sky in sight within `SKY_TILES`
  (2). A stair dug down from the surface is one tile wide with sky a step
  or two behind, so it **peeks** instead: a second factor (`SightView::peek`,
  eased the same) cuts only the near-wall box over the feet and lights
  the roofed floor there by the lamp, the town stays as it is; the view
  goes inside once the sky is three steps back (3.5 m down the demo's
  stair). `sight::blocks` now needs the eye layer and the one under it
  solid, so the next step down a stair is in sight. **Things out of sight**
  inside are hidden by their tile (`SightView::shows`: the body alpha
  under `SHOWN` 0.35; over the hidden height or more than `BELOW` 2 m
  under the player's feet, since the grid is for the player's level):
  the settlement's rigs (sprites, haulers, ghouls, buildings and depots,
  piles and their items, transfers), their glows and captions, the pile
  lab's piles and labels, `cutaway::Placed` scene objects (the Tunnel
  Lab's collector) and the test walker as before; the outdoor dressing
  with no place of its own (the baked environment, the ground cover and
  grass, fireflies, the canopy's leaves and the night lanterns,
  `cutaway::Outdoor`) is hidden while inside. The player always shows.
  F7 keeps the cutaway; outside and heightfield scenes unchanged. Tests:
  `sight::tests::rock_beyond_a_one_tile_rim_round_what_you_see_is_void`,
  `a_tile_at_a_wedge_side_is_partly_near`,
  `a_shallow_stair_stays_outside_and_a_tunnel_goes_inside` (2.5 and 3 m
  down a stair not deep, 4 and 6 m deep; a 5-wide room deep at its mouth,
  a one-wide tunnel only two tiles in), `down_a_stair_the_next_step_is_in_sight`,
  `the_light_channel_is_zero_until_a_light_grid_fills_it`,
  `open_sky_is_where_nothing_is_over_the_eye`;
  `cutaway::tests::with_no_lantern_light_roofed_ground_has_only_the_lamp_and_ambient`,
  `inside_things_out_of_sight_are_hidden_and_outside_all_show`;
  `//kestrel/app:movement_test` 295/295. `app/demos/sacred_tree_depths.json`
  now digs the stair on to 5.5 m and a 4-tile tunnel from its bottom,
  then walks to the cave and its passage (the miners' wait and the first
  cave visit went to fit 120 s); a `sacred_tree_depths_demo_video`
  target. Screenshots (local plain build): the town outside (unchanged),
  the stair at 2.5 and 3 m (the notch), at 4.5 m (inside), deep in the
  tunnel, the cave's mouth, the cave and its passage, leaving the cave
  mid-fade, and the Tunnel Lab's outside, west mouth, tunnel, the passage
  under the bridge and the stair; a 720p video. Night not shown: the
  first night is at 150 s and a demo script is capped at 120 s.
  Not done, for later: the interact prompt over a pile on the surface
  still floats in the void from the stair; the notch is the near-wall
  box (5 x 5 tiles), wide for a one-tile stair; a stair down shows only
  the next step or two ahead (the grid is one level); trees and lamp
  posts seen out of the cave's mouth are hidden with the rest of the
  baked environment (it is one mesh per material); the sun-lit wedge out
  of a mouth is much brighter than the lamp-lit room by day.

### M8.4 Miners mine walls

- **Status:** done (branch `kestrel-sacred-tree-depths`)
- **Why:** sprites harvest the tunnels the elder digs (T3).
- **What to do:** miner sprites pick exposed stone faces they can reach
  and dig level into the wall beside them, never down or up.
- **Where:** `sim/src/settlement/miners.rs`.
- **Found in:** depths design.
- **Tests:** a miner digs a tunnel wall, never its floor or ceiling; it
  walks to the face in 3D.
  Note (from M8.2): sprites already walk on 3D paths in volume scenes
  (`haulers::route_at`, steps by `walk_at`), but `miners::search` and
  `stand_spot` are still 2D (pits, `terrain.steep`).
  Note (from M8.3): in the town's volume a miner digs the column's top
  cell (`Terrain::dig_top`, `dig_refusal`'s volume branch with no
  stranding check) and `miners::search` steps between tops at most a layer
  apart; `Step::Dig`'s depth is `Terrain::dug`. Replace these with wall
  faces. Placement refuses footprints over tunnels and on wall edges
  (`buildings::check`'s `hollow`).

- **Done:** in a volume (the town's), a miner mines walls
  (`miners::wall_search`, `Step::DigWall`): a level dig of the
  `HEADROOM` (4) cells of the tile beside the one it stands on, from its
  floor layer up, through `View::dig_volume` with `DigDirection::Level`
  (so never down or up, never under itself: `volume_dig`'s `OwnTile`),
  never a protected column (the hill's undiggable ranges, and
  `dig_blocked` checked on every plan as before: tree, buildings, sites,
  piles, docks). Housekeeping clears the cells as the elder's level dig
  does (`Terrain::clear_column`); stone and amber land on piles on the
  miner's floor (`PileLab::mined_at`, whose "pile beside it" now must be
  on that floor too, not on the mountain over a tunnel). A wall is a face:
  a terrace step or a pit's side on the surface, a tunnel's or the cave's
  sides underground; flat open ground has none. The search: from the
  stone pile's top floor, a breadth-first walk over floors, four ways, to
  a neighbour's floor within a layer (`Volume::floor_near`), between
  obstacles, out to `VOLUME_SEARCH_TILES` (64, past the cave's back wall)
  and at most `WALL_SEARCH_NODES` (24576) floors, cached by terrain
  revision as before. Beside every floor it reads the wall's worth
  (`wall_worth`: `STONE_WORTH` 1 and `AMBER_WORTH` 4 per cell of the four,
  earth 0; the same diggable and "something left under it" rules as
  `volume_dig`) and keeps the `TARGETS` best by walk less
  `TILES_PER_WORTH` (2) times worth: the nearest first, as on the
  heightfield, a richer wall winning a near tie (a first try, worth per
  tile walked, sent both miners 90 tiles to the cave's amber and halved
  the stone at the tree: a miner carries 5 a trip, so distance is what
  pays). With no wall worth anything a miner waits by the stone pile, as
  with no stone. Miners carry amber too: piles of stone or amber (within
  `VOLUME_SEARCH_TILES` of the tree), amber to the AMBER PILE
  (`Settlement::amber_depot`, `Town::amber`, `deliver` by the carried
  item); a pile's stand spot in a volume is where the body's footprint
  stands within a layer of the pile's floor (`Terrain::footprint_at`), not
  the mountain's top over a cave. A `GoTo` before a `DigWall` plans to the
  wall's floor. A refused wall dig (it stands on another floor) gives the
  spot up until the ground changes. Heightfield scenes unchanged (their
  search, `Step::Dig`, pits); `Terrain::dig_top` and `dig_refusal`'s
  volume branch stay for the elder's refusal hint and tests, no miner uses
  them. Save format 44 (`Step::DigWall`, appended). A walking fix found on
  the way: sloped heights come out of fixed point a hair off a layer
  (-1.4999999972 for -1.5), so an exact one-layer step down measured more
  than a layer and `walk_at` refused it: a miner stood on a terrace's edge
  for minutes. `volume::ROUNDING` (1 mm) now pads `walk_at`'s and
  `walkable_at`'s drop and `Volume::floor_at`'s reach. The soak's elder
  (`dig_stone`) digs the search's walls level from a stand whose tile
  behind is on its floor, instead of a step down into the old top-cell
  targets (with Down digs into walls the elder spent its time walking and
  the tree fell in the wave soak). Tests (`miner_tests`):
  `a_miner_digs_a_tunnel_wall_beside_it_never_its_floor_or_ceiling` (a
  walled trench and tunnel at the north foot, amber in the end wall and in
  its floor and ceiling cells: it digs the four over its floor in 9 s from
  the tunnel's end, its own column and the floor and ceiling cells stay,
  and it carries the 18 amber to the tree),
  `a_miner_walks_into_the_mountain_cave_and_mines_amber` (ore only in the
  mountain: round the hill, in at the mouth, a hall wall from the -1 m
  floor; amber at the tree at 194 s), `a_miner_never_digs_a_protected_column`
  (amber all through the plateau's north edge is no target; a pile on the
  amber wall's column keeps it whole while two miners dig elsewhere for
  two minutes, every dig checked), `with_no_faces_a_miner_idles_by_the_stone_pile`
  (the town stripped of ore), `miners_are_deterministic_across_threads_and_saves`
  (saved with a `DigWall` in a plan); `two_miners_bring_a_steady_modest_income`
  now on walls: 49 stone in 5 min (in 20..=120). The heightfield miner
  tests are as before; the wave soak and the happy path pass unchanged
  otherwise. `app/demos/sacred_tree_depths.json` now sets Reed and Moss to
  mine first and waits by them at the west ramp's foot: their first walls
  are a terrace step 31 m west (stone 7 at the tree by the end); they
  don't come to the elder's stair or the cave in the demo's two minutes.
  Screenshots (local plain build): the two miners digging the step, full
  view and a close-up.
  Not done, for later: light (M8.7, notes there); the walker can still be
  stuck by a route cut diagonally off a sloped stair (M8.8, note there);
  amber worth and the distance trade (M8.8).

### M8.3 The town as a volume

- **Status:** done (branch `kestrel-sacred-tree-depths`)
- **Why:** the town's mine can't have tunnels on a heightfield (T1, T2,
  T4, T6).
- **What to do:** convert the Sacred Tree's generated ground and hill into
  a volume at load (the surface the same), 16 layers deep; stone veins
  richer deeper; amber, a new ore, only below 4 m and in the mountain
  (content, piles, HUD); an authored mountain spur 8–10 m high, 40–60 m
  from the tree, with a cave mouth; the hill, tree, buildings, sites and
  piles undiggable at any depth; the elder digs in 3D there. Update the
  golden, soaks and wave tests that change.
- **Where:** `sim/src/mine.rs`, `sim/src/volume.rs`, `terrain.rs`,
  `app/assets/levels/sacred_tree.ron`, its supply, `app/src/digging.rs`.
- **Found in:** depths design.
- **Tests:** surface heights match the old heightfield; protected columns
  refuse digs; amber only below 4 m and in the mountain; the happy path
  and soaks pass.
  Note (from M8.2): every body already has a feet height and walks in 3D
  once the terrain has a volume; buildings, depots, sites and the tree are
  assumed on the top floor at their middle (`terrain.height`), for
  reaches, ballista targets, fire and goal floors. Haulers and sprites
  start on the floor nearest 0 m (`follow::start_height`): for a town
  that is a hill, start them at the top floor instead. The forest/arena
  golden total lines change with any state shape change; behaviour lines
  are the check.

- **Done:** at load (`Settlement::from_scene`, `SceneSetup::mine_volume`)
  the mine's terraces and hill become the terrain's volume
  (`Terrain::volume_from_mine`, `Mine::to_volume`): every tile a column
  from its top down 16 layers (the scene's `dig_layers: Some(16)`, 8 m) and
  one of bedrock, its veins in its cells (mine layer `k` is volume layer
  `top - 1 - k`); then the scene's `volume:` boxes apply over it (the
  mountain), and the rock they raise gets stone veins by depth under its
  face (`Mine::rock_vein`), the generated veins put back under a box that
  reached into the ground. The mine stays (`Terrain::mine`: hill, veins,
  the generated top, `Terrain::generated_top`); pits don't count. **The
  surface is not quite the old heightfield**: terraces step 0.25 m and
  cells are 0.5 m, so a tile an odd number of steps up rounds down a
  quarter meter (`Mine::volume_top`, `div_euclid`); even steps (the
  plateau at 0 m, the foot at -2 m, every other terrace) are exact; two
  tiles at most two steps apart stay at most a layer apart (walkable).
  The ramps fall half a meter every other tile instead of a quarter each.
  Veins: `mine::STONE` is now 3 (after the dig lab's three minerals), so a
  town's volume holds stone and amber (`mine::AMBER`, 0) side by side;
  stone as before in the top six layers, richer down to layer 8 then as
  rich (`RICHEST_STONE`); amber pockets from 4 m down (`AMBER_DEPTH`,
  `mine::amber`: about 0.3% of the deep cells) and authored in the
  mountain. Content: an `amber` item ("AMBER", skin `item_amber`, a new
  forest-style model in the `magic` and `flowers` materials) and an AMBER
  PILE depot (`tree_amber`, only amber) at (0, 1.7), between the wood and
  stone piles, so the depots' midpoint (the Wood Gatherer's output
  direction) is unchanged; the HUD shows AMBER as it does STONE. A
  volume dig's stone and amber cells yield their items on ground piles
  (plain cells are earth: nothing), no tosses (`dig_housekeeping`).
  The mountain spur (`sacred_tree.ron`'s `volume:`, 15 boxes): tiles x
  -16..4, z -68..-47 from the tree, north (-z) of the town, away from the
  wave lanes along x (rings at x ±28, z 0); nine 1 m tiers a tile in each
  (its +z face a cliff for the first four), topped at 8 m (layer 16) over
  ground at -1 m, so 9 m high; its foot 47 m from the tree. The cave: a
  hall x -8..-4, z -53..-47 (mouth at z -47, 7 tiles in), floor -1 m
  (layer -2), 3 m high, and a passage west x -11..-9, z -52..-50, 2.5 m
  high; amber in the hall's west wall by the mouth (x -9, z -49..-48), its
  back wall (x -7..-5, z -54), the passage's end (x -12) and a pocket deep
  inside (x -13..-11, z -60..-58, layers 4..8). assetc accepts a volume
  with a mine (its extent the mine's) and `lockstep::scene_setup` passes
  it as `mine_volume`. Protected columns: the hill and ramps by the
  undiggable ranges (`volume_dig`), the tree, buildings, sites and piles by
  `dig_blocked` (tile-based, so every depth); placement refuses a
  footprint over a tunnel or cave, or on a wall's edge (a neighbour's top
  more than a layer off: a volume's cliff is not steep ground), and over
  dug ground (`Terrain::is_dug`). The elder digs in 3D in the town (F,
  Ctrl+F, Alt+F, as in the Tunnel Lab): to a floor 8 m under the surface,
  no deeper (`Bottom` at the bedrock); a stair down past 2 m becomes a
  descending tunnel (a dig clears `HEADROOM` cells), and the sight view
  goes inside there. Miners: still surface pits, in the volume a dig of
  the column's top cell (`Terrain::dig_top`; `dig_refusal`'s volume branch:
  the depth rule, `Bottom` when the top span is one cell thick, no
  steepening or stranding check); their search reads the top cell's ore
  and steps between tiles whose tops are at most a layer apart (a cliff is
  a wall); `Step::Dig`'s depth is `Terrain::dug` (the pit, or `128 - top`).
  Haulers and sprites start on the top floor in a mine's volume
  (`follow::start_height`). Speed: `Volume` caches its state hash (it is
  hashed every tick) and keeps a lazily built grid of its columns
  (`spans` was a binary search over 66 049 columns; the sim suite went from
  227 s to 17 s with it), and `volume_mesh::changed_tiles` merges the two
  volumes' sorted columns and ore instead of looking every tile up (0.7 ms
  over the town). Drawing: the town uses the volume mesher and the sight
  view; open-sky tops are coloured by terrace steps from the hill's foot
  and tinted lightly by their stone as the heightfield did
  (`chunk_mesh_with`, `ore_color`: stone grey); ground piles are drawn on
  their own floor (`GroundPile::feet`). The 289 chunks (331 254 triangles)
  mesh in 82 ms (`the_towns_volume_meshes_quickly`, remote opt build); the
  town's save is 1.5 MB with the volume (`.gsav` of the demo). The control
  snapshot's `pits` and `nearest_ore` read the volume. Golden unchanged
  (no golden scene is the town); save format unchanged (43): no state
  shape changed. Tests in `sim/src/settlement/town_volume_tests.rs`:
  `the_surface_matches_the_old_heightfield` (tile by tile: exact or a
  quarter meter lower, always the old height in whole layers),
  `the_ground_is_sixteen_layers_deep`,
  `veins_go_down_and_amber_lies_deep_and_in_the_mountain`,
  `the_mountain_stands_north_of_the_town_8_to_10_m_high`,
  `the_caves_mouth_walks_in_from_the_surface`,
  `protected_columns_refuse_digs_at_every_depth` (plateau, east ramp,
  tree, a site, a ground pile, from a shaft at each of its 16 floors),
  `the_elder_digs_a_stair_down_8_m_but_no_deeper` and
  `bodies_start_on_the_top_floor`; updated: `hill_tests` (cliffs as walls,
  ramps rounded, 3D paths, a pit's goal flood), `lane_tests` (moats and
  pits dug as top cells, routes walked in 3D, the planted ghoul's feet),
  `miner_tests`, `construction_tests` (three items, a Ctrl+F dig),
  `bundle_tests`, `soak_tests` (the elder's stone digs are Ctrl+F); in the
  app `volume_mesh::tests::the_towns_volume_meshes_quickly` and the happy
  path (a Ctrl+F then an F dig; the stone still on its way counts). The
  Sacred Tree soaks and wave tests pass unchanged otherwise.
  `app/demos/sacred_tree.json` digs with `dig_front`; a new
  `sacred_tree_depths_demo` (`app/demos/sacred_tree_depths.json`) walks to
  the mountain, into the cave and its passage, and digs five steps down at
  the north foot. Screenshots (local plain build): the town from the usual
  camera at dawn, the mountain spur and its cave mouth (amber in its
  wall), the elder in the cave (sight view, the mountain dark), the stair
  from outside, and inside its tunnel.
  Not done, for later: the heightfield town's quarter-meter terraces (only
  a finer volume layer would keep them); stone tosses for volume digs in
  the town (`digging::toss_stone` reads pits; volume digs burst dust);
  saves carry the whole volume (1.5 MB; a diff against the generated
  ground would be small).

### M8.2 Every body on a floor

- **Status:** done (branch `kestrel-sacred-tree-depths`)
- **Why:** in volume scenes only the avatar, sprites and piles have a
  feet height; haulers and ghouls walk on the top floor (T5).
- **What to do:** haulers and threats get a feet height (hashed, saved),
  3D paths and chases (`chase.rs`, `Threat::position`); fix the rest of
  "Ghouls and haulers with a height" in `verticality.md` (jetpack `GoTo`,
  sprite respawn, pile-haul drops, spills, `TossEnd::Pit`). Other scenes
  unchanged.
- **Where:** `sim/src/settlement/haulers.rs`, `chase.rs`, `combat.rs`,
  `sprites.rs`, `piles.rs`.
- **Found in:** depths design.
- **Tests:** in a test volume a ghoul chases a sprite into a tunnel and
  under a bridge; a hauler carries down a stair; golden unchanged for
  heightfield scenes.
  Note (from M8.1): plan with `nav::path_at` (a full route or none) for
  haulers and `nav::path_toward_at` (`PathAt { route, reached, expanded }`:
  else a route to the reached node nearest the goal) for a ghoul's chase,
  so a ghoul that can't climb to its target waits below it. Back each
  body's replans off after a plan that didn't reach (`!reached`), as the
  walker does (`follow::FAILED_REPLAN_TICKS`, 2 s): a plan to a goal on a
  big floor it can't reach still costs up to `nav::PATH_BUDGET` (16384)
  nodes. The walker's backoff is stateless (a refused step doesn't cut a
  failed plan's wait short while more than `REPLAN_TICKS` of it is left);
  with the save format going up here anyway, a failure count per body
  (doubling waits) is an option. Not built: a cached flood of connected
  floor components keyed by `Terrain::revision` (O(1) rejection, but every
  dig floods the window again); the goal's flood already settles shut-in
  goals (pits, tops, sealed rooms) in about twice their size, so build it
  only if the town soak shows many plans at the budget.

- **Done:** haulers (`HaulerState::height`) and threats (`Threat::height`)
  have a feet height, as sprites (`SpriteState::height`) already did;
  `AgentState::height` reads any agent's. Haulers start on the floor
  nearest 0 m (`follow::start_height`, as sprites), threats on the top
  floor where their wave spawns (`chase::surface_feet`), and a sprite comes
  back from dying on home's floor. In volume scenes every walker steps on
  foot: `haulers::move_worker` (haulers and sprites on errands) checks
  every step with `Terrain::walk_at`, its feet following, skips waypoints
  only where it walks, steers round bodies on its own floor only
  (`Footprint::height`) and drops a route the ground refuses, for its brain
  to plan again; threats walk their routes with `chase::walk_route_at`
  (sliding along a corner a path's samples skipped, as the walker does).
  Paths: haulers and sprites plan with `nav::path_at` (`haulers::route_at`:
  any floor at the goal, or the pile's floor when a sprite picks one up
  next); a hauler's link check plans from the pickup's top floor; waves and
  give-ups plan their route home with `nav::path_toward_at` to the target's
  top floor (`chase::route_home_at`, `stranded` when it doesn't reach),
  and a threat whose route went stale plans again from where it stands. A
  chaser walks straight at its quarry while both are on one floor and the
  walk is clear, else follows a 3D path to the quarry's floor
  (`Threat::chase`, `path_toward_at`, every `follow::REPLAN_TICKS`), so a
  ghoul that can't reach waits at the nearest point. In a volume it spots
  a quarry within its detect radius in 3D that it sees (`chase::sees_at`:
  eye to eye, 1.5 m up, through no solid cell) and keeps it within the
  leash in 3D, without needing sight. Backoff: `Backoff { at, failures }`
  (`follow.rs`) on haulers, sprites and threats: a plan that didn't reach
  waits `FAILED_REPLAN_TICKS` (2 s), doubling per failure in a row up to
  8 s; written only in volume scenes. Combat reach: `combat::FLOOR_REACH`
  (1.5 m; stacked floors are at least 2.5 m apart) via `same_floor`, only
  in volume scenes: swings at agents (their floor, or for an avatar in the
  air the floor under it) and buildings (the top floor at their middle),
  player strikes (sword, spear, arc chain, laser targets), projectile hits
  (`Projectile::floor`, an arrow's shooter's feet; a ballista's shot the
  top floor at its launcher), ballista and defender targets
  (`pick_target`), fireball hits and fire patches (the top floor where they
  land), building spotting, and sprites' alarm. The jetpack's `GoTo` plans
  `path_at` from the floor under it in a volume (2D if there is none);
  piles from a pile haul's drop lie on the dropper's floor and from a
  broken building's `spill` on its floor; `TossEnd::Pit` carries the
  floor it flies from (the dig's cleared bottom, the pit's floor). Save
  format 43. The behaviour digest maps projectiles to their old fields and
  adds haulers', threats' and arrows' floors only when one is nonzero:
  golden behaviour lines unchanged; the forest and arena *total* lines
  changed (the state's shape grew: `HaulerState`, `Threat` and
  `Projectile` fields) and were regenerated. View: `legacy` gives haulers
  and threats `feet` while off 0 m, and the town view draws haulers and
  ghouls at their feet in volume scenes (`settlement::in_volume`); tosses
  from a pit start at `TossEnd::Pit::height`. Tests in
  `sim/src/settlement/floors_tests.rs`:
  `a_ghoul_chases_a_sprite_into_the_tunnel` (round the mountain's corner,
  0 m under the roof, hits it there),
  `a_ghoul_chases_a_sprite_under_the_bridge`,
  `a_ghoul_cant_hit_a_sprite_on_another_floor` (bridge over cave, both
  ways), `a_ghoul_that_cant_reach_waits_at_the_nearest_point_and_backs_off`
  (a sheer pillar 3 m high: waits by it, plans 2, 4, 8, 8 s apart),
  `a_hauler_carries_wood_down_a_dug_stair` (down to -2 m and back),
  `a_sprite_comes_back_on_homes_floor`, `chases_in_3d_are_deterministic`,
  `other_scenes_keep_bodies_at_zero`; in `save`,
  `a_volume_scene_keeps_haulers_and_threats_heights`;
  `combat_tests::threat_size_is_pinned` at 224 bytes.
  Not done, for later M8 tasks: buildings, depots and sites have no floor
  (the top floor is assumed: M8.3/M8.6 must give an underground lantern's
  site one, and its deliveries a goal floor); the dawn dispels ghouls on
  every floor (M8.7: underground is dark); impaled bodies (`Pinned`) and
  the test walker are as before (no height; the walker steers round no
  one); a towing hauler plans for its body only in 3D; miners' targets and
  pit digs are 2D (M8.4); pile reach is still 2D ("Pile reach in 3D").

### M8.1 Bounded 3D paths

- **Status:** done (branch `kestrel-sacred-tree-depths`)
- **Why:** dozens of bodies will plan in 3D, and one plan to an
  unreachable floor took 1.2 s in the Tunnel Lab (see "3D paths to an
  unreachable floor" in `verticality.md`).
- **What to do:** bound `nav::path_at` (a node budget, or a reachability
  check of the goal's floor first), and back off replans after a failed
  plan. Deterministic.
- **Where:** `sim/src/nav.rs`, `sim/src/settlement/follow.rs`.
- **Found in:** depths design ([`SACRED_TREE_DEPTHS.md`](../SACRED_TREE_DEPTHS.md)).
- **Tests:** a plan to an unreachable goal stays under a budget; paths
  that exist are still found; the Tunnel Lab soak still passes.
- **Done:** `nav::plan_at(terrain, start, z, goal, goal_z, radius, extra,
  budget, toward) -> PathAt { route, reached, expanded }` is the 3D
  search; `nav::path_at` is it with `PATH_BUDGET` and no partial route
  (same signature as before: a full route or none), and
  `nav::path_toward_at` (new) is it with `PATH_BUDGET` and a partial one.
  Two bounds, both counting nodes, not time, so plans stay deterministic.
  The budget: `PATH_BUDGET` (16384) nodes expanded, search and flood
  together; the longest route across the Tunnel Lab (from the tunnel's
  east side round the mountain to the bridge's stairs and up) takes
  11075, a cross-map one 4329, through the tunnel 764. The goal's flood:
  from the search's 256th node on (`FLOOD_AFTER`, so short plans pay
  nothing), a breadth-first flood back from the goal's nodes (the nodes
  that walk onto them, by `Terrain::walk_at` from the neighbour, so drops
  count right) takes a node for each node of the search, up to
  `GOAL_FLOOD` (1024). Meeting the search (a node both reached) proves a
  way and the flood stops; running out of nodes proves the goal shut in (a
  pit, the mountain's top, a sealed room): `path_at` returns at once, in
  about twice the floor's size (968 nodes to the Tunnel Lab's mountain top
  from the spawn, which searched the whole map before). Past `GOAL_FLOOD`
  the goal's floor is big and the search goes on alone to the budget
  (3429 nodes past a wall across a small map, the whole near side). A
  cached flood of floor components keyed by `Terrain::revision` was not
  built (every dig would flood the window again; noted on M8.2). Partial
  routes (`path_toward_at`, `reached` false): to the node, of those the
  search reached, nearest the goal in 3D (the squared distance plus the
  squared height to the wanted floor; the lowest node on a tie),
  string-pulled like a full route; when the goal is shut in, the search
  stops once `STALL` (256) nodes in a row came no nearer. Empty only when
  the start attaches to no node; without a volume there are no partial
  routes (the full 2D `path_avoiding` route, `reached` if not empty). For
  callers it means: come as close as you can (a ghoul under the bridge
  someone stands on, a walker at a pit's edge), and plan again later.
  Node numbers now interleave the flood's nodes, so tie-breaks (and so
  some 3D routes) can differ from V.7's; without a volume nothing changed,
  and the golden test passes as is. The walker (`follow.rs`) plans with
  `path_toward_at`: after a plan that doesn't reach the player it walks
  the partial route and waits `FAILED_REPLAN_TICKS` (120, 2 s) instead of
  `REPLAN_TICKS` (30) to plan again; a refused step plans again next tick
  only when no failed plan's wait has more than `REPLAN_TICKS` left
  (stateless: no save change). The Tunnel Lab soak plans from the walker
  to the avatar every half second as the walker does and asserts every
  plan stays within the budget (240 plans, at most 902 nodes). Tests in
  `sim/src/settlement/path_tests.rs`:
  `paths_across_the_tunnel_lab_fit_in_the_budget` (five routes found,
  under three quarters of the budget, `path_at` the same as
  `path_toward_at`),
  `a_plan_to_the_mountains_top_is_settled_by_the_goals_flood` (no route,
  within `2 * GOAL_FLOOD` nodes; the partial one under the top on the
  tunnel's floor, the same twice and on a clone),
  `a_plan_past_a_wall_stops_at_the_budget` (stops at budgets of 500 and
  2000 exactly; the partial route ends by the wall),
  `a_partial_plan_ends_at_the_reachable_point_nearest_the_goal` (at a sheer
  pit's edge by the goal; out of budget on the way to a reachable goal,
  partial and closer) and
  `the_walker_backs_off_when_it_cant_reach_the_player` (the player on the
  mountain's top: every plan waits `FAILED_REPLAN_TICKS`, the walker ends
  under it in the tunnel); in `soak_tests`,
  `the_tunnel_lab_soaks_walking_and_digging_at_random` checks the plans.

### The night's fight comes late

- **Status:** done (branch `kestrel-sacred-tree-daynight`, PR #134)
- **Why:** measured on the map's own nights (M7.6), ghouls walk 0.6 m/s
  and take about 41 s from the 40 m ring to the tops of the ramps and
  63.7 s to the tree; a column of five sets off over the night's first
  10 s, so its last breaches at 73.7 s, 1.3 s before dawn. With a manned
  ballista by each ramp the first bolt flies 55.4 s after dusk and the
  kills come 56.5 to 66.9 s in: the fight is about 11 s of a 75 s night,
  nothing is on the plateau for its first 41 s and its last 8 s are
  empty. Dawn burns no ghoul on any of the four nights, defended or not
  (0 dispelled); it only burns ghouls that were held up (lured by the
  elder, as in the demo, smashing a building, or stranded by a moat).
- **What to do:** decide with a playtest. The recommendation: move the
  ring in to about 28 m (just past the ramps' feet at 23.5 m and the
  hill's flat ring at 25.5 m), so ghouls top the ramps about 21 s after
  dusk and reach the tree about 43 s in, and widen `COLUMN_GAP` (2.5 s)
  to about 5 s so a column of five spans 20 s: the fight then runs from
  about 35 s to 65 s into the night, and a slowed column's tail is still
  out at dawn. A longer night only adds empty time and puts four nights
  past 15 minutes (100 s nights: 4 x 4:10 = 16:40). A ring that close
  may spawn ghouls in the town camera's view (it shows ground 15 to 60 m
  out): spawn them in the dark or fade them in. Then retune the nights,
  the `waves_tests` restock time (`RESTOCK`, 47 s) and the demo's waits.
- **Where:** `app/assets/levels/sacred_tree_supply.ron` (wave positions),
  `sim/src/settlement/combat.rs` (`COLUMN_GAP`),
  `sim/src/settlement/waves_tests.rs`, `app/demos/sacred_tree.json`.
- **Found in:** M7.2, M7.6.
- **Done:** the supply's lanes start 28 m out (`(28.0, 0.0)` and
  `(-28.0, 0.0)`, just past the ramps' feet at 23.5 m and the hill's flat
  ring at 25.5 m; `hill_tests::ghouls_come_from_the_ring_up_the_ramps`
  checks 26 to 30 m, a walkable route and the crossing on each ramp) and
  `COLUMN_GAP` is 5 s. A column of five at 5 s sets off over 20 s, inside
  the night's first 25 s, so `lane_starts`' rule is unchanged (six would
  bunch to 4.2 s). The gap stays a constant: in content it would be a new
  `WaveState` field, so a save format bump and new state hashes in the
  forest's and arena's golden for no gain. Measured with a probe as in
  M7.6 (four nights, the tree's health raised; removed, and its undefended
  half kept as `waves_tests::the_nights_fight_comes_in_its_middle`), in
  seconds after dusk:

  | | Ramp tops | At the tree | First bolt | Kills | Dispelled |
  |---|---|---|---|---|---|
  | Undefended | 20.9 to 30.9 (night 1), 20.9 to 40.9 (nights 2 to 4) | 43.7 to 53.7, then 43.7 to 63.7 | none | none | 0 each night |
  | A ballista by each ramp | as undefended | none | 35.4 | 36.5 to 46.7 (night 1), 36.5 to 56.7 (nights 2 to 4) | 0 each night |

  Before: tops 41 s, the tree 63.7 to 73.7 s, the first bolt 55.4 s, the
  kills 56.5 to 66.9 s. So a defended fight runs from about 35 s to 57 s
  and an undefended one breaches from 44 s to 64 s, 11 s before dawn;
  dawn burns none unless they are held up. In the wave soak (lured, chased
  and smashing ghouls) dawn dispels 3 over five nights. The nights are
  unchanged (3+2, 5+4, 5+5, 5+5 repeating): ignored, the tree falls on the
  third night at 653 s (53 s after dusk, was 668 s); one ballista by the
  east ramp: tree 4/20 after four nights, 16 breaches, 18 kills; one by
  each: 20/20, all 34 killed. `waves_tests`: `RESTOCK` 47 s to 30 s (the
  ballistas start empty, and at 47 s they first shot after the ghouls
  reached the tree: tree 3/20 with two); the sunrise test sends its night
  of eight from 40 m so some are still out at dawn; the column test checks
  5 s. The happy path must end before 170 s (was 190 s; ghouls top the
  ramps at about 171 s). Spawning is in view: from the west ramp's top the
  camera shows the ring, and a ghoul popped in at 150 s. The view
  (`settlement::rise_offset`) now draws a threat up to 1.9 m under the
  ground at its spawn, rising (eased) over its first 0.6 m walked, about
  a second; in the video the head shows at 0.3 s and it stands at 0.9 s.
  Demo (`demos/sacred_tree.json`, the night rebuilt; 3562 frames at 30 fps,
  1:59): the day as in M7.6, the elder waiting at (-12, 4.5) by the west
  ramp's top; dusk at 2x (144 to 158 s), 4x to 166 s, 2x up the ramp, 1x
  from 172 s; the elder steps down to (-12, 2.2) beside the column's way at
  about 178 s, so both west ghouls (5 s apart, 3 m) turn on it within the
  same second, and leads them round the plaza outside the pines (90 hops
  of 0.3 m to about 45 degrees; they stay 1.7 and 2.1 m behind, no swing)
  while Fern's ballista kills the east column (187.1, 191.7, 196.7 s); at
  sunrise (225 s) both lured ghouls catch fire and fall (227.1, 227.6 s).
  Tree 20/20. Tuned in seven simulation runs against the frame trace.
  Save format, input log, net protocol, commands and golden unchanged.

### M7.6 Tuning, soak and demo

- **Status:** done (branch `kestrel-sacred-tree-daynight`)
- **Why:** the playtest needs four nights that feel right in 15 minutes.
- **What to do:** tune night sizes and the first day; update the demo to
  show a full cycle (a day of building, dusk, a night attack, dawn);
  screenshots; full suite once; a `docs/test-time.md` row; mark the
  design done.
- **Where:** `sacred_tree_supply.ron`, `app/demos/sacred_tree.json`,
  `SACRED_TREE_DAYNIGHT.md`.
- **Found in:** day and night design.
- **Done:** tuning: the nights (3+2, 5+4, 5+5, 5+5 repeating), day 150 s,
  night 75 s and the 40 m ring are kept; `waves_tests` and the soak pass
  as they are. A probe (four nights, the tree's health raised so it
  plays them all) measured the fight: ghouls top the ramps about 41 s
  after dusk and breach 63.7 to 73.7 s in; with a ballista by each ramp
  the bolts fly from 55.4 s and the kills come 56.5 to 66.9 s in, and dawn
  burns none (0 dispelled, defended or not). So the fight is about 11 s
  at the end of each night; see the new open task "The night's fight
  comes late" for the numbers and the recommendation (a ring at about
  28 m and a 5 s column gap). Dusk (`sacred_tree.ron`): the red, dark
  key ((1.0, 0.52, 0.30) 12000 lux, ambient (0.74, 0.62, 0.70) 720, sky
  (0.28, 0.16, 0.17)) is now golden: sun (1.0, 0.74, 0.42) 13000 lux,
  ambient (0.80, 0.71, 0.60) 820, sky (0.34, 0.25, 0.15); the ground reads
  warm brown and gold instead of rust, distinct from dawn's rosy sky and
  lavender ambient, the shadows long to the east. Demo
  (`demos/sacred_tree.json`, rebuilt; 3282 frames at 30 fps, 1:49): the
  day as in M6.7 at 2x (the elder digs two stone layers at the east
  vein, Reed mines, the ballista by the east ramp's end, loaded, Fern
  mans it, by 53 s); 4x through the afternoon (the elder loads Reed's
  stone and walks to the top of the west ramp at 132 s; "NIGHT FALLS IN"
  from 120 s); dusk at 2x (144 to 158 s, the lanterns and the tree's glow
  coming up); 4x to 186 s; 2x while the west column climbs its ramp; 1x
  from 192 s: the west column tops the ramp under the lanterns facing the
  camera, eyes glowing, turns on the elder at 3 m, and the elder leads
  both ghouls round the plaza outside the pines (61 hops of 0.3 m, 9
  frames apart, about 0.65 m/s on a 12.5 m arc from (-12, 2.2) to (2.1,
  12.3); they stay 1.8 and 3.2 m behind, no swing starts) while Fern's
  ballista shoots the east column (3 kills, 206 to 212 s); at sunrise
  (225 s) the two lured ghouls catch fire beside the elder and fall
  (227.5 s). Tree 20/20, no breach. Its waits were tuned in simulation
  runs against the frame trace (threat and elder positions by
  `settlement.seconds`); the stills were cut from the video by the
  trace's seconds (no frame dump). The eyes read at the town zoom on a
  ghoul facing the camera (two yellow dots), small as M7.4 found; the
  `PLAYER 1` tag can sit over the nearest chaser. `app/README.md`
  describes days and nights, the dial, the waves by night, the dawn burn
  and the new demo. Save format, input log, net protocol, commands and
  golden unchanged. The design is marked done.

### M7.5 The time on the HUD

- **Status:** done (branch `kestrel-sacred-tree-daynight`)
- **Why:** you plan the last trip before dark (N8, N10).
- **What to do:** a small sun-and-moon dial with the night's number; a
  "Night falls in 0:20" warning near dusk; nights survived once the last
  listed night repeats. Reads M7.1's clock.
- **Where:** `app/src/settlement.rs` (HUD).
- **Found in:** day and night design.
- **Tests:** HUD text at day, near dusk and at night (the control tests).
- **Done:** a new view module, `app/src/clock_hud.rs`, spawned by the
  settlement view in towns (`Settlement::clock_hud`, despawned at shutdown)
  and shown only while the mirror's settlement has a clock and the tree
  stands (`tree_fallen` hides it; the fallen overlay speaks). Top right
  (right 36 px, top 32 px, where the off-town contact panel sits), right
  aligned: a 64 x 34 px dome (2 px rim in the HUD's cream, a dim blue sky by
  day, dark navy by night) whose 12 px sun (warm yellow) by day or moon
  (pale blue-white) by night rides a 22 px arc from half-risen on the
  left (east) at dawn or dusk, over the top halfway, to setting on the
  right; it moves at the mirror's eased tick (`day_cycle::hour` at `tick +
  alpha`). Under it, in the HUD's font: "DAY 1" before the first dusk,
  "NIGHT 1" through it, "DAY 2" after (14 px, cream); "NIGHT FALLS IN 0:14"
  from `NIGHT_WARNING_SECONDS` (30) before dusk, rounding part seconds up
  (16 px, orange); and "NIGHTS SURVIVED n" (`Settlement::nights_survived`)
  always from the first dawn, the simpler choice N10 allows (12 px,
  muted). Lines that are empty are `Display::None`. The status line's wave
  part moved to `clock_hud::waves_text`: with a clock, a wave due at the
  next dusk or later is no longer counted down there ("NEXT WAVE" would say
  what the dial says); one due before dusk (a timed wave) still is, and
  "GHOULS n" is unchanged; scenes without a cycle keep "NEXT WAVE". The
  control snapshot's `settlement.clock` gives the dial's `label`,
  `warning`, `survived` and `marker` (null while hidden). No sim state:
  the clock, `nights_survived` and `next_wave` were all there. Tests
  (pure functions, `movement_test`): `clock_hud::tests::by_day_the_dial_names_the_day_and_warns_only_near_dusk`
  (no warning 31 s out, 0:30 at 30 s, 0:20, 0:01 on the day's last tick,
  "DAY 2" with one night survived), `by_night_the_dial_names_the_night_and_the_nights_survived`
  (night 1, night 4 past the three listed ones),
  `with_days_and_nights_the_status_line_leaves_dusk_to_the_dial` and
  `the_sun_and_moon_rise_in_the_east_and_set_in_the_west` (inside the
  dome). Screenshots (the M7.4 4x script, frames by `settlement.seconds`):
  near dusk (137 s) the sun low on the right, "DAY 1", "NIGHT FALLS IN
  0:14" and no NEXT WAVE in the status line; midnight (187.5 s) the moon at
  the top of a navy dome, "NIGHT 1", "GHOULS 5" in the status line. Save
  format, input log, net protocol and golden unchanged (view only). See
  "Clock dial polish" for what was left.

### M7.4 Lights at night

- **Status:** done (branch `kestrel-sacred-tree-daynight`)
- **Why:** the town and the threat stay readable in a dark night (N7).
- **What to do:** the tree glows (a warm point light rising at dusk,
  fading at dawn); ghouls' eyes glow (two small emissive dots); a few
  fixed lanterns placed in the scene file, lit at dusk: at the top of each
  ramp and round the tree's plaza.
- **Where:** `app/src/scene.rs`, `settlement.rs` (ghoul drawing),
  `sacred_tree.ron`, the scene schema.
- **Found in:** day and night design.
- **Tests:** a midnight screenshot shows the plaza, both ramp tops and an
  approaching ghoul.
- **From M7.3:** the moon is a 1800 lux key with ambient 150
  (`sacred_tree.ron`'s `day_cycle.midnight`); lights rising at dusk can
  read the hour with `voxel_assets::day_cycle::hour` and the cycle's
  `MORNING`/`EVENING` blends (`app/src/daylight.rs` samples it each frame
  at the mirror's `tick + alpha`). The built-in deck's three
  `style::AccentLight` point lights still shine in the town (a warm patch
  by the stone pile at night): keep, move or switch them off for the
  town. Dusk reads darker than dawn because the camera sees the sun's back
  side then; the tree's glow can help.
- **Done:** schema (`voxel_assets::day_cycle`, re-exported): `Scene::lanterns:
  Vec<Lantern>` (`Lantern { position }`, a post's foot) and `Scene::glow:
  Option<NightGlow>` (`NightGlow { position, color, intensity, range }`,
  lumens at full glow, meters), both `#[serde(default)]` (no pack version
  bump), checked by `validate_night_lights` (finite places, up to 64
  lanterns, colors 0..1, up to 10 million lumens, range 0.1 to 100 m). Drawn
  only: the sim doesn't know of them, so lanterns don't block walks (the
  happy path and routes unchanged). The curve, `day_cycle::night_glow(hour)
  -> 0..1`: out by day, lit up (smoothstep) from `LIGHTING_UP` (80% of the
  day, 30 s before dusk) to `DUSK_GLOW` 0.5 at dusk, on to full over the
  night's `EVENING` (30%), full until `SMALL_HOURS` (75%), down to
  `DAWN_GLOW` 0.5 at dawn and out by `PUTTING_OUT` (10% of the day, 15 s);
  continuous at both swaps. App (`app/src/daylight.rs`):
  `spawn_night_lights` (from `SceneView::init`, `WorldScoped`) spawns the
  glow's `PointLight` and, per lantern, four cuboids (a base, a 1.6 m post,
  a glowing head at 1.75 m, a cap) and a warm point light (`LANTERN_LUMENS`
  250000, `LANTERN_RANGE` 10 m), all with shadows off; `NightLight { full }`
  marks them. `glow_at(pack, time)` is the curve at the mirror's eased
  tick (always 1 without a cycle); `follow` and `dress` write `full * glow`
  to each light and the heads' emissive (10, 5, 1.5 at full, only when it
  changes). Accent lights: in a scene with a day cycle `dress` turns the
  built-in deck's three `AccentLight`s off (the night's lights replace
  them); other scenes keep them. Ghouls' eyes: a new `eyes` material in
  `styles/forest.ron` (emissive (14, 10, 2)) and two 0.11 x 0.08 m boxes
  under the `ghoul` skin's brow; always lit, so the forest's ghouls have
  them too. The Sacred Tree (`sacred_tree.ron`): the glow at (0, 2.6, 0)
  under the canopy, (1.0, 0.72, 0.40), 2 million lumens, 20 m; lanterns at
  (+-14.6, 0, +-4.4) (each side of the top of both ramps; the plateau's edge
  is 15.5 m out, the ramps 7 m wide) and round the plaza at (+-3.4, 0, 6.4)
  and (+-4.6, 0, -4.6): nine point lights in all. Tests: `voxel_assets`
  `day_cycle::tests::the_nights_lights_rise_at_dusk_and_fade_at_dawn` (out
  by day, full at midnight, continuous at dusk and dawn, rising and falling
  steadily) and `night_lights_out_of_range_fail_validation`;
  `style::tests::a_day_cycle_wins_over_the_style_and_other_scenes_keep_their_fixed_light`
  now also checks the Sacred Tree's glow and lanterns are lit by
  `glow_at` and its accent lights are off, and that the industrial scene
  has no night lights and its accents back. Screenshots (the M7.3 demo
  script at 4x, 30 fps, frames by `settlement.seconds`): noon (75 s) the
  lanterns are small dark posts, unlit; dusk (152 s) their heads lit and a
  soft warm pool under the tree; midnight (187.5 s) a warm pool over the
  plaza (piles, sprites, the wood gatherer), lantern pools at both ramp
  tops, the west column's yellow eyes readable and the east column on its
  ramp. A first try (400000 lm tree, 60000 lm lanterns) barely lit the
  plaza against the 1800 lux moon. Save format, input log, net protocol,
  commands and golden unchanged (view only). For later: the eyes are a pixel
  or two at the town zoom and show only on a ghoul facing the camera (the
  east column, walking away, shows none); the tree's pool shows faint
  concentric bands on the grass; see "Night lights polish".

### M7.3 The sun, the moon and the colors

- **Status:** done (branch `kestrel-sacred-tree-daynight`)
- **Why:** you read the hour from the shadows and the light's color (N5,
  N6).
- **What to do:** a `day_cycle` section in the scene file with four keys
  (dawn, noon, dusk, midnight), each with sunlight color and illuminance,
  ambient color and brightness, sky color and fog; blended between keys.
  The scene's one `DirectionalLight` follows an east-to-west arc tilted
  south, elevation clamped near 10° at the horizon; at night the same
  light is a dim blue moon on the same arc. The moon is dim: night is
  dark enough that M7.4's lights matter. Tune the shadow cascades for the
  256 m map and low sun. Scenes without the section keep their fixed
  light; a style switch keeps the cycle's light.
- **Where:** `kestrel/tools/assetc/core` (scene schema),
  `app/src/scene.rs`, `style.rs`, `app/assets/levels/sacred_tree.ron`.
- **Found in:** day and night design.
- **Tests:** keyframe blending at and between keys; screenshots at dawn,
  noon, dusk and midnight.
- **Done:** schema: `Scene::day_cycle: Option<DayCycleLight>`
  (`voxel_assets::day_cycle`, `#[serde(default)]`, so other scenes read
  none; no pack version bump, as with earlier optional scene fields):
  `DayCycleLight { dawn, noon, dusk, midnight }`, each a `LightKey {
  sunlight, illuminance, ambient, ambient_brightness, sky, fog_density }`
  (sRGB 0..1, the style profile's ranges). The fog's color is `sky`, as a
  style's fog is its background. A scene with one must have a scenario
  with a day cycle (`ScenePack::validate`). Pure functions:
  `day_cycle::hour(&Clock, time: f64) -> Hour { night, fraction }` (the
  sim's clock at a fractional tick, so the view eases between ticks);
  `DayCycleLight::at(hour) -> Light { key, toward }`: each key holds at its
  hour and eases (smoothstep) to the next, dawn to noon over the day's
  first 30% (`MORNING`), noon from 70% (`AFTERNOON`) to dusk, dusk to
  midnight over the night's first 30% (`EVENING`), midnight from 75%
  (`SMALL_HOURS`) to dawn; the illuminance dims to zero over the 3% at
  each end of a phase (`SWAP`), so the sun setting in the west comes back
  as the moon rising in the east unseen. `day_cycle::sky_arc(fraction)`:
  the unit direction towards the sun (by night the moon): `(cos a * cos
  e, sin e, sin a * cos e)` with `a = pi * fraction`, so east (+x) at the
  phase's start, due south (+z) in its middle and west (-x) at its end;
  elevation `e = 10 + 48 * sin a` degrees (`HORIZON_ELEVATION` 10,
  `PEAK_ELEVATION` 58), so shadows are at most about 5.7 times a caster's
  height. App (`app/src/daylight.rs`): `follow`, every frame in the scene
  view's Present phase, samples the cycle at the mirror's `tick + alpha`
  and writes the DirectionalLight's color, illuminance and rotation
  (shadows on), `GlobalAmbientLight`, `ClearColor` and the camera's
  `DistanceFog`. `style::apply` calls `daylight::dress` after the style,
  so the cycle wins over the style's sunlight, ambient, background and
  fog on a scene switch; without a cycle `dress` puts the light back at
  its fixed place (`fixed_transform`, the deck's (-8, 12, 5)) with the
  default cascades, so other scenes are as before. Cascades in a cycle
  scene (`cycle_cascades`): 3 cascades, 10 to 80 m, first far bound 32 m,
  overlap 0.2 (the town's orthographic camera is about 35 m out and shows
  ground 15 to 60 m deep; the default's first cascade, 0.1 to 10 m, was
  empty air and its last ran to 150 m at about 5 cm texels, now the far
  one is about 2.8 cm with the default 2048 map). Long low-sun shadows
  fall on ground on the caster's own light ray, so they stay inside a
  cascade's box; none clipped in the shots. The Sacred Tree's keys
  (`sacred_tree.ron`): dawn sun (1.0, 0.68, 0.42) 11000 lux, ambient 600;
  noon the forest style's (1.0, 0.96, 0.88) 14000, ambient 800; dusk
  (1.0, 0.52, 0.30) 12000, ambient 720, a red sky; midnight a moon (0.55,
  0.68, 1.0) at 1800 lux, ambient (0.32, 0.42, 0.78) 150, sky (0.015,
  0.025, 0.06). Tests: `voxel_assets` `day_cycle::tests` (keys hold at
  their hours, dawn and dusk dim at the swap, eased blends between every
  pair, the arc's east, south and west and its elevations, `hour` against
  the sim's `Clock::at` tick by tick, out-of-range keys fail);
  `style::tests::a_day_cycle_wins_over_the_style_and_other_scenes_keep_their_fixed_light`
  (the Sacred Tree twice from the industrial scene: dawn's colors, not the
  forest's, low in the east, 3 cascades, following the clock frame by
  frame; back in the industrial scene its fixed light, place and
  cascades). Screenshots (a demo script at 4x, 30 fps, frames picked by
  the trace's `settlement.seconds`: 9 s, 75 s, 143 s, 187.5 s): dawn warm
  with long shadows to the west; noon as before; dusk red-orange, the
  shadows long to the east; midnight dark blue, the ground, trees and the
  first ghouls on the east ramp faintly readable. Save format, input log,
  net protocol, commands and golden unchanged (view only). For later: the
  camera looks west-north-west, so at dusk the faces it sees are back-lit
  and the town reads darker than at dawn (dusk's ambient is raised to
  make up); the built-in deck's three accent point lights still light the
  town (a faint warm patch by the stone pile at night), worth deciding in
  M7.4; a demo script at under 30 fps with rate 4 runs at one tick a
  frame (the host's `MAX_CATCH_UP` of 8 ticks resets the pacer), so keep
  `fps * 8 >= 60 * rate` in M7.6's demo; the control protocol can't skip
  time, so shots of an hour need the whole run up to it.

### M7.2 Night waves and dawn

- **Status:** done (branch `kestrel-sacred-tree-daynight`)
- **Why:** ghouls attack at night, one wave per night, and the day starts
  clean (N2, N3, N10).
- **What to do:** in a scenario with a day cycle, waves are lanes (id,
  position, target, character) plus a `nights` list, each night giving the
  ghouls per lane; a night's ghouls spawn spread over its first third; the
  last night repeats. At sunrise every live ghoul catches fire (the fire
  effect) and falls within a few seconds, as dispelled (its own event, no
  kill). Replace the Sacred Tree's waves (start from today's 3/5/6/7/8/8
  and retune for 150 s days and 75 s nights). Scenarios without a cycle
  keep `first`/`every`/`count`. Save format up.
- **Where:** `kestrel/content` (scenario), `sim/src/settlement/combat.rs`
  (spawning), `fire.rs`, `mod.rs` (loading), `waves_tests.rs`,
  `lane_tests.rs`, `soak_tests.rs`, the happy path,
  `sacred_tree_supply.ron`.
- **Found in:** day and night design.
- **Tests:** no spawn by day; listed counts per lane within the first
  third; the last night repeats; dawn dispels all and the balances hold;
  ignored, the town falls on a known night; one ballista per ramp holds
  four nights; the soak runs several cycles; other scenes' golden
  behaviour lines unchanged (regenerate state hashes in their own commit
  if needed).
- **Done:** content: a wave with `nights: [..]` is a lane
  (`content::scenario::Wave::nights`, `Wave::lane()`): how many ghouls it
  sends on each night, the first night first; `first`/`every`/`count` are
  now `#[serde(default)]` and must be left out of a lane. Validation: a
  lane needs the day cycle, every lane lists the same number of nights (up
  to 64, each up to 64 ghouls); timed waves keep their old checks and may
  still run beside a day cycle (tests use one to send a ghoul at a set
  time); other scenes are unchanged. The supply reads
  `(id: "east", position: (40.0, 0.0), target: (1.8, -0.6), character:
  "ghoul_sacred_tree", nights: [3, 5, 5, 5])` and west `[2, 4, 5, 5]`
  (one lane per ramp, no more lane-per-ghoul workaround). Sim:
  `Settlement::clock: Option<Clock>` (from `Clock::of` in
  `from_scenario`; `Clock` is serde now) and `Settlement::dispelled: u64`;
  `WaveState::nights` (empty for timed), `lane()`, `sending()` (timed until
  its count, a lane always; the reroutes use it, so a moat dug by day
  replans a lane's route), `on_night(n)` (the last listed night repeats).
  `combat::lane_starts(count, night_ticks)`: a lane's ghouls set off
  `COLUMN_GAP` (2.5 s) apart from dusk, closer (evenly over the night's
  first third) only if that would run past it; `combat_housekeeping`
  spawns `due` ones per tick (`spawn` is the old body). Clock helpers:
  `since_dusk(t) -> Option<u64>`, `dawn(t)` (a tick that starts a day after
  a night; tick 0 isn't). Dawn (`Settlement::dawn`, from housekeeping on a
  `dawn` tick): every threat gets `Burn { dawn: true, damage: 0, .. }`
  lasting `DAWN_BURN` (2 s) plus up to 2 s by id, drops its windup, stands
  still (`follow_route`), starts no swing and doesn't breach; a fire patch
  doesn't touch a dawn burn; when it runs out `Settlement::burn` removes it
  with `Event::Dispelled { threat, position }` and `dispelled += 1`: no
  `Hit`, no kill, no breach. The app maps it to
  `legacy::Event::Dispelled { target, position }` (no effect or sound yet:
  the threat's `burning` flames show, then it's gone). For the HUD:
  `Settlement::next_wave(tick) -> Option<u64>` (the next tick any wave
  sends, timed or by night; the status line's "NEXT WAVE" now uses it, so
  it counts down to dusk), `nights_survived(tick)` (sunrises so far),
  `clock.map(|c| c.night(tick))` for the night's number, `dispelled` for
  the dawn's toll. Tuning (measured by `waves_tests`: ballistas manned by
  sprites and topped up from the tree 47 s after each dusk):

  | Number | Before | After | Measured |
  |---|---|---|---|
  | Waves | 165 s calm, then 3/5/6/7/8/8 every 90 s over 8 one-ghoul lanes | the first day calm (dusk at 150 s), then a night's columns of east 3/5/5/5, west 2/4/5/5: 5, 9, 10 and 10 ghouls, the fourth repeating | Ghouls take about 63 s from the ring, so a column's 5th (10 s behind its head) reaches the tree just before dawn (73 s); a 6th would only burn on the hill, hence columns of at most 5. Ignored, the tree falls on the third night, at 668 s (68 s after dusk, 20 breaches, with ghouls of that night still to come); before, the fourth wave at 504 s. One ballista by the east ramp: tree 4/20 after four nights, 16 breaches, 18 kills. One by each ramp: 20/20, all 34 killed, none dispelled. |
  | Dawn | none | 2 to 4 s burn | With the real nights all columns arrive before dawn, so dawn burns only stragglers (chasers, broken-building fighters, stranded ghouls). |

  Tests: `clock_tests::sunrises_and_the_time_since_dusk`; `waves_tests`
  (rewritten): `a_lanes_ghouls_set_off_in_a_column_within_the_nights_first_third`,
  `ghouls_come_only_by_night_as_listed_and_the_last_night_repeats` (20 s
  days, 9 s nights, six nights: every spawn by night within the first
  third, each lane's listed count per night, nights 5 and 6 repeat night
  4, every ghoul dispelled, no kill), `the_sunrise_burns_every_ghoul_away`
  (real clock, an east night of 8: at dawn all alight, standing still, gone
  within 2 to 4 s, no hit, kill or breach, then a calm day),
  `an_undefended_town_has_its_calm_day_then_falls_on_the_third_night`
  (also `next_wave(0)` is the first dusk, `nights_survived` 2), and
  `a_ballista_guards_one_ramp_and_one_by_each_holds_four_nights`; every
  run checks spawned = out + breached + killed + dispelled.
  `lane_tests::a_moat_dug_by_day_sends_the_next_nights_column_round_by_the_other_ramp`.
  The wave soak plays five days and nights (1130 s) with `validate` and the
  balances every tick and checks every night's ghouls came and five nights
  survived. `content_test`: the lanes, and lanes without a cycle, with
  timing or with unequal night lists fail. The app's happy path (control
  tests) must end before 190 s (ghouls set off at the 150 s dusk). Save
  format 39 (`WaveState::nights`, `Settlement::clock`, `dispelled`,
  `Burn::dawn`); input log, net protocol and `CommandKind` unchanged.
  Golden: forest and arena state hashes moved (the new fields), behaviour
  lines unchanged; regenerated in its own commit. `Threat` is 8 bytes
  smaller (the `dawn` flag gives `Option<Burn>` a niche). Not done: the
  demo `demos/sacred_tree.json` was not rerun; its fixed frame waits were
  timed for a first wave at 165 s and three east ghouls, and night 1 now
  falls at 150 s with 3 east and 2 west, so its fireball and bolts likely
  miss the column (M7.6).

### M7.1 The clock

- **Status:** done (branch `kestrel-sacred-tree-daynight`)
- **Why:** the night decides when ghouls come, so the sim must know the
  hour ([day and night design](../SACRED_TREE_DAYNIGHT.md), N1, N4).
- **What to do:** a `day_cycle: Some((day: 150.0, night: 75.0))` setting
  in the supply's scenario (seconds; start at dawn); pure functions of the
  tick for the phase (day or night), the fraction through it, the night's
  number and the time to the next dusk, in `Fx`/ticks. No new settlement
  state. Scenarios without a cycle report none.
- **Where:** `kestrel/content` (scenario schema), `kestrel/sim/src/settlement`
  (a new `clock.rs`), `app/assets/levels/sacred_tree_supply.ron`.
- **Found in:** day and night design.
- **Tests:** boundaries for unequal lengths, night numbers, time to dusk;
  no cycle means no clock.
- **Done:** content: `Scenario::day_cycle: Option<DayCycle>`
  (`content::scenario::DayCycle { day, night }`, `Fx` seconds, each 1 to
  3600 s), `#[serde(default)]` so other scenarios read none; written
  `day_cycle: Some((day: 150.0, night: 75.0))`. The Sacred Tree's supply
  has it; its waves are unchanged. Sim: `settlement::clock` (re-exported
  `settlement::{Clock, TimeOfDay}`; the phase enum is `clock::Phase`, since
  `settlement::Phase` is the haulers'). `Clock::of(&Scenario) ->
  Option<Clock>` converts with `scenario::ticks` (150 s is 9000 ticks);
  `Clock { day, night }` in ticks, `cycle()`, and for a tick `phase(t)`
  (`Day` below `day` ticks into the cycle, then `Night`), `fraction(t)`
  (`Fx`, 0 at the phase's first tick, under 1 at its last), `night(t)`
  (nights begun: 0 the first day, `n` through the `n`th night and the day
  after it), `ticks_to_dusk(t)` (`day` at dawn, 1 on a day's last tick, a
  whole cycle at dusk itself), and `at(t) -> TimeOfDay` with all four. The
  cycle starts at dawn at tick 0; there is no start offset (the design's
  `start`), add one with a field if a scene needs it. Nothing is stored in
  `Settlement`, so saves, hashes and the golden don't change; the app can
  build the clock from its scene's scenario. M7.2 needs it inside the
  sim's tick: keep a `Clock` (or the cycle's ticks) in `Settlement` from
  `from_scenario` then, with that task's save format bump. Tests
  (`settlement/clock_tests.rs`, 6 s days and 3 s nights): phases and
  fractions at dawn and dusk, night numbers, time to dusk counting down and
  wrapping; the Sacred Tree's clock (dusk at 2:30, night 1 at dusk and the
  next day, four nights by 15 minutes); no cycle means no clock, and
  fractional seconds round up as the loader's do. `content_test`: only the
  Sacred Tree has a cycle, and out-of-range lengths fail validation.

### M6.7 Waves, tuning, soak and demo

- **Status:** done (branch `kestrel-sacred-tree-mining`)
- **Why:** the big map changes travel times and the stone supply.
- **What to do:**
  - Tune the waves, starting stone, dig speeds and yields on the big map.
  - From M6.3: a wave's route is planned once, so a pit dug on it later
    doesn't re-route ghouls, and a moat round a ramp's foot can cut a lane.
    Replan routes when the terrain changes (the chase's `replan_routes`),
    and decide what a cut lane means.
  - From M6.3: mid-wave reloads are no longer needed and ghouls take about
    63 s to arrive; the demo still expects the old lanes.
  - The soak test runs with digging and miners, with `validate` every tick.
    (From M6.5: the wave soak already rotates sprites over mining.)
  - From M6.5: tune the miner's 9 s dig and 6-unit bundle; miners walk
    into ghouls off the hill and don't flee, and they lure chasers away
    from the tree.
  - Update `demos/sacred_tree.json` and `app/README.md`.
- **Where:** `kestrel/sim/src/settlement/soak_tests.rs`,
  `sacred_tree_supply.ron`, `kestrel/app/demos/`.
- **Found in:** mining design.
- **Done:** routes: a dig replans the routes that pass within 2 m of the
  dug tile (`Settlement::reroute_round`, from `dig_housekeeping`): every
  threat on its way to the tree whose remaining route does, and each wave
  still sending whose planned route does (at its next spawn); routes far
  from a dig are kept, so a dig costs a few searches at most. A cut lane:
  routes are planned with `nav::path_toward`, which, when no route reaches
  the goal, returns the route to the searched node nearest it that the
  search reached (`path_with` shares the search and gives the same routes
  as before). A ghoul with such a route is `Threat::stranded`: it walks to
  its end and waits there (the route keeps its last point, so it never
  breaches from afar), still chasing anyone who comes in sight, and plans
  again every 5 s (`retry_stranded`, spread by id; a failed search costs
  the whole window, so not on every dig) and on any blueprint. A wave cut
  off spawns its ghouls stranded. Cutting one ramp sends its ghouls round
  the hill to the other; cutting both leaves them waiting at the cliff's
  foot 16 m from the tree (new open task "Moats make the tree safe").
  Fleeing: stage 2 hands sprites the chasing enemies (`Ctx::danger`); a
  sprite off the hill's plateau (`Hill::on_plateau`) that one chases or is
  within its detect radius of (`Alarm::Threatened`) drops its plan and its
  route and plans `[GoTo(home), Wait]`, the sprites' plan shape; at home it
  waits a retry and stays fled while one chases it or is within 9 m
  (`Alarm::Wary`), then plans its job. One holding a slot claim or moving
  a bundle's units finishes that first; scenes without a hill never flee.
  Tests (`settlement/lane_tests.rs`): a pit dug (the last layer by the
  elder's dig) on a walking ghoul's route turns it round and it still
  breaches; a moat round the east ramp sends the east ghoul up the west
  ramp; with both moated, one on its way and one sent later wait stranded
  for 150 s without a breach; a miner digging off the hill flees a ghoul
  2.5 m away, outruns it up the hill and digs again once it is gone.
  `hill_tests`' `walks` compared each leg with itself; it checks the real
  legs now (the ramp routes still pass). Tuning (all in
  `sacred_tree_supply.ron`; measured by `waves_tests`, ballistas manned by
  sprites and topped up from the tree 5 s before each wave's march ends,
  by `miner_tests::two_miners_bring_a_steady_modest_income` and by the
  wave soak):

  | Number | Before | After | Measured |
  |---|---|---|---|
  | Calm start | first ghoul at 210 s, at the tree about 273 s | first at 165 s, the hill's top about 205 s, the tree about 228 s | Ghouls walk 63 s from the ring; this puts them back near M5.6's arrival. The happy path (no cheats) still ends in the calm start, at 131 s. |
  | Waves | 3/4/5/7/8/8: east column 3 to 5 (5 lanes), west 1 to 3 from the third wave; 35 ghouls | 3/5/6/7/8/8: east 3, then 4 from the second (4 lanes), west one more a wave from the second, to 4; 37 ghouls | Ignored, the tree fell in the fifth wave (633 s), now in the fourth (504 s). One ballista by the east ramp: tree 11/20 with 9 breaches before, 6/20 with 14 after. One by each ramp: 20/20 both (35, then 37 killed). No reload mid-wave: a ramp brings at most 4, 8 bolts of a 10-stone store. |
  | Elder dig | 2 s | kept | Settled (M6.2). No starting stone: the first mechanism's 2 stone are dug, so the elder digs first (the happy path digs at 65 s). |
  | Miner dig / bundle | 9 s / 6 | kept | Two miners alone bring 45 stone in 5 min (10, 8, 15, 8, 4 a minute, slowing as the near stone goes): about the 10 a minute two ballistas spend over the waves, so they keep them stocked between waves and the elder's digs top up. |
  | Stone yield / veins | 3–6 a layer; 4 exposed veins 18–37 m out | kept | A vein's ten stone layers give about 45 stone in a minute of the elder's digging; stone is never short, it costs trips. Yields and veins are code (new open task). |
  | Wave soak | one ballista: tree 11/20; no elder digs | a ballista by each ramp (Ash and Fern), the elder digging stone off the hill in quiet cycles | Tree 18/20: 42 cycles, 35 kills, 2 breaches, 7 buildings broken, 3 sprite deaths and returns, 1 elder death, 10 elder digs, 216 stone dug. |

  Soak: `defending_against_the_waves_strands_no_sprite` plays all six
  waves on the real map (the hill in its mine) with miners and the elder
  digging (`dig_stone`: down to a stone tile the miners' search finds,
  from the tile behind its stand so it faces it, then F), `validate` and
  the balances every tick; both ballistas and two Wood Gatherers go up in
  the calm start (by 41 s). Demo `demos/sacred_tree.json`, at 2x then 4x:
  the elder digs two stone layers at the east ramp's vein, picks up a
  bundle, sends Reed to mine, builds a ballista by the east ramp's end
  from the tree's wood and the dug stone and loads the leftover, and Fern
  mans it; at 4x the elder loads the stone Reed brought, then possesses
  the ballista as the first column tops the ramp, throws a fireball at its
  head, fires four bolts and leaves, and Fern finishes it (tree 20/20).
  Only the wood is stocked. `app/README.md` describes the hill, F, miners,
  fleeing, the waves and the demo. The M6.6 stripes: the demo's frames by the east ramp's foot show them (pale, tall, one-tile-wide walls on the far terraces at the screen's lower right); `sloped_chunk_mesh` draws walls only down to a lower neighbour or at the arena's edge, and the generator makes no drop tiles, so the cause is not found yet (new open task). Save format 38
  (`Threat::stranded`); input log, net protocol and `CommandKind`
  unchanged. Golden: the arena's state hashes moved (the new field),
  behaviour lines unchanged; regenerated in its own commit.

### M6.6 Drawing sloped ground and stone

- **Status:** done (branch `kestrel-sacred-tree-mining`)
- **Why:** the ramps, pits and veins must read on screen.
- **What to do:**
  - A ground mesh from the corner heights (sloped pit edges and terraces),
    with the hill's cliff sides.
  - Stone tints on the ground and pit walls; a dig bar and dust for sprites.
  - Keep the 256 m map fast to draw and re-mesh only what a dig changes.
  - From M6.3: the HUD shows Dig Lab's line ("PIT x/6 / F TO DIG / HOLD
    SPACE TO FLY OUT") in the town; show the town's own dig hint instead.
  - From M6.5: `digging::shown_digs` shows avatars' digs only; a miner's
    is `AgentState::digging()` (a `Dig` as the elder's, 9 s long).
- **Where:** `kestrel/app/src/digging.rs`, the town drawing.
- **Found in:** mining design.
- **Done:** `digging.rs` meshes sloped scenes
  (`sloped_chunk_mesh`; the Dig Lab keeps `chunk_mesh`, its colors
  unchanged): a gentle tile's top is two triangles from its corner heights
  (each the lowest of the four tiles round it, as `Terrain::patch`; split
  on the flatter diagonal), so pit edges, ramps and terraces slope; a
  steep tile (`Terrain::steep`'s rule) is drawn flat at its own height with
  walls down to lower neighbours, so the hill's sides, the ramps' edges and
  pits two or more layers deep stand as cliffs. A gentle tile's corners
  are never above a neighbour's top, so walls only fall from steep tiles
  (tested: no holes). Walls are clipped into quarter-meter bands through
  the scene's `max_layers` (six in the town; `layer_of` darkens over
  however many there are), then rock. A slope's corners are colored by how
  far they are dug below the lowest undug ground round them, so terraces
  stay surface. Stone: `tint` gives `STONE_TINT` (cool grey) for
  `Veins::Stone`, the dig lab's ore colors otherwise. Chunks stay 16 x 16
  tiles; a dig rebuilds the chunks of the tiles within two of it in sloped
  scenes (its four corners move, reshaping the 3 x 3 round it, whose walls
  reach one further; `redrawn`, `dirty_chunks`), the old plus elsewhere.
  Cost (release demo, 256 m map): the 289 chunks build in 29-30 ms at
  world start; a dig rebuilds 1 or 2 chunks in 0.2-0.44 ms (logged at
  info). Miners: `shown_digs` reads `AgentState::digging()` for every
  agent, a sprite's bar 1.18 m over its feet (under its tag); dust and the
  layer burst as the elder's. Dug stone: three grey cubes fly 0.45 s from
  the pit onto the nearest ground pile within 3.5 m and land with a clack
  (`toss_stone`, `fly_stones`; the pile itself still fills at once). Town
  drawing stood every sprite, ghoul, pile, building and caption on the
  scene pack's height (0), so off the plateau they floated 2 m over the
  ground; `settlement::town_ground` reads the sim's terrain in scenes with
  a mine. HUD: F works in the town (keyboard F is `Utility::Dig` there
  too, `input::tests::keyboard_f_digs_in_a_town`); `gameplay::town_dig`
  asks the sim (`Settlement::dig_in_front_of`, new) and shows the refusal
  over the elder ("Too deep from here: step down first", "A pile is in
  the way", ...) instead of sending a dig that would be refused; the town
  help line reads "... E INTERACT / F DIG / B BUILD ..."; the digging HUD
  line in sloped scenes reads "PIT x/6 / F DIG IN FRONT" (no flying).
  Tests (`digging::tests`): drawn corners equal the sim's ground and
  steepness on level ground and generated terraces, edges close, a dig
  changes nothing outside `redrawn`, chunk dirtiness at borders and
  corners, band clipping, stone tint. Save format, input log, net
  protocol, commands and golden file unchanged.
  For later: the digging HUD line sits in the contact panel, which towns
  hide, so the town's hint is the help line and the refusal text; the
  refusal is checked against the confirmed world (a joined client's
  prediction can differ by a tick). The camera can't zoom from the control
  protocol, so the shots are crops. Pits at the hill's foot lie in its
  shadow and read dark; far terraces seen at a grazing angle show pale
  vertical stripes (probably distant steep tiles' walls in the fog), worth a
  look with M6.7's demo. Slope colors interpolate between corners (soft,
  not banded like walls).

### M6.5 Miner sprites

- **Status:** done (branch `kestrel-sacred-tree-mining`)
- **Why:** sprites dig, slowly (Settled, M9).
- **What to do:**
  - A "Mine" sprite job. A miner goes to the nearest tile reachable from
    the tree whose top layer is stone, digs it 4–5 times slower than the
    elder (within the M3 rule), and carries the drop to the tree's stone
    pile.
  - With no exposed stone, it waits at the stone pile and replans when the
    ground changes.
  - The plan keeps the sprites' shape (goal, plan, invalidate, replan).
  - Add the job to the job menu.
- **Where:** `kestrel/sim/src/settlement/sprites.rs`, `kestrel/app/src/legacy.rs`.
- **Found in:** mining design.
- **Done:** `SpriteJob::Mine` (appended; the job command takes it, so
  the input log, net protocol and `CommandKind` are unchanged) and
  `settlement/miners.rs`. A miner plans one errand at a time and plans
  again when it ends: carrying something, to the tree's stone pile
  (`Settlement::stone_depot`, the depot that takes only stone; any job
  delivers first); else the nearest stone ground pile within 48 m of the
  tree (its own drops, the elder's, dropped bundles: stone on the ground
  is worth more than stone in it, so it clears drops before digging on);
  else `[ClaimTile, GoTo, Dig]` on the nearest stone tile; else it walks
  to a spot 1.2 m off the stone pile (one per sprite) and `Wait`s,
  planning again every `RETRY_TICKS`. The search (`miners::search`)
  walks tile middles four ways from the stone pile's tile over ground
  that isn't steep and between obstacles, out to 48 tiles, and keeps the
  nearest 8 stone-topped tiles `dig_refusal` lets it dig from the tile it
  reached them from, cached by terrain revision in `SpriteState::targets`
  (serde-skipped, equal to any other: the ground gives it again after a
  load), so waiting replans are cheap; claims, what stands on a tile
  (`View::dig_blocked`, split out of `dig_in_front`) and given-up goals
  are checked on every plan. Claims: `SpriteState::dig_claim`, asked in
  stage 3 (`ClaimDigTile`), granted in stage 5 to the lowest agent index
  for a tile no sprite holds (`claim_dig_tiles`), given up when the
  sprite drops its plan. `Dig` faces the tile and starts only if
  `dig_in_front` still gives it; it is done once the pit is deeper (by
  anyone); housekeeping deepens only if the layer is still there. Dig
  540 frames (9 s, 4.5x the elder; `sprite.ron`'s new `sprite.mine:
  (dig, capacity: 6, transfer: 20)`); every sprite's hold is now a 6-unit
  bundle. `Pickup`/`Deposit` start their carry job in stage 5
  (`SpriteBundle`, `sprite_bundles`, by `avatar::bundle_job`) and move
  units through `player_carry` from stage 3. A `GoTo` that finds no route
  twice gives its goal tile up until the terrain revision changes
  (`unreachable`). Death or a job change ends the carry job, dig and claim
  (`drop_sprite_errand`); the stone carried is kept and delivered. Dug
  stone no longer lands on a tile made steep by a deep pit (`mined`; the
  miner test found a drop no one could reach). `AgentState::digging()`
  gives an elder's or a miner's dig for M6.6. App: MINE in the job menu
  where the ground has stone veins and the tree a stone pile, `"mine"` in
  the control protocol's job fields, `SpriteView::job` "mine" and
  activities `digging`/`hauling`, MINER tags. Happy path: step 6 sends
  Reed to mine before the deposit, and at the end the tree holds the 3
  left plus the 1 to 7 stone left on the ground (all under 210 s). Tests:
  `settlement/miner_tests.rs` (exposed stone fetched and delivered, two
  miners never share a tile, waiting until the elder uncovers a buried
  vein in a 12 m town, thread and save determinism); the wave soak's rota
  mines instead of idling, and the elder defends only against ghouls
  within 15 m of the tree (else miners lured chasers away and the elder sat
  in the ballista all wave, so Ash never shot). Save format 37; golden
  unchanged.
  For later: M6.6 draws sprite dig bars and dust (`digging::shown_digs`
  reads avatars only; use `AgentState::digging`); miners walk into ghouls
  (no fleeing); the elder can dig a tile a miner claimed (the miner just
  replans); M6.7 tunes the miner's 9 s and 6-unit bundle; the search is a
  tile walk, not nav, so a goal it reaches nav may still refuse (given up
  until the ground changes).

### M6.4 No more Stone Gatherer

- **Status:** done (branch `kestrel-sacred-tree-mining`)
- **Why:** all stone comes from digging (Settled).
- **What to do:**
  - Remove the Stone Gatherer from the Sacred Tree's supply and build menu;
    test fixtures may keep it as a generic kind.
  - Rewrite the happy-path test: the Stone Gatherer step becomes the elder
    digging stone (and, after M6.5, a miner fetching it).
  - A small starting stone stock if the first mechanism needs it.
- **Where:** `kestrel/app/assets/levels/sacred_tree_supply.ron`, the sim
  and control tests that build one.
- **Found in:** mining design.
- **Done:** the supply's building kinds are `wood_gatherer` and
  `ballista` (so the ballista is kind 1; tests' constants moved), and its
  comments say stone is dug; the job menu reads IDLE, GATHER WOOD, MAN
  BALLISTA (it lists kinds generically; no app-side special case used the
  Stone Gatherer). No starting stone: the happy path digs its first stone
  at 65 s, has it at the tree at 77 s and the first mechanism in at 92 s,
  all of it in 131 s of the 210 s calm start. The happy path
  (`control::tests::the_happy_path_plays_from_a_fresh_start`, no cheats)
  steps 4 to 6 now: walk down the east ramp to arena tile (20, -11), facing
  west; `{"op":"dig"}` (works in the town, a `Digging` scene since M6.3)
  digs the vein's east arm (19, -11); walk into that pit and dig the
  vein's middle (18, -11); two stone layers drop 6 to 12 stone on one
  pile; "PICK UP 5 STONE"; deposit at the tree's stone pile. Step 7's
  Wood Gatherer is building 1, and leaves the tree 6 wood and 3 stone.
  The vein is the east one `Hill::exposed_veins` gives for seed 7,
  hard-coded and checked through the state's `digging.nearest_ore`. The
  control build test builds a Wood Gatherer (Ash staffs the prebuilt one
  first; wood, the leftover back, then stone) and the job-menu test picks
  the ballista. The bootstrapping test became
  `construction_tests::both_kinds_build_from_an_empty_tree_and_dug_stone`
  on the map (dig two layers of the east vein, carry every dug stone up,
  then a Wood Gatherer and a ballista). Tests that only needed a second
  gathering kind (the construction steps, the build soak) add a test-only
  Stone Gatherer after the ballista (`tests::with_stone_gatherer`); the
  wave soak builds a Wood Gatherer where its Stone Gatherer stood.
  `app/README.md` says where stone comes from; the demo script needs
  nothing for M6.4 (it never built a Stone Gatherer; it stocks stone with
  `settlement_stock`), its old lanes stay M6.7's. Save format, input log,
  net protocol, commands and the golden file unchanged.
  For later: M6.5 extends the happy path where its step 6 says
  (Reed to Mine, a miner fetching the dug stone left on the ground, 1 to
  7 units) and the bootstrapping test's note; M6.7 can tune the yields
  against the 77 s first stone.

### M6.3 The big map and the hill

- **Status:** done (branch `kestrel-sacred-tree-mining`)
- **Why:** room to explore for stone, and a hill that makes the defence a
  question of two entrances (M5, M7).
- **What to do:**
  - The Sacred Tree becomes 256 × 256 m of generated ground with an
    authored plateau about 2 m high, wide enough for the 26 m town, with two
    ramps on opposite sides. The hill and plateau can't be dug.
  - The generator guarantees a few stone veins exposed at the surface 15–40
    m from the tree.
  - Ghouls spawn on a ring about 40 m from the tree, one lane per ramp.
  - The town camera follows the elder over the big map.
- **Where:** `kestrel/sim/src/mine.rs`, `terrain.rs`,
  `kestrel/app/assets/levels/sacred_tree.ron`, `sacred_tree_supply.ron`.
- **Found in:** mining design.
- **Done:** `sacred_tree.ron` has `features: [Digging, SlopedGround]`,
  `dig_layers: Some(6)` and `mine: Some((extent: 128.0, seed: 7, hill:
  Some((plateau: 15, height: 8, ramp_width: 7, ramp_length: 8, foot:
  10))))`; the supply's extent is 128. The hill (`voxel_assets::HillLayout`
  in whole tiles and 0.25 m steps, `SceneSetup::hill`, `mine::Hill`,
  `Terrain::raise_hill`, centered on the tree's tile) is a 31 m square
  plateau 8 steps up, cliffs on every side, and a ramp 7 tiles wide down
  the middle of its west (-x) and east (+x) sides falling a step a tile
  over 8 tiles. The generator applies it over its terraces (integer only):
  flat for `foot` tiles round the plateau, then a step at most every 3
  tiles, as round Dig Lab's landing. The plateau's top is the mine's datum
  (`Mine::height` subtracts the hill's steps), so the town and its
  architecture stay at zero and the ground round it lies 2 m below
  (terraces far off rise past the plateau). `raise_hill` pushes the
  plateau and both ramps into `Terrain::undiggable`. A town's hill
  guarantees `EXPOSED_VEINS` (4) stone veins, one per quarter, a plus of
  five tiles stone two layers down, 18 to 37 m from the tree, off the hill
  (`Hill::exposed_veins`). Waves: the five north lanes became an east
  column from (40, 0) to (1.8, -0.6) and the west/east lanes three west
  lanes from (-40, 0) to (-1.8, -0.6), same timings; each route is a
  straight walk up its ramp. Ramps went west and east, not north and
  south: the sprites idle on the tree's +z side, and a south lane killed
  them 34 times in a run. Nav: big-arena windows always hold the hill and
  its ramps' feet (`HILL_MARGIN`, 4 tiles), else a route off the plateau
  found no ramp. Cost (unoptimized test build): a wave's route 0.1 ms, the
  elder's walk to the foot 7–10 ms, an errand on the plateau 1–2 ms, a
  failing search (a walled pit at the foot) 49 ms; the wave soak (13 game
  minutes) and the build soak take 3.3 s together; no height cache needed.
  Chasers steer, are knocked back, and see a quarry only over walkable
  ground in sloped scenes (`chase::walk_clear`), so none climbs a cliff.
  Placement refuses a footprint over a pit or a steep tile
  (`PlacementRefusal::Uneven`, "UNEVEN GROUND"); prebuilt buildings are
  placed before the mine is generated, so they aren't checked against it
  (the Wood Gatherer is on the plateau). The app's camera follows the elder
  as in Dig Lab (`digging::follow_camera`, on in `Digging` scenes; Z/C
  orbit round the followed target). `digging.rs` bands pit walls from the
  datum. The grass box is lifted 1 cm off the drawn plateau. Save format
  36 (`MineSpec::hill`, `PlacementRefusal::Uneven`); input log, net
  protocol and commands unchanged; golden unchanged. Tests:
  `settlement/hill_tests.rs` (reads the scene's mine line; the shape,
  undiggable hill, exposed veins, ramp routes from the ring, placements,
  the elder down a ramp and back, nav timings) and
  `save::a_town_mine_keeps_its_hill`. Changed for the map: waves and both
  soaks play on it (`hill_tests::sacred_tree_map`); their restocks and
  end wait the 52 s longer walk; the defence test became
  `a_ballista_guards_one_ramp_and_two_keep_the_tree_standing` (one by the
  east ramp kills the whole column, tree 11; one per ramp, tree 20); the
  wave soak's buildings moved to the new lanes (tree 11) and the build
  soak has no waves (the east column crossed its spots); the 8.5 m
  outside-the-arena checks moved to 127.5/130 m; the Burst test respawns
  the elder at its start (a bare scenario's spawn is the world corner,
  past the big map's edge).
  For later: M6.7 must retune: one ballista beside the tree no longer
  runs dry (the east column brings 3–5 a wave, 10 bolts), so the
  "reload mid-wave" lever is gone; ghouls take about 63 s to arrive; the
  demo script still expects the old lanes. Ghoul routes walk straight
  through a pit dug later at a ramp's foot (routes plan once a wave; a dig
  doesn't replan them as a blueprint does), and digging a moat round a
  ramp's foot can cut a lane. In a town the Dig Lab HUD line ("PIT x/6 / F
  TO DIG / HOLD SPACE TO FLY OUT") shows. M6.5 miners: veins are at
  `Hill::exposed_veins`; routes off the plateau go by a ramp.

### M6.2 Digging in front, and the stone yield

- **Status:** done (branch `kestrel-sacred-tree-mining`)
- **Why:** digging must never trap the digger, and stone comes from the
  ground (M3, M5, M6).
- **What to do:**
  - In sloped scenes, a dig takes the faced tile, at most one layer below
    the digger's own tile. No dig under a building, site, pile, the tree or
    undiggable ground (M7). The elder digs in about 2 s, reusing Dig Lab's
    dig command, bar and dust.
  - Stone becomes a mineral in the generator's layers; digging out a stone
    layer drops 3–6 stone onto a town ground pile beside the pit.
  - Put the drops in `balances` and `validate`.
  - A randomized property test: no sequence of allowed digs leaves a walker
    without a walkable way to undug ground.
  - **From M6.1:** a dig can make a tile it doesn't touch steep (a
    diagonal neighbour of the dug tile, whose shared corner drops a
    second layer below its far corners), and `walkable` is strict: a body
    whose footprint is on steep ground can't move or path at all. Either
    refuse such digs while someone stands there, or let a walker step off
    steep ground it already stands on (without letting nav chain nodes
    across steep tiles). Today's underfoot dig (Dig Lab's) still works in
    sloped scenes and is what M3 replaces there.
- **Where:** `kestrel/sim/src/settlement/avatar.rs`, `terrain.rs`,
  `mine.rs`, `piles.rs`.
- **Found in:** mining design.
- **Done:** in a sloped scene `DigDown` (unchanged command) digs
  `Terrain::front_tile`: the tile beside the digger's along the axis its
  `body.heading` points most. `View::dig_in_front` refuses it under a
  terrain obstacle (tree, buildings, sites, props), a ground pile, a
  depot dock, or a building's output pile or slot spot (`DigRefusal`);
  `Terrain::dig_refusal` refuses an undiggable scene or tile (outside, a
  drop, a `Terrain::undiggable` tile range: the seam M6.3's hill sets,
  empty today), the bottom layer, a dig leaving the tile more than a layer
  below the digger's (`TooDeep`), and a dig that strands anyone
  (`Strands`): on a clone with the layer dug, from the middle of every
  tile within two of it, a body of the digger's radius must walk (tile
  middle to middle, 8 ways, `walkable`) to an undug tile that isn't steep
  (a search past `WALK_OUT_TILES`, 256, counts as out). The depth rule
  alone isn't enough: the property test trapped the digger by deepening a
  stair's last step from below, and by deep pits dragging a step's corners
  down. The steep-tile issue from M6.1: `walkable` lets a body whose
  footprint starts on steep ground walk off it as long as no 0.1 m step
  climbs faster than `MAX_SLOPE`, and never back onto steep ground once
  off it, so nav can't enter steep ground from gentle ground. A digger's
  own tile can't get steep from an allowed dig (shown in the doc; the
  `Steepens` check is a guard). Digging in front stands still (no
  centering); the dig lab still digs and centers underfoot. A town's mine
  (`from_scene`: building kinds and no `PileSetup`) has
  `MineSpec::veins: Veins::Stone` (broad veins, over 1 in 50 surface
  tiles on seed 7, richer each layer down) and needs the scenario's
  `stone` item (`mine::STONE_ID`); a stone layer drops `yield_of` (3–6)
  stone onto a ground pile beside the pit or one within 2.2 m
  (`PileLab::mined`, now kept clear of docks, output piles and slot
  spots), counted as `generated`, so `balances` and `validate` cover it;
  no `PileToss` events in a town (the pile just appears; M6.6 can animate
  it). The elder has the exosuit's Dig (120 frames). Save format 35
  (`MineSpec::veins`, `Terrain::undiggable`); input log, net protocol and
  commands unchanged; the golden file is unchanged. Tests:
  `settlement/dig_tests.rs` (stone in front onto a pile and merging,
  carried as a bundle, every refusal, the seeded random-dig property test
  on level ground and on terraces, walking off steep ground) and
  `scene_tests::sloped_scenes_dig_a_stair_in_front` (a six-layer stair,
  refusing to strand its bottom). For later: M6.3 makes the Sacred Tree a
  mine (`mine:` and `Digging` in the level; the stone veins follow) and
  sets `Terrain::undiggable` for the hill and plateau (scene plumbing
  still to add), and guarantees exposed veins 15–40 m out; M6.5 miners
  can call `Terrain::dig_refusal`/`View::dig_in_front` from a standing
  tile (it clones the terrain per call, so cache plans, don't poll it);
  `Strands` uses the digger's radius, so a wider body (a ghoul brute)
  could still be boxed in; obstacles aren't counted, so buildings can wall
  a pit in; a building placed later isn't refused over a pit; M6.6 draws
  the stone tint (`Mine::ore` gives `STONE` = 0, which `digging.rs` would
  color as amber) and the town dig's bar and dust; the app sends the dig
  only in `Digging` scenes.

### M6.1 Sloped ground

- **Status:** done (branch `kestrel-sacred-tree-mining`)
- **Why:** pit edges become ramps, so no pit traps a walker
  ([mining design](../SACRED_TREE_MINING.md), M1, M2, M4, M8).
- **What to do:** scene settings for sloped ground and the depth limit;
  corner heights and bilinear ground; walkable by slope; paths and enemy
  routes follow it; Dig Lab unchanged.
- **Where:** `kestrel/sim/src/terrain.rs`, `mine.rs`, `nav.rs`,
  `settlement/mod.rs` (`SceneSetup`), `kestrel/tools/assetc/core`
  (scene schema), `kestrel/app/src/lockstep.rs`.
- **Found in:** mining design.
- **Done:** a scene opts in with `features: [SlopedGround]` and sets its
  depth with `dig_layers: Some(n)` (scene schema in `voxel_assets`, the
  pack stays version 16 since both default); `SceneSetup::sloped` and
  `dig_layers` carry them to `Terrain::sloped` and `Terrain::max_layers`
  (default `MAX_LAYERS`, 3). A generated mine takes the terrain's layers
  at `generate_mine` (`MineSpec::layers`, ore arrays sized by it), so set
  `max_layers` first. In a sloped scene a tile's height is `top` at its
  center less its pit (surfaces snap to tiles), each corner is the lowest
  of the four tiles touching it (drop tiles skipped; a drop tile itself
  has no ground), and `ground` is bilinear in `Fx` (`Terrain::patch`).
  `walkable` checks, every 0.1 m from `a` on, all five footprint points:
  no drop, and no slope over `MAX_SLOPE` (a layer per tile) along x or y
  at that point; a diagonal may be up to √2 steeper, so a one-layer pit's
  diagonal neighbours stay walkable. `Terrain::steep(tile)` says whether
  any of a tile's edges falls more than a layer (for M6.2's dig rules and
  M6.6's drawing). A terrain with no pits, mine, surfaces or drops is
  level and skips all of it, so the Sacred Tree (now sloped, six layers)
  behaves exactly as before; only its world's terrain hash moved, since
  the settings hash only when set. `nav::path`/`path_avoiding` (threat
  waves, chasers, sprites, haulers) keep to `walkable` in sloped scenes;
  `path_with` callers pass their own check as before (the avatar's
  jetpack still ignores slopes). Not slope-checked: the articulated
  turning circle of towed haulers (none in the Sacred Tree). Save format
  34; input log, net protocol and commands unchanged; the sim golden file
  is unchanged. App: the Dig Lab HUD and ore probe read
  `terrain.max_layers`; `digging.rs`'s pit walls still draw
  `MAX_LAYERS` bands (M6.6). Cost: a sloped ground sample reads 9 tile
  heights; with M6.3's generated 256 m map, watch nav cost (every edge
  now runs `walkable`). Tested: a pit's flat floor and sloping
  neighbours, a tile between neighbours two layers apart is steep (and
  the body's edge counts), a route around a deep pit and in by a
  one-layer ramp, walking out of a one-layer pit and digging six deep in
  a sloped scene, the Dig Lab keeping steps and three layers, and a save
  round trip of a sloped six-layer mine.

### Rotating the town camera

- **Status:** done (branch `kestrel-sacred-tree-ballista`)
- **Why:** the camera looks from +x, +z at a fixed angle, so the tree's
  canopy hides what stands behind it (a ballista north of the tree).
- **What to do:** turn the orthographic camera in 90° steps in the town,
  keeping input screen-relative; view only.
- **Where:** `kestrel/app/src/input/town.rs` (`ViewTurn`),
  `input/gameplay.rs` (`camera_relative`, `view_yaw`), `input/mod.rs`,
  `effects.rs` (`camera_feedback`).
- **Found in:** user request after M5.
- **Done:** in a town, Z and controller D-pad left orbit the camera a
  quarter turn counter-clockwise around its target (seen from above), C
  and D-pad right clockwise, smoothstepped over 0.25 s at the same zoom
  and height (`input::town::ViewTurn`, kept in the dispatcher's town
  state, so every world starts at the default angle; never saved; no sim,
  command or format change). There is no reduced-motion setting to honor
  yet. Z/C pick no weapon in a town, and the D-pad's left and right cycle
  no targets there (the town has one target, the sim's); elsewhere both
  keep what they did. `camera_relative` takes the live yaw (eased
  mid-turn), so WASD, the stick, the right stick moving the ballista
  reticle and the job menu's ring stay screen-relative; mouse picking, the
  reticle under the mouse, labels, prompts and cards project through the
  live camera, the ghost stands in front of the elder, and the canopy's
  fade tests the segment from the live camera, so it fades for whatever it
  hides from the new angle. Lights and shadows stay world-fixed. The help
  lines end in "Z/C ROTATE". The control protocol has `camera_rotate
  {turns}`, and the demo pauses the sim, turns half way round to show the
  ballista behind the tree, and turns back. Tested: Z/C and the D-pad turn
  in a town and pick weapons and targets elsewhere, `camera_relative`
  matches the turned camera's up and right at each quarter turn and eases
  mid-turn, W walks up the turned screen in the Sacred Tree, and the
  canopy hides the other side when turned half way round. Cosmetic, left
  as is: pines at the clearing's edge can hide things from any angle, as
  they could before, and only the tree's canopy fades.

### M5.6 Waves on the map, soak and tuning

- **Status:** done (branch `kestrel-sacred-tree-ballista`)
- **Why:** the defence has to be playable on the Sacred Tree map.
- **What to do:**
  - Add a calm start, then ghoul waves on a timer, in
    `sacred_tree_supply.ron`.
  - Extend the soak test with waves, with `validate` every tick.
  - Update the demo and capture screenshots.
  - Tune the numbers so a first play isn't lost early.
- **Where:** `kestrel/app/assets/levels/sacred_tree*.ron`,
  `kestrel/sim/src/settlement/soak_tests.rs`, `demos/sacred_tree.json`.
- **Found in:** ballista design.
- **Done:** waves: `sacred_tree_supply.ron` has a calm start of 210 s
  (3.5 min), then a wave every 90 s of 3, 4, 5, 7, 8 and 8
  `ghoul_sacred_tree`s, the last at 660 s. Why 3.5 min: the happy path
  (Stone Gatherer, a second Wood Gatherer, possession and Burst) takes
  175 s of game time when scripted, and a first player also needs a
  ballista (4 wood, 2 stone) and stone in it. A scenario holds at most 8
  waves (`Scenario::validate`), so each entry is a lane sending one ghoul a
  wave (`every: 90`) from its `first` on: three north lanes 2.5 s apart
  from the first wave, a fourth from the second, a west lane from the
  third, a fifth north and an east lane from the fourth and a second east
  lane from the fifth. North comes from (0, -8.5) to (0, -1.8); west from
  (-8.5, -2.5) and east from (8.5, -2.5) to (∓1.8, -0.6): all outside the
  trunk's disc. Routes: a blueprint going down now replans every threat on
  its way to the tree from where it stands, and resets the routes of waves
  still sending (`Settlement::replan_routes`, `chase.rs`, called from
  `place_blueprints`), so ghouls walk round a new site instead of through
  it; skirting a building at the route's 0.2 m clearance is within their
  0.9 m reach, so they still attack it in passing (the M5.5 test "breaks a
  building that blocks its way" became
  `a_ghoul_walks_round_a_building_put_on_its_way_and_breaks_it_in_passing`).
  `give_up`/`replan_route` are shared with `threat_goals`. Chasers of a
  leaving player: `Settlement::leave` takes the terrain; a leaver's chasers
  give up and plan their way to the tree, and `remap_agents` shifts the
  other `Goal::Chase(Quarry::Agent)`s
  (`combat_tests::chasers_of_a_leaving_player_go_back_and_the_others_follow_their_quarry`;
  the task "Chasers of a player who leaves" is done). Tuning, measured by
  `waves_tests` on the real waves (a ballista manned by a sprite, stone
  topped up from the tree):

  | Number | Before | After | Why |
  |---|---|---|---|
  | Waves | none | 210 s calm, then 3/4/5/7/8/8 every 90 s | Ignored, the tree falls in the fifth wave (581 s); one ballista reloaded once mid-wave keeps 15/20; two stocked between waves stop all 35. |
  | Fireball cooldown | 20 s | 25 s | About one throw a wave, two in a long one. |
  | Fireball impact | 2 within 0.8 m | 1 within 1.0 m | 2 killed any ghoul (2 health) outright: a wave-clear button. |
  | Fire | 1.5 m for 6 s | 1.5 m for 4 s | The column walked into a 6 s fire one after another. |
  | Burn | 1 every 1 s, linger 3 s | 1 every 1.5 s, linger 1 s | A ghoul crossing the fire took 3–5 hits. Now a fireball at a column's head kills about 2 of 6 and hurts one (was 4 of 6). |
  | Building health | 10 | 10 (kept) | An empty ballista on a lane is broken in a big wave; a stocked one isn't. Losing one is the stake. |
  | Ghoul detect / leash | 3 m / 5 m | kept | The soak sees 4 sprite deaths in 12 min, all back after 15 s; buildings shelter. |
  | Sprite health / return | 3 / 15 s | kept | As above. |
  | Ballista ammo / range | 10 / 3.8 m | kept | Ammo is the binding limit (range 4.5 changed nothing): a ghoul takes 2 bolts, so a 10-stone store covers 5 and the bigger waves need a reload mid-wave or a second ballista. That is the "defence matters, not every second" lever, and it ties defence to stone. |

  Soak: `soak_tests::defending_against_the_waves_strands_no_sprite` plays
  all twelve minutes: a ballista, a Stone Gatherer on the east lane and a
  Wood Gatherer by the north one go up in the calm start, then cycles
  reassign the sprites, stock and man the ballista, possess it to shoot
  and throw fireballs while ghouls are out, Burst a gatherer otherwise,
  and rebuild what breaks (84 cycles: 6 fireballs, 13 fire kills of 27,
  8 buildings broken, 4 sprite deaths and returns, tree 12/20). Every tick
  validates, checks balances, no sprite waiting past a retry with a slot
  free, and (both soaks) none dead past its return. The happy path asserts
  it ends in the calm start (175 s). Tests that send their own enemies
  clear the map's waves (`scenario.waves.clear()`). App: the possessed
  building's card says "E  LEAVE" as its last line instead of a prompt
  over the slot that hid its fireball line (`leave_prompt`); the status
  line adds "NEXT WAVE 1:20", or "GHOULS 3" while any are out. Demo
  `demos/sacred_tree.json`: stocks the tree, builds and loads a ballista
  at 2× speed, waits at 4× to the first wave, Fern mans it and shoots, the
  elder possesses it (Fern steps out), throws a fireball at the column and
  fires four bolts, leaves, and Fern takes over. No format changed (no
  save, input or net version bump); golden unchanged.
- **Left open:** the "THE TREE HAS FALLEN" overlay isn't in the demo (it
  needs 20 breaches; its test covers it). Wave lanes are a workaround for
  the 8-wave limit; if waves grow, a wave with groups (or a higher limit)
  would read better in content. Repair (todo "Repairing buildings") and a
  faster possessed ballista stay for the playtest.

### M5.5 Sprite health, building health and the tree

- **Status:** done (branch `kestrel-sacred-tree-ballista`)
- **Why:** enemies need something to hurt, and defence needs a stake (B6,
  B7, B8).
- **What to do:**
  - **Sprites:** sprites get health. A dead sprite returns at the tree
    after about 15 s with its job, and occupants of a building can't be
    hit.
  - **Buildings and sites:** they get health, about 10. An enemy with no
    elder or sprite to chase attacks a building that blocks it or is in
    reach.
  - **A building at 0** becomes a site again with its parts lost, puts its
    occupant out, and spills its output and ammo onto a ground pile.
  - **The tree:** it has about 20 health and takes breaches. At 0, show
    "The tree has fallen" and restart.
- **Where:** `kestrel/sim/src/settlement/sprites.rs`, `buildings.rs`,
  `combat.rs`, `piles.rs`, the app.
- **Found in:** ballista design.
- **Done:** content: a Sprite character may have a `sprite` section
  (`content::character::SpriteDef`: health 3, respawn 900 frames, 15 s),
  left out of the encoding like `chase` and added to `rules_hash` when any
  has one; a building kind has `health` (default 10, written out in the
  supply); the Sacred Tree supply has `base_health: 20` (the tree) and
  `player_health: 6` (the elder, decided: enemies target the elder, who
  dies and respawns as on the forest map, Burst and fireball cooldowns
  kept). Sim: a swing names its target, `combat::SwingTarget` (`Agent` or
  `Building`) on `Windup`, `StartSwing` and `SwingLands` (an agent hashes
  as the bare index it did; agents sort before buildings in
  `start_swings`). `Quarry::Building(BuildingIx)`: in `read_chase`, a
  threat on its way to the tree with no elder or sprite to spot attacks
  the nearest breakable building (`Settlement::breakable`: built, or a
  site with a part in or materials delivered) whose edge is within its
  attack's reach, or within detect and in the way of its route's next leg
  (`spot_building`, `segment_disc` with its radius); one attacking a
  building looks for an elder or sprite on its look ticks and turns on
  one (buildings come second); it gives up when the building has nothing
  left to lose or is past the leash. It steers to stop its usual distance
  short of the building's edge and swings at the edge gap
  (`building_gap`). Sprites: `Body` health from the character; a hit in
  `swing_lands` takes the damage, no push (a sprite walks routes and has
  no velocity); at 0 `hurt_sprite` releases its slot (a claim; one inside
  is never hit), drops its plan and route and sets
  `SpriteState::respawn_at`; a dead sprite doesn't think, walk, claim, block
  steering or show up as an interact target, and `sprite_housekeeping`
  puts it back at home (its start spot beside the tree), healed, with its
  job and an empty plan, so it replans (`Event::SpriteReturned`). Shelter:
  `swingable` and `threats_in_reach` skip an avatar seated in a building.
  Buildings: `BuildingState::health`; at 0 `break_building` lands units
  flying to or from it, ends carry jobs on its output or site
  (`drop_carry`/`reserve_stock`), puts a sprite inside out at
  `eject_spot` (it replans) and drops a claim, unseats a possessing elder
  at `leave_spot` with the reticle back to a unit facing, then spills its
  output (ammo too) and a site's delivered materials onto new ground piles
  beside it, one per item (`Settlement::spill`, chosen over counting them
  destroyed: conservation holds with no new account), and resets it to a
  site at part 0 with empty materials, a default turret and full health;
  the footprint stays an obstacle (`Event::BuildingStruck`,
  `BuildingWindup`, `BuildingBroken`). The tree: `Settlement::tree_fallen`
  (a town at base health 0); the breach that takes the last pushes
  `Event::TreeFell` once and the waves stop spawning. `validate` checks
  building health, that a dead sprite holds no slot, and a chaser's
  building or windup target exists. Save format 33; input and net
  unchanged (no command). `Threat` shrinks to 184 bytes (the windup's
  target packs smaller). Golden: behaviour lines unchanged; the arena's
  last state hash regenerated in its own commit (a windup encodes its
  target's variant). App: the status line reads "SACRED TREE 20/20
  ELDER 6/6 ..."; a damaged building's card adds "HP 7/10" (its detail
  "Health: 7/10"); a hurt sprite's tag "ASH 2/3"; a dead sprite falls over
  and fades in 1.5 s, its flame out and its tag "BACK IN 12 s"
  (`SpriteActivity::Dead`, `SpriteView::dead`); a blow on a building
  strikes sparks (Stone), a break bursts it (Pierce) and says "WOOD
  GATHERER broken: it is a site again"; a returning sprite plays
  Dispense. `fallen.rs`: once the tree falls an overlay says "THE TREE HAS
  FALLEN" and takes input; R, Enter or confirm restarts the scene from
  scratch (`Flow::restart`, single player) or leaves to the menu when
  networked. Tests: `health_tests` (a sprite dies after three hits, lies
  still, isn't swung at or chased, is back at home after exactly 15 s with
  its job and takes the slot that came free; a ghoul breaks a staffed
  gatherer without hitting its sprite, which is put out and waits, the
  output on a ground pile; a ghoul breaks a possessed, stocked ballista
  without hitting the elder, who is put out at the leave spot with the
  reticle a facing, the ammo on a ground pile; a building put down on a
  ghoul's way is attacked from beyond reach, then left as a bare site and
  walked round to a breach; a broken site spills its materials one pile
  per item; the elder dies after six hits and respawns at home; the tree
  at 0 falls once and no more enemies come; balances and `validate` every
  tick), `chase_tests` updated (a hit on a sprite and on the elder takes
  1; a ghoul passing occupants attacks the gatherer instead and never
  chases or swings at them), the content tests
  `a_sprite_section_parses_and_only_sprites_have_one` and the kind's
  health, `fallen::tests::a_fallen_tree_shows_its_overlay_and_the_scene_restarts`.
  Demo script not changed (no waves on the map yet); a scratch copy with a
  temporary wave past the Wood Gatherer rendered the screenshots.
- **From M5.5 for M5.6:** a wave's route is planned once, when its first
  enemy spawns, and threats walk it through anything built later: a
  building on it is attacked (as it blocks), but a bare site left on it is
  walked through (it has nothing to lose). Replanning wave routes when
  `terrain.revision` changes may be worth it once waves run. Ghouls passing
  within 0.9 m of a building break it (about 15 s for 10 health at one
  ghoul's 1.45 s swing); with the WEST way past the Wood Gatherer a lone
  ghoul broke it in the scratch demo, so wave paths and building health
  need tuning together. The elder now has 6 health and is chased like any
  sprite; the soak should keep them alive or let them respawn. The tree
  falling stops the waves (`tree_fallen`); the soak should check
  `TreeFell` isn't reached early.
- **Left open:** a leaving player's chasers keep `Goal::Chase(Quarry::Agent)`
  pointing at the removed index (`players::leave` remaps windups but not
  goals; from M5.4). Single player only for now: the task "Chasers of a
  player who leaves".

### M5.4 Enemies chase, then return

- **Status:** done (branch `kestrel-sacred-tree-ballista`)
- **Why:** enemies should turn on whoever comes close, then go back to the
  tree (B5).
- **What to do:**
  - About every 0.5 s, an enemy picks the nearest visible elder or sprite
    within about 3 m and steers straight at it, using the existing swing.
  - Past a leash of about 5 m, when the target dies, or when it leaves
    sight, the enemy plans a fresh route to the tree from where it stands.
  - Other scenes' enemies must behave the same, so their golden behaviour
    lines stay unchanged. Gate the chase by character or by scenario.
- **Where:** `kestrel/sim/src/settlement/combat.rs`, `kestrel/content/`
  (the character's detect and leash).
- **Found in:** ballista design.
- **Done:** gated by character: a FollowRoute character with an `attack`
  may have `chase` (`content::character::ChaseDef`: detect 3 m, leash
  5 m, look 30 frames), left out of the encoding like `attack` and added
  to `rules_hash` when any has one. Only the new `ghoul_sacred_tree`
  (base `ghoul_forest`) has one, so the forest and arena ghouls keep their
  routes. The sim side is `settlement::chase`, in the sprites' shape:
  `Threat::goal` is the goal (`Goal::Tree`, the plan being `route`, or
  `Goal::Chase(Quarry::Agent(a))`, the plan a straight line at the
  quarry), `read_chase` invalidates and decides, `route_home` replans.
  Stage 3: a threat on its way to the tree looks on its look ticks
  (`(tick + id) % look == 0`, spread over threats) for the nearest
  chaseable agent within the detect radius and in sight, lowest index on
  a tie; a chaser checks its quarry every tick (alive, not seated or
  inside a building, within the leash, in sight). Either sends a
  `ThreatGoal` (a new late queue). Stage 5 (`threat_goals`, after the
  burns) applies it; giving up plans `nav::path` from where the threat
  stands to its wave's target (the route's last point without a wave),
  keeping the old route if there is no way. Stage 2 stays parallel-safe:
  `follow_route` steers a chaser at where its quarry stood at the end of
  last tick (agents move after threats in stage 2), straight, stopping at
  4/5 of its reach, each step only if the ground is clear at the route's
  0.2 m clearance. "In sight" is a thin (0.02 m) line to the quarry plus a
  clear 0.2 m walk to where the chaser would stop, so a sprite at a slot
  spot is seen and a seen quarry is walkable. The route waits while
  chasing (never empty, so no breach mid-chase). A chaser swings at its
  quarry with its usual attack (`swing_at_quarry`; players keep
  `threats_in_reach`, the same swing twice is one in `start_swings`). A
  swing that lands on a sprite, or on a player where player combat is
  off (the Sacred Tree has no `player_health`), is a `ThreatSwing` with
  `hit` and no damage, no push. The behaviour digest hashes goals and
  their routes only while any threat chases (behaviour lines unchanged;
  state hashes regenerated in their own commit: the new character and
  the new field). Save format 32; input and net unchanged (no command).
  `Threat` grows to 192 bytes. `validate` checks chasers
  (`validate_chases`). App: the threat view has `chasing` (the agent's
  id) in snapshots; nothing new is drawn (a chaser faces where it walks).
  Tests: `chase_tests` (leaves its route for a sprite on a look tick and
  swings at it for no damage; not behind a wall; not a sprite inside the
  gatherer nor the elder possessing it; gives up past the leash on the
  elder, replans from where it stands and breaches; gives up at once out
  of sight; a forest ghoul keeps its route; two runs hash the same), the
  content test `chases_parse_and_only_the_sacred_tree_ghoul_has_one`.
  No waves on the map yet, so no demo or screenshot change.
- **From M5.4 for M5.5:** hurting a sprite goes in `swing_lands`
  (`combat.rs`), in the branch for a sprite or a player with combat off;
  `Quarry` takes a `Building(BuildingIx)` variant for B8, with
  `chaseable`/`keeps`/`spot` in `chase.rs` the places to extend (a
  building is no agent, so `swing_at_quarry`, `Windup::agent` and
  `StartSwing::agent` need a target type then). `Body::alive` already
  makes a dead sprite no quarry once sprites have `max_health`. The
  elder: the Sacred Tree has no `player_health`, so a chaser's swings at
  it do no harm but still play as hits in the app (blood, shake); decide
  whether the elder gets health or is never chased. A seated elder put
  out of a broken building becomes chaseable again on the next look.
- **From M5.4 for M5.6:** waves on the map must name
  `ghoul_sacred_tree` and target a point outside the trunk's disc (a
  target inside an obstacle plans an empty route, and the wave never
  spawns). Detect 3 m and leash 5 m are first guesses; a chaser stops at
  0.72 m. The soak should walk sprites and the elder through a wave's way.

### M5.3 The fireball and burning ground

- **Status:** done (branch `kestrel-sacred-tree-ballista`)
- **Why:** the possessed ballista's special ability (B4).
- **What to do:**
  - R (or RB) throws a fireball at the reticle, with a cooldown on the
    elder (about 20 s). Choose a new appended command or `Burst` by
    building kind.
  - The fireball deals impact damage and leaves a fire patch (a new table:
    about 1.5 m, about 6 s).
  - Threats in a patch get a refreshed burn: 1 damage per second, until
    about 3 s after leaving.
  - Draw the patches and burning enemies, and add a control verb.
- **Where:** `kestrel/sim/src/settlement/combat.rs` (or a new module),
  `avatar.rs`, the app.
- **Found in:** ballista design.
- **Done:** no new command: the Burst key is the fireball in a ballista.
  `burst_plan` (`buildings.rs`) checks the possessed kind first: a
  ballista whose `Defence::fireball` is set returns
  `Ability::Fireball { to }` (the reticle, `AvatarState::aim` cut to
  range, half the range ahead before any aim), a gatherer
  `Ability::Burst { units }`; `burst_check` returns that `Ability` now.
  The fireball has its own cooldown, `AvatarState::fireball_ready`, kept
  across leaving and respawning like `burst_ready` and apart from it, so
  hopping between a ballista and a gatherer neither resets nor blocks
  either (`BurstRefusal::FireballCooling`, appended: "Fireball ready in
  12 s"). It costs no stone. Content: `scenario::Fireball` (cooldown 20 s,
  speed 6 m/s, damage 2 within 0.8 m, fire radius 1.5 m for 6 s, burn 1
  every 1 s, linger 3 s), resolved into `FireballDef`/`BurnDef`
  (`fire.rs`). Stage 5 throws it (`throw_fireball`: `Settlement::fireballs`,
  `Event::FireballThrown`); after the shots, `Settlement::burn` lands
  fireballs (impact on every threat within the radius, then a
  `FirePatch` in `Settlement::fires`, `Event::FireballLanded`), drops
  fires on their `expires` tick, refreshes the burn (`Threat::burn`:
  `until` the linger after the last tick in any fire, `next` hit a period
  after catching; never stacked), and hits each burning threat once a
  period while `next <= until`. Fire only reads threats, so elders and
  sprites are never hurt. Hits are `Event::Hit` with `Impact::Fire`
  (appended); kills count in `combat.kills`. The behaviour digest hashes
  fireballs, fires and burns only when any exist (golden behaviour lines
  unchanged; state hashes regenerated in their own commit: the new empty
  fields add bytes). Save format 31; input and net unchanged (no
  command). `Threat` grows to 184 bytes (`threat_size_is_pinned`). App:
  R (keyboard) and RB (controller, where RT fires) send `Burst` in a
  ballista; the card and HUD read "FIREBALL READY (R)" / "FIREBALL 12 s";
  the help line "MOUSE AIM / LMB FIRE / R FIREBALL / E OR G LEAVE". Drawn
  as glows (`settlement::fire_glows`): the fireball arcs with a trail,
  a fire is 11 flickering flames over its disc that die down in its last
  second, with a flickering ring (`input::hud`); a burning enemy carries
  two flames; the landing bursts in sparks (`effects::deflection`). Fire
  hits don't bleed; a burn tick is silent, a fire kill sounds as any; the
  throw plays `Toss`, the landing `LaserImpact`. Control verb
  `settlement_fireball { x, y }`; the snapshot's `settlement.burst` has
  `fireball_cooldown` and `fireball_to`, the simulation `fires`,
  `fireballs` and a threat's `burning`. Tests: `fire_tests` (throw at the
  reticle, own cooldown and its refusals, kept over a gatherer Burst and a
  respawn; impact damage and a kill; patch radius and expiry; a burn's
  hits a period apart until the linger; two fires refresh, not stack, and
  leaving ends it after the linger; fire spares the elder and sprites;
  two runs match and a replay predicts no event),
  `input::tests::the_fire_keys_work_only_while_possessing_a_ballista` (RB
  is the Burst key there), `control::tests::the_elder_throws_a_fireball_from_a_possessed_ballista`.
  Demo script not changed (the map has no waves yet; a scratch copy with a
  temporary wave rendered the screenshots).
- **From M5.3 for M5.4:** a threat chasing an elder or sprite keeps its
  `burn`; chasing changes nothing in `Settlement::burn`, which reads only
  positions. A fire is a good place to lure a chaser.
- **From M5.3 for M5.5:** fire stays enemies-only once sprites and
  buildings get health: `fire_hit` reads `threats` only. A ballista broken
  back into a site has no fireball (`burst_plan` needs a built building);
  fireballs in flight from it still land (they keep their `FireballDef`).
- **From M5.3 for M5.6:** the soak with waves should throw fireballs too
  (a Burst in a possessed ballista every 20 s), so burns and kills run
  under `validate`. Fireball numbers (20 s, 2 + 1/s, 1.5 m, 6 s) are first
  guesses; in the scratch demo a fireball ahead of a 6-ghoul column killed
  4 of them. The card's third line ("FIREBALL 17 s") sits under the
  "E LEAVE" prompt, as M5.2's left-open note says.

### M5.2 Aiming and firing while possessing

- **Status:** done (branch `kestrel-sacred-tree-ballista`)
- **Why:** the elder's reward for possessing a ballista is aiming (B3).
- **What to do:**
  - While seated in a ballista, the avatar's aim point is the reticle,
    clamped to range, drawn in the app.
  - `Attack` fires a bolt there, at most once per period.
  - Left mouse fires; on a controller, the right stick aims and RT fires.
  - The possessed ballista stops its own targeting.
- **Where:** `kestrel/sim/src/settlement/avatar.rs`, `buildings.rs`,
  `kestrel/app/src/input/town`.
- **Found in:** ballista design.
- **Done:** while the elder sits in a ballista, `AimAvatar` keeps the
  reticle in `AvatarState::aim` as an offset from the ballista, cut to its
  range (`BuildingKind::reticle`); leaving turns it back into a unit
  facing. `Attack` there pushes an `ElderFire` action message (no new
  command), and stage 5 (`Settlement::elder_fire`, after Burst) shoots
  through `launch_shot` if `fire_plan` allows it: the elder still in the
  slot, stone in the store, the period over. Its bolt has no `target`
  (`launch_shot` takes an `Option` now) and hits whatever it meets on the
  way. Spam within the period and a second `Attack` on one tick do
  nothing; a seated elder never swings. `ready_to_shoot` needs
  `Occupant::Sprite`, so a possessed ballista picks no targets.
  `Settlement::fire_check` (`FireRefusal`: `NotInBallista`, `NoAmmo`,
  `Reloading`) is the app's check. Prediction needed nothing: the replay
  runs the same seated arm (tested). App: `input::town::aim_reticle`
  follows the mouse's ground point, or the right stick moves it (4 m/s),
  and sends `AimAvatar` when it moves 5 cm; `input::town::fire` sends an
  `Attack` at the reticle whenever `fire_check` passes, one per confirmed
  tick, while left mouse or RT is held (`Gameplay::fire_held`), so holding
  repeats at the period; a fresh press on an empty ballista says "No
  ammo: bring stone". RT fires only in a ballista (it still Bursts in a
  gatherer); R still Bursts and a ballista refuses it ("This building has
  no Burst"). `lockstep::attack_input` stands down while seated in a
  building. The HUD draws the range ring, the reticle and a faint line of
  fire; the turret turns to the elder's reticle (`legacy`); the help line
  reads "MOUSE AIM / LMB FIRE / E OR G LEAVE". Control verb
  `settlement_fire { x, y }` (meters from the ballista). Tests:
  `ballista_tests` (possessed doesn't auto-fire, reticle clamped, once a
  period, bolt hits on the way, no ammo no shot, replay matches),
  `input::tests::the_fire_keys_work_only_while_possessing_a_ballista`,
  `control::tests::the_elder_fires_a_possessed_ballista_at_the_reticle`
  (builds, loads, possesses and fires one). No format changed: the
  reticle lives in an existing field and `ElderFire` hashes only when
  sent; golden file unchanged. Demo script not changed (it builds no
  ballista; a scratch copy with one rendered the screenshot).
- **From M5.2 for M5.3:** R/RB are free in a ballista: R reaches
  `Edge::Burst` → `town::burst` → `NoBurst` today, RB is the lift. The
  fireball can aim at `AvatarState::aim` (the reticle offset) from the
  seated arm in `avatar::act`, as `Attack` does, and reuse
  `possessing_ballista` for its keys and the help line.
- **From M5.2 for M5.5:** a seated elder put out of a broken ballista
  should turn its aim back into a facing, as leaving does
  (`avatar::act`, the `Seat::Building` arm); `fire_plan` already refuses
  a site.
- **Left open:** the "E LEAVE" prompt sits over the possessed card's
  last line (seen in the M5.2 screenshot; it was so before). The reticle
  is drawn on the flat ground plane (y 0), fine on this map.

### M5.1 The ballista building and its ammo

- **Status:** done (branch `kestrel-sacred-tree-ballista`)
- **Why:** a defence building that fires only while occupied
  ([ballista design](../SACRED_TREE_BALLISTA.md), B1, B2).
- **What to do:**
  - Extract target picking and bolt launching from `aim_defender` and
    `fire` into shared helpers. The Woodland Ballista keeps its behaviour:
    the golden file's behaviour lines must not change.
  - Add a ballista building kind with a `defence` section in content, and
    an `Only(stone)` ammo store filled by interact like a site delivery.
  - A built, occupied ballista with ammo fires at the nearest enemy
    through the helpers, one stone per bolt.
  - Add a "Man Ballista" sprite job.
  - The card shows ammo, and "NO AMMO" when empty.
  - Put ammo in `balances` and `validate`.
  - Add it to the build menu and the Sacred Tree supply.
- **Where:** `kestrel/sim/src/settlement/combat.rs`, `buildings.rs`,
  `sprites.rs`, `interact.rs`, `kestrel/content/src/scenario.rs`,
  `kestrel/app/assets/levels/sacred_tree_supply.ron`, the app's town
  drawing and job menu.
- **Found in:** ballista design.
- **Done:** the target picking and shot launching are
  `Settlement::pick_target` and `Settlement::launch_shot` (`combat.rs`),
  which `aim_defender`/`fire` and the building ballista both call (golden
  file unchanged, behaviour lines and state hashes alike). A building kind
  has either a `product` or a `defence` (`content::scenario::Defence`:
  ammo, ammo cap, range, period, damage, projectile speed, muzzle height,
  projectile kind); the supply's `ballista` (BALLISTA, `turret` skin,
  radius 0.7) is the Woodland Ballista's numbers with stone, cap 10, built
  from foundation 2 wood and mechanism 2 wood + 2 stone. Its ammo store
  is the building's `output` (`Kinds::Only(stone)`, capacity the ammo
  cap; `BuildingKind::store_capacity`/`store_kinds`), so balances,
  `validate`, transfers and saving needed nothing new: interact while
  carrying stone delivers as much as fits (`Interaction::Bundle`,
  Deposit, "LOAD 5 STONE"), a pickup is `DepositOnly`, wood is
  `WrongPile`, and a full store is no target. A built ballista with any
  occupant, stone in the store and its period over (`ready_to_shoot`,
  `BuildingState::turret`) aims in stage 3 (`aim_buildings`, a
  `BuildingFire` message, a new late queue) and shoots in stage 5
  (`building_fire`), one stone spent a shot, `Event::BuildingShot` (the
  app plays it as a defender's `Shot`). A stock shot ignores the obstacle
  centred on its launch point (`fly`), so it leaves its own footprint.
  Possessed, it shoots as staffed (the simpler choice). A ballista has no
  Burst (`BurstRefusal::NoBurst`, appended). App: the turret turns and
  recoils (`legacy::Building::turret`), the card says "MANNED / ASH",
  "AMMO 7/10 STONE" or "NO AMMO" (`BuildingStatus::NoAmmo`), the job menu
  has "MAN BALLISTA". Tests: `ballista_tests` (unmanned or empty never
  fires, nearest first at its period one stone a shot, out of ammo stops
  until stocked, delivery caps), `save::tests::a_town_saved_while_a_ballista_shoots_continues`
  (save format 30). No new command, target or control verb: the existing
  `settlement_interact`, `settlement_sprite_job`/`settlement_job_menu` and
  `settlement_build` drive it; the state's `buildings[i].output` is its
  ammo and `turret` its aim.
- **From M5.1 for M5.2:** "the possessed ballista stops its own
  targeting" is one line in `ready_to_shoot` (`buildings.rs`): require
  `Occupant::Sprite`, then fire the elder's `Attack` through
  `launch_shot` with the same `Launcher` and `turret.ready_at`.
- **From M5.1 for M5.3:** `burst_plan` refuses a ballista with `NoBurst`;
  the fireball can take that branch by building kind.
- **From M5.1 for M5.5/B8:** a broken ballista's ammo is its `output`, so
  spilling the output spills the ammo; reset `turret` when it becomes a
  site (`validate_buildings` only checks a gatherer's turret is default).
- **From M5.1 for M5.6:** no wave spawns on the map yet; tests push a
  staggered `Threat` by hand (`ballista_tests::threat`), the save test
  adds a scenario wave. Threats still read `waves[t.wave]` in the app's
  view, so a hand-pushed threat in a scene with no waves would not draw.

### M4.1 Happy path and stability

- **Status:** done (branch `kestrel-sacred-tree`)
- **Why:** the 10-step happy path must play from a fresh start, and
  repeated cycles must never strand a sprite (A6, A8).
- **What to do:** the happy path as a headless control script, the
  stability soak and the bootstrapping test (handoff, "Tests"); fix what
  they find, then tune rates.
- **Where:** `kestrel/sim/src/settlement/` tests, `kestrel/headless/`.
- **From M1.3:** `sprites_tests::no_sprite_keeps_waiting_while_a_matching_slot_is_free`
  already reassigns sprites over two gatherers for 90 s; the soak can grow
  from it with possession and building.
- **From M2.3:** the bootstrapping order already runs in the sim
  (`construction_tests::both_kinds_build_from_an_empty_tree`, through
  `Interact` only); the headless happy path can drive it with
  `settlement_build`, `settlement_place_blueprint` and `settlement_interact`
  (the control test `the_elder_builds_a_stone_gatherer_that_a_sprite_works`
  is a start). Placing checks that the new building's own pile and slot
  spot can be reached, not that it leaves the others reachable: a ring of
  buildings could wall one off. A new footprint can also land on a route a
  sprite is walking; the soak should show whether either needs a rule.
- **From M2.4:** `settlement_drop` drops the bundle and waits for it to
  land; the happy path's "drop before possessing" step can use it, and
  interact picks a ground pile back up (control test
  `the_elder_drops_wood_and_picks_it_back_up`). A town holds at most
  `MAX_PILES` (32) ground piles; the soak can drop and pick up in its
  cycles, with `validate` checking piles' reservations.
- **From M3.1:** `settlement_board` (walks there first) and
  `settlement_leave` drive possession; `settlement.seat` reads it.
  `seat_tests::repeated_board_and_leave_cycles_stay_valid` is a short
  possess/leave soak over two gatherers. A staffed building is boarded with
  G (or the verb): E there reaches the sprite inside, whose job menu wins
  the tie at the slot spot. The elder's leaving spot and a put-out sprite's
  spot ignore bodies (the action's view reads no agents), so a crowd beside
  a building can overlap them for a moment; the soak should show whether
  that strands anyone.
- **From M3.2:** `settlement_burst` Bursts the possessed building (R or RT
  in the app) and fails with the sim's reason ("Burst ready in 12 s",
  "Output full"); `settlement.burst` has the `cooldown` left in seconds and
  whether one would go off (`ready`), so the happy path's Burst step can
  check the output gained its two units, and the soak can Burst in its
  possess cycles (`burst_tests` cover a cooldown kept across leaving,
  another building and a respawn). For tuning: a Burst adds 2 against the
  4x multiplier's 24 units in the 30 s cooldown, and a Burst into a nearly
  full output adds only what fits, still spending the cooldown; both are
  content (`burst.units`, `burst.cooldown`, `possessed_cap`).
- **Found in:** Sacred Tree handoff.
- **Done:** the happy path as a control script
  (`control::tests::the_happy_path_plays_from_a_fresh_start`), one-to-one
  with the requirements' section 5 (A6), with no cheat or speed-up
  commands (about four minutes of play at 20 frames a second, 2.3 s of
  test time); the soak
  (`soak_tests::possess_leave_reassign_and_build_cycles_strand_no_sprite`,
  16 cycles, four buildings put up, `validate` and the no-stranding check on
  every tick); bootstrapping was already
  `construction_tests::both_kinds_build_from_an_empty_tree`. E possesses a
  staffed building ("POSSESS, ASH STEPS OUT"); the build key refuses while
  possessing ("Leave the building first (E)"); sprite tags decluttered;
  Burst tuned to 5 units (one full bundle).
- **Left open:** the soak strands nobody, so no rule was added for a
  footprint walling another building off, landing on a sprite's route, or
  a leaving spot overlapping a crowd; it doesn't build a deliberate ring,
  so walling off stays untested. A Burst into a nearly full output still
  spends the whole cooldown for what fits (content: `possessed_cap`).
  Rates other than Burst are untouched until the playtest says otherwise.

### M3.2 Burst

- **Status:** done (branch `kestrel-sacred-tree`)
- **Why:** the stand-in for building-specific elder abilities (D3).
- **From M1.4:** a Burst's units arrive in the output as any unit does;
  `Event::BuildingProduced` flares the lamp and plays the dispenser cue, so
  a Burst event can reuse both (`settlement::tick`, `town`).
- **What to do:** `Burst { agent }`, its cooldown on the elder, its control
  verb and its ready indicator.
- **Where:** `kestrel/sim/src/settlement/avatar.rs`, `buildings.rs`,
  `kestrel/app/`.
- **From M3.1:** the possessed building is `avatar.seat` (`Seat::Building`).
  A seated elder's actions go through the `Some(Seat::Building(b))` arm at
  the top of `avatar::act`, which lets only Board, a Slot interact, Aim,
  Equip, Release and Respawn through: add `Burst` there (its cooldown on
  `AvatarState` survives leaving; `respawn` resets `AvatarState`, so keep
  the cooldown across it as `attack_ready` is). The units go into the
  output up to `possessed_cap` in stage 5 (a message, as `Install`).
  Bind the key in a town only while seated; G and E are taken.
- **Found in:** Sacred Tree handoff.

### M3.1 Building seats and ejection

- **Status:** done (branch `kestrel-sacred-tree`)
- **Why:** the elder becomes a building, and the sprite it displaces
  replans (D3, A3).
- **What to do:** `driving` becomes `seat`, `Board` takes building slots,
  ejection beside the building, the raised cap and the multiplier.
- **Where:** `kestrel/sim/src/settlement/vehicle.rs`, `avatar.rs`,
  `buildings.rs`.
- **From M1.3:** eject in the boarding handler with
  `Settlement::release_slot(sprite)` (it clears the sprite's `holds` and
  empties the slot), then set the slot to `Occupant::Elder`; the sprite's
  brain finds its claim or seat gone on its next step and replans. A sprite
  on its way in (`Slot::Claimed`) loses its claim the same way. Move the
  ejected sprite off the slot spot, or the elder's own leaving spot and the
  sprite's spot may coincide: a walker whose goal sits under another body
  holds back (`haulers::move_worker`). `validate_buildings` already wants an
  elder's slot to name an avatar; `players::leave` empties a leaving
  player's elder slot and `remap_agents` shifts slot agents.
- **From M1.4:** the view already draws an elder occupant: the building's
  lamp turns violet and its card says "POSSESSED" (`settlement::town`,
  `legacy::Occupant::Elder`). The elder's own rig is still drawn where its
  body stands; hide it, or draw its flame form, while it is seated.
- **From M2.3:** a site is no building to board: `validate_buildings`
  insists a site's slot stays empty, so `Board` and a `Slot` target must
  take built buildings only (as `interact_target` lists a site as `Site`,
  not `Output`). Placing checks bodies, so a footprint never lands on an
  elder; an ejected sprite is put beside its building, clear of others'
  footprints already.
- **From M2.2:** a building slot is a new `InteractTarget` variant,
  appended (`Slot(BuildingIx)`), empty-handed only (D5): `interaction` says
  board, and `Action::Interact` sends the `Board` message for it. Today
  boarding is its own key (G, the left stick), and `interact_target` gives a
  seated agent no target (`avatar.driving`): with `seat`, decide whether
  interact also leaves the seat. A slot spot and the output pile are about
  1.7 m apart, so with the 1.5 m reach the nearer one wins.
- **From M2.4:** dropping exists for the boarding refusal: Q or controller
  B drops the bundle (`CommandKind::Drop`), so the refusal can say "drop
  the bundle first". A slot target goes after `InteractTarget::Pile` (the
  enum is append-only). A seated agent's `Drop` does nothing (`avatar::act`
  returns for it while `driving`); keep that for a building seat. A drop
  keeps its pile clear of slot spots and output piles (`drop_plan`), so a
  ground pile never sits on a seat.
- **Found in:** Sacred Tree handoff.
- **Done:** `AvatarState::driving` is `seat: Option<Seat>` (`Seat::Vehicle`,
  `Seat::Building`; `Settlement::seat`, `driving` kept for vehicles).
  `Board` (G, the left stick) takes the nearest free vehicle (as before) or
  built building whose slot spot is within `INTERACT_REACH`; between the
  two the nearer point wins (vehicle center against slot spot), a tie to
  the vehicle, then the lower building index; command order otherwise
  (players in order, so of two elders on one tick the lower player wins).
  `InteractTarget::Slot(b)` (appended) is offered empty-handed for built
  buildings ("E  POSSESS") and names its building (`Board { building }`);
  seated in a building it is the one target ("E  LEAVE"). Refusals
  (`interact::board_refusal`, `Event::BundleRefused`): `Carrying` ("Drop
  the bundle first (Q)"), `Busy`, `Possessed` ("Another elder is in
  there"), and `OutOfReach` for an interact. Boarding puts a sprite inside
  out at the first clear `PUT_OUT` spot on the slot spot's ring that isn't
  the elder's leaving spot (`buildings::eject_spot`), and one on its way
  loses its claim where it stands; both stop and replan. The elder stands at
  the slot spot, out of the steering footprints; movement and `Drop` do
  nothing. Leaving (boarding again or interacting) is the avatar's own part,
  as a vehicle's: it puts the elder at the first clear `LEAVING` spot
  (`leave_spot`, behind the slot); `seat_housekeeping` empties a slot whose
  elder left, died or respawned, and `players::leave` empties it at once.
  `validate_buildings` checks slot and seat both ways. The view hides a
  possessing elder's rig (local or a peer's, `hide_possessing`); the camera
  follows its body at the slot spot. Help "... / G POSSESS"; verbs
  `settlement_board` (walks there first) and `settlement_leave`; the state's
  `settlement.seat`. Save format 28, input logs 9, net protocol 11. The
  golden file didn't change. Tests: sim `seat_tests` (ejection and return,
  another free building, a claim lost on the way, 4x and cap 20 and back,
  refusals, a site, interact and the tie, seated movement, two elders,
  leaving the game and respawning, 12 board/leave cycles with `validate`
  every tick), save `a_town_saved_while_possessed_continues`, input
  `e_and_g_possess_a_building_in_a_town_and_g_still_drives_elsewhere`,
  control `the_elder_possesses_the_wood_gatherer_and_its_sprite_goes_back_in`.
  The demo ends possessing the Wood Gatherer.

### M2.4 Dropping bundles on ground piles

- **Status:** done (branch `kestrel-sacred-tree`)
- **Why:** the elder must drop a bundle before possessing (requirements
  3.1); piles reuse the pile lab.
- **What to do:** let piles hold any item, not only minerals (handoff,
  "Open questions"), then drop into and pick up from ground piles of wood
  and stone.
- **From M2.1:** `avatar::bundle_job` refuses `InventoryRef::Pile` as "not
  a holder"; add piles there (a `OneKind` pile's item is `kind()`, as an
  output's is), so a pickup tops up from a ground pile by the same rule. A
  pile haul is still a `PileJob` on its own path (`piles::haul`).
- **Where:** `kestrel/sim/src/settlement/piles.rs`.
- **From M2.3:** in a town controller B backs out of a blueprint being
  placed (`Edge::CancelBuild`) and is `Edge::Drop` otherwise; keep that
  order if the drop goes on B. Interact while placing puts the blueprint
  down (`input::town::interact`), so a drop fallback must not apply then.
  A site's deliveries come from the bundle only; a ground pile is no
  holder `bundle_job` delivers from.
- **From M2.2:** a ground pile is a new `InteractTarget` variant, appended
  (`Pile(PileId)`), usable as an output is (top up, by `bundle_job`). The
  ground (drop) is no candidate: at distance zero it would always win.
  Make it the fallback while carrying when `interact_target` finds nothing
  (an `Interaction::Drop`), or keep the drop on its own key (controller B)
  as the pile lab does. `Action::Interact` refuses a target with no
  `position` as out of reach today.
- **Found in:** Sacred Tree handoff.
- **Done:** piles hold any item. A pile scene's minerals keep their
  numbering after the scenario's items, unchanged; a town gets
  `PileLab::town` from `Settlement::from_scenario` (no minerals, dispensers
  or collector; drop limit the extent less 1 m, `MAX_PILES` 32). A haul is
  a mineral moved with a pile or the collector (`PileLab::is_haul`); a
  bundle of a depot item on a pile is a carry job that moves as a depot
  transfer's (`read_avatar`), and a town refuses `Pile` commands ("Nothing
  here is hauled"). `CommandKind::Drop { agent }` (appended) is
  `Action::Drop`: `piles::drop_plan` (also `Settlement::drop_plan`) puts
  every unit of the bundle's item on the nearest pile of it within the
  dock's reach with room, else a new pile `DROP_GAP` (0.5 m) in front of the
  elder, clear of obstacles, other piles, docks and buildings' pile and
  slot spots; stage 5 (`drop_bundles`, in command order) makes the pile and
  holds both sides. Refusals: `NothingToDrop` ("Nothing carried"),
  `TooManyPiles`, `NoRoom`, `Busy`. `InteractTarget::Pile(id)` (appended)
  is a pickup through `bundle_job` (`InventoryRef::Pile`, dispenser piles
  excluded), walking in to `stand_spot` (which now takes where the agent
  stands); an emptied pile is pruned once nothing flies. In the app,
  keyboard Q and controller B (B backs out of placing first) drop; nothing
  drops while placing. The view mirrors `ground_piles` and draws them in
  the item's skin with "5 WOOD" (`settlement::town`); the pile lab's own
  view mirrors only scenes with a pile layout. Help line "... / Q DROP";
  verb `settlement_drop`. Save format 27 (a town keeps a pile lab), input
  logs 8, net protocol 10. The golden file didn't change. Tests: sim
  `ground_pile_tests`, save `a_town_saved_mid_drop_continues`, input
  `the_drop_key_follows_the_town`, control
  `the_elder_drops_wood_and_picks_it_back_up`. The demo ends with a drop.


### M2.3 Blueprints, sites and installing parts

- **Status:** done (branch `kestrel-sacred-tree`)
- **Why:** both kinds are built from two installed parts (D4, A5).
- **What to do:** `PlaceBlueprint` and its checks, the ghost and the
  two-option build menu, deliver and install, the needs readout.
- **Where:** `kestrel/sim/src/settlement/buildings.rs`, `kestrel/app/`.
- **From M1.2:** reuse the prebuilt checks in `buildings::prebuilt`
  (footprint, docks, extent, output pile clear) for a placement, as one
  function. An output pile is not an obstacle, so a placement must also
  keep off the existing buildings' output piles.
- **From M1.3:** `prebuilt` also checks each slot spot
  (`BuildingState::slot_position`) is clear; a placement must keep it clear
  and off the other buildings' slot spots too. Nothing checks a slot spot is
  reachable: a sprite that claims a building it can't walk to holds the
  claim and asks for a route every `RETRY_TICKS`. Check reachability from
  the tree when placing, or let a sprite give up a claim it can't reach.
- **From M2.1:** a delivery is a bundle deposit into the site's
  `materials`: give `avatar::bundle_job` a site holder (an appended
  `InventoryRef` variant) that takes only what the next part still needs,
  sized to that need, and have `validate_buildings` count its reservations
  with `expected_reservations` as outputs do (it insists materials hold
  nothing reserved today).
- **From M1.4:** the camera looks from +x, +z at a fixed angle, so the
  tree's canopy hides a building just behind it (-x, -z): M1.4 moved the
  prebuilt Wood Gatherer from (-4.6, -2.2) to (-5.2, 0.6) for this. Keep
  the ghost and placements out of the canopy's shadow, or fade the canopy
  while it hides something. A new site draws through `settlement::town` as
  any building does (status `site`, card "BEING BUILT"); its needs readout
  can go on the same card.
- **From M2.2:** a site is a new `InteractTarget` variant, appended
  (`Site(BuildingIx)`): give it a `position` and an arm in `interaction`
  (deliver while carrying a needed item, through `bundle_job` on the site
  holder; install when the next part's materials are in; otherwise its
  needs), with an `Interaction` variant for install and needs, an arm in
  `Action::Interact` and a prompt in `input::town::highlight`
  ("DELIVER 2 WOOD", "INSTALL FOUNDATION"). `interact_target` lists every
  building's output, a site's too: it is empty, so never usable, but skip
  sites there once they have a target of their own.
- **Found in:** Sacred Tree handoff.
- **Done:** one placement check, `buildings::placement` (prebuilt
  buildings pass it too; `Settlement::placement_check` for the app):
  footprint inside the arena and clear of the terrain and live agents'
  bodies (`Blocked`), off every dock (`CoversDock`) and other buildings'
  output piles and slot spots (`CoversSpot`); its own pile and slot spot
  clear and uncrowded (`OutputBlocked`, `SlotBlocked`); both reachable on
  foot from every dock for a 0.3 m body (`Unreachable`). The ghost says
  "BLOCKED", "TOO CLOSE TO A PILE", "CAN'T BE REACHED" or "OUT OF BOUNDS".
  `CommandKind::PlaceBlueprint { kind, position }` (appended, any player)
  is checked in stage 5 in command order (`place_blueprints`): a site with
  an obstacle and `terrain.revision` bumped, or `Event::BlueprintRefused`.
  `InventoryRef::Site(b)` (appended) is the site's materials, at its output
  pile's spot: `bundle_job` delivers every unit of the bundle's item the
  next part still takes (`BuildingState::wanted`, which counts units on
  their way), refuses others (`BundleRefusal::NotNeeded`) and any pickup
  (`DepositOnly`); `reserve_stock` checks the need again for two deliveries
  on one tick. `InteractTarget::Site(b)` (appended; built buildings stay
  `Output`, sites aren't): deliver, else `Interaction::Install` once the
  part's materials have landed (also while carrying something it doesn't
  need, so the leftover bundle needs no trip to the tree; D5 listed install
  empty-handed only), else empty-handed `Interaction::Needs` (no sim
  effect; the app shows it). An install (stage 5, `Settlement::install`)
  counts the materials as `consumed` and moves on; the last part makes it
  `Built` (`Event::PartInstalled { built }`). `validate_buildings` checks a
  site's reservations with `expected_reservations`, its materials within
  the part, and an empty slot and output. In the app, keyboard B and
  controller X open a radial build menu; the ghost (`input::town::ghost`)
  stands 0.8 m past its footprint in front of the confirmed elder, the
  kind's skin tinted green or red with its reason, rings for the footprint,
  pile and slot spot; E or A places, B again, Escape or controller B backs
  out. A site is a staked plot, then the skin at half size per part, its
  card saying "FOUNDATION 1/2 WOOD". The canopy (raised architecture within
  4 m of the scene's center) is split out of the baked environment and
  fades while it hides the elder, a site or the ghost (`canopy.rs`). Verbs
  `settlement_build`, `settlement_place_blueprint`,
  `settlement_cancel_build`; the state has `settlement.interact.ghost`,
  `placing`, `build_menu` and each building's `site`. Save format 26,
  input logs 7, net protocol 9. The golden file didn't change. Tests: sim
  `construction_tests` (each refusal, two placements on one tick, delivery
  and needs, install, consumption, both kinds from an empty tree), save
  `a_town_saved_mid_delivery_continues`, input `the_build_keys_follow_the_town`,
  control `the_elder_builds_a_stone_gatherer_that_a_sprite_works`. The demo
  shows the ghost red and green, a half-built site and the canopy fading.

### M2.2 Interact target and highlight

- **Status:** done (branch `kestrel-sacred-tree`)
- **Why:** one key covers piles, the tree, sites, slots and sprites (D5).
- **What to do:** `interact_target` in the sim, `Interact { agent, target }`,
  the app's highlight from the same function, the radial menu on sprites.
- **Where:** `kestrel/sim/src/settlement/action.rs`, `kestrel/app/`.
- **From M2.1:** an output pile and a tree pile act through
  `avatar::bundle_job(carry, view, other, direction)`: Interact on
  `InventoryRef::Output(b)` is a `Direction::Pickup`; on a tree pile
  (`InventoryRef::Depot(d)`) a `Deposit` while carrying, a `Pickup` when
  empty-handed. Its `Err(BundleRefusal)` says why a target isn't usable, so
  `interact_target` can filter on `is_ok()`, and `Action::Bundle`'s body is
  what Interact runs for these kinds (it emits `Event::BundleRefused`, which
  the view can show with `BundleRefusal::message`). A job refused later in
  stage 5 (another player took the units first, `reserve_stock`) rolls back
  with no event; give it one if the feedback needs it. The units move only
  once the elder is within 0.85 m of the pile (`DOCK_REACH`); the verbs
  walk it there, and Interact's reach check must be at least that close. The
  view's `manual_jobs` lists depot jobs only, so an output pickup in
  progress doesn't show there yet.
- **Found in:** Sacred Tree handoff.
- **Done:** `sim::settlement::interact` holds `interact_target(view, me,
  sprites)` (and `Settlement::interact_target`, which the app and scripts
  read): building outputs, depots and sprites within `INTERACT_REACH`
  (1.5 m) of the agent, nearest first, a tie to the lower `InteractTarget`
  (kind, then index); the first one `interaction` can use wins. A bundle
  kind is usable when `bundle_job` is `Ok` (an output is a pickup; a depot a
  deposit while carrying, a withdrawal empty-handed); a sprite only
  empty-handed (D5), for the job menu. A dead or seated agent has none.
  `CommandKind::Interact { agent, target }` (appended) is `Action::Interact`:
  it checks reach and use again, refusing with `BundleRefused`
  (`BundleRefusal` gained `OutOfReach` and `HandsFull`), starts the carry
  job and walks the elder in to `stand_spot` when it is past the dock's
  reach, as the verbs do. A sprite target does nothing in the sim. In the
  app (`input::town`), keyboard E and controller A interact in a town (a
  settlement with building kinds), where the controller's nearby targets
  are off; the target is ringed on the ground (`input::hud`) with its
  prompt over it ("E  PICK UP 3 WOOD"), and refusals show over the elder
  (`settlement::town_interact`). A sprite opens `Panel::Radial`, a ring of
  Idle and one Gather entry per building kind around it, steered by WASD,
  the arrows, the stick or the D-pad, picked with E or A, sending
  `SetSpriteJob`. Control verbs `settlement_interact_target`,
  `settlement_interact` and `settlement_job_menu`; the state's
  `settlement.interact` has the target, prompt and open menu. Input logs 6,
  net protocol 8; the save format stays 25 (saves hold no commands, and
  interact adds no state). The golden file didn't change. The Sacred Tree
  demo ends with two interacts.


### M2.1 Bundles at piles and the tree

- **Status:** done (branch `kestrel-sacred-tree`)
- **Why:** resources move only by hand (D6, A4).
- **What to do:** a one-type cargo of 5 for the elder, top-up pickups,
  deposit and withdraw at the tree's wood and stone piles.
- **Where:** `kestrel/sim/src/settlement/avatar.rs`, `inventory.rs`.
- **From M1.2:** an output has no `InventoryRef` yet, and
  `validate_buildings` insists nothing is reserved on it. Picking up from
  one needs a variant (appended) and the reservation check to count the
  carry jobs and transfers on it, as `validate_lab` does.
- **Found in:** Sacred Tree handoff.
- **Done:** a depot's `only` (content) makes its inventory `Kinds::Only`
  (the tree's wood and stone piles take their own item); an agent's
  `one_kind` makes its cargo `OneKind` (the elder: one item, up to its
  capacity of 5). Other scenes set neither. `InventoryRef::Output(b)`
  (appended) names a building's output. `CommandKind::Bundle { agent,
  other, direction }` and `Action::Bundle` start a carry job sized by
  `avatar::bundle_job` (also `Settlement::bundle_job`, for the view): a
  pickup from an output or a depot tops the bundle up with the holder's
  item, as many as fit; a deposit empties it into a depot that takes its
  item. Units move once the elder is within the dock reach (0.85 m) of the
  holder and fly as transfers. Refusals (`BundleRefusal`: busy, not a
  holder, pickup only, nothing to take, other item, bundle full, nothing
  carried, wrong pile, pile full) come out as `Event::BundleRefused`, with
  `message()` for words; the app mirrors it. Depot transfers keep both
  rules. `validate_buildings` counts output reservations from jobs and
  transfers (`expected_reservations`, shared with `validate_lab`).
  Production's cap counts reserved units, which stay in the output until
  they launch. Control verbs `settlement_pick_up`, `settlement_withdraw`,
  `settlement_deposit`. Save format 25, input logs 5, net protocol 7. The
  golden file didn't change.

### M1.4 Drawing buildings, sprites and outputs

- **Status:** done (branch `kestrel-sacred-tree`)
- **Why:** occupancy, job and working, waiting or full must read from the
  game view (A7).
- **What to do:** building skins with idle, working and full states, a cue
  for `Event::BuildingProduced`, sprites with their job and state, output
  piles, the tree's counts; the `sacred_tree` scene pack in the app's scenes
  and launch menu.
- **Where:** `kestrel/app/src/legacy.rs`, `settlement.rs`, `assets.rs`.
- **Found in:** Sacred Tree handoff.
- **Done:** the scene is in the app and its menu (last, so other scenes keep
  their keys). `legacy.rs` mirrors the buildings (status idle, working,
  full or site; occupant; claim; progress; output and cap) and each
  sprite's job and activity (idle, moving, waiting, working), both left out
  of traces when empty. `settlement::town` draws each building in its
  kind's skin fitted to its footprint and facing its output, its rotor
  turning while it works; a lamp over it (amber working, red full, dim
  idle, violet for an elder) and a card ("WOOD GATHERER / WORKING / ASH");
  its output stacked on its pile with "3/10 WOOD". A sprite has a flame
  over its head in its activity's color and a tag with its name, job and
  activity; inside, it faces its building and bobs. The tree's piles say
  "WOOD PILE 3" and the HUD line counts the tree's wood and stone.
  `BuildingProduced` plays the dispenser cue and flares the lamp. The
  control test `a_sprite_staffs_the_wood_gatherer_and_wood_comes_out` and
  the manual video `//kestrel/app:sacred_tree_demo`. The Wood Gatherer moved
  to (-5.2, 0.6): behind the tree, the canopy hid it from the camera.

### M1.3 Sprites, jobs and slot claims

- **Status:** done (branch `kestrel-sacred-tree`)
- **Why:** sprites staff buildings on their own and replan when a step
  breaks (D2, A2).
- **What to do:** `Role::Sprite`, the plan builder and replan, `Claim` in
  stage 3 granted in stage 5, `SetSpriteJob` and its control verb. Make
  sprite agents from the scenario's `sprites` list (M1.1), after its
  `agents`, and reject a sprite whose character hasn't the `Sprite` brain.
- **Where:** `kestrel/sim/src/settlement/agent.rs`, new `sprites.rs`,
  `world.rs`, `kestrel/app/src/control/`.
- **Found in:** Sacred Tree handoff.
- **Done:** `sprites.rs` holds `SpriteState` (job, plan, step, the slot it
  `holds`, a retry tick and its steering) and `plan`: idle goes home; a
  staffing job claims the nearest free built building of its kind, walks to
  its slot spot (a quarter turn from the output pile) and steps in, or
  waits and plans again after `RETRY_TICKS`. Stage 2 runs the brain and the
  walk (the hauler's `move_worker`, now generic over a `Walker` trait);
  stage 3 sends `ClaimSlot` and `EnterSlot`; stage 5 grants claims (lowest
  agent index wins) and entries, writing the slot and `holds` together. A
  broken step (a refused claim, a lost hold, a new job) drops the plan;
  `Settlement::release_slot` is the one way a sprite loses a slot. Inside,
  a sprite leaves the steering footprints. `SetSpriteJob` and the
  `settlement_sprite_job` control verb; `validate_buildings` checks slots
  and holds both ways. Save format 24, input logs 4, net protocol 6. The
  golden file didn't change.

### M1.2 The buildings table and production

- **Status:** done (branch `kestrel-sacred-tree`)
- **Why:** buildings are made at runtime and only produce when occupied
  (D1, A1).
- **What to do:** `Settlement::buildings`, production in the economy step,
  output inventories in `balances` and `validate`, the behaviour digest,
  the save format. Make the table from the scenario's `building_kinds`
  and `buildings` (M1.1), which `Settlement::from_scenario` ignores until
  then; check prebuilt footprints are clear.
- **Where:** `kestrel/sim/src/settlement/` (new `buildings.rs`),
  `economy.rs`, `mod.rs`.
- **Found in:** Sacred Tree handoff.
- **Done:** `buildings.rs` holds `BuildingState` (kind, position, output
  position, stage, slot, progress, output) and the kinds resolved into
  ticks (`Settlement::building_kinds`). Prebuilt buildings load built and
  empty; a footprint on an obstacle, another building, a depot's dock or
  past the arena's edge fails the load. The output pile faces the tree
  (handoff, D1). Occupied buildings produce in the economy step, an elder
  at the multiplier up to the possessed cap; outputs count in `balances`
  and `validate`. Save format 23; the golden file's state hashes moved
  (two new settlement fields), behaviour lines unchanged.

### M1.1 Building kinds, the scene and its supply

- **Status:** done (branch `kestrel-sacred-tree`)
- **Why:** the map, its items and every tunable come from content (D1,
  "Scene").
- **What to do:** add building kinds (product, period, cap, possessed cap,
  multiplier, Burst, ordered parts) and prebuilt buildings to the supply
  format; the `Sprite` brain; the elder and sprite characters; the scene
  and supply files with the tree depot, four sprites and one Wood Gatherer.
- **Where:** `kestrel/content/src/scenario.rs`, `content/src/character.rs`,
  `kestrel/app/assets/levels/`, `docs/character-definition.md`.
- **Found in:** Sacred Tree handoff.
- **Done:** `building_kinds`, `buildings` and `sprites` in the scenario
  format (all default empty), the `Sprite` brain, the `elder` and `sprite`
  characters, and `levels/sacred_tree.ron` with `sacred_tree_supply.ron`
  (the `//kestrel/app/assets:sacred_tree` pack, not in the app yet). The
  sim reads none of the new lists yet: the scene loads with the elder and
  the tree's two piles only.
