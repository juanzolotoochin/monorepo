# Kestrel inventory (design)

Status: agreed direction, not built. Part of character step 5
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
- `OneKind` refuses a reservation or a put of a second item while any of the first is held or reserved. Piles and the player's minerals use it.
- `validate()` checks one inventory: counts and reservations within capacity, `OneKind` held. `Settlement::validate` calls it for every inventory and then checks the totals across them.
- `counts` per item is dense. That's fine: items are few, and holders are depots, agents, piles and the collector, not threats. If a scene has many items and many piles, switch to a small sorted list later; the methods hide it.

### Holders

- `DepotState` keeps its id, position and production, and gets `inventory: Inventory`. `stock`, `out`, `incoming` and `capacity` go.
- `Carry` becomes `inventory` plus the job: `Carry { inventory, transfer_ticks, job: Option<CarryJob> }`.
- A `Pile` gets an `Inventory` with `OneKind` and `PILE_CAPACITY`. `material` and `units` go.
- The `Collector` gets an `Inventory` with `AnyMix`.
- `piles::Cargo` goes. A player's minerals are in its own `Carry` inventory.

### Moving goods

- `InventoryRef` names a holder: `Depot(d)`, `Agent(a)`, `Pile(id)`, `Collector`. `Endpoint` becomes it, so a pile unit is a `Transfer` like a depot unit.
- `CarryJob { other, item, direction, remaining, next_tick }` is "move `remaining` units of `item` between my inventory and `other`". A job reserves on both sides when it starts: out on the source, in on the target.
- A hauler's assignment is two jobs: a load at the source, then an unload at the target. The unload is reserved when the assignment starts, as today. The hauler keeps its link and phase machine.
- Messages:
  - `ReserveStock { agent, job }` replaces `Reserve`, `TransferStock` and the reserving half of `Equip`. Stage 5 checks it again and rolls the job back if it no longer fits.
  - `CarryUnit { agent }` replaces `Load`, `Unload`, `ManualUnit` and `PileStep`. It moves one unit of the agent's job and launches it.
- `drop_carry(agent)` is the one way a job ends early. It releases what the job still holds reserved on both sides and clears it. Cancel, the `Respawn` action, a death, a timed-out pile walk and any future case call it. Units in the air still land.

## Rules and the golden file

- **Depots and haulers:** no rule change. The golden scenes have haulers and depots, but no player commands and no piles.
  - Keep each new message at the queue position and apply order of the one it replaces. A hauler's reservation is applied in the economy step, and a player's first, in `apply_actions`. Then the golden behaviour lines stay.
  - The totals move, because the state and message queues are hashed.
- **Players at depots:** the job now reserves the agent's side too (cargo space on a pickup). That's a rule change only if a player's cargo can fill up mid-transfer today. Check it, and if it can, make it its own commit.
- **Piles:** two rule changes, last, each in its own commit:
  - pile units are reserved when a job starts, so the second of two players is refused at once instead of failing at the pile;
  - pile units fly like depot units (`TRANSFER_FLIGHT_TICKS`) instead of moving at once.
  - No golden scene has piles. The scene tests and app pile tests change.
- **Conservation:** `PileLab::conserved` becomes a total across inventories plus units in the air, checked by `validate`.

## Commit order

Step 5's order: digest, format, reading with the golden file unchanged,
gameplay last.

1. **`Inventory` type**, with methods and unit tests. Nothing uses it yet.
2. **Depots onto `Inventory`.** No behaviour change: golden behaviour unchanged, save format up.
3. **Agent cargo onto `Inventory`.** No behaviour change.
4. **`CarryJob`, `ReserveStock`, `drop_carry` for the player's depot transfer.**
   - `ManualJob` and `TransferStock` go.
   - The hand-written releases in cancel, respawn and `hurt_player` become `drop_carry`.
   - Golden behaviour unchanged.
5. **Hauler load and unload onto carry jobs and `CarryUnit`.**
   - `Reserve`, `Load` and `Unload` go.
   - Golden behaviour unchanged; totals move.
6. **Minerals into the item list; piles, pile cargo and the collector onto `Inventory`.**
   - Pile steps go through `CarryUnit`, still instant and unreserved.
   - Pile tests unchanged.
7. **Gameplay:** reserve pile units.
8. **Gameplay:** pile units fly.
9. **Remove what's left:** `piles::Cargo`, `PileStep`, `ManualUnit`, `PileLab::conserved`.

## Open questions

- **A player's capacity:** today a player has the scenario's capacity for items and the lab's for minerals. With one inventory, what is it? Options:
  - the larger of the two;
  - a `carry.capacity` on the player character;
  - one capacity per kind (minerals, items), which makes `Inventory` bigger.
- **One kind per player:** keep `OneKind` only for minerals (a rule on items, not on the inventory), or let a player carry wood and ore together?
- **Two reservations per hauler job:** a haul reserves both ends at once. Keep that, or reserve the target only after loading? The second changes hauler rules and the golden file.
- **Apply order:** one message type applied at two points in stage 5 keeps the golden file, but it's easy to break. Two queues of the same type may be clearer.
- **Prediction:** a pile haul isn't predicted. Once it's an action (`todo/characters.md`, "Move pile hauls and piano keys onto actions"), its walk could be predicted like a depot `GoTo`. That's out of scope here.
- **Notices:** pile notices and errors are UI text in sim state. Keep them in the lab, or make them events?
