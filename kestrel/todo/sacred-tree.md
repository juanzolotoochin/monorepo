# Sacred Tree

The town MVP's design and decisions are in
[`kestrel/SACRED_TREE_HANDOFF.md`](../SACRED_TREE_HANDOFF.md). Tasks follow
its milestones, in order; one PR per milestone.

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

## Done

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
