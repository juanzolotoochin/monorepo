# VFX

The spike's plan, decisions and evaluation are in
`kestrel/VFX_SPIKE_HANDOFF.md`. The spike adopted the fork (handoff,
"Decision"). The first tasks below follow its "After the spike" plan, in
order; "Simulate a pool's bursts in one dispatch" and "Draw only a pool's
used slots" come after the blood port.

## Port blood, deflection and the trial burst

- **Status:** open
- **Why:** they are plain bursts of cubes, still CPU particles.
- **What to do:** see the blood sketch in the handoff ("Authoring"):
  two droplet emitters. Decided (2026-10-28): a new fork emitter field,
  a world-space velocity added at spawn, for legacy's 1.5 m/s upward
  after normalizing; the floor splats as a separate `blood_splat` effect
  that game code triggers at the floor point under the cut (flat box,
  uniform scale range); the trial burst's 24 cubes from a new fork
  emission option that places N particles at even angles on a ring.
  Deflection is a plain spray. Measure blood's
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
  Turning `shadow_caster` off for the stone impact's sparks and debris
  (2026-10-28) changed no pixel in the boulder-lab hit frames (largest
  difference 6 of 255, in 6 pixels), another hint that particle shadows
  don't draw.
- **What to do:** capture a big shadow-casting emitter in a lit scene;
  check the shadow prepass path of `particle_material.wesl`.
- **Where:** `kestrel/vfx/sprinkles/src/shaders/particle_material.wesl`,
  `material.rs`.
- **Found in:** VFX spike step 3.

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

## Revisit particle shadows per effect

- **Status:** open (low priority)
- **Why:** small particles go without shadows by default (see "Decide
  whether small impact particles cast shadows"), but some effects may
  read better with them: large debris, slow chunks, effects near the
  camera, or scenes with strong directional light.
- **What to do:** once more effects are ported, look at each with and
  without `shadow_caster` in a few scenes and turn shadows back on where
  they help; note the choice in the effect file. Check the cost on the
  Deck if many shadow-casting pools are live at once.
- **Where:** `kestrel/app/vfx/*.ron`, `kestrel/app/vfx/README.md`.
- **Found in:** session of 2026-10-28 (shadow default decision).

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
- **What to do:** the single dispatch per pool is measured with it
  (handoff, "Pool costs"). The user's direction (2026-10-28): keep
  `--vfx-stress` as a performance regression check. Drop
  `KESTREL_SPRINKLES_NO_REUSE` and the spike-only counters in the fork's
  `stats.rs` later (`UPSTREAM.md`, "Burst performance", "Pooled bursts"),
  and `KESTREL_SPRINKLES_NO_POOL` with the legacy path.
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

## Let the stress harness stop firing

- **Status:** open
- **Why:** `--vfx-stress` fires until the run ends, so pools never sit
  empty and the 5 s pool freeing, and the memory it gives back, can't be
  measured with it.
- **What to do:** add a spec key (for example `stop_after=N`) that stops
  firing after N measured frames and keeps recording, so the report shows
  pools dropping and wgpu memory falling back toward idle.
- **Where:** `kestrel/app/src/vfx_stress.rs`; scratchpad `run_matrix.sh`.
- **Found in:** VFX pool costs (handoff, "Pool costs", 2026-10-28).

## Make bursts rows of their pool, not entities

- **Status:** open
- **Why:** after one dispatch per pool and drawing only used slots, what's
  left over legacy is mostly render-world CPU from bursts still being
  entities. At 200 impacts/s on the Intel UHD 770 the frame is 19.93 ms
  against legacy's 18.61: prepare +0.5 ms, graph +0.3 ms, extract
  +0.2 ms, and 0.3 ms of GPU in Bevy's `uniform_allocation` and
  `bin_unpacking`. The Deck's slower CPU will make this share larger.
- **What to do:** the handoff's "Adopt and rewrite parts now" option, for
  the pooled path only: keep a burst's runtime (time steps, seed, trigger)
  as a row in its pool instead of an emitter entity, and extract and
  prepare per pool. Measure with `--vfx-stress` on the UHD 770 and the Deck.
- **Where:** `kestrel/vfx/sprinkles/src/pool.rs`, `spawning.rs`,
  `extract.rs`, `compute.rs`.
- **Found in:** VFX pool costs (handoff, "Pool costs", 2026-10-28).

## Measure the pool work on the Deck

- **Status:** open
- **Why:** one dispatch per pool and drawing only used slots were measured
  on the RTX 4070 and the Intel UHD 770 (a GPU-bound stand-in), not on the
  Deck, whose CPU is slower.
- **What to do:** rerun the stress matrix on the Deck (idle, 10/50/200
  impacts/s, lasers, arcs, both mixes) for legacy and sprinkles; compare
  with the handoff's pooled Deck table and the UHD 770 numbers in "Pool
  costs". Needs the Deck and a Linux host, or "Let the Steam Deck launcher
  run from Windows" first.
- **Where:** `kestrel/VFX_SPIKE_HANDOFF.md` (step 4, "Pool costs").
- **Found in:** VFX pool costs (handoff, "Pool costs", 2026-10-28).

## Write emissive as color times intensity

- **Status:** open
- **Why:** effect files are sRGB everywhere, so glowing `emissive` values
  above 1 go through the sRGB curve too: linear 12 is written 2.9161,
  which reads oddly and is hard to tune by hand.
- **What to do:** write emissive as an sRGB color (0–1) plus a separate
  intensity or strength the loader multiplies in linear space; convert the
  three files and update the README's color paragraph. Keep the test that
  walks the asset's color fields.
- **Where:** `kestrel/app/src/vfx/catalog.rs` (`particles_to_linear`),
  `kestrel/app/vfx/*.ron`, `kestrel/app/vfx/README.md`.
- **Found in:** effect catalog work (sRGB colors), 2026-10-28.

## Skip disabled emitters when spawning a burst

- **Status:** open
- **Why:** a variant turns a base's emitter off with `enabled: false`
  (`impact_polymer.ron` turns off the sparks and the flash), but the fork
  still spawns it: each burst claims a slot in that emitter's pool, which
  is created and drawn (at least 8 slots) though nothing is simulated.
- **What to do:** outside the editor, don't spawn disabled emitters
  (`spawning.rs`); keep `emitter_entities` indexed by emitter for
  sub-emitters, and make an effect whose emitters are all disabled finish
  (or not spawn its particle system in `vfx::spawn`). Drop the pitfall
  from `docs/effect-definition.md` after.
- **Where:** `kestrel/vfx/sprinkles/src/spawning.rs`, `runtime.rs`
  (`check_particle_system_finished`), `kestrel/app/src/vfx.rs`.
- **Found in:** porting the impact materials (branch kestrel-vfx-ports).

## Done

## Port the impact materials

- **Status:** done (PR pending, branch kestrel-vfx-ports)
- **Why:** only stone uses an effect file; Metal (also the laser's arrival
  impact), Crystal and Polymer are still legacy.
- **What to do:** `impact_metal.ron`, `impact_crystal.ron` and
  `impact_polymer.ron` on `base: "impact_stone"` with the numbers from
  `impact::response` (Polymer: no sparks, flash or light). Pick the file
  by `ImpactResponse` in `effects::impact`. The struck target's emissive
  flash and kick stay in Rust. Capture each against legacy.
- **What was done:** the three files on `base: "impact_stone"`: counts,
  speeds (`speed × 0.5` to `× 1.5`), spark, debris and light colors and
  light intensity from `impact::response` and `FxAssets` (`scene.rs`),
  linear emissive converted to sRGB. Metal's sparks get an 8 m culling
  box for their 16.5 m/s. `effects::impact` picks the file by
  `ImpactResponse` (`impact_effect`). Captures of each material in
  boulder-lab and the laser pit's arrival, legacy against sprinkles, match
  in color, count, timing, flash and light; as with stone, the sprinkles
  sprays are a little tighter (45° cone against legacy's random box), and
  legacy's debris chips cast floor shadows the ported ones don't
  (`shadow_caster: false`, decided).
- **Where:** `kestrel/app/vfx/`, `kestrel/app/src/effects.rs`,
  `impact.rs`.
- **Found in:** VFX spike handoff, step 6 ("Authoring").

## Write the effect file schema doc

- **Status:** done (PR pending, branch kestrel-vfx-ports)
- **Why:** AI and people author effect files from the schema; today it is
  `app/vfx/README.md` plus the fork's Rust doc comments.
- **What to do:** a doc like `docs/character-definition.md`: one complete
  file read from the outside in, our layers, the sprinkles fields we use
  and their units, trigger parameters, pitfalls (one scalar scale,
  curves). Link it from `docs/README.md`.
- **What was done:** `docs/effect-definition.md`: `impact_stone` read
  from the outside in, both layers, the emitter fields we use with units,
  trigger parameters, sRGB colors, `base` merge rules with an example,
  pitfalls (effects must end, one scalar scale, curves, pooling
  eligibility, culling box), adding a file and the Reload/Fire loop.
  Linked from `docs/README.md` and the site's nav; `app/vfx/README.md`
  now points there.
- **Where:** `kestrel/docs/`, `kestrel/app/vfx/README.md`.
- **Found in:** VFX spike handoff, step 6.

## Build an effect catalog

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/129)
- **Why:** effects should be listed, authored and changed the way
  characters are. Today `vfx::FILES` is a hand-written list of
  `include_str!` files, there is no inheritance, and every change to a
  file is a rebuild.
- **What to do:** load every file in `app/vfx/` by name (the file name is
  the effect's ID), with strict loading as now; add `base` inheritance
  like character files (nested sections merge, lists replace); pick one
  color space for both layers; hot reload in debug builds; a debug-menu
  list that fires any effect in front of the player.
- **What was done:** `vfx::catalog` loads the compiled-in files by name
  (a test checks the list against the folder), with `base` merged as a
  RON tree that keeps names (emitters merge by `name`); colors are sRGB in
  both layers; Effects → Reload rereads the folder in `bazel run` runs,
  keeping the last good version of a file that fails; Effects → Fire
  triggers any effect in front of the player.
- **Where:** `kestrel/app/src/vfx.rs`, `vfx/catalog.rs`, `vfx/ron_tree.rs`,
  `kestrel/app/vfx/`, `kestrel/app/BUILD.bazel`.
- **Found in:** VFX spike handoff, step 6.

## Decide whether small impact particles cast shadows

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/129)
- **Why:** before pooling, each shadow-casting emitter was drawn in four
  cascade views, and turning shadows off for the stone impact cut the
  desktop's 200 impacts/s frame from 45.6 to 26.8 ms. Pooled, it's 4 extra
  draws per pool, so this is now a look question more than a cost one.
  Legacy sparks do cast shadows.
- **What to do:** decided (2026-10-28): small particles don't cast
  shadows. Set `shadow_caster: false` on sparks and debris in
  `impact_stone.ron` (and the materials based on it), and say in the
  README that small particles go without shadows.
- **What was done:** sparks and debris in `impact_stone.ron` set
  `shadow_caster: false` (the flash keeps its shadow); the README says
  small particles go without shadows and points to "Revisit particle
  shadows per effect".
- **Where:** `kestrel/app/vfx/impact_stone.ron`, `kestrel/app/vfx/README.md`.
- **Found in:** VFX spike step 4.

## Simulate a pool's bursts in one dispatch

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/129)
- **Why:** with pooled bursts, each burst still has its own compute
  dispatch on the pool's shared buffer: 2.5 ms of Deck GPU at 200
  impacts/s (1.4 ms on the desktop). That is most of what is left between
  pooled sprinkles (11.4 ms a frame) and legacy (9.9 ms). Dispatches that
  write one buffer may also be serialized by barriers.
- **What to do:** put each burst's parameters (the `EmitterUniforms` it has
  now) in a storage buffer indexed by slot, and dispatch each pool once
  over its used slots. Measure with `--vfx-stress impacts=200` on the Deck.
- **What was done:** bursts that share a bind group (a pool's) are
  simulated by one compute dispatch per step, up to 64 bursts each, with
  their uniforms consecutive in the shared uniform buffer. Particle compute
  at 200 impacts/s: 2.89 → 0.23 ms on the Intel UHD 770, 1.17 → 0.05 ms on
  the RTX 4070 (handoff, "Pool costs"). Not measured on the Deck yet (see
  "Measure the pool work on the Deck").
- **Where:** `kestrel/vfx/sprinkles/src/compute.rs`, `extract.rs`,
  `pool.rs`, `shaders/particle_simulate.wesl`.
- **Found in:** VFX spike step 4 (pooled bursts).

## Draw only a pool's used slots

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/129)
- **Why:** a pool's draw covers all its slots, live or not. Cheap for the
  stone impact (12–18 particles a slot), not for the arc (270 a slot, 8,640
  instances in its first pool): the Deck's arc-only load is +0.4 ms over
  legacy.
- **What to do:** draw up to the highest used slot (indirect draw or a
  mesh per capacity step), or give emitters with many particles smaller
  pools.
- **What was done:** bursts take the lowest free slot; each frame a pool
  draws and sorts only up to its highest used slot, rounded up to a power
  of two (at least 8), with a cached mesh of that many slots. Arcs alone on
  the UHD 770: 17.13 → 16.03 ms (legacy 15.57).
- **Where:** `kestrel/vfx/sprinkles/src/pool.rs`, `spawning.rs`
  (`create_pool`), `mesh.rs`.
- **Found in:** VFX spike step 4 (pooled bursts).

## Don't lose a burst when its first step can't run

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/129)
- **Why:** a one-shot burst spawns all its particles in its first time step.
  If that step's compute dispatch is skipped (GPU buffers, material or
  pipeline not ready yet), the step is spent anyway and the burst never
  spawns. Step 3 hit this with the laser (fixed by ordering, see
  `UPSTREAM.md`), but anything that delays GPU resources a frame would
  bring it back.
- **What to do:** don't advance an emitter's time until a compute dispatch
  has run for it (a feedback flag from the render world), or spawn
  particles in the first step that actually runs.
- **What was done:** the compute node records the emitters it simulated;
  until an emitter is recorded, extraction emits its first step from the
  start of its first cycle, so a burst spawns in the first step that runs.
  Works per burst within a pool's batched dispatch.
- **Where:** `kestrel/vfx/sprinkles/src/spawning.rs`
  (`step_particle_time`), `compute.rs`.
- **Found in:** VFX spike step 3.

## Free and update particle pools

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/129)
- **Why:** pools are never freed or shrunk, so GPU memory stays at the
  high-water mark (+7 MB at 200 impacts/s, +24 MB with lasers and arcs).
  Live edits to a pooled emitter's mesh, material or particle count don't
  reach existing pools. Fine for compiled-in files; not for live reload or
  an editor.
- **What to do:** drop a pool after it has been empty for a while; rebuild
  an asset's pools when the asset changes.
- **What was done:** a pool empty for 5 s of game time is dropped (except
  the first pool of each asset and emitter), and a modified or removed
  particle asset drops its pools and restarts their bursts on new pools.
  Freeing isn't measured yet: the stress harness never stops firing (see
  "Let the stress harness stop firing").
- **Where:** `kestrel/vfx/sprinkles/src/pool.rs`, `spawning.rs`
  (`sync_*`).
- **Found in:** VFX spike step 4 (pooled bursts).

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
