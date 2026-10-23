# Gameplay simulation core

This crate has no renderer, window, audio, or Bevy dependency. `Scenario` data
supplies arbitrary item IDs, depots, sources, prioritized haul links, workers,
production recipes, equipment upgrades, defenders, and threat waves.

`Simulation::advance` advances a deterministic 60 Hz simulation independently
of render cadence. Live callers can pause, change rate, enable/disable links,
machines and defenders, and add exact stock to unreserved depot space. Public
state and balances support programmatic inspection; events describe transfers,
production, equipment, shots, impacts, and breaches.

Haul assignments reserve source stock and destination capacity. Machines reserve
output capacity while processing and respect workers' stock reservations.
Transfers own one unit in flight until arrival: the material cannot be consumed
or counted again while airborne. Loading/unloading has explicit cadence, and
workers wait for transfers before departing. Dock ownership keeps concurrent
workers from transferring at the same station simultaneously.

Equipment moves a real item from a depot onto a carrier, changes its capacity
and speed, and creates a trailing transport pose. The trailer follows its hitch
rather than rotating rigidly through corners. Recipes consume explicit inputs
and produce explicit outputs. Defenders spend real ammunition and launch
projectiles that apply damage on arrival; no ammunition means no shots.

For each material, `balances()` checks:

```
initial + generated + produced
  == stored + carried + in_transit + equipped + consumed + spent
```

`validate_state` additionally checks source reservations, destination capacity,
and carrier capacity. Disabling a link stops new work while committed transfers
complete. Scenario validation rejects malformed dimensions, unknown references,
and duplicate IDs/stacks. Navigation is the same deterministic clearance-aware
A* grid used by the game's player adapter, with a per-agent body radius.

```sh
bazel test --config=full_local //game/core:core_test
```

Controlled actors use `controlled: true` with no automatic haul links. An
external movement controller supplies their validated pose. `transfer` reserves
an exact pickup/deposit and waits for the actor to reach the dock; unit transfers
then use the shared cadence and airborne ownership model. `cancel_transfer`
releases unlaunched reservations, preserving items already in flight. `strike`
applies a bounded forward melee strike to the same threats used by defenders.
Producer and defender `radius` fields supply physical navigation footprints.

Subsystems live under `src/model/`: workers, production, combat and controlled
interaction. `model.rs` retains the fixed-step coordinator, state, shared
inventory/reservation primitives and accounting invariants.

`Scenario.extent` defaults to the legacy eight-meter half-width and supports up
to 24 meters. The shared A* grid sizes itself to the requested bounds; the game
adapter uses the same extent for player pathfinding and collision boundaries.

Carriers steer around nearby agent and trailer footprints with bounded local
candidate steps. Idle haulers return to their starting positions so they do not
occupy unloading docks. Equipped carriers plan forward turning arcs for sharp
departures, validating both the body and trailer sweep against static obstacles;
a blocked departure waits for a valid route rather than unloading remotely.
`avoidance_checks`, `avoidance_steers`, and `traffic_wait_ticks` expose crowd cost
and congestion. `Equipment.radius` describes the trailer clearance footprint.

Visible packing is data: `CargoLayout` specifies columns, rows, XYZ spacing,
unit scale and roll. Agents and equipment have separate cargo layouts; depots
have `stock_anchor` and `stock_layout`, allowing floor piles and backpack bundles
to use different packing while preserving one visible item per inventory unit.
Producer and defender descriptions are optional authoring metadata. Production
status is shared by telemetry and hover inspection through `production_status`.
Dock reservations include the approach and departure envelope. Queued workers
step back when a larger carrier needs turning room, and the reservation releases
only after both carrier and trailer clear the dock. `deliveries_by_agent` makes
individual transport progress inspectable instead of hiding a stalled carrier
behind aggregate throughput.
