# Voxel asset rules

`defs.bzl` provides the first asset pipeline for the Bevy example. Source
recipes and layouts are RON; outputs are versioned JSON packs and a generated
Rust module embedding those bytes. Generated artifacts stay in Bazel's output
tree. The current generator supports boulders; more prefab kinds can be added
to the same renderer-independent compiler.

```starlark
load("//bazel/voxel_assets:defs.bzl", "boulder_prefab", "scene_assets")

boulder_prefab(
    name = "basalt",
    src = "basalt.ron",
    palette = "palette.ron",
)

boulder_prefab(
    name = "procedural_basalt",
    src = "basalt.ron",
    palette = "palette.ron",
    runtime = True,
)

scene_assets(
    name = "level_assets",
    layout = "level.ron",
    prefabs = [":basalt", ":procedural_basalt"],
    visibility = ["//visibility:public"],
)
```

The underlying `voxel_prefab` rule declares a `VoxelPrefab` action. It reads
only the recipe and palette, and invokes `//tools/assetc:assetc` with `cfg =
"exec"`. `VoxelPrefabInfo` carries its ID and compiled artifact. The
`boulder_prefab` macro selects baked or runtime mode.

`scene_assets` wraps a scene-pack rule and a `rust_library`. The scene action
consumes `VoxelPrefabInfo` dependencies, validates references, transforms and
parameter overrides, and emits `<name>_pack.scene.json` and `<name>_pack.rs`.
The Rust target exports `PACK_BYTES`. Depend on it from a game and decode the
bytes with `voxel_assets::ScenePack::decode`. It needs no working directory or
runtime asset file discovery.

Compiler tools run on the execution platform even when the game targets
Windows. The mesh data has no platform-specific paths or native handles.
Prefab actions cache independently; an edit to a layout repacks its scene
without remeshing unchanged prefabs. The asset compiler requires no Bevy,
window, GPU, audio device, or network access.

## Authoring

A recipe specifies parameters; all loops, hashing, voxelization and meshing
live in `//tools/assetc/core:voxel_assets`:

```ron
Boulder(seed: 42, radius: 1.2, height: 1.6, voxel_size: 0.25)
```

Optional recipe fields include `taper`, `ore_every`, `debris`, and `material_pattern`. Unknown
fields are errors. Dimensions, finite values, and a 262144-cell work budget
are checked before generation. The palette supplies four material definitions
in order: base, dark, light, ore. See the example's `assets/palette.ron`.

```ron
Scene(
    name: "Example",
    spawn: (0.0, 0.0, 2.0),
    industrial: false,
    instances: [
        (prefab: "basalt", position: (-2.0, 0.0, 0.0), scale: 1.1),
        (
            prefab: "procedural_basalt",
            position: (2.0, 0.0, 0.0),
            parameters: Some((seed: Some(99), radius: Some(1.4))),
        ),
    ],
)
```

`industrial` is the example's switch for its shared deck dressing; the scene
instances come from the pack. `yaw` is in radians and `scale` is uniform.
Seed/radius/height overrides are valid only for runtime prefabs. Bake mode
stores optimized buffers; runtime mode stores the validated recipe. Both
call the same generator and produce equivalent geometry for equal parameters.
The Bevy adapter generates each runtime parameter set on first use and caches
it across instances and scene switches. These generations run synchronously
on scene load, so large recipes can cause a load-time pause. They do not run
per frame. Baked geometry still needs deserialization and GPU upload at runtime.

The mesher removes internal faces and greedily merges coplanar faces of the
same material. Separate material submeshes preserve ore emission and impact
flashes. Collision metadata includes the conservative body radius and a radius
per occupied height layer, derived from the same voxels. The adapter uses the
height profile to account for scaled instances at the sword's contact height.
Debris is decorative and excluded from body collision.

## Validation

```sh
bazel test //tools/assetc:assetc_test //tools/assetc/core:core_test \
  //examples/rust/bevy_isometric:movement_test
```

Tests cover deterministic output, face winding, face reduction, invalid recipes,
missing references, baked/runtime equivalence, pack versions, scene switching,
cache reuse, collider visibility, and per-instance material cleanup.

## Material responses

Each of the four palette entries can select `impact: Stone`, `Metal`, `Crystal`,
or `Polymer` (omitting it defaults to `Stone`). Visual appearance and impact
response are independent, so one prefab can mix response types:

```ron
(name: "titanium", color: (0.45, 0.57, 0.68), emissive: (0.0, 0.0, 0.0),
 metallic: 0.95, roughness: 0.22, impact: Metal)
```

The response enum is renderer-independent asset data. The game's `impact.rs`
provides the preset tuning for flash, particles, lighting, displacement, and
hit pause; `sound.rs`/`sound.c` provide distinct procedural sound cues. Changing
preset tuning requires rebuilding the game. These are impact effects, not
material destruction or a physics simulation.

`material_pattern: Speckled` (the default) uses the original base/dark/light/ore
assignment. `material_pattern: Quadrants` gives all four slots large accessible
regions: -X/-Z = slot 0, +X/-Z = slot 1, -X/+Z = slot 2, +X/+Z = slot 3. It works
with baked and runtime recipes. Material boundaries remain intact during greedy
meshing, so the game can identify the actual surface struck. Scene instances
may also include `label: Some("METAL")` for a sample caption.

Pack version 2 records material response semantics, recipe patterns, and a
build-time geometry key shared across palette variants. Rebuild
packs together with their consumers; version 1 packs are rejected explicitly.

Scene pack version 3 adds optional `piles` logistics fixtures: a carrying
capacity, typed mineral dispensers with output locations, and initial piles.
The compiler validates capacities, quantities, finite coordinates, and deck
bounds. See `examples/rust/bevy_isometric/assets/levels/pile_lab.ron` for an
example. Recipes and layouts without logistics fixtures remain valid source
inputs and rebuild into the current pack version.
