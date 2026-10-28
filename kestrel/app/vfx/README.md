# Effect files

One RON file per visual effect, drawn by our fork of sprinkles
(`kestrel/vfx/sprinkles`). The file name is the effect's ID
(`impact_stone.ron` is `"impact_stone"`). The loader is `app/src/vfx.rs`.

**The format, its fields and units, `base` inheritance, trigger parameters,
pitfalls and how to add and try an effect are in
[`docs/effect-definition.md`](../../docs/effect-definition.md).**

**Experimental.** The sprinkles backend is off by default (`--vfx legacy`)
and isn't yet up to par with the legacy effects in `app/src/effects.rs`.
Until it is, new effects can still be written in the legacy backend; port
them here later along with the others (`kestrel/todo/vfx.md`). The bar for
"up to par" is the "Remove the legacy effect path" task.

The files live with the app, not in `kestrel/content/data/`: `content` is
read by the simulation, and effects are view-only. List every file in
`vfx::FILES` (a test fails when the list and the folder differ).

## Files

- `impact_stone.ron`: a blade or dig hitting stone (`effects::impact`).
  Seeded by the impact's number.
- `impact_metal.ron`, `impact_crystal.ron`, `impact_polymer.ron`: the
  other `ImpactResponse` materials, each `base: "impact_stone"` with the
  numbers from `impact::response`. Metal is also the laser's arrival
  impact. Polymer has no sparks, flash or light.
- `arc.ron`: the arc coil's discharge (`effects::arc`): `Segment` from the
  coil to the target, 9 cubes per meter.
- `laser_bolt.ron`: the laser's bolt (`effects::emit_lasers`): one bar at
  80 m/s with `lifetime_to_target`, placed at the muzzle with +Y toward the
  target. `effects::LaserBolt` still decides when the shot arrives; both
  use the same speed.
