# Kestrel cargo merge (proposal)

Status: a proposal, not built. Part of character step 5
(`docs/character-definition.md`, "Shared body"). The shared body is done:
every agent has a `Body` and a `Carry`, and its role is a `Role` enum
(`sim/src/settlement/agent.rs`).

## Goal

Agents carry things in three ways today. Each one reserves, moves and
gives back units on its own, and each one handles cancel, death and
respawn on its own. Merge them into one carry job with one reserve path
and one release path, so a new carrier (a hurt hauler, a hunter that drops
loot) gets correct accounting for free.

## What each model does today

### Hauler haul (`haulers.rs`, `economy.rs`)

- State: `HaulerState::assignment` (link, units, loaded, unloaded), the
  phase machine, and the agent's `Carry` (cargo per item, `pending_in`,
  `pending_out`).
- Reserve: stage 3 `decide` sends `Reserve`. Stage 5 checks it again
  (`job_size`) and reserves both ends at once: `out` at the source depot
  and `incoming` at the target.
- Move: `Load` and `Unload` move one unit each, every `transfer_ticks`. A
  unit flies as a `Transfer` (`Endpoint::Depot` to `Endpoint::Cargo`, or
  back). Arrival fills the cargo or the depot and clears the reservation.
- Equipment: `Equip` reserves one unit of the equipment item; `Attach`
  takes it.
- Release: none. A hauler can't die or cancel, and a disabled link lets a
  started job finish.

### Player depot transfer (`avatar.rs`)

- State: `AvatarState::job: Option<ManualJob>` (depot, item, remaining,
  deposit, next tick), and the same `Carry` as a hauler.
- Reserve: the `Transfer` action sets the job and sends `TransferStock
  { change: Reserve }`. Stage 5 (`transfer_stock`) checks it again
  (`transfer_limit`) and reserves one side only: `out` for a pickup,
  `incoming` for a deposit. A refused reservation rolls the job back.
- Move: stage 3 sends `ManualUnit` when the player is at the dock. Stage 5
  launches one unit, the same kind of `Transfer` a hauler uses.
- Release: `CancelTransfer` and `Respawn` send `TransferStock { change:
  Release }` for the units not moved yet. A death releases them directly
  (`hurt_player`). Units in the air still land.

### Pile haul (`piles.rs`)

- State: `PileLab::haulers`, one `piles::Hauler` per avatar, with its own
  `Cargo` (one mineral, a unit count, the lab's capacity), the
  `PileJob`, a job serial, landing ticks and UI notices. It is not the
  agent's `Carry`.
- Reserve: none. A pile's units are checked when the job starts and again
  at every step. Two players can walk to the same units, and the second
  one fails or takes less.
- Move: the `Pile` command plans the walk and writes the lab directly.
  Stage 3 sends `PileStep`; stage 5 moves a unit at once and shows a toss
  event. Nothing is in the air in the accounting.
- Release: nothing to release. Moving, attacking, `Respawn` (`DropHaul`)
  and death (`end_pile_job`) end the job; the cargo stays.
- Prediction: pile commands are not predicted (`AvatarReplay::apply`
  returns false).

## Proposal

### State

- `Carry` gains the job: `Carry { cargo, capacity, pending_in,
  pending_out, transfer_ticks, job: Option<CarryJob> }`.
- `CarryJob { place, item, direction, remaining, next_tick, pace,
  reserved }`:
  - `place` is where units come from or go to: `Depot(d)`, `Pile(id)`,
    `Collector`, or `Ground(position)` for a new pile.
  - `direction` is `Load` (into the cargo) or `Unload`.
  - `reserved` is the units still promised at `place`.
- A hauler's assignment becomes a plan of two jobs: a `Load` at the source
  and an `Unload` at the target. The second job's reservation is made when
  the assignment starts, as today, so it carries its own `reserved`. The
  hauler keeps the link and the phase machine; the jobs hold the units.
- Pile minerals become items in the cargo (see the open questions), so
  the lab's `piles::Cargo` and `Hauler::cargo` go away. The lab keeps the
  UI state (notice, last error, job serial) per avatar.
- `Endpoint` gains `Pile(id)` and `Collector`, so a pile unit is a
  `Transfer` like a depot unit.

### Messages

- `ReserveStock { agent, place, item, units, change: Reserve | Release }`
  replaces `TransferStock` and the reserving half of `Reserve` and
  `Equip`. Stage 5 checks a `Reserve` again and rolls the job back if it no
  longer fits, as `transfer_stock` does now.
- `CarryUnit { agent }` replaces `Load`, `Unload`, `ManualUnit` and
  `PileStep`. Stage 5 moves one unit of the agent's job (or the whole
  amount when `pace` is 0) and launches it.
- `drop_carry(agent)`, one function, ends a job: it sends a `Release` for
  `reserved` and clears the job. Cancel, `Respawn`, a death, a timed-out
  pile walk and any future role (a hurt hauler) call it. Units in the air
  still land.
- `validate` recomputes every reservation from the carry jobs in one loop.

## Rules and the golden file

- Haulers: no rule change. The golden scenes have haulers but no player
  commands and no piles. Keep each new message in the queue position and
  apply order of the one it replaces (hauler `Reserve` in the economy
  step, a player's reservation first in `apply_actions`), and the golden
  behaviour lines stay. The totals move, because state and message queues
  are hashed.
- Players at depots: no rule change.
- Piles: two rule changes, each its own commit, last:
  - pile units are reserved when a job starts, so the second of two players
    is refused at once instead of failing at the pile;
  - pile units fly like depot units (`TRANSFER_FLIGHT_TICKS`) instead of
    moving at once. This changes when a carried unit counts.
  No golden scene has piles; scene tests and app pile tests change.

## Commit order

Step 5's order: digest, format, reading with the golden file unchanged,
gameplay last.

1. Behaviour digest: hash the carry job, keeping today's values for agents
   without one. Golden unchanged.
2. Format: `CarryJob` and `ReserveStock`. Move the player's `ManualJob`
   onto them, with `drop_carry` for cancel, respawn and death. Save format
   up. Golden behaviour unchanged.
3. Move the hauler's load and unload onto carry jobs and `CarryUnit`.
   Golden behaviour unchanged; totals move.
4. Move pile cargo into `Carry` and pile steps onto `CarryUnit`, with
   instant moves and no reservation kept. Pile tests unchanged.
5. Gameplay: reserve pile units.
6. Gameplay: pile units fly.
7. Remove what is left (`piles::Cargo`, `ManualJob`, `Load`, `Unload`,
   `ManualUnit`, `PileStep`).

## Risks and open questions

- **Items and minerals.** Settlement items are per-scenario strings
  (`ItemIx`); pile minerals are a fixed `u8` list from the mine. Map
  minerals into the item list when a scene has piles, or keep two kinds of
  stack in `Carry`? A mine scene has both piles and depots.
- **Capacity.** A player has the scenario's capacity for depot items and
  the lab's capacity for minerals. Which one wins in a single cargo?
- **One mineral at a time.** Pile cargo holds one mineral; depot cargo
  holds any mix. Keep the rule for minerals only, or drop it?
- **Prediction.** A pile job reads pile lab state, so it isn't predicted.
  Once a pile haul is an action (`todo/characters.md`, "Move pile hauls
  and piano keys onto actions"), its agent part can read piles through
  `View`, and the walk could be predicted like a depot `GoTo`.
- **Apply order.** One message type applied at two points in stage 5 (the
  actions' part and the economy step) keeps the golden file, but it is
  easy to break. Two queues of the same type may be clearer.
- **Two reservations per hauler job.** A haul reserves both ends at once.
  Keep that (a plan of two jobs, both reserved) or reserve the target only
  when the load ends? The second changes hauler rules and the golden
  file.
- **Size.** `Carry` is on every agent, inside the 100k-entity budget. Keep
  the job small, or box it.
- **Notices.** Pile notices and errors are UI text in sim state. Decide if
  they stay in the lab or become events.
