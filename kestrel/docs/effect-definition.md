# Effect definitions

An effect definition is one visual effect: a burst of particles and the lights that go with it, such as sparks off a rock or an arc coil's discharge. The game triggers it by name at a point; the file says what it looks like. The file name is the effect's ID: `kestrel/app/vfx/impact_stone.ron` defines `impact_stone`.

Effects are drawn by our fork of the [sprinkles](https://github.com/doceazedo/sprinkles) GPU particle library (`kestrel/vfx/sprinkles`), when the run uses the sprinkles backend (`--vfx sprinkles`). The default is still the hand-written effects in `kestrel/app/src/effects.rs`; see "Trying an effect" below. The loader is `kestrel/app/src/vfx.rs` and `vfx/catalog.rs`.

Effects are view-only. They live with the app, not in `kestrel/content/data/`, because nothing in them may change what the simulation computes or hashes. Gameplay timing stays in Rust: when a laser shot arrives, how far a struck rock kicks, hit-stop and screen shake.

## Start with one complete file

This is `impact_stone`, a blade hitting stone, trimmed to two of its three emitters:

```ron
(
    version: 1,
    particles: (
        sprinkles_version: "0.3",
        name: "impact_stone",
        dimension: D3,
        despawn_on_finish: true,
        emitters: [
            (
                name: "sparks",
                initial_transform: (translation: (0.0, 0.06, 0.0)),
                time: (lifetime: 0.8, lifetime_randomness: 0.65, one_shot: true, explosiveness: 1.0),
                draw_pass: (
                    mesh: Cuboid(half_size: (0.0225, 0.15, 0.0225)),
                    material: Standard((
                        base_color: (1.0, 0.38, 0.08, 1.0),
                        emissive: (2.9161, 1.9303, 0.9822, 1.0),
                    )),
                    transform_align: Some(YToVelocity),
                    shadow_caster: false,
                    visibility_aabb: (center: (0.0, 0.0, 0.0), half_extents: (4.0, 4.0, 4.0)),
                ),
                emission: (particles_amount: 12),
                scale: (range: (min: 0.67, max: 1.33), scale_over_lifetime: Some((
                    name: Some("Shrink"),
                    x: (points: [
                        (position: 0.0, value: 1.0, mode: SingleCurve),
                        (position: 0.5, value: 0.64, mode: SingleCurve),
                        (position: 1.0, value: 0.0, mode: SingleCurve),
                    ], range: (min: 0.0, max: 1.0)),
                ))),
                velocities: (
                    initial_direction: (0.0, 1.0, 0.0),
                    spread: 45.0,
                    initial_velocity: (min: 2.5, max: 7.5),
                    world_bias: (0.0, 0.46, 0.0),
                ),
                accelerations: (gravity: (0.0, -14.0, 0.0)),
                collision: (mode: Some(Rigid(friction: 0.35, bounce: 0.32)), base_size: 0.09),
            ),
            (
                name: "flash",
                time: (lifetime: 0.065, one_shot: true, explosiveness: 1.0),
                draw_pass: (
                    mesh: Cuboid(half_size: (0.08, 0.08, 0.08)),
                    material: Standard((
                        base_color: (1.0, 0.38, 0.08, 1.0),
                        emissive: (2.9161, 1.9303, 0.9822, 1.0),
                    )),
                ),
                emission: (particles_amount: 1),
                velocities: (spread: 0.0),
                accelerations: (gravity: (0.0, 0.0, 0.0)),
            ),
        ],
    ),
    lights: [
        (color: (1.0, 0.38, 0.08), intensity: 18000.0, range: 4.5, radius: 0.15, offset: 0.2, fade_seconds: 0.1),
    ],
)
```

Read it from the outside in:

1. The outer `(...)` is one **effect file**: our format, `version: 1`. It has two layers, `particles` and `lights`, and optionally a `base`.
2. `particles` is the **particle layer**: a sprinkles particle system, in sprinkles' own schema. It has a list of emitters, and `despawn_on_finish: true` removes the whole effect once every emitter is done.
3. Each **emitter** is one kind of particle. `sparks` fires 12 thin glowing bars at once, 2.5 to 7.5 m/s in a 45° cone around the surface normal, leaning upward. They fall at 14 m/s², bounce on the floor, shrink to nothing and are gone within 0.8 s. `flash` is one hot cube at the contact for about four frames.
4. `lights` is the **light layer**: a point light at the contact, 0.2 m off the surface, fading out in 0.1 s. GPU particles can't light the scene, so lights are their own layer.

The effect is placed at the contact point with its local +Y along the surface normal: emitters aim with `initial_direction: (0.0, 1.0, 0.0)`. Gravity, `world_bias`, `world_velocity` and the floor are in world space.

The loader rejects unknown fields, in our layers and in the sprinkles asset. A field left out takes sprinkles' default (listed below where it matters), or comes from the `base`.

## The file schema

| Outer field | Type | Meaning |
| --- | --- | --- |
| `version` | `1` | Our format version. |
| `base` | Optional effect ID string | Start from another effect, then override the fields written here. See "Variants reuse an effect". |
| `particles` | Section | The particle layer: a sprinkles `ParticlesAsset` (`kestrel/vfx/sprinkles/src/asset/mod.rs`). |
| `lights` | List of lights, default empty | Point lights spawned with the particles. |

`particles` holds `sprinkles_version: "0.3"` (the fork's schema version; another version is an error), `name`, `dimension: D3`, `despawn_on_finish: true` (required) and `emitters`.

Each light in `lights`:

| Light field | Unit | Meaning |
| --- | --- | --- |
| `color` | sRGB `(r, g, b)` | The light's color. |
| `intensity` | lumens | Brightness at the start. |
| `range` | meters | How far the light reaches. |
| `radius` | meters | The light's size, for soft highlights. |
| `offset` | meters | Distance from the trigger point along the normal. Default 0. |
| `fade_seconds` | seconds | The intensity falls to zero as `(1 - t / fade)²`, then the light despawns. |

## Emitter fields we use

An emitter has many more fields than these; the fork's doc comments in `kestrel/vfx/sprinkles/src/asset/` describe all of them. These are the ones our effects use, with their units:

| Field | Unit | Meaning |
| --- | --- | --- |
| `name` | | Identifies the emitter, and is how a variant finds it. |
| `enabled` | | `false` turns a base's emitter off in a variant. Default `true`. |
| `initial_transform.translation` | meters, local | Where the emitter sits relative to the trigger point; +Y is the normal. |
| `time.lifetime` | seconds | How long a particle lives. Default 1. |
| `time.lifetime_randomness` | ratio 0–1 | Each particle lives `lifetime × (1 − random × this)`: 0.65 with 0.8 s gives 0.28 to 0.8 s. |
| `time.one_shot` | | Must be `true`: one burst, then the emitter ends. |
| `time.explosiveness` | ratio 0–1 | 1 emits every particle at once; our bursts use 1. |
| `time.lifetime_to_target` | | Ours: see "Trigger parameters". |
| `emission.particles_amount` | count | Particles per burst. Default 8. Pools are sized by it. |
| `emission.shape` | | `Point` (default), `Sphere(radius)`, `Box(extents: (x, y, z))` (half-extents, local; `blood_splat` uses `(0.55, 0.0, 0.55)` for a flat patch), ... or ours, `Segment(jitter)`. |
| `emission.particles_per_meter` | per meter | Ours: see "Trigger parameters". |
| `draw_pass.mesh` | meters | The particle's shape: `Cuboid(half_size: (x, y, z))` is a box twice that size. |
| `draw_pass.material` | | `Standard((...))`: Bevy's PBR material. We use `base_color`, `emissive`, `perceptual_roughness` (default 0.5), `metallic` (default 0) and `unlit`. |
| `draw_pass.transform_align` | | `Some(YToVelocity)` turns the mesh's local +Y along the particle's flight. Default none. |
| `draw_pass.shadow_caster` | | Default `true`. Set `false` on small particles (see "Pitfalls"). |
| `draw_pass.visibility_aabb` | meters, local | The whole emitter is culled when this box around it is off screen; make it as big as the effect reaches. |
| `scale.range` | multiplier | Each particle's size, a random factor of the mesh between `min` and `max`. Default 1. |
| `scale.scale_over_lifetime` | curve | Multiplies the size over the particle's life (see "Pitfalls"). |
| `velocities.initial_direction` | unit vector, local | The cone's axis. Default +X, so set it. |
| `velocities.spread` | degrees | The cone's half-angle around `initial_direction`. Default 45. |
| `velocities.initial_velocity` | m/s | A random speed between `min` and `max`. Default 0. |
| `velocities.world_bias` | world vector | Ours: added to each spawn direction, which is normalized again. `(0.0, 0.46, 0.0)` makes a spray lean upward however the effect is placed. The speed doesn't change. |
| `velocities.world_velocity` | m/s, world vector | Ours: added to each particle's spawn velocity after the direction, `world_bias` and the speed. `(0.0, 1.5, 0.0)` lifts every particle 1.5 m/s however the effect is placed (blood). Not scaled by the trigger's `scale`. Default zero. |
| `velocities.even_ring` | | Ours: instead of a random direction in the cone, particle `i` of the `n` spawned leaves exactly `spread` degrees off `initial_direction`, at `360° × i / n` around it, the same for every seed; `flatness` is ignored. `spread: 90.0` is a flat ring (the trial burst). Default `false`. |
| `accelerations.gravity` | m/s², world | Default `(0.0, -9.8, 0.0)`; set it, the legacy effects use 14 or none. |
| `collision.mode` | | `Some(Rigid(friction, bounce))` bounces on the floor: `bounce` is the share of the normal speed kept, `friction` the share of the sliding speed lost. Default none: particles fall through. |
| `collision.base_size` | meters | The particle's collision diameter: 0.09 rests the particle's center 4.5 cm above the floor. |

Every scene gets a floor collider at y = 0 for particles to bounce on. Dug pits and raised decks aren't modeled.

## Trigger parameters

Game code triggers an effect with a `vfx::Trigger` (`kestrel/app/src/vfx.rs`) and `vfx::spawn`. The trigger's values come from the game, not the file; the file says which emitters use them.

| Trigger field | Meaning |
| --- | --- |
| `position` | Where the effect is placed, in world space. |
| `normal` | The effect's local +Y, such as the struck surface's normal. |
| `target` | Optional: a world-space point the effect reaches for, such as a line's end or a bolt's destination. |
| `seed` | Optional: every random choice (direction, speed, size, lifetime, jitter). The same seed always draws the same burst; each emitter mixes in its index. Without one, random per trigger. The impacts use the impact's number. |
| `scale` | Uniform size of the whole effect: positions, speeds, gravity and particle sizes, but not lifetimes or lights. Default 1. |

Emitter fields that read the target, all fork additions:

- `emission.shape: Segment(jitter: (x, y, z))`: particles spread evenly along the line from the emitter to the target, particle `i` of `n` at `(i + 0.5) / n` of the way, each moved by a random offset within `jitter` (half-extents, local). It ignores `emission.offset` and `emission.scale`. Without a target, a point. The arc uses it.
- `emission.particles_per_meter: Some(9.0)`: with a target, a burst spawns `ceil(distance × 9)` particles, at least one and at most `particles_amount`, which stays the cap.
- `time.lifetime_to_target: true`: each particle lives until it has traveled from where it spawned to the target at its initial speed, so a bolt dies on arrival. `lifetime` is the cap. The burst ends after the emitter-to-target trip at the slowest `initial_velocity`. The laser bolt uses it, and is placed at the muzzle with +Y toward the target.

`vfx::spawn` may be called in `Update`, before transform propagation, or in `PostUpdate` after it but before `bevy_sprinkles::SprinklesLateSetup`, for example at a posed weapon's muzzle. Either way the effect draws that frame.

## Colors are sRGB

Every color in an effect file is **sRGB**, like every authored color in Kestrel: particle colors (`base_color`, `emissive`, `specular_tint`, `attenuation_color`, `initial_color`, gradient stops) and light colors. The loader converts particle colors to the linear values sprinkles takes; its doc comments say linear because they describe the loaded asset, not the file. Alpha isn't converted.

Glowing colors go above 1, for bloom, and convert with the same curve. A hand-written effect's `emissive: LinearRgba::rgb(12.0, 4.56, 0.96)` is `(2.9161, 1.9303, 0.9822, 1.0)` in a file: write the linear value in a comment next to it.

## Variants reuse an effect

A file with `base: "<effect ID>"` starts from that effect and overrides what it writes, like [character definitions](character-definition.md). This variant of `impact_stone` has blue sparks, no debris and its own light:

```ron
(
    version: 1,
    base: "impact_stone",
    particles: (
        name: "impact_blue",
        emitters: [
            (name: "sparks", draw_pass: (material: Standard((base_color: (0.18, 0.65, 1.0, 1.0))))),
            (name: "debris", enabled: false),
        ],
    ),
    lights: [
        (color: (0.18, 0.65, 1.0), intensity: 28000.0, range: 4.5, radius: 0.15, offset: 0.2, fade_seconds: 0.1),
    ],
)
```

- **Sections** (`(field: ...)`) merge field by field, at any depth. The sparks keep their mesh, speeds and `emissive`; only `base_color` changes.
- **Lists** replace the base's whole list: `lights` here, and a curve's `points` or a gradient's `stops`. `lights: []` removes the base's lights.
- **`particles.emitters`** is the exception: emitters merge by `name`. A name that matches a base emitter merges into it field by field; a new name is appended after the base's emitters, in the variant's order. A base emitter can't be removed, only turned off with `enabled: false`.
- **Struct and variant names**: a value with another name replaces the base's (`shape: Sphere(radius: 0.5)` over `Box(...)`); with the same name it merges (`Standard((...))`, `Some((...))`).

The base stays a usable effect, and can have its own base. Unknown bases, cycles, and a value of another shape than the base's (a list over a section) are errors that name the file and the field. The merged result is checked as strictly as any file; its parse errors point into the merged text, not the variant's, and say so.

## Pitfalls

- **Effects must end.** `despawn_on_finish: true` and `one_shot: true` on every emitter are required; the loader rejects a file without them. Looping effects (fireflies, lightning) stay in `kestrel/app/src/atmosphere.rs` for now.
- **Scale is one number.** A particle's size is the mesh times one random factor, the same on every axis. For long thin pieces, shape the mesh (`Cuboid(half_size: (0.0225, 0.15, 0.0225))`) and vary only its overall size; a random length at a fixed width can't be expressed. The trigger's `scale` is uniform too.
- **Curves can't do exponents below 1.** A curve point's easing can't make `(1 − t)^0.65`; sample it as a few linear points (`mode: SingleCurve`), as the shrink curves do. A curve's `value` multiplies the property; `position` runs from 0 (birth) to 1 (death).
- **Set gravity and direction.** sprinkles defaults to gravity −9.8 and a +X direction; the legacy effects used 14 m/s² down and the normal.
- **Small particles don't cast shadows.** Set `draw_pass.shadow_caster: false` on sparks, chips and the like. Turn it on only where a shadow reads better (large or slow pieces).
- **Keep bursts pooled.** An emitter is pooled, sharing one buffer and one draw with every other burst of the same emitter, only when it is in world space (`use_local_coords: false`, the default), has no trail mesh and is no sub-emitter's source or target. Any other emitter gets its own buffers, material and draw per burst, which is far slower when effects fire often.
- **A disabled emitter still costs a pool slot** in each burst, though it draws nothing.
- **Long-lived bursts hold their slots.** A burst keeps its pool slot until its longest particle dies: `blood_splat`'s 3 s splats at 50 cuts/s keep about 150 bursts per emitter alive, three chained pools (32 + 64 + 128 slots). On the Intel UHD 770 that was 0.4 ms (2.5%) slower than legacy and 2 MB more GPU memory (`kestrel/todo/vfx.md`, "Port blood, deflection and the trial burst").
- **The culling box.** An effect that reaches far (an arc, a bolt, fast sparks) needs a `visibility_aabb` that big, or it vanishes when its center leaves the screen.
- **Slow particles stick.** `Rigid` collision bounces only above `2 / (1 + bounce)` m/s of normal speed; slower particles stop and slide.

## Adding an effect

1. Write `kestrel/app/vfx/<id>.ron`. Start from an existing effect, with `base` if it's a variant.
2. Add `("<id>", include_str!("../vfx/<id>.ron"))` to `vfx::FILES` in `kestrel/app/src/vfx.rs`. Files are compiled in; the `vfx::catalog::tests::the_list_matches_the_folder` test fails when the list and the folder differ.
3. Trigger it from game code: `vfx::spawn(world, "<id>", vfx::Trigger::new(position, normal).seed(seed))`. It returns false when the run draws the legacy effects, so the caller draws its hand-written one instead.
4. Run `bazel test --config=remote //kestrel/app:movement_test`: `vfx::tests::every_effect_file_parses` loads every file, and a bad file also panics at startup.

## Trying an effect

`--vfx sprinkles`, or Effects → VFX backend in the debug menu (`/`), draws effects from these files; `--vfx legacy`, the default, draws the hand-written ones. Runs without GPU particles (the simulation, tests, the web build for now) always use the legacy effects.

The Effects window lists every effect:

- **Fire** triggers one 3 m in front of the player, 0.5 m up, with the normal up and a target 4 m further along the player's facing. An effect that flies to its target (`lifetime_to_target`) is placed with +Y toward it instead.
- **Reload effect files** rereads `kestrel/app/vfx/` from the source tree, in runs started with `bazel run` (it finds the folder through `BUILD_WORKSPACE_DIRECTORY`; otherwise the button is off and says why). Effects that load replace the running ones in place. A file that fails keeps its last good version, and its error shows in the window and the log. New files appear; a deleted file's effect stays until a restart.

So the loop is: `bazel run --config=remote //kestrel/app:demo -- --vfx sprinkles`, edit the file, Reload, Fire, repeat. Add a new file to `vfx::FILES` before you commit, or the next start won't have it.
