# Upstream

Kestrel's fork of [sprinkles](https://github.com/doceazedo/sprinkles), a GPU
particle system for Bevy, by DoceAzedo and contributors. MIT OR Apache-2.0;
see `LICENSE-MIT` and `LICENSE-APACHE`.

Why it's vendored and what the fork is for: `kestrel/VFX_SPIKE_HANDOFF.md`.

## Base

- Upstream branch `bevy-0.20` at `7d8cae40af1213990e2586dd20474ba4c91d8539`
  ("fix flaky asset loader tests", 2026-10-10), the unreleased Bevy 0.20
  port.
- Plus upstream `main`'s `ec76358` ("minimal dependencies"), which that
  branch didn't have yet.
- Only the core crate, `crates/bevy_sprinkles`. Not the editor, benches or
  example assets.

## Changes from upstream

- Removed the `.ron` asset loader (`ParticlesAssetLoader`). It claimed every
  `.ron` file; Kestrel parses effect files itself with
  `asset::versions::migrate_str`.
- Removed the preset textures (`src/textures/assets/*.png`, 4.8 MB). The
  `preset-textures` feature is never enabled.
- Replaced the asset loader integration tests with `tests/parse.rs`.
- Built with Bazel (`BUILD.bazel`) from the root `Cargo.toml`'s crates.
- Asset schema denies unknown fields (`#[serde(deny_unknown_fields)]` on
  every deserialized type in `asset/` and on `TextureRef`), so a typo in an
  effect file is an error instead of a silent default. Test:
  `tests/parse.rs` `a_misspelled_field_is_rejected`.
- `ParticlesAsset::sprinkles_version()`: Kestrel embeds the asset in its own
  effect files and checks the version itself.
- Motion vectors: the particle vertex shader never wrote
  `previous_world_position`, so the motion-vector prepass read an unset
  position and TAA smeared particles into dark, dithered blobs that faded in
  late. The shader now writes the position minus velocity × the frame's
  simulated time (`ParticleEmitterUniforms::delta_time`, new, taking one
  padding slot). Trail meshes get zero motion. Kestrel's camera uses TAA.
- Frame timing: the per-frame systems (`setup_particle_systems`,
  `update_particle_time`, the syncs, finish and cleanup) run in `PostUpdate`
  before transform propagation instead of unordered in `Update`, and
  `write_emitter_uniforms` after propagation. A system spawned during
  `Update` now draws that same frame; before, bursts appeared two frames late.

### Per-trigger parameters (VFX spike step 3)

- `ParticleTrigger` component (`runtime.rs`), inserted with `Particles3d`:
  `target: Option<Vec3>` (world space) and `seed: Option<u32>`. The seed
  replaces the random per-spawn seed (each emitter mixes in its index;
  `fixed_seed` is the fallback). Scale is the system's `Transform` scale.
- New schema fields (all default off, `deny_unknown_fields` still applies):
  `EmissionShape::Segment { jitter }` (particles evenly along the line to the
  target, `(i + 0.5) / n`, jittered), `EmitterEmission::particles_per_meter`
  (burst size from the distance to the target, capped by `particles_amount`),
  `EmitterTime::lifetime_to_target` (each particle dies when its trip to the
  target at its initial speed is done; the cycle lasts the emitter-to-target
  trip at the slowest speed, capped by `lifetime`), and
  `EmitterVelocities::world_bias` (a world-space vector added to every spawn
  direction).
- `EmitterRuntime::{spawn_count, lifetime}` and `EmitterRuntime::time()`:
  the trigger's burst size and cycle length, computed at setup from the
  system's `Transform` (Kestrel's systems are root entities) and used by the
  time step and extraction instead of the asset's `EmitterTime`.
- `EmitterUniforms` / `EmitterParams` gain `trigger_target` (emitter-local),
  `trigger_flags`, `world_bias` (spawn space) and `spawn_count`, appended at
  the end. The simulation shader reads them in `get_emission_offset`
  (Segment), the new `get_spawn_velocity` (bias, also used to rebuild the
  emission direction), `spawn_particle` (trip lifetime) and the spawn test
  (`particle_idx < spawn_count`).
- Particle sizes scale with the system's transform (`scale_min/max ×
  transform_scale`); upstream scaled positions, speeds and gravity but not
  sizes.

### Fixes found while porting the arc and the laser

- Late setup: `setup_particle_systems` also runs in `SprinklesLateSetup`
  (public set, `PostUpdate` after propagation, before `AssetEventSystems`
  and `VisibilityPropagate`), with a late time step for the emitters it
  creates (`SetUpLate`) and `write_emitter_uniforms` after it. A system
  spawned after propagation (the laser at its posed muzzle) draws that frame.
  Emitters spawn with their `GlobalTransform` already set. Ordering before
  the asset events and visibility matters: otherwise the new buffers,
  material and visibility arrive a frame late, after the burst's only spawn
  step was spent on missing GPU buffers, and nothing ever spawns.
- Skipped first steps (later, 2026-10-28): the ordering above fixes the
  laser, but anything else that skips a burst's first dispatch (the compute
  pipeline still compiling, a texture or buffer not uploaded yet) still
  lost it. The compute node now records which emitters it simulated
  (`SimulatedEmitters`, render world, from the emitter list of
  `ParticleComputeBindGroups`, so it covers each burst of a batched pool
  dispatch and the node's early returns alike). Until an emitter is in it,
  extraction emits its frame's first step from phase 0 of its first cycle
  (`emission_start_phase`), so the burst spawns in the first step that
  runs. Time still advances as upstream; only the emission window widens.
  Test: `extract.rs` `a_burst_whose_first_step_was_skipped_spawns_in_the_next`.
- One-shot completion: a one-shot emitter stops emitting when its cycle
  ends but finishes (and its system despawns) a frame later. Particles
  spawn at age 0 at the end of their first step, so upstream cut the last
  frame of every burst; a two-frame laser bolt lost half its life.
- `YToVelocity` in world space (`particle_material.wesl`): the vertex shader
  rotated the velocity-aligned mesh by the emitter's rotation too, so a
  turned effect's particles pointed off their flight (the laser bar stood
  upright; step 2's sparks were turned by the surface normal).
- `draw_pass.visibility_aabb` is applied (`Aabb` + `NoAutoAabb` on each
  emitter). Upstream ignored it, so Bevy culled by one particle mesh at the
  emitter and far-flying particles vanished when the emitter was offscreen.

### Burst performance (VFX spike step 4)

- GPU timings: the compute and sort nodes record `particle_compute` and
  `particle_sort` spans with Bevy's `RenderDiagnosticsPlugin`
  (`render/particle_compute/elapsed_gpu` and so on).
- `stats` (new, public): relaxed atomic counters for the fork's per-frame
  CPU time (setup, time step and uniform writes, extraction, the two
  prepare systems) and for the GPU buffers and bind groups the prepare
  systems create. Kestrel's stress harness (`app/src/vfx_stress.rs`) reads
  and resets them each frame. Cheap enough to leave in; remove with the
  harness if unwanted. Its timers use `web_time::Instant` on wasm32
  (step 5): `std::time::Instant` panics there, and with the plugin now in
  the WebGPU web build the first timed system aborted the app.
- Reused GPU objects: upstream created, every frame, a colliders buffer, a
  uniform buffer and a 28-entry bind group per emitter per simulation step
  (`prepare_particle_compute_bind_groups`), and a new sort uniform buffer
  plus a bind group per emitter (`prepare_particle_sort_bind_groups`). Now
  `ParticleComputeCache` and `ParticleSortCache` (render world) keep them
  across frames:
  - Every emitter's step uniforms go into one shared uniform buffer, one
    aligned slot per step, written with a single `RenderQueue::write_buffer`
    per frame. The buffer only grows. The compute layout's uniform binding
    uses a dynamic offset, and `ParticleComputeBindGroups` holds one bind
    group per emitter plus an offset per step.
  - An emitter's bind group is rebuilt only when the hash of the ids of
    what it binds changes (a resized particle buffer, a new texture, a
    shared buffer that grew). Entries of emitters that are no longer
    extracted are dropped the same frame. The colliders buffer is one
    persistent buffer.
  - A first version kept a uniform buffer per emitter per step and wrote
    each with its own `write_buffer`. It was 5% faster on the desktop but
    4% slower on the Steam Deck: hundreds of small queue writes per frame
    cost more there than the allocations they replaced
    (`kestrel/VFX_SPIKE_HANDOFF.md`, step 4).
  - `KESTREL_SPRINKLES_NO_REUSE=1` drops the caches every frame (a fresh
    shared buffer and bind groups), for A/B measurements. Spike only;
    remove with the counters.

### Pooled bursts (VFX spike step 4)

Upstream gives every emitter of every particle system its own particle,
indices, sorted and uniform buffers, its own material and its own mesh
entity. Bevy then bins and draws each emitter of each burst separately in
every view. With many short bursts, that per-draw work dominated the frame
(`kestrel/VFX_SPIKE_HANDOFF.md`, step 4). `pool.rs` shares the draw
instead:

- **Pools.** One per (asset, emitter index), in `ParticlePools`. A pool
  owns a particle, indices and sorted buffer sized `particles_amount ×
  capacity`, one material (`ParticleEmitterUniforms::pooled = 1`, identity
  transform), and one entity (`ParticlePoolDraw`, `NoFrustumCulling`) that
  draws it. The first pool of an emitter has 32 slots; when every pool is
  full the next one doubles, up to 256 slots, then 256 per pool. An empty
  pool is hidden.
- **Drawn slots.** A burst takes the pool's lowest free slot. Each frame,
  `size_pool_draws` (last in `SprinklesLateSetup`, so after every claim and
  before the asset events flush) sets `drawn` to one past the highest used
  slot, rounded up to a power of two, at least 8, and gives the pool's
  entity a cached mesh of `particles_amount × drawn` particles. The mesh's
  particle index (`uv_b.x`) reads the sorted buffer in order, so the
  smaller mesh draws the first slots, which hold every live particle; the
  pool's sort covers the same `particles_amount × drawn`. A pool needs at
  most a mesh per power of two (about twice its full mesh).
- **Bursts.** `setup_particle_systems` still spawns one emitter entity per
  emitter with its `EmitterRuntime`: time steps, one-shot completion, seed,
  trigger and the late setup pass are unchanged. A poolable emitter's entity
  gets a `PoolSlot` and a copy of the pool's `ParticleBufferHandle` instead
  of buffers, a mesh and a material. Extraction sets the burst's
  `slot_offset` (`EmitterUniforms`, appended) and marks it not to sort; the
  pool gets one sort-only entry, over its drawn slots.
- **Simulation.** The head pass reads and writes
  `particles[slot_offset + idx]`. Particle indices, seeds and phases stay
  local to the burst, so the same seed gives the same particles, pooled or
  not.
- **Rotation.** A pool is drawn with an identity transform. A particle that
  isn't velocity-aligned would lose its emitter's rotation (the stone
  impact's flash cube is turned by the surface normal), so for pooled bursts
  (`TRIGGER_FLAG_KEEP_ROTATION`) the spawn stores the emitter's rotation as
  a quaternion in `ref_up` (unused without `YToVelocity`), and the vertex
  shader uses it when `pooled` is set.
- **Freeing.** Despawning a burst removes its `PoolSlot`, whose `on_remove`
  hook queues the slot. `maintain_pools` (in `Last`) frees it and queues its
  byte range. The compute node zeroes those ranges with `clear_buffer`
  before the frame's simulation, so a finished burst's last particles aren't
  drawn again. A pool whose last burst ended this frame is still drawn this
  frame, so it is sorted once more.
- **What isn't pooled.** Emitters in local space, with trails, sub-emitters
  (either side) or in editor mode keep the upstream path, as does
  everything with `KESTREL_SPRINKLES_NO_POOL=1` (spike A/B switch).
- **Dropping pools** (2026-10-28). A pool empty for `IDLE_SECONDS` (5 s of
  game time) is dropped in `maintain_pools`: its draw entity is despawned
  (the material goes with it, the buffers with their last handles), and a
  frame later its cached meshes leave `ParticleMeshCache` if nothing else
  holds them (`release_unused`, by the handle's strong count). The first
  pool of each (asset, emitter) is kept: the chained pools hold most of the
  memory, and an effect fired every few seconds would otherwise build a new
  pool (buffers, material, upload) per shot.
- **Asset changes** (2026-10-28). The `sync_*` systems only update unpooled
  emitters, and a pool's slot size, mesh and material come from the asset.
  On `AssetEvent::Modified` or `Removed` for a `ParticlesAsset`,
  `rebuild_changed_pools` (`PostUpdate`, after `AssetEventSystems`) drops
  the asset's pools and restarts every system with a burst in them: its
  emitter and collider entities are despawned and its
  `ParticleSystemRuntime` removed, so it is set up again next frame on new
  pools and plays from its start. A burst left on an old pool would be
  simulated with the new particle count over the old slots. A change made
  after the events flush (later in `PostUpdate`, or in `Last`) is seen a
  frame late; change assets in `Update`.

### Blood, deflection and the trial burst (2026-10-28)

- `EmitterVelocities::world_velocity`: a world-space velocity added to each
  particle's spawn velocity, after `world_bias` and the speed (legacy blood
  adds 1.5 m/s upward after normalizing its direction). Not scaled by the
  system's transform. `EmitterUniforms` / `EmitterParams` carry it in spawn
  space in the three padding slots before `slot_offset` (now `world_velocity:
  vec3, slot_offset: u32`), so the struct's size and the 64-burst uniform
  batch are unchanged. Only `spawn_particle` adds it; the emission direction
  `update_particle` rebuilds for directional velocity leaves it out.
- `EmitterVelocities::even_ring`: particle `i` of the `n` a cycle spawns
  (`spawn_count`) leaves exactly `spread` degrees off `initial_direction`, at
  `360° × i / n` around it, seed-independent; `flatness` is ignored. For the
  trial burst's 24 cubes flung out on a ring. It is a velocity option, not a
  `Ring { even }` emission shape: a ring of positions doesn't move outward
  by itself, and the radial velocity pushes away from the pivot in 3D, so a
  rising particle drifts up (1.1 m out and 2.5 m up after 0.8 s, against
  legacy's 2.4 m and 0.5 m). Rides in `trigger_flags`
  (`TRIGGER_FLAG_EVEN_RING`); `get_emission_velocity`, `get_spawn_velocity`
  and `update_particle` take the particle's index.
- Test: `tests/parse.rs` `world_velocity_and_even_ring_parse_and_default_off`.

Keep this list current as the fork grows.

## Size

Lines changed in `src/` against upstream (`git diff --no-index
--ignore-cr-at-eol --numstat`, 2026-10-26, after step 3): Rust +418 −145,
WESL +84 −10. About +320 −25 Rust (including ~50 lines of tests) and
+68 −9 WESL of that is step 3; most of the deletions are step 1's removed
asset loader.

After step 4 (2026-10-27), against `7d8cae4` alone (so `ec76358`'s
dependency changes count too): Rust +697 −221, WESL +84 −10. Step 4 itself
is roughly +250 −75 Rust and no WESL: `stats.rs` (65 lines), the caches in
`compute.rs` and `sort.rs`, and one-line timers in `spawning.rs` and
`extract.rs`.

With pooled bursts (2026-10-27, same base): Rust +1195 −206, WESL +148 −14.
Pooling is roughly +480 Rust (`pool.rs` is 271 lines, ~75 of them tests;
the rest is in `spawning.rs`, `extract.rs`, `compute.rs`, `sort.rs` and
`material.rs`) and +64 −4 WESL.

At the end of the spike (2026-10-27, step 6, same base and method): Rust
+1199 −206, WESL +148 −14. Step 5 added the 4 lines of `web_time` timers
in `stats.rs`.

- Total: `src/` is 8,974 lines of Rust and 2,521 of WESL (11,495); upstream's
  is 7,981 and 2,387 (10,368). Plus `tests/` (213 lines with fixtures).
- Ours: about 1,350 of the 11,495 lines (12%) are added or changed. Two
  files are new (`pool.rs` 271, `stats.rs` 69). The largest changes are in
  `spawning.rs` (+276 −19), `compute.rs` (+160 −35), `extract.rs`
  (+141 −12), `particle_simulate.wesl` (+107 −13) and `asset/mod.rs`
  (+71 −54). Of upstream's lines, about 220 (2%) were removed or rewritten.
- Used by Kestrel's three effects (an estimate from the schema and the
  shaders): the runtime, extraction, compute, sort, pooling, cube meshes,
  `Standard` materials, scale curves, velocities, gravity, `Rigid`
  collision and `YToVelocity`, roughly half the code. Not used yet: 2D,
  quads and billboards, color and alpha over lifetime, angles, radial,
  orbit and directional velocity, turbulence (curl noise), trails,
  sub-emitters, the `v0_1` migration (460 lines) and editor-mode paths.
  Several of these are what the unported effects would use (alpha for the
  dust ring and splats, angular velocity for tumbling debris, turbulence
  or orbits for fireflies).

## Upstream notes

- Upstream's `memory-leak` branch (2026-10) investigates per-frame GPU
  allocation in `prepare_particle_compute_bind_groups` and
  `prepare_particle_sort_bind_groups`. It adds only switches and counters,
  no fix.
- Activity (clone of 2026-10-19): 343 of 368 commits are by one author,
  DoceAzedo. Most were in January to March 2026; 13 since September. The
  `bevy-0.20` branch wasn't merged to `main` yet. Kestrel doesn't track
  upstream: fixes are taken by hand, when wanted.
