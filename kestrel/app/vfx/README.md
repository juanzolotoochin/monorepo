# Effect files

One RON file per visual effect, drawn by our fork of sprinkles
(`kestrel/vfx/sprinkles`). This is the first version of the format, from the
VFX spike (`kestrel/VFX_SPIKE_HANDOFF.md`); three effects use it so far
(see "Files"). The loader is `app/src/vfx.rs`.

**Experimental.** The sprinkles backend is off by default (`--vfx legacy`)
and isn't yet up to par with the legacy effects in `app/src/effects.rs`.
Until it is, new effects can still be written in the legacy backend; port
them here later along with the others (`kestrel/todo/vfx.md`). The bar for
"up to par" is the "Remove the legacy effect path" task.

## Where and why here

The files live with the app, not in `kestrel/content/data/`: `content` is
read by the simulation, and effects are view-only. Nothing in an effect file
may change what the simulation computes or hashes. The file name is the
effect's ID (`impact_stone.ron` is `"impact_stone"`). Files are compiled in
with `include_str!`: list each one in `vfx::FILES` (a test fails when the
list and the folder differ). A bad file fails the
`vfx::tests::every_effect_file_parses` test and panics at startup. The
loader is `vfx::catalog::load`.

## Format

```ron
(
    version: 1,
    base: "impact_stone", // optional
    particles: ( /* a sprinkles ParticlesAsset, sprinkles_version: "0.3" */ ),
    lights: [
        (color: (1.0, 0.38, 0.08), intensity: 18000.0, range: 4.5, radius: 0.15,
         offset: 0.2, fade_seconds: 0.1),
    ],
)
```

- `version`: our format version, `1`.
- `base`: optional, another effect's ID to start from (see "Base").
- `particles`: the particle layer, sprinkles' own `ParticlesAsset` schema
  embedded as is (see `kestrel/vfx/sprinkles/src/asset/mod.rs`). It must set
  `despawn_on_finish: true`, and every emitter `one_shot: true`: effects are
  bursts that end by themselves.
- `lights`: point lights spawned with the particles, because GPU particles
  can't light the scene. `intensity` in lumens at the start, `offset` in
  meters along the normal. The light fades out quadratically over
  `fade_seconds`, then despawns.

Colors are **sRGB** in both layers, like every authored color in Kestrel:
particle colors (`base_color`, `emissive`, `specular_tint`,
`attenuation_color`, `initial_color`, gradient stops) and light colors.
The loader converts particle colors to the linear RGBA sprinkles takes
(sprinkles' doc comments say linear: that is the loaded asset, not the
file). Alpha is not converted. Glowing colors go above 1 (`emissive` for
bloom) and convert with the same curve: linear 12 is sRGB 2.9161.

Unknown or misspelled fields are errors, in our layer and in the sprinkles
asset (the fork denies unknown fields).

## Base

A file with `base: "<effect ID>"` starts from that effect and overrides what
it writes, like character files (`docs/character-definition.md`):

```ron
// impact_metal.ron: stone's impact, with blue sparks and no debris.
(
    version: 1,
    base: "impact_stone",
    particles: (emitters: [
        (name: "sparks", draw_pass: (material: Standard((base_color: (0.5, 0.7, 1.0, 1.0))))),
        (name: "debris", enabled: false),
    ]),
)
```

- Sections (`(field: ...)`) merge field by field, at any depth: a field the
  variant leaves out comes from the base.
- Lists replace the base's whole list (`lights`, curve `points`, gradient
  `stops`), except `particles.emitters`.
- `particles.emitters` merge by `name`: an emitter whose name matches one in
  the base merges into it field by field; a new name is appended, after the
  base's emitters, in the variant's order. Turn a base emitter off with
  `enabled: false`.
- A value with another struct or variant name replaces the base's
  (`shape: Sphere(radius: 0.5)` over `Box(...)`); with the same name it
  merges (`Standard((...))`, `Some((...))`).
- Unknown bases, cycles, and a value of another shape than the base's (a
  list over a section) are errors that name the file and the field. A base
  can have its own base, and stays a usable effect itself.

The merged file is checked as strictly as any other. Its parse errors point
into the merged text, not the variant's (the message says so).

## Placement

An effect is triggered with a `vfx::Trigger` (`vfx::spawn`): a position and
a normal, plus optional per-trigger parameters (below). The effect's local
+Y points along the normal, so emitters aim with
`initial_direction: (0.0, 1.0, 0.0)`. Gravity, collisions and the floor are
in world space. Every scene gets a floor collider at y = 0 for particles to
bounce on (`ParticlesCollider3D` box; emitters opt in with `collision`).

`vfx::spawn` may be called before transform propagation (in `Update`) or
after it, before `bevy_sprinkles::SprinklesLateSetup` (in `PostUpdate`, for
example at a posed weapon's muzzle); either way the effect draws that frame.

## Trigger parameters

Values the game gives when it triggers an effect, not stored in the file.
The file says which emitters use them; they are our fork's additions to the
sprinkles schema.

| Trigger field | Meaning | Used by |
|---|---|---|
| `target` | A world-space point the effect reaches for. | `Segment` shape, `particles_per_meter`, `lifetime_to_target` |
| `seed` | Every random choice (direction, size, lifetime, jitter). The same seed always draws the same burst. Each emitter mixes in its index. Without one, random per spawn (or the emitter's `fixed_seed`). | every emitter |
| `scale` | Uniform size: positions, speeds, gravity and particle sizes (not lifetimes, not lights' range). Default 1. | every emitter |

Emitter fields that read them:

- `emission.shape: Segment(jitter: (x, y, z))`: particles spread evenly along
  the line from the emitter to the target, particle `i` of `n` at
  `(i + 0.5) / n`, each moved by a random offset within `jitter`
  (half-extents, local space). Ignores `emission.offset` and `scale`.
  Without a target, a point.
- `emission.particles_per_meter: Some(9.0)`: with a target, a burst spawns
  `ceil(distance × 9)` particles, at least one and at most
  `particles_amount` (the cap; buffers are sized for it).
- `time.lifetime_to_target: true`: each particle lives until it has
  traveled from where it spawned to the target at its initial speed (a
  bolt dies on arrival). `lifetime` is the cap. The burst ends after the
  emitter-to-target trip at the slowest `initial_velocity`.
- `velocities.world_bias: (x, y, z)`: a world-space vector added to every
  spawn direction before it is normalized again; `(0.0, 0.46, 0.0)` makes a
  spray lean upward however the effect is placed. Not a trigger value, but
  world space.

Gameplay timing stays in Rust. The laser's `effects::LaserBolt` still
decides when a shot arrives (`CoverContacts`, the impact); `laser_bolt.ron`
only draws it, and its arrival matches because both use the same speed.

## Files

- `impact_stone.ron`: a blade or dig hitting stone (`effects::impact`).
  Seeded by the impact's number.
- `arc.ron`: the arc coil's discharge (`effects::arc`): `Segment` from the
  coil to the target, 9 cubes per meter.
- `laser_bolt.ron`: the laser's bolt (`effects::emit_lasers`): one bar at
  80 m/s with `lifetime_to_target`, placed at the muzzle with +Y toward the
  target.

Small particles go without shadows: set `draw_pass.shadow_caster: false`
(sprinkles defaults to `true`) on sparks, chips and the like, as
`impact_stone.ron` does. Turn shadows on for an emitter only where they
read better (large or slow pieces); see "Revisit particle shadows per
effect" in `kestrel/todo/vfx.md`.

The fork also applies `draw_pass.visibility_aabb` (upstream ignored it):
the emitter is culled by that box around the emitter, so effects reaching
far (arcs, bolts) need a box that big.

## Trying it

`--vfx sprinkles` (or Effects → VFX backend in the debug menu, `/`) draws
effects from these files; `--vfx legacy` (the default) draws the
hand-written ones in `effects.rs`. Runs without GPU particles (simulation,
the web build for now) always use the legacy effects.

Effects → **Reload effect files** rereads this folder from the source tree,
in runs started with `bazel run` (it finds the folder through
`BUILD_WORKSPACE_DIRECTORY`; otherwise the button is off and says why).
Effects that load replace the running ones in place, and their pools are
rebuilt. A file that fails keeps its last good version; its error shows in
the window and in the log. New files appear; an effect whose file was
deleted stays loaded until a restart. Add new files to `vfx::FILES` too, or
the next start won't have them.

Each effect in the window has a **Fire** button: with the sprinkles
backend, it triggers the effect 3 m in front of the player, 0.5 m up with
the normal up, and a target 4 m further along the player's facing. An
effect that flies to its target (`lifetime_to_target`, the laser) is
placed with +Y toward it instead.

## Planned

Decals and a schema doc like `docs/character-definition.md`;
see `kestrel/todo/vfx.md`.
