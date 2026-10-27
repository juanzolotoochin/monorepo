# VFX

The spike's plan, decisions and evaluation are in
`kestrel/VFX_SPIKE_HANDOFF.md`. The spike adopted the fork (handoff,
"Decision"). The first tasks below follow its "After the spike" plan, in
order; "Simulate a pool's bursts in one dispatch" and "Draw only a pool's
used slots" come after the blood port.

## Build an effect catalog

- **Status:** open
- **Why:** effects should be listed, authored and changed the way
  characters are. Today `vfx::FILES` is a hand-written list of
  `include_str!` files, there is no inheritance, and every change to a
  file is a rebuild.
- **What to do:** load every file in `app/vfx/` by name (the file name is
  the effect's ID), with strict loading as now; add `base` inheritance
  like character files (nested sections merge, lists replace); pick one
  color space for both layers; hot reload in debug builds; a debug-menu
  list that fires any effect in front of the player.
- **Where:** `kestrel/app/src/vfx.rs`, `kestrel/app/vfx/`,
  `kestrel/app/BUILD.bazel`.
- **Found in:** VFX spike handoff, step 6.

## Write the effect file schema doc

- **Status:** open
- **Why:** AI and people author effect files from the schema; today it is
  `app/vfx/README.md` plus the fork's Rust doc comments.
- **What to do:** a doc like `docs/character-definition.md`: one complete
  file read from the outside in, our layers, the sprinkles fields we use
  and their units, trigger parameters, pitfalls (one scalar scale,
  curves). Link it from `docs/README.md`.
- **Where:** `kestrel/docs/`, `kestrel/app/vfx/README.md`.
- **Found in:** VFX spike handoff, step 6.

## Port the impact materials

- **Status:** open
- **Why:** only stone uses an effect file; Metal (also the laser's arrival
  impact), Crystal and Polymer are still legacy.
- **What to do:** `impact_metal.ron`, `impact_crystal.ron` and
  `impact_polymer.ron` on `base: "impact_stone"` with the numbers from
  `impact::response` (Polymer: no sparks, flash or light). Pick the file
  by `ImpactResponse` in `effects::impact`. The struck target's emissive
  flash and kick stay in Rust. Capture each against legacy.
- **Where:** `kestrel/app/vfx/`, `kestrel/app/src/effects.rs`,
  `impact.rs`.
- **Found in:** VFX spike handoff, step 6 ("Authoring").

## Port blood, deflection and the trial burst

- **Status:** open
- **Why:** they are plain bursts of cubes, still CPU particles.
- **What to do:** see the blood sketch in the handoff ("Authoring"):
  two droplet emitters; a world-space velocity added at spawn (legacy
  adds 1.5 m/s up after normalizing) or an approximation; floor splats as
  a separate effect triggered at the floor point, since they lie under the
  cut. Deflection is a plain spray. The trial burst needs 24 cubes evenly
  on a ring (an emission option) or random directions. Measure blood's
  3 s splats on the Deck: they hold pool slots longer than any effect so
  far.
- **Where:** `kestrel/app/vfx/`, `kestrel/app/src/effects.rs`,
  `palimpsest/mod.rs`, `kestrel/vfx/sprinkles/src/shaders/particle_simulate.wesl`.
- **Found in:** VFX spike handoff, step 6.

## Port blade echo and the dust ring

- **Status:** open
- **Why:** both are still hand-written; each needs something the fork or
  `vfx::Trigger` lacks.
- **What to do:** blade echo: give `Trigger` a full rotation (it has only
  a normal) and a non-uniform size, or keep it in Rust. Dust ring: a
  custom mesh in `ParticleMesh` (the ring) and alpha over lifetime on a
  pooled emitter. The cover shards stay in Rust (agreed decisions).
- **Where:** `kestrel/app/src/effects.rs`, `destruction.rs`,
  `kestrel/app/src/vfx.rs`, `kestrel/vfx/sprinkles/src/asset/mod.rs`,
  `mesh.rs`.
- **Found in:** VFX spike handoff, step 6.

## Decide whether the atmosphere moves to effect files

- **Status:** open
- **Why:** fireflies and lightning are continuous, not bursts, and their
  point lights move with them. Effect files allow only one-shot emitters,
  and the light layer only fades.
- **What to do:** after the bursts are ported, either allow looping
  effects owned by the scene, with lights driven from the CPU by the same
  formula, or leave both in `atmosphere.rs` and say so in the schema doc.
- **Where:** `kestrel/app/src/atmosphere.rs`, `kestrel/app/src/vfx.rs`.
- **Found in:** VFX spike handoff, step 6.

## Remove the legacy effect path

- **Status:** open
- **Why:** two paths for every ported effect double the upkeep. Until
  this is done, sprinkles is experimental: legacy stays the default and
  new effects may still be written in `effects.rs` (they join the port
  list). Sprinkles is up to par when every effect is ported with parity,
  the catalog has hot reload, and the pool tasks bring the Deck's
  stress numbers near legacy's.
- **What to do:** once each ported effect has parity: remove the
  `VfxBackend` toggle (`--vfx`, the debug menu entry), the legacy
  functions and unused `FxAssets` materials. `update_effects` keeps
  severed heads, the laser's `LaserBolt` timing and the rocks' flash.
  Runs without GPU particles (simulation, tests) draw no effects.
- **Where:** `kestrel/app/src/effects.rs`, `vfx.rs`, `scene.rs`,
  `control/mod.rs`, `kestrel/app/vfx/README.md`.
- **Found in:** VFX spike handoff, step 6.

## Match the arc's last frame

- **Status:** open
- **Why:** at 30 fps the sprinkles arc still draws small cubes on frame
  117 of `arc_hits.json`, where legacy has none (about 700 pixels). Likely
  the one-shot completion frame (`UPSTREAM.md`) or the shrink curve's end.
- **What to do:** compare the particle age and scale at that frame;
  shorten `arc.ron`'s lifetime or end the curve earlier if that's all.
- **Where:** `kestrel/app/vfx/arc.ron`,
  `kestrel/vfx/sprinkles/src/spawning.rs`.
- **Found in:** VFX spike handoff, step 6.

## Decide on an effect editor

- **Status:** open (low priority)
- **Why:** sprinkles' editor no longer reads our files. Hot reload plus
  debug sliders may be enough.
- **What to do:** once people tune effects regularly, decide between
  forking the editor onto our format and growing the debug UI.
- **Where:** `kestrel/app/src/vfx.rs`.
- **Found in:** VFX spike handoff, agreed decisions and step 6.

## Give trail particles motion vectors

- **Status:** open
- **Why:** the fork now writes motion vectors for mesh particles (TAA smeared
  them otherwise), but trail meshes get zero motion, so a fast trail may
  ghost under TAA. No effect uses a trail yet: the laser bolt (step 3) is a
  velocity-aligned mesh particle and shows no ghosting.
- **What to do:** before the first effect with a trail, reconstruct each
  trail vertex's previous position from the trail history, or exclude
  trails from the motion-vector prepass.
- **Where:** `kestrel/vfx/sprinkles/src/shaders/particle_material.wesl`.
- **Found in:** VFX spike step 2.

## Check particle shadows

- **Status:** open
- **Why:** the legacy arc's cubes cast a visible shadow streak on the floor;
  the `arc.ron` port (emitters default to `shadow_caster: true`) shows none
  in the combat arena captures. The particle shadow pass may not place
  vertices from the particle buffer, or the shadow is just too faint.
- **What to do:** capture a big shadow-casting emitter in a lit scene;
  check the shadow prepass path of `particle_material.wesl`.
- **Where:** `kestrel/vfx/sprinkles/src/shaders/particle_material.wesl`,
  `material.rs`.
- **Found in:** VFX spike step 3.

## Don't lose a burst when its first step can't run

- **Status:** open
- **Why:** a one-shot burst spawns all its particles in its first time step.
  If that step's compute dispatch is skipped (GPU buffers, material or
  pipeline not ready yet), the step is spent anyway and the burst never
  spawns. Step 3 hit this with the laser (fixed by ordering, see
  `UPSTREAM.md`), but anything that delays GPU resources a frame would
  bring it back.
- **What to do:** don't advance an emitter's time until a compute dispatch
  has run for it (a feedback flag from the render world), or spawn
  particles in the first step that actually runs.
- **Where:** `kestrel/vfx/sprinkles/src/spawning.rs`
  (`step_particle_time`), `compute.rs`.
- **Found in:** VFX spike step 3.

## Simulate a pool's bursts in one dispatch

- **Status:** open
- **Why:** with pooled bursts, each burst still has its own compute
  dispatch on the pool's shared buffer: 2.5 ms of Deck GPU at 200
  impacts/s (1.4 ms on the desktop). That is most of what is left between
  pooled sprinkles (11.4 ms a frame) and legacy (9.9 ms). Dispatches that
  write one buffer may also be serialized by barriers.
- **What to do:** put each burst's parameters (the `EmitterUniforms` it has
  now) in a storage buffer indexed by slot, and dispatch each pool once
  over its used slots. Measure with `--vfx-stress impacts=200` on the Deck.
- **Where:** `kestrel/vfx/sprinkles/src/compute.rs`, `extract.rs`,
  `pool.rs`, `shaders/particle_simulate.wesl`.
- **Found in:** VFX spike step 4 (pooled bursts).

## Draw only a pool's used slots

- **Status:** open
- **Why:** a pool's draw covers all its slots, live or not. Cheap for the
  stone impact (12–18 particles a slot), not for the arc (270 a slot, 8,640
  instances in its first pool): the Deck's arc-only load is +0.4 ms over
  legacy.
- **What to do:** draw up to the highest used slot (indirect draw or a
  mesh per capacity step), or give emitters with many particles smaller
  pools.
- **Where:** `kestrel/vfx/sprinkles/src/pool.rs`, `spawning.rs`
  (`create_pool`), `mesh.rs`.
- **Found in:** VFX spike step 4 (pooled bursts).

## Free and update particle pools

- **Status:** open
- **Why:** pools are never freed or shrunk, so GPU memory stays at the
  high-water mark (+7 MB at 200 impacts/s, +24 MB with lasers and arcs).
  Live edits to a pooled emitter's mesh, material or particle count don't
  reach existing pools. Fine for compiled-in files; not for live reload or
  an editor.
- **What to do:** drop a pool after it has been empty for a while; rebuild
  an asset's pools when the asset changes.
- **Where:** `kestrel/vfx/sprinkles/src/pool.rs`, `spawning.rs`
  (`sync_*`).
- **Found in:** VFX spike step 4 (pooled bursts).

## Find the pooled laser capture's difference near the impact

- **Status:** open
- **Why:** with pooling, `laser_pit.json` frames 40–47 (after the bolt
  lands) differ from the per-burst run by up to 37 pixels, all within x
  740–782, y 323–354, where the legacy Metal impact sparks. The bolt frames
  are identical, and two pooled runs are identical, so it is deterministic.
  Step 5 (after the WebGPU switch, same box, x 756–773, y 335–352): the
  pooled run now matches step 4's per-burst run (`v2laser_sprinkles`)
  exactly, and a `KESTREL_SPRINKLES_NO_POOL=1` run shows the old
  difference instead. So it follows the build, not pooling. Possibly the
  legacy Metal sparks or the impact light depend on something like entity
  order.
- **What to do:** crop and compare those frames; check whether the pool's
  draw entity (no frustum culling, drawn the frame its bursts end) changes
  lighting, shadows or TAA history there.
- **Where:** `kestrel/vfx/sprinkles/src/pool.rs`; scratchpad
  `step4/poollaser_sprinkles` vs `step3/laser_sprinkles`.
- **Found in:** VFX spike step 4 (pooled bursts).

## Decide whether small impact particles cast shadows

- **Status:** open
- **Why:** before pooling, each shadow-casting emitter was drawn in four
  cascade views, and turning shadows off for the stone impact cut the
  desktop's 200 impacts/s frame from 45.6 to 26.8 ms. Pooled, it's 4 extra
  draws per pool, so this is now a look question more than a cost one.
  Legacy sparks do cast shadows.
- **What to do:** compare captures with and without `shadow_caster: false`
  on sparks and debris; pick a default for effect files.
- **Where:** `kestrel/app/vfx/impact_stone.ron`, `kestrel/app/vfx/README.md`.
- **Found in:** VFX spike step 4.

## Find why reusing GPU objects is slower on the Deck at 200/s

- **Status:** open (low priority: concerns the per-burst path, which no
  longer carries Kestrel's effects)
- **Why:** keeping the compute/sort uniforms and bind groups across frames
  cuts the fork's own prepare on the Deck from 10.1 to 1.6 ms at 200
  impacts/s with per-burst emitters. Yet the frame gets 4% slower (87.3 →
  90.7 ms, repeated in back-to-back pairs): the rest of the render world's
  prepare grows by about 13 ms. At 50/s it is 4% faster.
- **What to do:** only if per-burst emitters matter again (local space,
  trails, sub-emitters): profile one Deck run each way (Tracy or `perf`).
- **Where:** `kestrel/vfx/sprinkles/src/compute.rs`, `sort.rs`; the
  `KESTREL_SPRINKLES_NO_REUSE` and `KESTREL_SPRINKLES_NO_POOL` switches
  give the A/B.
- **Found in:** VFX spike step 4.

## Remove the VFX stress harness and counters after the spike

- **Status:** open
- **Why:** `--vfx-stress`, `effects::stress_bolt`, `bevy_sprinkles::stats`
  and the `KESTREL_SPRINKLES_NO_REUSE` / `KESTREL_SPRINKLES_NO_POOL`
  switches exist to measure the spike.
- **What to do:** the fork is adopted (step 6), so keep `--vfx-stress`
  until "Simulate a pool's bursts in one dispatch" is measured with it.
  Then keep it only if we want a performance regression check, or delete
  `app/src/vfx_stress.rs`, its hooks in `control/mod.rs`, `main.rs` and
  `effects.rs`, and the fork's `stats.rs` and switches (`UPSTREAM.md`,
  "Burst performance", "Pooled bursts"). Drop the switches with the
  legacy path either way.
- **Where:** `kestrel/app/src/vfx_stress.rs`,
  `kestrel/vfx/sprinkles/src/stats.rs`.
- **Found in:** VFX spike step 4.

## Let the Steam Deck launcher run from Windows

- **Status:** open
- **Why:** `//kestrel/app:steamdeck` refuses non-Linux hosts, so step 4
  copied a remotely built Linux binary by hand (handoff, step 4).
  `steamdeck.local` also isn't in this PC's `known_hosts`, though the
  Deck's IP is.
- **What to do:** build the Linux `demo` for the target platform when the
  host is Windows (it's already built remotely) and allow the Windows host
  in `steamdeck.go`; document the `known_hosts` step.
- **Where:** `kestrel/app/steamdeck.go`, `kestrel/app/BUILD.bazel`,
  `kestrel/app/README.md`.
- **Found in:** VFX spike step 4.

## Pass options to the web build from the URL

- **Status:** open
- **Why:** the wasm build has no command line, so `--vfx sprinkles` and
  the other `Options` flags can't be given in the browser; step 5 had to
  switch the backend in the debug menu (Effects → VFX backend) by hand.
- **What to do:** on wasm32, turn the page's query string (for example
  `?vfx=sprinkles`) into the arguments `control::Options::from_args`
  reads. Flags that need files (`--demo-script`, `--frames-dir`) stay
  native only.
- **Where:** `kestrel/app/src/main.rs`, `kestrel/app/src/control/mod.rs`.
- **Found in:** VFX spike step 5.

## Check the web build in Firefox, Safari and on Linux

- **Status:** open
- **Why:** the WebGPU web build was only run in Chrome 156 on Windows
  (RTX 5080). Firefox, Safari and Linux Chrome have their own WebGPU
  implementations and bugs. The particle compute pass binds 5 storage
  buffers, under WebGPU's default limit of 8, so limits shouldn't be the
  problem.
- **What to do:** load the web build in each, switch Effects → VFX backend
  to Sprinkles, hit stone, fire a laser and an arc; record console errors.
- **Where:** `kestrel/app/README.md`, `kestrel/vfx/sprinkles/src/compute.rs`.
- **Found in:** VFX spike step 5.

## Done

## Write up the spike and decide

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/126)
- **Why:** the spike ends in a decision: adopt the fork, rewrite parts, or
  build our own.
- **What was done:** the handoff's "Decision" section: adopt the fork as
  Kestrel code and rewrite parts only when a task needs it. Results table
  (parity, determinism, desktop and Deck frame times, memory, web, fork
  size), the three options with effort and risks, an authoring review with
  sketches of the blood and impact-material ports, what's missing to list
  effects, and pointers to the scratchpad evidence. Fork size in
  `UPSTREAM.md` (11,495 lines, about 12% ours). "After the spike" is an
  ordered plan; its tasks are above.
- **Where:** `kestrel/VFX_SPIKE_HANDOFF.md`,
  `kestrel/vfx/sprinkles/UPSTREAM.md`, `kestrel/todo/vfx.md`.
- **Found in:** VFX spike handoff, step 6.

## Move the web build to WebGPU

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/126)
- **Why:** both GPU particle libraries need compute shaders, which WebGL2
  lacks. The web build is for debugging, so dropping WebGL2 is fine.
- **What was done:** Bevy's `webgl2` feature is now `webgpu`; the TAA and
  SSAO exclusion and the `wasm32` exclusion around `SprinklesPlugin` are
  gone; the WebGL-only `wgpu-hal` `gles` annotation is out of
  `MODULE.bazel`; the fork's `stats.rs` timers use `web_time` on wasm
  (`std::time` panicked at startup). Ran in Chrome 156 on Windows over
  the DevTools protocol: a WebGPU adapter (NVIDIA, not a fallback), no
  console or validation errors, and the stone impact, laser bolt and arc
  all drawn by sprinkles (handoff, step 5).
- **Where:** root `Cargo.toml`, `Cargo.lock`, `MODULE.bazel`,
  `kestrel/app/src/scene.rs`, `kestrel/app/src/main.rs`,
  `kestrel/vfx/sprinkles/src/stats.rs`, `kestrel/app/README.md`.
- **Found in:** VFX spike handoff, step 5.

## Measure burst performance

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/126)
- **Why:** sprinkles allocates GPU buffers per spawn and per frame; our
  effects are many short bursts.
- **What was done:** a debug stress harness (`--vfx-stress`,
  `app/src/vfx_stress.rs`); desktop and Steam Deck runs of both backends
  (handoff, step 4).
  - Desktop (RTX 5080), mean frame ms, legacy vs sprinkles with the fix:
    2.6 vs 4.6 at 10 impacts/s, 2.9 vs 12.5 at 50/s, 4.0 vs 45.6 at 200/s.
  - Steam Deck (measured): 7.3 vs 10.1, 7.8 vs 25.5, 9.6 vs 90.7.
  - 8 lasers or 8 arcs/s: at most 0.25 ms over idle on either backend.
  - No GPU memory growth.
  - Per-frame allocation wasn't the main cost. One shared uniform buffer
    plus bind groups kept across frames (fork, `UPSTREAM.md`) cut what the
    fork creates per frame at 200/s from 352 buffers + 700 bind groups to
    0 + 20.
  - Frame time: −3 to −5% on the desktop; −4% at 50/s on the Deck but +4%
    at 200/s (see "Find why reusing GPU objects is slower on the Deck at
    200/s").
  - The cost is Bevy drawing every emitter as its own bin in six views (see
    "Draw all live bursts of an effect emitter in one draw").
  - Runs can stop early (`max_seconds`, `abort_ms`), reported as aborted.
- **Where:** `kestrel/app/src/vfx_stress.rs`, `kestrel/vfx/sprinkles/src/`
  (`compute.rs`, `sort.rs`, `stats.rs`), `effects.rs`.
- **Found in:** VFX spike handoff, step 4.

## Draw all live bursts of an effect emitter in one draw

- **Status:** done as a prototype
  (https://github.com/juanzolotoochin/monorepo/pull/126)
- **Why:** sprinkles made every burst's every emitter its own mesh entity
  with its own material, so each was its own bin in every view. At 200
  impacts/s the Deck took 89 ms a frame against legacy's 9.9.
- **What was done:** pooled bursts (`vfx/sprinkles/src/pool.rs`,
  `UPSTREAM.md` "Pooled bursts").
  - One buffer, material and draw per (asset, emitter), split into burst
    slots. A burst simulates its slot with an offset and frees it when it
    despawns; the freed range is zeroed.
  - Effect files and `vfx::Trigger` are unchanged. Same seed, same pixels.
    Arc and laser bolt frames match step 3's captures.
  - Mean frame ms, legacy vs pooled vs per-burst, 200 impacts/s: Deck
    9.86 vs 11.40 vs 89.06; desktop 4.10 vs 4.28 vs 47.40. At 50/s: Deck
    8.06 vs 8.67 vs 25.87.
  - Follow-ups: "Simulate a pool's bursts in one dispatch", "Draw only a
    pool's used slots", "Free and update particle pools", "Find the pooled
    laser capture's difference near the impact".
- **Where:** `kestrel/vfx/sprinkles/src/pool.rs`, `spawning.rs`,
  `extract.rs`, `compute.rs`, `sort.rs`, `material.rs`, both shaders.
- **Found in:** VFX spike handoff, step 4.

## Add per-trigger parameters, port the arc and laser

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/126)
- **Why:** effects need values given when they're triggered (target point,
  seed, scale). Upstream has no runtime overrides.
- **What was done:** `ParticleTrigger` (target, seed; scale is the
  transform) and four schema fields: `Segment` shape, `particles_per_meter`,
  `lifetime_to_target`, `world_bias` (`app/vfx/README.md`, "Trigger
  parameters"). `arc.ron` and `laser_bolt.ron` behind the `VfxBackend`
  toggle; `LaserBolt` keeps the laser's timing and cover hold. A late setup
  pass so effects spawned after transform propagation draw that frame, and
  three upstream fixes (one-shot bursts lost their last frame,
  `YToVelocity` double-rotated in world space, `visibility_aabb` ignored).
  Captures match legacy for both effects; same seed, same pixels. Fork
  size in `kestrel/vfx/sprinkles/UPSTREAM.md`.
- **Where:** `kestrel/vfx/sprinkles/`, `kestrel/app/vfx/`,
  `kestrel/app/src/vfx.rs`, `effects.rs`.
- **Found in:** VFX spike handoff, step 3.

## Aim impact sprays like the legacy ones

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/126)
- **Why:** the legacy impact leans its sparks upward (`normal * 1.4 +
  random`, y in 0..1.3); `impact_stone.ron` sprayed a plain cone.
- **What was done:** `velocities.world_bias` in the fork;
  `impact_stone.ron` uses `(0.0, 0.46, 0.0)` (the legacy's mean up offset
  over 1.4). Boulder lab captures match the legacy spread.
- **Where:** `kestrel/vfx/sprinkles/src/shaders/particle_simulate.wesl`
  (`get_spawn_velocity`), `kestrel/app/vfx/impact_stone.ron`.
- **Found in:** VFX spike step 2.

## Port the stone impact

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/126)
- **Why:** proves one effect can match today's look in an authored file.
- **What was done:** the first effect file format (`app/vfx/README.md`):
  a sprinkles `ParticlesAsset` layer plus a light layer, strict loading
  (`app/src/vfx.rs`, unknown fields are errors in both layers).
  `app/vfx/impact_stone.ron` (sparks, debris, flash, light) is drawn from
  `effects::impact` for stone when `--vfx sprinkles` or Effects → VFX
  backend picks it; a floor collider per world. Fixed two fork bugs on the
  way (TAA motion vectors, bursts two frames late; `UPSTREAM.md`). Offscreen
  captures in the boulder lab match the legacy effect frame for frame;
  sprays fall a little sooner (see "Aim impact sprays like the legacy ones").
- **Where:** `kestrel/app/vfx/`, `kestrel/app/src/vfx.rs`, `effects.rs`,
  `control/mod.rs`, `kestrel/vfx/sprinkles/`.
- **Found in:** VFX spike handoff, step 2.

## Vendor sprinkles as a Kestrel crate

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/126)
- **Why:** we fork sprinkles heavily, so it lives in the monorepo as our code
  and builds with Bazel like the rest of Kestrel.
- **What was done:** the core crate from upstream's `bevy-0.20` branch plus
  `main`'s "minimal dependencies" commit, without the editor, preset
  textures or `.ron` asset loader (`kestrel/vfx/sprinkles/UPSTREAM.md`).
  `bytemuck`, `bitflags`, `paste` and `thiserror` became direct root
  dependencies (no version changes). `SprinklesPlugin` is added only in
  rendered, native runs (`app/src/main.rs`). The demo and web builds and
  the Kestrel tests pass; an offscreen run of `demos/combat_arena.json`
  rendered without errors, but no effect spawns yet, so no particle shader
  has run.
- **Where:** `kestrel/vfx/sprinkles/`, root `Cargo.toml`, `kestrel/app/`.
- **Found in:** VFX spike handoff, step 1.
