# Kestrel inventory (design)

Status: done (branch `kestrel-inventory`, steps 1–10). Part of character step 5
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
   - Done in step 6: save format 17. The golden file is byte-for-byte unchanged (`update_golden` rewrote it identically, totals included: no golden scene has piles, and the renamed queue is empty there). `testdata/*.hashes` unchanged.
     - **Items:** `Settlement::from_scene` adds `mine::MINERAL_IDS` (`amber_ore`, `blue_crystal`, `green_malachite`) after the scenario's items when the scene has piles (`add_items`, which resizes every inventory and `accounts`; a scenario item with a mineral's name is an error). `PileLab::first_mineral` is the first; `lab.item(mineral)` and `lab.mineral(item)` map at the boundaries: the scene's `PileSetup` (still `u8`, mapped in `PileLab::new`), `mine.ore` in `dig_housekeeping`, `Event::PileToss { material: u8 }` (unchanged, so its presentation is too), and the views (`pile_mineral`, `carried`, `collected`, `Settlement::carried_minerals`). No recorded command carries a mineral (`CommandKind::Pile` names piles and points; the automation's `Grab { material }` is checked against the app's own mirror), so `CommandKind` and the input log are unchanged. Minerals are accounted like items: initial piles are `initial`, a dispenser's and a dig's units `generated`, and `balances` counts piles and the collector as `stored`, so `validate`'s balance check covers minerals.
     - **Holders:** `Pile { position, inventory, source: Option<ItemIx> }` (`OneKind`, `PILE_CAPACITY`); `units()` and `item()` (held, else the dispenser's) replace `units` and `material`, so an empty dispenser pile still takes only its mineral. `Collector { position, inventory }` (`AnyMix`, `u32::MAX`). `piles::Cargo` and `Hauler::cargo` are gone; the lab's `Hauler` keeps the job, serial, `in_flight`, notice and error. `PileLab`'s transfer methods take the cargo `&mut Inventory` and move units with `remove` and `add` (no reservations).
     - **Two limits, as before.** A player's minerals sit in its `Carry` beside its depot items. `from_scene` sets each avatar's cargo capacity to the scenario's plus the lab's (joiners copy it), so the inventory never refuses either. Depot transfers keep the scenario's: `transfer_limit` takes depot items only (`Settlement::depot_items`, so no mineral goes to a depot; `add_stock` too) and its pickup room is `depot_room` (capacity minus the lab's, minus depot items held or coming). Minerals keep the lab's: `PileLab::pickup_amount` (and `pile_action`'s "carrier is full", and the pile job's sizing) check `lab.capacity - carried` and one mineral at a time, with `carried(cargo)` summing the mineral items only. `validate_lab` checks both per agent. Step 7 replaces this: capacity becomes the larger of the two, so drop the sum in `from_scene`, `depot_room`, `depot_cargo`, the lab-capacity checks and `pickup_amount`'s "Unload before collecting a different mineral", and the per-player checks in `validate_lab`. Note `carried` returns the first mineral found, which deposit, drop and deliver move; with mixed cargo they need the item named.
     - **`CarryUnit` in `pile_carry`:** `PileStep` is gone; stage 3 pushes `CarryUnit` into `Messages::pile_carry`, after `player_carry` where `pile_step` was, and `Settlement::pile_unit` (the old `pile_step` body) handles it. It doesn't use `carry_unit` or `Carry::job`: the pile job stays a `PileJob` in the lab, since it reserves nothing yet and `validate` counts a `Carry::job` as held on both sides. Keeping it off `Carry::job` also keeps `act`'s "busy" check, digging and `Release` as they were during a pile haul. `metrics.picked` keeps the old counting (pile pickups don't count): `pile_unit` never touches it.
     - **`InventoryRef`:** `Pile(PileId)` and `Collector`, last (derives `Ord` now). `inventory_mut` and `position_of` resolve them (both expect the pile to exist), `arrivals` puts a landed unit there, `ReserveStock::hash_into` writes variant 2 with the `u32` id and 3 alone (Depot and Agent hash as before). `app/src/legacy.rs` leaves pile and collector transfers out (the pile view draws them) and shows only depot items in an agent's cargo (`Settlement::depot_cargo`); the control API's transfer check counts depot items the same way.
     - **Conservation:** `PileLab::total(cargo)` and `conserved(cargo)` take every agent's cargo (`created` = piles + carried minerals + collector). `Settlement::validate` calls `validate_lab`: each pile's and the collector's `Inventory::validate`, their reservations against transfers to them and carry jobs on them (empty for now; written generically, so steps 8 and 9 only add jobs and transfers), mineral-only contents, the per-player limits above, and `conserved`. The scene tests' `conserved()` helper now runs `validate` too.
     - **For step 8:** a pile job becomes a `CarryJob { other: InventoryRef::Pile(id) | Collector, .. }` in `Carry::job`; then `act`'s "busy" check and `dig`/`Release`'s `carry.job.is_some()` start seeing pile hauls, so decide those. A `Drop` has no pile to reserve until its first unit lands (`PileJob::destination`). `validate_lab` already counts jobs on piles.
     - **For step 9:** a pile is removed when empty (`pickup_amount`); with units in the air it must stay while anything is reserved in or out there, or `inventory_mut`/`position_of` panic on landing. `Event::PileToss` and `Hauler::in_flight` then describe real transfers; `TossEnd` slots come from `carried`.
     - **For step 10:** `piles::Cargo` and `PileStep` are already gone.
7. **Gameplay:** one capacity per player (the larger of the two), and minerals mix in a player's cargo.
   - Done in step 7: save format 18. The golden file is unchanged (`update_golden` rewrote it identically: no golden scene has piles, and without piles a player's capacity is the scenario's as before). `testdata/*.hashes` unchanged.
     - **Capacity:** `from_scene` sets each avatar's cargo capacity to `max(scenario, lab)`; in both labs that is `max(8, 12) = 12` (step 6 had 20, split 8 + 12). `PileLab::capacity` is gone (and `PileLab::new`'s argument): nothing reads a lab capacity after setup. The inventory's `free()` is the only limit: `depot_room`, the lab-capacity checks in `pickup_amount`, `pile_action` and the pile job's sizing, the "Unload before collecting a different mineral" refusal and `validate_lab`'s per-player checks went. `transfer_limit` no longer takes the lab.
     - **Mixing:** `PileLab::carried` returns the mineral units only; `carried_by_mineral` and `first_carried` name them. A deposit moves the pile's mineral; a drop puts down the first mineral carried (piles are `OneKind`); a delivery takes every mineral, in mineral order (unit by unit at a pace). The pace-zero notice reads "Carrying total/capacity", wood included. A cargo's toss slots put minerals in mineral order (`cargo_slot`), as the backpack draws them.
     - **Views:** `Settlement::carried_minerals` returns units by mineral; `depot_cargo` returns the depot items only (its capacity is the cargo's). The app's `pile_model::Cargo` holds `(Mineral, units)` in mineral order and its capacity is the avatar's cargo capacity; the backpack, the HUD and the capacity ticks colour each unit by its mineral. Clicking (or selecting) a pile of a mineral the player carries deposits it; any other pile is collected, beside the cargo. The control API's `DropAll` and `DepositAll` expect what is left after the one mineral they move, and `Grab` no longer refuses a second mineral.
     - Tests: `a_cargo_mixes_minerals_within_its_capacity` (lab) and `a_player_carries_wood_and_minerals_within_one_capacity` (scene: wood and ore together, a pickup capped by the one total, a second mineral accepted, and a full cargo refusing both). `every_avatar_carries_its_own_load` and the app's `another_mineral_is_collected_beside_the_cargo` replace their one-mineral assertions.
8. **Gameplay:** reserve pile units.
   - Done in step 8: save format 19. The golden file is unchanged (`update_golden` rewrote it identically; no golden scene has piles). `testdata/*.hashes` unchanged.
     - **The haul is a carry job.** `pile_action` (a command, applied in stage 1, which writes the lab directly as before) sizes the haul with `PileLab::plan`, which replaces `pickup`, `pickup_amount`, `deposit`, `deposit_amount`, `drop_new`, `drop_new_amount` and `deliver_amount`, and sets `Carry::job` to a `CarryJob { other: Pile(id) | Collector, .. }` held on both sides at once (out on the source, room on the target). `plan` counts only units not promised (`available`) and room not held (`free`), so the second of two players going for the same units gets what is left or is refused with "No units available" when the command applies. The lab's `PileJob` keeps the walk, the pace and the counts shown; `remaining` became `started`, and `next_toss` and `destination` went (the carry job's `next_tick` and `other` hold them).
     - **Sized at the command, not on arrival.** A pickup takes what was free when it was ordered: units a dispenser adds while the player walks stay on the pile. That is what "holds its units when it starts" means; the old arrival-time sizing let two players count the same units.
     - **A drop makes its pile at the command**, empty, holding room for the drop (`reserved_in`), since a job needs its other side. `plan` checks the pile limit and the deck then. Its id is taken then too, so later piles number as before when nothing else is made meanwhile.
     - **Ending early is `drop_carry`.** `end_pile_job` (cancel by `DropHaul`, death, and now the walk timeout and route failures in `pile_housekeeping`, which used `Hauler::fail`) ends the lab job with its notice and error, then `drop_pile_carry` drops the carry job and applies both releases at once (`reserve_stock`, now `pub(super)`). `Respawn` drops it in the action (`drop_carry`, as for a depot transfer) and `hurt_player` already did. A new haul replaces a running one the same way. The serial, notices and errors are as before.
     - **Units still move at once**, in `pile_unit`, out of what the job holds (`take` at the source, `put` at the target, one unit or all at pace zero). Its toss events are as before. A delivery delivers every mineral: when one mineral's job ends, `next_delivery` starts the next.
     - **Empty piles** go in `pile_housekeeping` (`PileLab::prune`), not when the last unit leaves: a pile stays while it holds anything or anything is held there (an empty drop pile's room) or a transfer starts or ends there (none yet; step 9). Removal moved from stage 5 to stage 6 of the same tick, so no command sees the difference. `mined` and a dispenser's `add` now leave room held for units on their way.
     - **Busy, digging, Release and cancel** keep their player-facing behaviour by looking at depot transfers only (`act`'s `transferring`): a pile haul doesn't stop a dig, doesn't keep a released route (`view.hauling()` decides that, as before), and `CancelTransfer` doesn't end it. `read_avatar`'s dock check skips hauls; `read_pile_job` reads the carry job's `next_tick`. One change: a depot `Transfer` is refused while a pile haul runs, and a pile haul while a depot transfer runs ("Finish the depot transfer before hauling"), since an agent has one carry job. Before, the two could run at once; no shipped scene has both depots and piles.
     - Tests: `hauls_hold_their_units_when_they_start` (lab: the second haul gets what is left, the third is refused, a release offers the units again, a drop's pile holds its room), `two_players_hold_pile_units_from_the_command` (scene: refused at once, released by a move and taken by the other player, a smaller second share, `validate` clean throughout) and `a_timed_out_haul_releases_its_units`. The lab tests' helpers go through `plan` and the reservations.
9. **Gameplay:** pile units fly.
   - Done in step 9: save format stays 19. Nothing saved changed shape: transfers already took `Pile` and `Collector` ends, and `arrivals` already put a unit there; a step 8 save loads and plays on under the new rule. The golden file is unchanged (`update_golden` rewrote it identically). `testdata/*.hashes` unchanged.
     - **A unit is a `Transfer`.** `pile_unit_launch` does what `carry_unit` does for a depot unit: `take` at the source (its share of the job's hold), then `launch` (`TRANSFER_FLIGHT_TICKS`, 0.35 s), the target's room passing from the job to the unit until `arrivals` puts it. At pace zero every unit launches at once. The toss used to last 0.85 × the pace (0.3–1.0 s) and was only shown; now it is the flight, and every unit has one, at pace zero too.
     - **The haul ends when the last unit lands**, at any pace: `pile_unit` keeps the lab job, and `pile_housekeeping` ends it once the carry job is done and `in_flight` (now the real landing ticks) is empty. Its notice is then the pace-zero summary ("Collected 8 units. Carrying 8/12.", from `done_notice`) or "Transferred N units. All items landed." as before. So a script or test that waits for the job to end sees the cargo landed.
     - **Piles stay while units fly** from or to them: `prune` already kept a pile any transfer names. An emptied pile goes in the housekeeping of the tick its last unit lands.
     - **Toss slots** say where the unit will land: after what the holder has and the units of that item already flying there (`toss_end`).
     - **Conservation:** `PileLab::total` and `conserved` take the transfers and count minerals in the air. `balances` already counted them (`in_transit`).
     - **App:** the pile view draws a pile or backpack from its counts, which leave out units in the air, and the toss (`Event::PileToss`, a `Transit` for `ticks`) draws the unit in the air; depot units stay drawn from `transfers` by the settlement view, which leaves pile and collector ends out (`legacy.rs`), so nothing is drawn twice. `render` hides a landing slot while its toss is still running, so a unit that has landed is not drawn in the pile and in the air at once; in a backpack, whose slots shift as minerals mix, a slot is hidden only while it holds the flying unit's mineral (`Transit::material`). `Stock::elsewhere` counts minerals in the air, so the control API's `conserved` holds mid-flight.
     - Tests: `pile_units_fly_to_the_cargo` (all units in the air at once, none in the cargo, room held, the emptied pile kept, `validate` and the totals clean mid-flight; then landed, pile gone, notice). The other pile tests wait for the haul to end and pass unchanged.
10. **Remove what's left:** `piles::Cargo`, `PileStep`, `ManualUnit`, `PileLab::conserved`.
   - Done in step 10: no behaviour change; save format stays 19 (nothing saved changed). The golden file is unchanged (`update_golden` rewrote it identically). `testdata/*.hashes` unchanged.
     - `ManualUnit`, `piles::Cargo` and `PileStep` were already gone (steps 5 and 6). `PileLab::total` and `PileLab::conserved` went: `validate_lab` no longer checks them, since minerals are items accounted like any other (`balances`: piles and the collector `stored`, cargo `carried`, units in the air `in_transit`, against `initial`, `generated`), which `validate` checks first. `PileLab::created` stays: the pile view shows it ("CREATED", "MINED"). The scene tests' `total()` reads the minerals' balances; the lab unit tests count their own piles, cargo and collector.
     - Stale comments on `CarryUnit` and the `pile_carry` queue ("moved at once") now describe the flight.
     - Nothing else was left over: the lab's old transfer methods went in step 8 (`plan`), `depot_room` and the lab capacity in step 7, `Hauler::fail` in step 8.

## Decisions

- **Scope:** stock only.
- **Capacity:** one total per inventory. A player's is the larger of the scenario's cargo capacity and the pile lab's.
- **Mixing:** a player's cargo holds any mix. Only piles are `OneKind`.
- **Mixed cargo at piles (step 7):** `PileAction` names no mineral, and the input log stays as it is, so a deposit moves the pile's mineral, a drop the first mineral carried (in mineral order), and a delivery all of them. A pile of a mineral not carried is collected when clicked; one of a mineral carried takes it back (Ctrl collects more).
- **Hauler reservations:** both ends when the job starts, as today.
- **Pile hauls are carry jobs (step 8):** sized and held when the command applies; a drop's pile is made then, empty; an agent runs one carry job at a time, so a depot transfer and a pile haul refuse each other; `CancelTransfer` ends only depot transfers; empty piles go in housekeeping.
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
