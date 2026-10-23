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
bazel test --config=full_local //tools/gameplay/core:core_test
```
