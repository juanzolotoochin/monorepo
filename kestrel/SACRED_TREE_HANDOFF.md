# Kestrel Sacred Tree town MVP (design)

Status: design agreed (2026-10-30), M1–M4 done (PR #131). The ballista
prototype that follows is in [`SACRED_TREE_BALLISTA.md`](SACRED_TREE_BALLISTA.md) (PR #132), and
mining after it in [`SACRED_TREE_MINING.md`](SACRED_TREE_MINING.md). Tasks in
[`todo/sacred-tree.md`](todo/sacred-tree.md), one per step, by milestone.

## Goal

A new map that tests one loop: assign sprites → they staff buildings →
buildings produce physical piles → the player carries them to the Sacred
Tree → builds more buildings from two installed parts → reassigns sprites →
possesses a building as the elder, which speeds it up and unlocks Burst.

It is a sandbox for a 10–15 minute playtest, single player. The full
requirements are "Sacred Tree — Town MVP Requirements (v0.1)" (session of
2026-10-30). The numbers that matter here:

| Building | Product with a sprite | Parts (in order) |
|---|---|---|
| Wood Gatherer | 1 wood / 5 s | foundation: 2 wood; mechanism: 2 wood + 2 stone |
| Stone Gatherer | 1 stone / 8 s | foundation: 2 wood; mechanism: 2 wood |

- An output holds at most 10 units; production pauses when it is full.
- A possessing elder produces at 4× and the cap goes up (tunable, e.g. 20).
- Burst: +2 units at once, 30 s cooldown (tuned to +5, one full bundle,
  in M4.1: +2 was lost against the 24 units 4× makes in the cooldown).
- A bundle holds up to 5 units of one type. The tree holds any amount.
- Every rate, cap, recipe and multiplier lives in content, not code.

**Guardrail:** a feature that doesn't help answer one of the requirements'
prototype questions (section 9) is deferred.

## What exists today

| Requirement | Reuses | New |
|---|---|---|
| Sacred Tree stockpile | A depot (`DepotState`) | Large capacity |
| Bundle, one type, up to 5 | `Carry`, `Action::Transfer`, `Kinds::OneKind` | Capacity 5, top-up (D6) |
| Conservation (A4) | `Settlement::balances`, `validate` | Cover building outputs and site materials |
| One occupant per slot (A1) | The dock claim pattern, contested in stage 5 | A slot claim (D2) |
| Possession | Vehicle boarding (`Board`, `AvatarState::driving`) | A building seat, ejection, Burst (D3) |
| Sprites | `Body`, `move_worker` steering, `Move(GoTo)` | `Role::Sprite` and its brain (D2) |
| Construction | Runtime obstacle changes (`terrain.revision`, as the memory door does) | Buildings made at runtime (D1) |
| Dropped bundles | The pile lab's ground piles | Piles for any item, not only minerals |
| Radial menu, blueprint ghost, highlight | None | App UI over new commands |

Depots and producers are only made when a scenario loads, and their indices
stay fixed for the world's life. That is the one real gap.

## Settled before the decisions

- Possession raises a building's output cap (tunable). The mechanics may
  change after the playtest.
- A displaced sprite replans: it may take any free matching building, or
  wait. It has no loyalty to the building it left.
- Dropped bundles reuse the pile lab's ground piles.
- Players set jobs from a small radial menu. The menu only sends a command
  (`SetSpriteJob`), which scripts and tests send through the control
  protocol too (`app/src/control`).
- Everything lives in the deterministic sim, even for one player, so co-op
  later is the network path the game already has.

## Decisions

### D1. A buildings table

- `Settlement::buildings: Vec<BuildingState>`, indexed by `BuildingIx`
  (`u16`). Append-only, never reordered, as vehicles and threats keep
  their own tables.
- A `BuildingState` holds its kind, position, stage (`Site { part,
  materials }` or `Built`), its slot (empty, claimed by a sprite on its way,
  or occupied by a sprite or an elder), its production progress and its
  output `Inventory` (`OneKind`, capped).
- The Sacred Tree stays a depot. It is no building kind.
- **Output side:** the output pile sits `output_distance` from the center,
  toward the midpoint of the scenario's depots (the tree's two stock piles),
  or toward the arena's center when there are none (decided in M1.2).
- **Kinds are content:** the supply file lists building kinds with label,
  skin, radius, product, period, cap, possessed cap, multiplier, Burst units
  and cooldown, and the ordered parts with their materials. The scenario
  also lists the buildings that start built (the first Wood Gatherer).
- **Placing:** `PlaceBlueprint { kind, position }`. Stage 5 checks the
  ground is clear (terrain, other buildings, the tree, inside the extent),
  appends a site, adds its obstacle and bumps `terrain.revision`. Two
  placements on one tick resolve in command order.
- **Production** runs in the economy step: a built building that is
  occupied and under its cap advances; at its period it puts one unit in
  its output. An elder divides the period by the multiplier and raises the
  cap. An empty building does nothing (A1).
- **Why not grow depots and producers:** a building would be spread over
  three tables that must stay in sync, and producers are recipes, which a
  gatherer with no ingredients bends.
- The table is hashed in the behaviour digest and saved, so the save format
  goes up.

### D2. The sprite brain: a plan with a replan hook

- A new `Role::Sprite(SpriteState)`, next to `Hauler` and `Avatar`, and a
  `Sprite` brain in character definitions (no `carry`, no `health`).
- `SpriteState` holds the job (`Idle` or a building kind to staff), the
  current plan (a list of steps) and the step it is on.
- **The plan builder** is one function, `plan(job, view) -> Vec<Step>`.
  For a gathering job it picks the nearest built building of that kind
  with a free slot (lowest index on a tie) and returns
  `[Claim(b), GoTo(slot), Enter(b)]`. With none free it returns
  `[Wait]`, which retries every `RETRY_TICKS`.
- **Claims:** `Claim` is a stage 3 message, granted in stage 5 like a dock
  claim (lowest agent index wins a tie). A granted claim holds the slot
  while the sprite walks, so two sprites never race to one building.
- **Replanning:** anything that breaks a step drops the whole plan and
  plans again: a refused claim, being ejected, an elder taking the claimed
  slot first, or a new job. Nothing else edits a plan.
- While inside, the sprite stands at the slot and leaves the steering
  footprints. It stays visible in the building, so who occupies it reads
  at a glance (A7).
- **Why this shape:** it is a state machine's cost with GOAP's shape (goal,
  plan, invalidate, replan). A real planner later replaces `plan` and
  nothing else. Preconditions and effects get designed when sprites have
  more than one way to reach a goal.

### D3. Possession generalises boarding

- `AvatarState::driving: Option<VehicleIx>` becomes
  `seat: Option<Seat>`, with `Seat::Vehicle(VehicleIx)` and
  `Seat::Building(BuildingIx)`.
- `Board` enters the nearest free vehicle or built building in reach, or
  leaves the seat it has. Entering a building is refused while the elder
  carries a bundle. The refusal is an event the view explains.
- **Ejection:** a sprite in the slot (or walking to it on a claim) loses
  it in the same stage 5 handler. An occupant is put beside the building
  on a clear spot (as `trailer_spot` searches), and its plan breaks, so it
  replans (D2).
- While seated in a building, movement commands do nothing, the elder is
  drawn as the building's flame, and `Burst` works. Leaving puts the elder
  beside the building, and the slot is free for any waiting sprite.
- **Burst:** `Burst { agent }` adds the kind's Burst units to the output,
  up to the possessed cap. Its cooldown is on the elder (`AvatarState`),
  so it can't be reset by hopping between buildings.
- Boarding is already contested and predicted; the seat change keeps that.
  Production isn't predicted.
- The vehicle tests guard the rename. Save format goes up.

### D4. Construction: deliver, then install by hand

- A site's `materials` is an `Inventory` that accepts only what the next
  part still needs, and only up to that need.
- **Deliver:** the site works like the dig lab's collector, a drop-off
  that nothing is taken back out of. One interact while carrying a needed
  item moves every unit of it the next part still needs from the bundle
  into the site, through a carry job like any depot transfer; the rest
  stays in the bundle. The units leave the player's bundle on delivery
  (A4).
- **Install:** interacting with the site when the next part's materials are
  complete installs it: its materials are consumed (counted in the
  balances as consumed) and the next part starts taking materials.
- Interacting while materials are missing shows what the next part still
  needs. Installing the last part makes the building `Built`; it stays
  inert until someone occupies it.
- Only the next part takes materials, so the site's needs read as one list.
- Installing works while carrying an item the site doesn't need (M2.3), so
  leftover units after a delivery don't force a trip to the tree first.
- Dropping a bundle on the ground (M2.4) is its own key, not an interact
  fallback, so a press that just misses a target never drops the bundle.

### D5. Interact acts on the nearest usable thing, highlighted

- One sim function, `interact_target(view, agent)`, lists what is in reach
  and usable in the agent's current state, and returns the nearest (lowest
  kind, then index, on a tie).
  - **Carrying:** the tree (deposit), sites that need that item, output
    piles and ground piles of that item (top up). The ground is no target:
    dropping is its own key (D4).
  - **Empty-handed:** output piles, ground piles, the tree (withdraw),
    sites ready to install, sites to read, building slots (board, a
    staffed one too, putting its sprite out: decided in M4.1), sprites
    standing outside buildings (radial menu).
- The app highlights the target from the same function, so the sim and
  the view never disagree. This avoids the drift that "Share the busy
  carrying rule" in `todo/characters.md` has.
- `Interact { agent, target }` names the target the client highlighted;
  the sim checks it is still in reach and usable, and acts by its kind.
  A sprite target sends nothing to the sim; the app opens the radial menu,
  which sends `SetSpriteJob`.

### D6. Bundles top up freely

- A pickup takes as many units as fit (up to 5) of the bundle's type, from
  any output pile, ground pile or the tree. A bundle of another type is
  refused.
- The tree's stock is two piles beside it, one for wood and one for stone,
  each its own depot and interact target. Depositing empties the bundle
  into the matching pile; withdrawing from a pile fills the bundle with its
  item.

## Commands

New `CommandKind` variants, appended (the enum is append-only for input
logs): `SetSpriteJob { agent, job }`, `PlaceBlueprint { kind, position }`,
`Interact { agent, target }`, `Burst { agent }`. `Board` exists. Each gets a
control protocol verb, so the happy path runs as a script with no UI.

## Scene

A new scene and supply pair, as `forest_outpost.ron` and
`forest_supply.ron`: a clearing with the tree (architecture and a depot)
at the center, four sprites, the elder, one built Wood Gatherer and room
for at least two more buildings. Items: wood and stone. The elder is a new
`Player` character with no weapon or abilities; the sprites a new `Sprite`
character with small skin variants.

## Tests

- One sim test per acceptance criterion where it fits: A1 occupancy, A2
  waiting sprites resume, A3 possession and return, A4 conservation (with
  `validate` every tick), A5 both kinds built and staffed.
- The 10-step happy path (A6) as a headless script through the control
  protocol, from a fresh start.
- A stability soak (A8): repeated possess, leave, reassign and build cycles
  with `validate` every tick, asserting no sprite stays waiting while a
  matching slot is free.
- Bootstrapping: from an empty tree, the full build order is reachable
  (8 wood and 2 stone in total), with no softlock.

## Open questions

None left. Answered (2026-10-30): ground piles hold any item, not only
minerals: the pile lab stops numbering them after the scenario's items
(`first_mineral`) and draws any item, rather than adding wood and stone as
minerals. The tree's stock is two separate piles (D6); a
site is a deposit-only drop-off like the dig lab's collector (D4); a
sprite stays visible inside its building (D2). The output pile sits in
front of its building at a distance set per kind (default 1.2 m), so a
pickup and boarding never compete for one target.

## Rules for the work

- Branch off `master` (not a feature branch), one PR per milestone.
- Other scenes behave the same: the new state is empty for every existing
  scene, and the golden file's behaviour lines don't change. State hashes
  may move whenever the settlement gains state (new characters shift
  indices, since characters are numbered by name; a new field adds bytes
  even when empty). Regenerate the golden in a commit of its own and say
  why (agreed 2026-10-30).
- Commands are appended, never reordered.
