# Sacred Tree

The town MVP's design and decisions are in
[`kestrel/SACRED_TREE_HANDOFF.md`](../SACRED_TREE_HANDOFF.md). Tasks follow
its milestones, in order; one PR per milestone.

## M1.4 Drawing buildings, sprites and outputs

- **Status:** open
- **Why:** occupancy, job and working, waiting or full must read from the
  game view (A7).
- **What to do:** building skins with idle, working and full states
  (`Event::BuildingProduced` is ignored by `app/src/legacy.rs` so far; give
  it a cue),
  sprites with their job and state, output piles, the tree's counts. Add
  the `sacred_tree` scene pack to the app's scenes and launch menu
  (`app/BUILD.bazel`, `app/src/assets.rs`).
- **Where:** `kestrel/view/`, `kestrel/app/`.
- **From M1.3:** `app/src/legacy.rs` shows a sprite as an idle hauler with
  no cargo. Show its job (`SpriteState::job`) and step (`current()`:
  `Step::Wait` is waiting, `Enter` with `holds` `Inside` is working). Inside,
  it stands at `BuildingState::slot_position`. The control verb
  `settlement_sprite_job` exists; once the scene is in the app, give it a
  script test that sets a job and waits for wood.
- **Found in:** Sacred Tree handoff.

## M2.1 Bundles at piles and the tree

- **Status:** open
- **Why:** resources move only by hand (D6, A4).
- **What to do:** a one-type cargo of 5 for the elder, top-up pickups,
  deposit and withdraw at the tree's wood and stone piles.
- **Where:** `kestrel/sim/src/settlement/avatar.rs`, `inventory.rs`.
- **From M1.2:** an output has no `InventoryRef` yet, and
  `validate_buildings` insists nothing is reserved on it. Picking up from
  one needs a variant (appended) and the reservation check to count the
  carry jobs and transfers on it, as `validate_lab` does.
- **Found in:** Sacred Tree handoff.

## M2.2 Interact target and highlight

- **Status:** open
- **Why:** one key covers piles, the tree, sites, slots and sprites (D5).
- **What to do:** `interact_target` in the sim, `Interact { agent, target }`,
  the app's highlight from the same function, the radial menu on sprites.
- **Where:** `kestrel/sim/src/settlement/action.rs`, `kestrel/app/`.
- **Found in:** Sacred Tree handoff.

## M2.3 Blueprints, sites and installing parts

- **Status:** open
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
- **Found in:** Sacred Tree handoff.

## M2.4 Dropping bundles on ground piles

- **Status:** open
- **Why:** the elder must drop a bundle before possessing (requirements
  3.1); piles reuse the pile lab.
- **What to do:** let piles hold any item, not only minerals (handoff,
  "Open questions"), then drop into and pick up from ground piles of wood
  and stone.
- **Where:** `kestrel/sim/src/settlement/piles.rs`.
- **Found in:** Sacred Tree handoff.

## M3.1 Building seats and ejection

- **Status:** open
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
- **Found in:** Sacred Tree handoff.

## M3.2 Burst

- **Status:** open
- **Why:** the stand-in for building-specific elder abilities (D3).
- **What to do:** `Burst { agent }`, its cooldown on the elder, its control
  verb and its ready indicator.
- **Where:** `kestrel/sim/src/settlement/avatar.rs`, `buildings.rs`,
  `kestrel/app/`.
- **Found in:** Sacred Tree handoff.

## M4.1 Happy path and stability

- **Status:** open
- **Why:** the 10-step happy path must play from a fresh start, and
  repeated cycles must never strand a sprite (A6, A8).
- **What to do:** the happy path as a headless control script, the
  stability soak and the bootstrapping test (handoff, "Tests"); fix what
  they find, then tune rates.
- **Where:** `kestrel/sim/src/settlement/` tests, `kestrel/headless/`.
- **From M1.3:** `sprites_tests::no_sprite_keeps_waiting_while_a_matching_slot_is_free`
  already reassigns sprites over two gatherers for 90 s; the soak can grow
  from it with possession and building.
- **Found in:** Sacred Tree handoff.

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
- **Found in:** M1.1.

## Done

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
