# Kestrel inventory (design)

Status: decided, being built (branch `kestrel-inventory`). Part of character step 5
(`docs/character-definition.md`, "Shared body"). It replaces the earlier
cargo merge proposal.

## Goal

Countable goods (wood, ore, minerals and other scenario items) live in five
places today, each with its own counts, capacity and reservation code. Give
them one `Inventory` type, and move goods between two inventories with one
transfer path. Cancel, death and respawn then release reservations through
one function. A new holder of goods gets correct accounting for free.

**Scope: stock only.** An inventory holds counts of items. It holds no
weapons, tools or equipment (`HaulerState::equipment`, the scenario's
weapons): those stay as they are, and could be a later step on the same
type. Threats get no inventory, since they're 100k entities. Loot would drop
onto a ground pile.

## What exists today

| Holder | Counts | Capacity | Reserved | Kinds |
|---|---|---|---|---|
| Depot (`DepotState`, `mod.rs`) | `stock` per item | `capacity` | `out` (promised to haulers, recipes, players), `incoming` (hauled, flying, produced) | any mix |
| Agent cargo (`Carry`, `agent.rs`) | `cargo` per item | `capacity` | `pending_out`, `pending_in` (units in the air) | any mix |
| Pile (`piles::Pile`) | `units` of one `material` | `PILE_CAPACITY` (96) | none | one mineral |
| Player's pile cargo (`piles::Cargo`, in `PileLab::haulers`) | `units` of one `material` | `PileLab::capacity` | none | one mineral |
| Collector (`piles::Collector`) | `stored` per mineral | none | none | any mix |

And three ways to move goods:

- **Hauler haul** (`haulers.rs`, `economy.rs`):
  - `Reserve` reserves both ends when the haul starts.
  - `Load` and `Unload` move one unit every `transfer_ticks`. The unit flies as a `Transfer` (`Endpoint::Depot` ↔ `Endpoint::Cargo`).
  - Nothing is released, since a hauler can't die or cancel.
- **Player depot transfer** (`avatar.rs`):
  - The `Transfer` action sets `AvatarState::job` (`ManualJob`) and sends `TransferStock { Reserve }`. Only the depot side is reserved.
  - `ManualUnit` launches the same flying `Transfer`.
  - Cancel, respawn and death each release on their own.
- **Pile haul** (`piles.rs`):
  - The `Pile` command writes the lab directly, with no reservation.
  - `PileStep` moves a unit at once.
  - Units are conserved by `PileLab::conserved` (created = piles + cargo + collector), not by `validate`.

Minerals are a fixed `u8` list from the mine. Settlement items are
per-scenario strings (`ItemIx`).

## Design

### Items

One item list per settlement. When a scene has piles, the mine's minerals
are added to the scenario's items (after them, in mineral order), so a
mineral is an `ItemIx` like wood. A scene with both depots and piles (a
mine) uses one list.

### `Inventory`

```rust
pub struct Inventory {
    pub counts: Vec<u32>,     // per ItemIx
    pub capacity: u32,        // total units; 0 means none
    pub reserved_out: Vec<u32>,
    pub reserved_in: Vec<u32>,
    pub kinds: Kinds,         // AnyMix | OneKind
}
```

- Methods hold the rules that are scattered today:
  - `available(item)` = `counts - reserved_out`;
  - `free()` = `capacity - counts - reserved_in`;
  - `reserve_out` and `reserve_in`, which refuse when the units don't fit;
  - `release_out` and `release_in`;
  - `take(item)` and `put(item)`, which move a reserved unit.
- `OneKind` refuses a reservation or a put of a second item while any of the first is held or reserved. Only piles use it; a pile is drawn as a mound of one mineral.
- `validate()` checks one inventory: counts and reservations within capacity, `OneKind` held. `Settlement::validate` calls it for every inventory and then checks the totals across them.
- `counts` per item is dense. That's fine: items are few, and holders are depots, agents, piles and the collector, not threats. If a scene has many items and many piles, switch to a small sorted list later; the methods hide it.
- **Capacity is one total, in units of any item.** Depots already work this way. If items ever need to differ ("5 stone but 20 wood"), add a size per item counted against the same total, not a cap per type. The methods hide which one is used.

### Holders

- `DepotState` keeps its id, position and production, and gets `inventory: Inventory`. `stock`, `out`, `incoming` and `capacity` go.
- `Carry` becomes `inventory` plus the job: `Carry { inventory, transfer_ticks, job: Option<CarryJob> }`.
- A `Pile` gets an `Inventory` with `OneKind` and `PILE_CAPACITY`. `material` and `units` go.
- The `Collector` gets an `Inventory` with `AnyMix`.
- `piles::Cargo` goes. A player's minerals are in its own `Carry` inventory, which is `AnyMix`: a player can carry wood and ore together.
- **A player's capacity** is the larger of today's two: the scenario's cargo capacity and the pile lab's `capacity`.

### Moving goods

- `InventoryRef` names a holder: `Depot(d)`, `Agent(a)`, `Pile(id)`, `Collector`. `Endpoint` becomes it, so a pile unit is a `Transfer` like a depot unit.
- `CarryJob { other, item, direction, remaining, next_tick }` is "move `remaining` units of `item` between my inventory and `other`". A job reserves on both sides when it starts: out on the source, in on the target.
- A hauler's assignment is two jobs: a load at the source, then an unload at the target. The unload is reserved when the assignment starts, as today. The hauler keeps its link and phase machine.
- Messages:
  - `ReserveStock { agent, job }` replaces `Reserve`, `TransferStock` and the reserving half of `Equip`. Stage 5 checks it again and rolls the job back if it no longer fits. (Built differently: a hauler's `Reserve` became `ReserveHaul`, and `Equip` stayed; see step 5's note.)
  - `CarryUnit { agent }` replaces `Load`, `Unload`, `ManualUnit` and `PileStep`. It moves one unit of the agent's job and launches it.
- `drop_carry(agent)` is the one way a job ends early. It releases what the job still holds reserved on both sides and clears it. Cancel, the `Respawn` action, a death, a timed-out pile walk and any future case call it. Units in the air still land.


### Why a haul reserves both ends

A haul link is a standing order, like "keep 30 wood at the sawmill, taken
from the forest depot". A hauler that takes a job on it reserves both ends
at once, before it walks to the source:

- **Source (`reserved_out`):** the units are promised, so no other hauler, recipe or player takes them.
- **Target (`reserved_in`):** the target counts them as already coming. That holds space, so the hauler never arrives at a full depot. It also answers demand: a link's demand is `target - (counts + reserved_in)` (`job_size` in `haulers.rs`). Without it, every idle hauler would see the full demand and grab a job for it, and goods would be over-delivered.

Keep this. Reserving the target only after loading would change hauler
rules and the golden file, and need a rule for a hauler holding cargo with
nowhere to put it.

## Rules and the golden file

- **Depots and haulers:** no rule change. The golden scenes have haulers and depots, but no player commands and no piles.
  - Keep each new message at the queue position and apply order of the one it replaces. A hauler's reservation is applied in the economy step, and a player's first, in `apply_actions`. Then the golden behaviour lines stay.
  - The totals move, because the state and message queues are hashed.
- **Players at depots:** the job now reserves the agent's side too (cargo space on a pickup). That's a rule change only if a player's cargo can fill up mid-transfer today. Check it, and if it can, make it its own commit.
- **Players at piles:** two rule changes from the decisions above, in their own commit: a player's capacity becomes the larger of the two, and a player can hold more than one mineral (and minerals with items).
- **Piles:** two more rule changes, last, each in its own commit:
  - pile units are reserved when a job starts, so the second of two players is refused at once instead of failing at the pile;
  - pile units fly like depot units (`TRANSFER_FLIGHT_TICKS`) instead of moving at once.
  - No golden scene has piles. The scene tests and app pile tests change.
- **Conservation:** `PileLab::conserved` becomes a total across inventories plus units in the air, checked by `validate`.

## Commit order

Step 5's order: digest, format, reading with the golden file unchanged,
gameplay last.

1. **`Inventory` type**, with methods and unit tests. Nothing uses it yet.
   - Done in step 1: `settlement/inventory.rs`, exported as `settlement::{Inventory, Kinds}`.
     - `take(item, units)` and `put(item, units)` move reserved units, and take a count, so a recipe's products land in one call.
     - `add` and `remove` change counts with no reservation: a source's new units, used-up ingredients, a unit sent off at once. `add` refuses past `free()` and a second kind; `remove` expects the units to be available.
     - `reserve_*` and `add` return `false` and change nothing when refused. Releases, `take` and `put` subtract plainly, like the code they replace.
     - `kind()` is the item held or reserved; `accepts(item)` applies `OneKind`. Capacity 0 holds nothing; use `u32::MAX` for no limit (the collector).
     - `validate()` returns a short reason (`overbooked`, `stock overbooked`, ...) for the caller to prefix with the holder.
2. **Depots onto `Inventory`.** No behaviour change: golden behaviour unchanged, save format up.
   - Done in step 2: `DepotState { id, position, inventory }`; its `total`, `free` and `available` went too, so callers use the inventory's. Save format 13 (master took 12 for `Accounting::abandoned`).
     - `behaviour_digest` hashes each depot in its old shape, `(id, position, capacity, counts, reserved_out, reserved_in)`, without `kinds`. That keeps every golden behaviour line; the tick totals moved, since `kinds` is in the state. Keep that projection when later steps change depots.
     - Reservations that were checked first now go through `reserve_*` with a `debug_assert!` on the result (equip, haul `Reserve`, `TransferStock`, a recipe's products). Sources and `add_stock` use `add`; ingredients and ammunition use `remove`.
     - `Settlement::validate` still recomputes the reservations, then calls `Inventory::validate` per depot. Its messages are the same as before.
3. **Agent cargo onto `Inventory`.** No behaviour change.
   - Done in step 3: `Carry { inventory, transfer_ticks }`. Save format 14. Golden behaviour lines unchanged (`behaviour_digest` hashes `inventory.counts` where it hashed `cargo`); tick totals moved.
     - `pending_in` maps one to one onto `reserved_in` per item: a unit is reserved in its item when it launches (`reserve_in`, with a `debug_assert!`) and `put` when it lands. Checks of `pending_in == 0` read `total_in() == 0`.
     - `pending_out` has no slot. A unit leaving a cargo bed leaves `counts` at launch (`remove`), so it is not `reserved_out` (that would break `available` and `validate`). Its `Transfer` (`from: Endpoint::Cargo(agent)`) is the only record, so `leaving_cargo` scans the transfers: `Settlement::leaving_cargo`, `View::leaving_cargo` and `Ctx::transfers` for the hauler's unloading check. A replay reads the confirmed settlement's transfers, as it read its cloned avatar's `pending_out` before; both are from the confirmed tick. Step 4's `CarryJob` reserves out on the cargo before the unit launches, and the unit stays a transfer in the air.
     - `Settlement::validate` now also recomputes each cargo's `reserved_in` from the transfers and checks it has no `reserved_out`.
     - For step 4: a player's cargo can't fill up mid-transfer today. Only its own depot job fills it, one job at a time, and the job checks the space when it starts. So reserving the agent's side is not a rule change.
4. **`CarryJob`, `ReserveStock`, `drop_carry` for the player's depot transfer.**
   - `ManualJob` and `TransferStock` go.
   - The hand-written releases in cancel, respawn and `hurt_player` become `drop_carry`.
   - Golden behaviour unchanged.
   - Done in step 4: save format 15. Every golden behaviour line unchanged; the settlement tick totals moved (the job moved from the avatar to `Carry`). `testdata/*.hashes` unchanged.
     - `Endpoint` is now `InventoryRef { Depot(d), Agent(a) }` (`Cargo` became `Agent`), since a job's `other` and a flying unit's ends name the same holders. Same variant order, so `behaviour_digest` (which hashes transfers) didn't move. Step 6 adds `Pile` and `Collector`.
     - `Carry { inventory, transfer_ticks, job: Option<CarryJob> }`, `CarryJob { other, item, direction: Direction::{Pickup, Deposit}, remaining, next_tick }` (`agent.rs`). `CarryJob::gives(Side::{Own, Other})` says which side is the source; `reserve(inventory, side)` and `release(inventory, side)` hold or give back `remaining` there: out on the source, in on the target.
     - **The agent's side is held in stage 1, by the action** (an action may write its own agent): `Transfer` reserves it with a `debug_assert!` (`transfer_limit` checked it) and sets `carry.job`. `ReserveStock { agent, job, change: StockChange::{Reserve, Release} }` carries only the other side to stage 5 (`Settlement::reserve_stock`, same queue position and order as `TransferStock`). Holding the cargo side in stage 5 instead would leave a cancelled job's room held until stage 5, so a cancel and a new transfer in one turn would be refused, and the replay (which never runs stage 5) would mispredict the next transfer.
     - Stage 5 now checks only the other side (`reserve` refuses), since the cargo side was checked and held when the action ran and nothing changes the cargo before `apply_actions`. A refused job rolls back through `drop_carry`, which gives back its cargo side; its other-side release is discarded, since nothing was held there.
     - `Carry::drop_carry(agent) -> Option<ReserveStock>` is the one early end: it takes the job, releases its own side, and returns the other side's release. `CancelTransfer` and `Respawn` push it to `ActionMessages::reserve_stock`; `hurt_player` applies it at once through `reserve_stock`.
     - Counting: on a pickup the cargo's `reserved_in` is the job's `remaining` plus units flying in; a launched unit keeps its room until it lands (`manual_unit` no longer reserves at launch). On a deposit the cargo's `reserved_out` is the job's `remaining`; a unit launches with `take`, so it leaves `counts` and `reserved_out` together and, in the air, holds nothing in the cargo (step 3's rule). `Settlement::validate` recomputes both per agent; no clash with how units in the air are counted.
     - `transfer_limit` uses the cargo's `available` for a deposit (was `counts`). With no job running the two are equal, so `act` decides as before; while a job runs, the app's limits now leave out what the job holds.
     - `ManualUnit` stays and reads `carry.job` (avatars only), with `InventoryRef` ends and `Settlement::position_of` for the dock. `Settlement::inventory_mut(holder)` resolves a holder.
     - **For step 5:** a hauler's `Reserve` happens in stage 5, so its handler must hold both sides there (`job.reserve(cargo, Side::Own)` and `Side::Other`) and set `carry.job`; `reserve_stock` assumes the own side is already held. Either give the hauler path its own function that calls the same `CarryJob` methods, or add the own side to `reserve_stock` behind a flag; that is the "Apply order" open question. `CarryUnit` replaces `ManualUnit` with `manual_unit`'s body (it is already written in `InventoryRef` terms). `validate` already counts any agent's `carry.job` on both sides, so remove the hauler-assignment terms as assignments become jobs. `app/src/legacy.rs` (`manual_jobs`) shows avatars' depot jobs only.
5. **Hauler load and unload onto carry jobs and `CarryUnit`.**
   - `Reserve`, `Load` and `Unload` go.
   - Golden behaviour unchanged; totals move.
   - Done in step 5: save format 16. All 16 golden behaviour lines unchanged (diffed); the tick totals moved. `testdata/*.hashes` unchanged.
     - **Two jobs:** `Carry` gained `next: Option<CarryJob>`. `Settlement::reserve_haul` sets the load as `carry.job` (held on both sides: out at the source, room in the cargo) and the unload as `carry.next` (held at the target only, `reserved_in`). The unload's own side (`reserved_out` in the cargo) can't be held yet, since its units aren't in the cargo; `Carry::start_next` holds it when loading ends (stage 2, the `Loading` branch: no job and `total_in() == 0`, the old `loaded == units` check). Keeping the pending job on `Carry`, not in `Assignment`, keeps `validate` generic: it counts every agent's `job` on both sides and its `next` on the other side, with no hauler term. `Assignment` keeps only `link` (for the docks); `units`, `loaded` and `unloaded` went, as did the hauler's per-unit `transfer_at` (now only the end of attaching; units use `CarryJob::next_tick`).
     - **`ReserveHaul`, not `ReserveStock`:** the hauler's request (`ReserveHaul { agent, link, units, route }`, at `Reserve`'s queue position) carries the route and the link, is sized again by `job_size` in stage 5 and may shrink, as before. A player's `ReserveStock` carries a fixed job, already held on its own side, refused whole. So `reserve_haul` is its own function that builds both jobs and holds every side through `CarryJob::reserve`. See "Apply order" in Decisions.
     - **`CarryUnit { agent }`** replaces `Load`, `Unload` and `ManualUnit`; `Settlement::carry_unit` (`economy.rs`) is `manual_unit`'s body without the avatar check. It has three queues, `hauler_load`, `hauler_unload` and `player_carry`, applied in that order where the three old ones were: each unit launched takes the next transfer ID, which `behaviour_digest` hashes, so one merged queue would renumber units. The hauler picks its queue by the job's direction. Per unit it does what `Load` and `Unload` did; the one difference is that the load's cargo room is held for the whole job at assignment instead of per unit at launch (`reserved_in` isn't in `behaviour_digest`).
     - `drop_carry` returns a `Vec<ReserveStock>`: the job's release and, if there is one, `next`'s (its other side only). Nothing calls it on a hauler yet (`todo/characters.md`, "Let a hauler's haul end early").
     - **Equipment stays as it was.** `Equip` reserves one unit out at the equipment depot and `Attach` takes it: the unit becomes a trailer, not cargo, and doesn't fly, so a pickup job would need a job with no own side and no flight, and making it fly would change the golden file. Equipment is out of this work's scope ("Scope"); `todo/characters.md`, "Move hauler equipment onto the carry path". `validate` keeps its `ToEquip` term.
     - **For step 6:** `CarryUnit` is the one unit path; give pile steps a `CarryUnit` queue of their own at `PileStep`'s position (after `player_carry`), so they keep their order against depot units. `InventoryRef` gets `Pile` and `Collector`: extend `inventory_mut`, `position_of`, `ReserveStock`'s `hash_into` and `arrivals`. `carry_unit` calls `metrics.picked += 1` on every pickup; decide whether pile pickups count. `validate`'s job loop counts only `InventoryRef::Depot` on the other side; piles will need their own `out`/`incoming` arrays there.
6. **Minerals into the item list; piles, pile cargo and the collector onto `Inventory`.**
   - Pile steps go through `CarryUnit`, still instant and unreserved.
   - The player keeps the lab's capacity and the one-mineral rule for minerals, checked in the pile job for now.
   - Pile tests unchanged.
7. **Gameplay:** one capacity per player (the larger of the two), and minerals mix in a player's cargo.
8. **Gameplay:** reserve pile units.
9. **Gameplay:** pile units fly.
10. **Remove what's left:** `piles::Cargo`, `PileStep`, `ManualUnit`, `PileLab::conserved`.

## Decisions

- **Scope:** stock only.
- **Capacity:** one total per inventory. A player's is the larger of the scenario's cargo capacity and the pile lab's.
- **Mixing:** a player's cargo holds any mix. Only piles are `OneKind`.
- **Hauler reservations:** both ends when the job starts, as today.
- **Apply order (step 5):** two messages in two queues, not one type applied at two points. A player's `ReserveStock` stays in `ActionMessages::reserve_stock`, applied first in stage 5 (`apply_actions`), and holds only the other side (the action held the cargo). A hauler's `ReserveHaul` is in `Messages::reserve_haul`, at the old `Reserve` position (after `retry` and `equip`), and holds every side of both its jobs. They are different types because the hauler's carries a route and a link and is sized again when it applies; both use `CarryJob::reserve`. Each keeps the queue position and apply order of the message it replaced, so the golden file holds. Likewise `CarryUnit` is one type in three queues (`hauler_load`, `hauler_unload`, `player_carry`), so units keep their transfer IDs.

## Open questions

- **Prediction:** a pile haul isn't predicted. Once it's an action (`todo/characters.md`, "Move pile hauls and piano keys onto actions"), its walk could be predicted like a depot `GoTo`. That's out of scope here.
- **Notices:** pile notices and errors are UI text in sim state. Keep them in the lab for now.

## Rules for the work

- Each numbered step is its own commit on branch `kestrel-inventory`, in order. Each commit builds and passes the tests.
- Bazel: `bazel test --config=remote ...` (see `AGENTS.md`).
- Golden file and hash files change only where the step explains it. Commits 1–6 keep every golden behaviour line.
- Bump the save `FORMAT_VERSION` whenever saved or hashed state changes.
- Things found but out of scope go in `kestrel/todo/`.
- When a step is done, add a "Done in step N" note under it here.
