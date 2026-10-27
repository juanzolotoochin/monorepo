# Kestrel VFX spike handoff

## Goal

Effects are written by hand today: each one is a Rust function in
`app/src/effects.rs` that spawns one entity per particle, with its numbers in
the code. We want effects standardized the way characters are: one authored
file per effect, a catalog that lists them, a preview, and tuning without a
recompile. Authoring will be a mix of AI writing the files and people tuning
the numbers.

This spike decides whether a forked copy of
[sprinkles](https://github.com/doceazedo/sprinkles), a GPU particle system for
Bevy, can carry that system. It ends in a write-up and a decision. It doesn't
build the catalog, the viewer preview, the schema doc or migrate every effect.

## How effects work today

- `effects::Particle` (velocity, age, life, size, gravity, bounce) on a scaled
  cube, moved on the CPU by `update_effects`: gravity, bounce off the floor at
  y = 0.045, face the velocity, shrink with age, despawn at end of life.
- Shared art is `FxAssets` (`scene.rs`): one cube and fixed material arrays
  picked by index (`spark[2]` is the blue glow).
- Effects: `impact` (sparks, debris, flash cube, point light; tuned per
  material kind in `impact.rs`), `blade_echo` / `blade_trail`, `blood`
  (droplets and 3 s floor splats), `arc`, `deflection`, the laser bolt
  (`LaserBolt`), the trial burst in `palimpsest/mod.rs`. Ambient fireflies and
  lightning come from the style's `atmosphere` block (`atmosphere.rs`). Cover
  shards and the dust ring are in `destruction.rs`.

## Agreed decisions

- **One system for (nearly) every effect.** An effect is one file of ours,
  with layers: particle emitters (drawn by the fork), a light layer (real
  point lights can't be GPU particles, so we spawn them next to the
  particles), later maybe decals or a sound cue. The catalog, strict loading,
  preview and docs are at this level, not the library's.
- **Per-trigger parameters are required.** Every effect is triggered with a
  position and normal, plus optional extras: target point, seed, material
  kind. The arc (from, to) and the laser (target, so length and lifetime) need
  them. Upstream sprinkles has no runtime overrides, so the fork adds them.
- **Out of the effect system:** severed heads and cover shards (they tumble the
  real rig part or cover mesh), and hit-stop and screen shake (game feel; an
  effect may name a feedback preset later).
- **Game code keeps gameplay timing.** For example the laser's arrival, which
  holds cover intact (`CoverContacts`), stays in Rust; the bolt it draws
  becomes an effect.
- **Web moves to WebGPU.** The web build is for debugging, so WebGL2 isn't a
  requirement. Both GPU particle libraries need compute shaders. Bevy's
  `webgpu` feature overrides `webgl2`, so there is no fallback in one build.
- **Heavy forking is fine.** sprinkles is vendored as Kestrel code, not used
  as a crate.
- **No editor in the spike.** Once our format diverges, sprinkles' editor no
  longer reads our files. Tuning in the spike is live reload of the effect
  file plus sliders in the debug UI. Forking the editor is a later decision.

## Why sprinkles over bevy_hanabi

Both were evaluated (2026-10-26) on authoring, performance, physics,
platforms, Bevy 0.20, effect coverage, integration, extensibility and risk.
sprinkles won on authoring (plain serde RON, versioned, an editor), on fit
(lit PBR mesh particles, so our voxel cubes; sphere/box collision with bounce)
and on coverage of our effects. hanabi is more mature and has per-instance
properties, but has no collision, an effect file generated from Rust
expressions (`"#N"` handles) that is hard to write by hand, and seeds it can't
fix. Both need compute shaders and have one main maintainer.

Known gaps in sprinkles, to fix in the fork:

- No runtime parameter overrides; no line or segment emission shape.
  (Fixed in step 3.)
- Per-spawn GPU allocation: every `Particles3d` spawn creates 4–5 buffers and
  a material per emitter, and every frame each emitter gets a new uniform
  buffer and bind group (`compute.rs`). No pooling.
- Typos are silent: no `deny_unknown_fields`, `#[serde(default)]` everywhere.
  (Fixed in step 2.)
- The asset loader claims the bare `.ron` extension.
- Seeding: `fixed_seed` is per asset; the runtime `global_seed` is never read.
  (Step 3: a per-trigger seed.)
- At most 32 colliders in the scene (`extract.rs`), sphere and box only.
- No CPU feedback except "finished".

## Steps

Each step is a task in `todo/vfx.md`. Split the work at step 2 so the
decision to fork further is made halfway.

1. **Vendor.** Copy the core crate from upstream's `bevy-0.20` branch to
   `kestrel/vfx/sprinkles/`, recording the upstream commit and keeping its
   licenses. Leave out the editor and preset textures. Build it with
   `rust_library` and `@crates` dependencies; move `ron` to our pinned
   0.12.2. Remove the `.ron` loader (we parse effect files ourselves).
   Don't add the plugin in headless apps. Check upstream's `memory-leak`
   branch first. Done when `bazel build --config=remote //kestrel/app:demo`
   passes.
2. **Stone impact at parity.** Our first effect file, for example
   `content/data/vfx/impact_stone.ron`: sparks, debris, flash, plus a light
   layer. A floor collider per scene. Triggered from the `effects::impact`
   call site, with a debug toggle between old and new. Done when preview
   screenshots look as good as today's.
3. **Per-trigger parameters.** A per-instance parameter block (target, seed,
   scale) read by the simulation shader; a segment emission shape; lifetime
   from distance ÷ speed; our per-shot seed. Port the arc and the laser bolt.
   Record how many lines of Rust and WGSL changed.
4. **Burst performance.** A stress scene: 10 / 50 / 200 stone impacts per
   second plus 8 players firing lasers. Frame, CPU and GPU time on desktop and
   Steam Deck (`app/deck_launch.sh`), against today's CPU path. If per-spawn
   allocation dominates, try preallocated emitter slots per effect, reset on
   trigger, and measure again.
5. **Web on WebGPU.** Switch Bevy's `webgl2` feature to `webgpu`, drop the
   TAA exception in `app/src/scene.rs`, run the web build in Chrome with the
   new effects.
6. **Write-up and decision.** Screenshots, numbers, fork size. Adopt the
   fork, fork and rewrite parts, or build our own (fallback idea: ballistic
   particles computed in a vertex shader from spawn values and time, drawn
   with one instanced call per effect).

## Decision (2026-10-27)

**Adopt the fork.** Make the vendored sprinkles Kestrel's effect system, as
our own code, and move the remaining burst effects onto it. Keep rewriting
parts of it where Kestrel needs it, as pooling already did, but don't
rewrite it up front. Don't build our own.

Reasons:

- It does the job today. Three effects of different shapes (a spray with
  floor bounces, a segment, a bolt sized by its target) match legacy, are
  deterministic and run on desktop, Deck and the web.
- Pooling removed the only blocker. Unpooled, the Deck took 89 ms a frame
  at 200 impacts/s. Pooled it takes 11.4 ms, against legacy's 9.9.
- The format works for AI writing and for tuning (see "Authoring" below).
  Each of the three files was written in one pass, and strict loading
  catches typos.
- What we still need is mostly ours to write in any option: the catalog,
  inheritance, hot reload, the schema doc, the light layer. Building our
  own system would add about 2–3 weeks (estimate) just to get back to
  today's state, and we would hit again what the spike fixed (TAA motion
  vectors, frame timing, shadows).
- The fork carries features the unported effects need (alpha over
  lifetime, angular velocity, turbulence, orbits, colliders), which a
  ballistic vertex-shader system would have to grow one by one.

### Results

| | Result |
|---|---|
| Visual parity | Stone, arc and laser match legacy in offscreen captures, frame for frame. Differences: the arc's cubes stay one 30 fps frame longer (frame 117, about 700 pixels) and cast no floor shadow; stone sprays rest a little more on the floor. |
| Determinism | Same seed, same pixels: arc and laser runs are identical, pooled or not. Stone runs differ only by scene noise (2–17 pixels, before the hit as well). |
| Desktop (RTX 5080) | Pooled within 0.25 ms of legacy at every load. 200 impacts/s: 4.28 vs 4.10 ms. |
| Steam Deck | 200 impacts/s: 11.40 vs 9.86 ms (unpooled 89.06). 50/s: 8.67 vs 8.06. 10/s: 7.93 vs 7.59. 8 lasers or 8 arcs/s: +0.1 to +0.4 ms. |
| Memory | Flat in every run. Pools keep their high-water mark: +7 MB at 200/s, +24 MB with lasers and arcs. |
| Web | WebGPU in Chrome 156 on Windows (RTX 5080): all three effects, no console or validation errors. Other browsers not checked. |
| Fork size | 11,495 lines in `src/` (8,974 Rust, 2,521 WESL); against upstream +1,199 −206 Rust and +148 −14 WESL, so about 12% is ours (`UPSTREAM.md`, "Size"). |
| Build and tests | `bazel build --config=remote //kestrel/app:demo` and the 9 test targets of step 5 pass. |

### The options

Efforts are estimates for one person, with today's knowledge of the code.

**Adopt the fork** (recommended).
- To finish: port the burst effects (3–5 days, below), the catalog with
  inheritance and hot reload (3–5 days), one dispatch per pool and
  drawing only used slots (2–3 days), removing the legacy path (1 day).
  About 3–4 weeks in all. The atmosphere is extra, if it is moved at all.
- Gain: most effects in one system soon; a feature set we don't have to
  write; a working base for an editor later.
- Risks:
  - Upstream has one main author (343 of 368 commits), and activity has
    slowed since March. We don't depend on it: the code is vendored and
    already diverges.
  - The `bevy-0.20` branch we started from is unmerged. It doesn't matter
    for us now, but there is no upstream release to compare against.
  - Bevy upgrades are ours to do. The fork touches render internals that
    change every release (render graph nodes, bind groups, specialized
    mesh pipelines, extraction): perhaps 1–3 days per Bevy release on top
    of the app's own upgrade.
  - Drift: upstream fixes come in by hand, through `UPSTREAM.md`. Once our
    core diverges more (one dispatch per pool), merging gets harder. We
    accept that.
  - Editor: sprinkles' editor doesn't read our files (our wrapper, new
    fields). Hot reload of the file plus the debug UI is enough for the
    next phase; forking the editor is a later decision, once people tune
    effects regularly.
  - The per-burst path (local space, trails, sub-emitters) is still slow
    and has the burst-loss risk. Effects should stay on pooled features,
    or that path gets pooled first.

**Adopt and rewrite parts now.** Rewrite the runtime around pools (no
emitter entity per burst, one dispatch per pool) and delete what we don't
use (2D, editor mode, the `v0_1` migration, perhaps trails and
sub-emitters).
- Effort: 1–2 weeks before any porting.
- Gain: perhaps a third less code to carry through Bevy upgrades, a
  simpler core.
- Risk: we would delete features the unported effects may want, before we
  know which. Better done one piece at a time, when a task needs it (the
  dispatch per pool is the first).

**Build our own** (ballistic particles computed in a vertex shader from
spawn values and time, one instanced draw per effect).
- Effort: 2–3 weeks to reach today's state (format, loader, pooling,
  motion vectors, shadows, lights), then the same porting and catalog
  work.
- Gain: small (perhaps 2–3k lines), deterministic by construction, no
  compute shaders, so WebGL2 would work again, and few Bevy internals.
- Risks: collision only with an analytic floor; curves, turbulence,
  colliders and trails written one by one; it redoes work that now
  works. It stays the fallback if a Bevy upgrade of the fork ever costs
  more than a rewrite.

### Authoring

The `.ron` format is workable, with fixes the catalog step should make:

- AI writing: each file took one pass from the schema. Strict loading
  (`deny_unknown_fields` in both layers) turns typos into errors, and
  `vfx::tests` parses every file.
- Verbose: `impact_stone.ron` is 130 lines. Sparks and debris repeat the
  same timing, velocities, curve and collision, and the shrink curve is
  written three times. No inheritance or shared pieces yet.
- Pitfalls: particle colors are linear, light colors sRGB; scale is one
  scalar; curves can't do exponents below 1 (sampled by hand).
- Tuning: the files are compiled in with `include_str!`, so every change
  is a rebuild. Hot reload is the first thing tuning needs.

Sketches of two ports (not implemented):

- **Metal, Crystal and Polymer impacts.** The same structure as stone with
  other numbers from `impact::response` (sparks 48/20/0, debris 3/16/10,
  speed 11/6/2.8, colors, light 45,000/28,000/0; Polymer has no flash and
  no light). Each file is a copy of `impact_stone.ron` with about 15
  values changed, or 20 lines with a `base: "impact_stone"`. `effects::impact`
  then picks the file by `ImpactResponse`. The struck rock's emissive
  flash and kick (`ImpactTarget`) stay in Rust. Metal is also the laser's
  arrival impact, so that moves to the GPU too. About 1–2 hours each with
  captures (estimate).
- **Blood.** Placed at the cut, +Y along `direction`. Two droplet emitters
  (the two blood materials, 11 cubes each): spread about 30°, 2–4.8 m/s,
  gravity 13, `Rigid` floor collision, life 0.48–0.93 s, the shared
  shrink curve. Legacy adds a constant 1.5 m/s upward after normalizing,
  which `world_bias` can't express: a small fork field (a world-space
  velocity added at spawn) or an approximation. The 7 floor splats are
  harder: they lie on the floor under the cut (y = 0.012), not at it, and
  have random width and depth, but scale is one scalar. Options: a second
  effect, `blood_splat`, triggered by game code at the floor point, with
  a fixed flat box and a uniform scale range; or an emission option that
  projects onto the floor. About half a day (estimate).

The rest: deflection is a plain spray (easy). The trial burst needs 24
cubes evenly spaced on a ring (a small emission option, or random
directions). Blade echo needs the trigger's full rotation and a
non-uniform size (`Trigger` has only a normal). The dust ring needs a ring
mesh and alpha over lifetime (`ParticleMesh` has no custom mesh yet).
Fireflies and lightning are continuous and their point lights move with
them, which needs looping effects and lights driven from the CPU; they may
stay in `atmosphere.rs`.

### Listing effects

Characters have a directory of files whose names are their IDs, `base`
inheritance, a schema doc (`docs/character-definition.md`) and the
Character Studio preview. Effects have only `vfx::FILES`, a hand-written
list of `include_str!` names, and `app/vfx/README.md`. Missing: a catalog
built from the directory, `base`, a way to list and fire every effect
(the debug UI or the viewer), and the schema doc.

### Caveats

- The Deck numbers come from one session, on AC power. The stress
  harness's effects are all within about 7 m of the camera.
- The web was checked only in Chrome on Windows with an NVIDIA GPU.
- Nothing above covers the unported effects; their cost on the Deck is
  unknown until they are ported (blood's 3 s splats will hold pool slots
  longer than any effect so far).
- Two small capture differences are unexplained: the laser capture near
  the Metal impact (follows the build, not pooling) and the arc's extra
  frame.

### Evidence

All in the session scratchpad
(`%TEMP%\claude\C--cdc-personal-monorepo2\1c07bc38-df62-4d72-a031-7955ec2da889\scratchpad\`),
which is temporary:

- Parity, legacy top, sprinkles bottom: `step3/compare_stone_bias_f104-114_legacy_top_sprinkles_bottom.png`,
  `compare_arc_f113-117_...png`, `compare_laser_f37-39_f48-50_...png`,
  `compare_laser_zoom_f38_f49_...png`.
- Web: `step5/14_ab.png` (legacy | sprinkles hit), `17_bolt_frames.png`,
  `19_arc_frames.png`.
- Measurements: `step4/runs/<set>/summary.csv` (`pool` and `deckpool` are
  the pooled tables above), per-frame CSVs and JSON reports beside them;
  scripts `run_matrix.ps1`, `summarize.ps1`, `deck_*.sh`, and step 3's
  `capture.ps1` and `framediff.ps1`.

Worth keeping, in the PR description rather than the repo: the four step 3
comparison images (2.3 MB together), `step5/14_ab.png` and the `pool` and
`deckpool` summary CSVs. If they should live in the repo, put downscaled
copies in `kestrel/docs/vfx-spike/`. The rest (about 24 GB of frames and
runs) can be deleted, as can the Deck's `~/kestrel-vfx-spike/`.

## After the spike

In order. Each is a task in `todo/vfx.md`.

1. **Effect catalog.** Load every file in `app/vfx/` by name instead of
   `vfx::FILES`, with `base` inheritance (like characters), one color
   space, and hot reload in debug builds. A debug-menu list that fires
   any effect at the player.
2. **Schema doc** (like `docs/character-definition.md`), grown from
   `app/vfx/README.md`, with the sprinkles fields we use.
3. **Port the impact materials** (Metal, Crystal, Polymer) on `base`.
4. **Port blood, deflection and the trial burst.**
5. **One dispatch per pool** and **draw only used slots**; measure on the
   Deck with the stress harness.
6. **Port blade echo and the dust ring** (trigger rotation, custom mesh,
   alpha over lifetime in pools).
7. **Decide the atmosphere** (fireflies, lightning): port with looping
   effects or leave in `atmosphere.rs`.
8. **Remove the legacy path** once every ported effect has parity: the
   `VfxBackend` toggle, the legacy functions and `FxAssets` parts no
   longer used; `update_effects` keeps severed heads and laser timing.
9. **Stress harness:** keep `--vfx-stress` as a performance check until
   item 5 is measured, then keep it only if we want a regression
   check; drop `stats.rs` and the `KESTREL_SPRINKLES_NO_*` switches with
   the legacy path.
10. **Editor:** not needed now. Revisit when people tune effects
    regularly and hot reload plus sliders aren't enough.

## Findings

### Step 2: stone impact (2026-10-26)

**Format.** One RON file per effect in `kestrel/app/vfx/`, documented in
its `README.md`, loaded by `app/src/vfx.rs`. Top level is ours:
`version: 1`, `particles:` (a sprinkles `ParticlesAsset` embedded as is) and
`lights:` (color, intensity, range, radius, offset along the normal,
fade_seconds). The files are app-side, not in `content/data/`, because
`content` is read by the simulation; effects are view-only. They are compiled
in with `include_str!`; `vfx::parse` rejects unknown fields (both layers),
another `version` or `sprinkles_version`, emitters that aren't `one_shot`,
and systems that don't `despawn_on_finish`. `vfx::tests` parses every file
(it runs in `//kestrel/app:movement_test`). Placement: local +Y along the
trigger's normal; gravity and the floor are world space. Room to grow:
per-trigger parameters and decals become new top-level fields.

**Toggle.** `VfxBackend { Legacy, Sprinkles }` on the `Vfx` App service:
`--vfx legacy|sprinkles` (default legacy) and Effects → VFX backend in the
debug menu. `effects::impact` checks it for `ImpactResponse::Stone` only, as
a queued command (the choice needs the world); every other material, and any
run without GPU particles, draws the legacy effect. Covers the sword hit
(`combat.rs`) and the dig burst (`digging.rs`). Hit stop, shake and the rock's
emission flash are unchanged.

**Floor.** `vfx::place_floor` keeps one `ParticlesCollider3D` box (400 × 2 ×
400 m, top at y = 0) per world, `WorldScoped`. Every scene's floor is at
y = 0 today, as the legacy particles assumed. Dug pits and raised decks are
not modeled.

**Easy.** Authoring the effect by hand from the schema took one pass: cube
mesh particles with `YToVelocity` alignment, lit PBR materials with emissive
for bloom, a sampled shrink curve, `Rigid` collision with bounce and
friction all map directly onto the legacy numbers. sprinkles' shaders
compiled and ran on Vulkan (RTX 5080) without validation errors.

**Awkward.**
- Particle scale is one scalar (`position.w`), so the legacy sparks' random
  length with a fixed width became a uniform 0.67–1.33 scale of a 0.045 ×
  0.30 bar.
- No world-space bias on spawn directions: legacy sparks arc upward off a
  wall, ours spray a cone around the normal (todo: "Aim impact sprays like the
  legacy ones").
- Material colors are linear, our light colors sRGB (documented; the
  catalog step should pick one).
- A curve's power easing can't make exponents below 1, so `(1 - t)^0.65`
  is sampled as six linear points.
- `Rigid` collision only bounces above `2 / (1 + bounce)` m/s normal speed;
  slower particles stick and slide. Looks fine for sparks.
- Seeds are still random per spawn (`fixed_seed` per asset only), so two
  captures differ in detail. Step 3.

**Fork bugs fixed** (in `UPSTREAM.md`):
1. Motion vectors: the particle vertex shader never wrote
   `previous_world_position`, so with Kestrel's TAA every particle smeared
   into a dark, dithered blob and faded in late. Upstream likely never ran
   with TAA. Now position − velocity × the frame's simulated time.
2. Timing: the fork's per-frame systems ran unordered in `Update`, so a
   burst spawned by game code drew two frames late. Moved to `PostUpdate`
   before transform propagation; bursts now appear on the hit frame.
3. Typos: `deny_unknown_fields` on the whole asset schema (a known gap above).

**Visual result.** Offscreen captures in the boulder lab (a scratch demo
script striking the small ore rock three times; `combat_arena.json` strikes
zombies, never stone) match the legacy effect frame for frame after the two
fixes: the flash and light on the hit frame, the same spark glow and debris,
a similar spread, sparks bouncing and sliding on the floor, gone by the same
frame. Our sprays sit a little lower (no up bias), and a few more sparks
rest on the floor before shrinking away. Good enough to call parity for the
spike. `combat_arena.json` with `--vfx sprinkles` also renders cleanly.

**For later steps.**
- `bazel build --config=remote //kestrel/app:demo` builds the Linux binary
  for the remote platform and downloads nothing, so it does **not** update
  `bazel-bin/kestrel/app/demo.exe`. To get a Windows binary to run
  offscreen, use `bazel run --config=remote //kestrel/app:demo -- <args>`
  (for example with a bad `--vfx` value, which builds, then exits at once).
- A capture script that hits stone: `{"scene": "boulder-lab", ...}` with
  `walk [1.6, 1.6]`, then `strike [1.6, -0.1]` three times, 45 frames apart;
  the first hit renders at frame 104 at 30 fps.
- Per-spawn cost is as described in the gaps: each stone hit creates three
  emitters with their buffers and materials. Not measured yet (step 4).
- Trail meshes get zero motion vectors; check the laser bolt under TAA
  (todo: "Give trail particles motion vectors").

### Step 3: per-trigger parameters, arc and laser (2026-10-26)

**Parameter design.** A trigger is `vfx::Trigger { position, normal,
target, seed, scale }` (`app/src/vfx.rs`). `vfx::spawn` turns it into the
system's `Transform` (position, +Y along the normal, uniform scale) plus the
fork's `ParticleTrigger { target, seed }` component. The file says which
emitters use the trigger, with four new fields in the sprinkles schema
(documented in `app/vfx/README.md`, "Trigger parameters"):

```ron
emission: (shape: Segment(jitter: (0.06, 0.06, 0.06)),
           particles_amount: 270, particles_per_meter: Some(9.0)),
time: (lifetime: 1.0, one_shot: true, lifetime_to_target: true),
velocities: (world_bias: (0.0, 0.46, 0.0)),
```

Some values are resolved on the CPU at setup: the seed (replaces the random
per-spawn seed, mixed with the emitter index), the burst size
(`particles_per_meter`, capped by `particles_amount`, which sizes the
buffers) and the cycle length (the trip to the target at the slowest
speed). The rest go to the GPU: four fields appended to the emitter
uniforms (`trigger_target` in emitter space, `trigger_flags`, `world_bias`
in spawn space, `spawn_count`), read by `particle_simulate.wesl`. Scale is
the transform's scale. The fork already scaled positions, speeds and
gravity with it, and now scales particle sizes too. No arc- or
laser-specific code is in the fork.

**Arc.** `effects::arc` calls `vfx::spawn("arc", from → to, seed)`; `arc.ron` is
one `Segment` emitter, 9 cubes per meter, ±6 cm jitter, 0.13 s shrink.
Same call sites (`settlement.rs`, both `ArcFired` and chained `Impact::Arc`
hits). The legacy's minimum of 3 segments isn't kept (only matters under
0.33 m).

**Laser.** `emit_lasers` keeps spawning the `LaserBolt` entity for timing
(`laser_reached`, `CoverContacts`, the Metal impact on arrival, prediction
and `hand_over_cover` all unchanged), without a mesh when the effect draws
the bolt. `laser_bolt.ron` is one velocity-aligned 0.65 × 0.12 m bar at
80 m/s with `lifetime_to_target`, placed at the muzzle with +Y toward the
target and offset so its front starts 0.18 m out, as the legacy one did.
I kept the bookkeeping entity because gameplay must not depend on the GPU,
and the effect has no CPU feedback. The two agree because both use 80 m/s.

**Things that had to change to make the laser draw at all** (all in
`UPSTREAM.md`):
- `emit_lasers` runs after transform propagation (it needs the posed
  muzzle), after sprinkles' setup. Added a late setup pass
  (`SprinklesLateSetup`) and ordered it before `AssetEventSystems` and
  `VisibilityPropagate`. Without that, the new emitter's buffers and
  visibility arrive a frame late, the burst's only spawn step runs with no
  GPU buffers, and nothing ever spawns. Effects spawned in `Update` were
  unaffected.
- Upstream despawned a one-shot system the moment its cycle wrapped, one
  frame before its particles' last frame. Invisible on a 0.8 s spray; a
  two-frame bolt lost its in-flight frame.
- `YToVelocity` in world space was rotated by the emitter's rotation too.
  The laser bar stood upright. Step 2's sparks were also turned by the
  surface normal; nobody noticed.
- `visibility_aabb` was never applied; it is now (a far bolt or arc would
  have been culled with its emitter).

**Determinism.** Same seed, same particles: two arc runs are
pixel-identical over the arc's frames; two laser runs match on the bolt
frames. Stone runs differ by ≤ 9 pixels per frame, the same before the hit
(frame 103) as after, so that is scene noise, not particles. Impacts are
now seeded by the impact `number`, arcs by the tick or struck agent, bolts
by `laser_impact_seed`.

**Visual parity** (captures in the scratchpad's `step3/`, legacy top,
sprinkles bottom). Laser pit: the bolt is in the same place, at the same
angle and length on the same frames (38 and 49), and the impact lands on the
same frame. The spawn frame shows the bar at the muzzle a bit more than
legacy's 0.18 m stub. Combat arena arc: same chain, same density and
spread, same 4 frames. Legacy's arc casts a faint shadow streak on the
floor and ours shows none (todo: "Check particle shadows"). Boulder lab
stone with `world_bias`: the spray now leans up like legacy. TAA: no
ghosting on the bolt (mesh particle, not a trail, so the trail
motion-vector gap doesn't apply).

**Fork size** (`src/` vs upstream, CR-insensitive): Rust +418 −145, WESL
+84 −10 in total. Step 3 is about +320 −25 Rust (~50 lines are tests) and
+68 −9 WESL. Most deletions are step 1's removed loader.

**Easy.** Appending uniform fields (bytemuck layout, no encase) and new
serde fields. The simulation shader is readable and had obvious places to
hook in: `get_emission_offset`, spawn velocity, spawn lifetime.

**Awkward.**
- Frame timing. The fork's per-frame systems race Bevy's asset, visibility
  and transform systems, and a one-shot burst has exactly one spawn step,
  so any one-frame delay silently loses the whole burst (todo: "Don't lose
  a burst when its first step can't run").
- Three upstream bugs surfaced in one step. Upstream evidently isn't used
  with rotated world-space emitters, very short bursts or effects spawned
  late in the frame.
- `particles_amount` is still a fixed buffer size, so a distance-sized
  burst needs a cap (the arc reserves 270 slots, about 30 m).
- `ParticleTrigger` values are resolved at setup from the root entity's
  `Transform`; parented systems would need `GlobalTransform` after
  propagation.

**For steps 4–6.**
- Every laser shot is its own particle system set up in the late pass
  (buffers and material per shot, as with impacts). Step 4's 8 lasers
  will hit this path; measure both passes.
- Captures: `bazel run --config=remote //kestrel/app:demo -- --vfx bad`
  refreshes `demo.exe`. The scratch scripts are `step3/arc_hits.json`
  (combat arena, arc at frame 114), `laser_pit.json` (bolts at 38 and 49)
  and `stone_hits.json`; `step3/capture.ps1`, `framediff.ps1` (pixel diff
  of two runs) and `redcount.ps1` (finds bolt frames).
- The flicker tests (`laser_pit_flicker_test`, `laser_repair_flicker_test`)
  depend on GPU video targets that need local tools, and they run the
  default (legacy) backend, so they don't exercise this path. Not run.

### Step 4: burst performance (2026-10-27)

**Harness.** `app/src/vfx_stress.rs`, debug only:
`--vfx-stress impacts=N,lasers=P,arcs=A,frames=F,warmup=W,out=path`
fires effects around the player at fixed rates of game time, records every
frame and writes `path.json` (summary, once-a-second gauges) and `path.csv`
(per frame), then exits. It hooks in at three places: `Options::vfx_stress`,
the plugin in `main.rs`, and `effects::stress_bolt`. Delete those and the
file to remove it.

Two keys stop a run early, for fast iteration: `max_seconds=S` (a
wall-clock budget after the warmup) and `abort_ms=M` (the mean of the last
300 measured frames is over M ms). The report still has the stats of the
frames collected, with `aborted: true` and `abort_reason`, and
`summarize.ps1` marks such rows "OVER BUDGET (aborted, N frames)".

- Stone impacts go through `effects::impact(.., Stone)`, the call the sword
  and dig use, at seeded random points 1–7 m around the player, 0.1–1.2 m
  up, normals facing outward and up.
- Lasers: the bolt visual only, spawned directly with `stress_bolt`, which
  shares `spawn_bolt` with `emit_lasers` (same late-setup path, same
  `LaserBolt`). Eight muzzles on a 5 m ring, each firing every 0.4 s (the
  laser's 24-tick cooldown at 60 Hz) at a random point 4–12 m away (reach
  12): 20 bolts/s. No arrival impacts (those are legacy Metal impacts on
  both backends). Wiring 8 real players wasn't needed to load this path.
- Arcs: `effects::arc`, 8/s, 2–4 m long (8 arc-coil players at the 65-tick
  cooldown, without chains).
- Same seeded sequence on both backends.

**Method.** Windowed 1440×960, vsync off (the harness sets
`AutoNoVsync`), with a demo script (`boulder-lab`, 60 fps, one long wait).
A script run uses `TimeUpdateStrategy::ManualDuration`: every frame is
1/60 s of game time however long it takes, so each frame carries a 60 Hz
game's load and its wall time is what that load costs. Offscreen runs are
not usable for timing: the schedule runner sleeps each frame up to 1/fps and
every frame is read back. 4200 frames (70 s of game time), the first 600
left out; two runs each. Wall time per run: 12 s (idle) to 200 s
(sprinkles, 200/s). Measured per frame:

- `frame`: wall time `First` to `First`.
- `main`: the main schedule, `First`..`Last`.
- `extract`: Bevy's extract function, wrapped.
- Render-world segments, from marker systems between `RenderSystems` sets:
  queue, prepare, and graph (encoding, submit, present).
- GPU time per pass: `RenderDiagnosticsPlugin` timestamp spans. The fork
  now records `particle_compute` and `particle_sort` too.
- The fork's own systems: new `bevy_sprinkles::stats` counters.
- Once per game second: entity, emitter and asset counts, and wgpu's
  allocator report.
- Separately: Windows' per-process "GPU Process Memory\Dedicated Usage",
  sampled every 1.5 s.

Rendering is pipelined, so the render world runs a frame behind on its own
thread, and the frame takes as long as the slower of main + extract and
render. Raw results, scripts and per-frame CSVs are in the step 4
scratchpad (`run_matrix.ps1`, `summarize.ps1`, `runs/<set>/`).

**Desktop (RTX 5080, Vulkan).** Frame ms: mean / p95 / p99 / max. Mean of
2 runs; max over both. Run-to-run spread of the mean ≤ 0.2 ms, except
sprinkles at 200/s (≤ 1.3 ms). "Before" is upstream's per-frame
allocation. "After" is the first version of the fix below (v1: a uniform
buffer per emitter step). The final version (v2: one shared uniform
buffer) measured the same where its runs were clean: 45.5 and 46.1 ms at
200/s, 12.6 at 50/s, 12.9 for 50/s + lasers + arcs, 47.0 for the full
load. Some v2 runs were paced at exactly 16.7 ms (p95 ≈ 17 ms), probably
the display idling (it turns off after 30 min), and were thrown out.

| Load | Legacy | Sprinkles before | Sprinkles after |
|---|---|---|---|
| idle | 2.51 / 3.00 / 3.39 / 5.7 | 2.48 / 2.96 / 3.40 / 5.7 | 2.53 / 3.00 / 3.47 / 6.1 |
| 10 impacts/s | 2.56 / 3.05 / 3.48 / 5.2 | 4.46 / 5.17 / 5.66 / 11.5 | 4.56 / 5.33 / 5.91 / 11.2 |
| 50 impacts/s | 2.94 / 3.44 / 3.90 / 6.7 | 12.95 / 14.07 / 14.86 / 21.0 | 12.50 / 13.58 / 14.14 / 16.4 |
| 200 impacts/s | 4.01 / 4.60 / 5.14 / 7.3 | 47.79 / 50.76 / 53.74 / 60.3 | 45.56 / 48.10 / 50.53 / 55.7 |
| 8 lasers (20 bolts/s) | 2.54 / 3.08 / 3.83 / 7.1 | 2.70 / 3.23 / 3.68 / 6.5 | 2.66 / 3.17 / 3.58 / 7.1 |
| 8 arcs/s | 2.56 / 3.04 / 3.52 / 6.9 | 2.69 / 3.19 / 3.62 / 8.3 | 2.63 / 3.14 / 3.55 / 4.9 |
| 50/s + lasers + arcs | 2.97 / 3.52 / 3.97 / 8.8 | 13.45 / 14.60 / 15.39 / 19.8 | 12.96 / 14.09 / 14.75 / 20.5 |
| 200/s + lasers + arcs | 3.97 / 4.54 / 5.05 / 8.7 | 48.14 / 50.71 / 52.83 / 75.5 | 46.64 / 49.32 / 52.05 / 90.3 |

Where the time goes at 200 impacts/s (ms per frame, means). The last column
is "after" with `shadow_caster: false` on the impact's emitters (a
temporary build, not kept):

| | Legacy | Sprinkles before | after | after, no impact shadows |
|---|---|---|---|---|
| main world CPU | 1.91 | 2.44 | 2.43 | 2.31 |
| ...of which the fork's systems | – | – | 0.09 | 0.09 |
| extract | 0.26 | 0.72 | 0.70 | 0.68 |
| render world CPU | 3.67 | 46.93 | 44.72 | 26.00 |
| ...prepare | 0.60 | 27.67 | 26.28 | 14.22 |
| ...of which the fork's two prepare systems | – | 5.83¹ | 1.57 | 1.52 |
| ...graph (encode, submit, present) | 1.18 | 16.49 | 15.85 | 9.44 |
| GPU, all passes | 0.56 | 11.14 | 11.15 | 7.02 |
| ...particle compute + sort | – | 1.42 | 1.42 | 1.42 |
| ...Bevy's `uniform_allocation` + `bin_unpacking` | ~0 | 9.15 | 9.15 | 5.03 |
| buffers + bind groups the fork creates per frame | – | 352 + 700¹ | 10 + 20 | 10 + 20 |
| live emitters (legacy: particle entities) | 3213 | 492 | 492 | 492 |

¹ Same binary with `KESTREL_SPRINKLES_NO_REUSE=1`. Its frame time
(47.50 ms) matches the "before" binary's (47.79 ms).

**Steam Deck (measured).** Fullscreen 1280×800 in Desktop Mode, on AC, the
v1 build. 2400 frames, the first 600 left out. Idle, 50/s and 200/s were
run twice (spread ≤ 0.6 ms); the rest once. A full sprinkles pass with v2
matched every row within 0.3 ms (for example 10.12, 25.52 and 90.67 ms at
10, 50 and 200/s).

| Load | Legacy | Sprinkles (after) | Sprinkles GPU, all passes |
|---|---|---|---|
| idle | 7.11 / 7.54 / 7.74 / 11.5 | 7.16 / 7.62 / 8.09 / 11.3 | 5.5 |
| 10 impacts/s | 7.29 / 7.79 / 8.07 / 9.5 | 10.13 / 10.70 / 11.36 / 13.8 | 7.6 |
| 50 impacts/s | 7.79 / 8.38 / 8.96 / 12.1 | 25.77 / 26.88 / 27.45 / 29.2 | 14.3 (legacy 5.9) |
| 200 impacts/s | 9.57 / 10.10 / 10.59 / 13.8 | 91.20 / 95.42 / 97.61 / 104.7 | 33.5 (legacy 7.4) |
| 8 lasers | 7.15 / 7.61 / 8.25 / 10.9 | 7.36 / 7.83 / 8.14 / 11.8 | 5.6 |
| 8 arcs/s | 7.25 / 7.79 / 8.88 / 10.2 | 7.39 / 7.87 / 8.16 / 10.1 | 5.6 |
| 50/s + lasers + arcs | 7.84 / 8.51 / 9.31 / 11.4 | 26.33 / 27.49 / 27.93 / 29.0 | 14.5 |
| 200/s + lasers + arcs | 9.62 / 10.29 / 11.15 / 12.9 | 91.59 / 96.20 / 98.40 / 107.1 | 33.9 |

At 50/s the Deck spends 12.6 ms in render prepare and 9.3 ms in the render
graph, about twice the desktop's CPU time, and 14.3 ms on the GPU.
Sprinkles costs 0.3–0.4 ms of Deck frame time per impact per second, so
a 16.7 ms frame is exceeded from roughly 25–30 impacts/s. Legacy adds
2.5 ms at 200/s.

How the Deck was reached: `//kestrel/app:steamdeck` refuses to run on a
non-Linux host (`steamdeck.go` checks `GOOS`), and `steamdeck.local` isn't
in this PC's `known_hosts` (its IP and `steamdeck` are). So I built the
Linux binary with `bazel build --config=remote --remote_download_toplevel
//kestrel/app:demo`, copied it with the existing `~/.ssh/steamdeck` key to
`deck@192.168.2.89:~/kestrel-vfx-spike/` (not the launcher's install
directory), and ran it in the desktop session with the scratchpad's
`deck_run.sh`, which imports the session's environment the way
`deck_launch.sh` does. That folder (210 MB) can be deleted.

**Memory.** No growth, with or without the fix. Over each 60 s measured
window, wgpu's allocated bytes stay within ±0.1 MB: 197 MB idle, 205 MB at
200/s (Deck: 183–192 MB on either backend, flat). Live wgpu allocations
hold at about 23.7k at 200/s (legacy: 220). These are churned, not leaked. Windows'
dedicated usage stays at 603–611 MB through 200 s of wall time at 200/s
(legacy: 563 MB). The per-frame buffers and bind groups upstream creates
are freed once the GPU is done with them. That is churn, not the leak
upstream's `memory-leak` branch was looking for.

**What dominates.** Bevy's per-draw work for each live emitter, not
allocation and not the simulation:

- Every burst is a particle system entity, and every emitter in it is a
  mesh entity with its own material (the material binds that emitter's
  particle buffer). So no two emitters batch.
- Each emitter is its own bin in every view: prepass, main pass and four
  shadow cascades. Bevy's per-bin work then dominates: prepare
  (batching, indirect parameters), graph (encoding draws) and the GPU
  preprocessing passes (`uniform_allocation` and `bin_unpacking`: 9.2 ms
  of the 11.1 ms of GPU at 200/s).
- That comes to about 82 µs of render-world CPU per live emitter per frame
  on the desktop and about twice that on the Deck.
- Turning off shadows for the impact emitters (6 views down to 2) cuts the
  200/s frame from 45.6 to 26.8 ms.
- Legacy's 3,200 cubes share one mesh and a few materials, so they land in a
  handful of bins: +1.5 ms at 200/s.
- A stone hit is 3 emitters, and each stays drawn for the whole 0.8 s
  cycle. The 65 ms flash emitter is dropped from simulation after about
  0.14 s, but its mesh entity is still drawn until the system despawns.
- Spawning is cheap: 20 bolt systems/s with about 3 alive costs nothing
  measurable over idle (2.66 vs 2.53 ms), and the fork's main-world
  systems (setup, time step, uniforms) take 0.09 ms a frame at 200/s.
  So pools of re-armed systems (the plan's idea) would save little and
  weren't built.

**What the fix changed.** `ParticleComputeCache` and `ParticleSortCache`
(fork, `UPSTREAM.md`) keep the compute and sort GPU objects across frames.

- v2, the version kept: every emitter's step uniforms go into one shared
  uniform buffer, with one `write_buffer` per frame. Each emitter has one
  bind group with a dynamic offset per step, rebuilt only when something it
  binds changes. The colliders buffer and the sort uniform buffer are
  persistent too.
- v1, measured first: a persistent uniform buffer per emitter step, each
  written with its own `write_buffer`.
- At 200/s, per-frame creation drops from 352 buffers + 700 bind groups to
  20 bind groups (compute and sort, for the 10 new emitters) and no
  buffers (v1: 10 + 20).
  The fork's prepare drops from 5.8 to 0.7 ms on the desktop (v1: 1.6) and
  from 10.1 to 1.6 ms on the Deck (v1: 2.8).
- Desktop frames: −5% at 200/s (47.8 → 45.6 ms), −3.5% at 50/s, nothing
  measurable at 10/s. v1 and v2 are the same.
- Deck frames, against upstream's allocation (the v1 binary with
  `KESTREL_SPRINKLES_NO_REUSE=1`), each run back to back with the other:
  - 50/s: −4% (26.62 → 25.47 ms).
  - 200/s: +4% (87.3 → 90.7 ms; two pairs for v2 and two for v1, which was
    just as slow at 90.6–91.2).
  - At 200/s the fork's prepare saves 8.5 ms, but the rest of the render
    world's prepare grows by about 13 ms and the main world by 1 ms. Cause
    not found. Candidates: the Deck's 4 cores shared differently between
    the parallel prepare systems, or long-lived descriptor sets slowing
    Bevy's own per-frame bind group allocation in RADV (todo).
- Timing and seeding are unchanged. Re-running step 3's captures with
  `--vfx sprinkles` gives a pixel-identical laser (frames 36–51) and arc
  (110–120) to step 3's final runs. Stone differs by 2–15 pixels per frame,
  before the hit as much as after (scene noise, as in step 3). The burst-loss
  risk is unchanged: the fix touches only render-world objects, not when an
  emitter's first step runs.

**Risks before pooling** (pooling, below, addresses the first).

- The cost scales with live emitters × views. Effects with long-lived or
  many emitters, or more shadow cascades, make it worse. The fix is to draw
  all live bursts of one effect emitter as one entity, with one material,
  one mesh and one particle buffer split into burst slots.
- Cheaper mitigations: hide an emitter once all its particles are dead (a
  third fewer draws for the stone impact), and no shadows on small sparks
  (−41% at 200/s; legacy sparks do cast shadows).
- Even 10 impacts/s, roughly 8 players hitting stone at the sword's
  cooldown, costs the Deck 2.8 ms more than legacy (+0.2 ms).
- The harness spawns bolts without arrival impacts, and its impacts aren't
  occluded or culled by distance (all within about 7 m of the camera
  target). Real fights spread effects wider, so some emitters would be
  culled from the main view, but not from the shadow cascades.
- The Deck numbers come from one session, on AC power with the "auto"
  power profile. A thermally throttled or battery-powered Deck would be
  slower.

#### Pooled bursts (step 4, second part)

**Design** (full description in `vfx/sprinkles/UPSTREAM.md`, "Pooled
bursts").

- Each (effect asset, emitter) gets pools: one particle buffer split into
  slots of `particles_amount`, one material, one mesh and one entity that
  draws every slot. Pools start at 32 slots, then double to 256 per pool.
- A burst keeps its emitter entity and runtime (time steps, seed, trigger,
  late setup), but simulates only its slot. Its `slot_offset` is added to
  buffer accesses only; indices and seeds stay local, so determinism holds.
- A despawned burst frees its slot, and the slot's bytes are zeroed before
  the next simulation. Each pool is sorted once, not per burst.
- Non-aligned particles carry their burst's rotation as a quaternion
  (needed for the stone flash).
- Effect files and `vfx::Trigger` are unchanged. All three effects pool.
  Emitters in local space, with trails or sub-emitters, keep the
  per-burst path. `KESTREL_SPRINKLES_NO_POOL=1` switches pooling off for
  A/B runs in the same binary.
- Fork size for pooling: about +480 Rust (`pool.rs` 271 lines, ~75 of them
  tests) and +64 −4 WESL.

**Method.** As above, all in one session per machine: legacy and pooled
twice each, per-burst once with `abort_ms=40`, which stops the 200/s runs
after 300 measured frames. The runner kept the display awake this time
(`SetThreadExecutionState`), and no run shows 60 Hz pacing.

Desktop (RTX 5080), frame ms mean / p95 / p99 / max:

| Load | Legacy | Sprinkles pooled | Sprinkles per-burst |
|---|---|---|---|
| idle | 2.60 / 3.15 / 3.65 / 5.0 | 2.70 / 3.34 / 3.81 / 7.4 | 2.60 / 3.12 / 3.57 / 4.6 |
| 10 impacts/s | 2.94 / 3.64 / 4.13 / 8.4 | 3.07 / 4.14 / 4.61 / 8.1 | 4.60 / 5.39 / 5.96 / 8.8 |
| 50 impacts/s | 3.03 / 3.61 / 4.12 / 6.5 | 3.13 / 3.69 / 4.21 / 7.5 | 13.02 / 14.47 / 15.20 / 16.7 |
| 200 impacts/s | 4.10 / 4.74 / 5.23 / 8.4 | 4.28 / 5.61 / 6.12 / 21.4 | 47.40 / 52.48 / 61.40 / 68.1 (aborted) |
| 8 lasers | 2.85 / 3.62 / 4.06 / 31.5 | 2.81 / 3.51 / 4.38 / 9.6 | 2.81 / 3.36 / 3.77 / 9.2 |
| 8 arcs/s | 2.90 / 3.72 / 4.15 / 20.5 | 2.91 / 3.53 / 3.97 / 5.5 | 2.79 / 3.37 / 3.82 / 5.8 |
| 50/s + lasers + arcs | 3.07 / 3.62 / 4.05 / 4.9 | 3.31 / 3.92 / 4.34 / 5.6 | 13.67 / 15.52 / 20.39 / 25.7 |
| 200/s + lasers + arcs | 4.20 / 4.88 / 5.39 / 7.5 | 4.45 / 5.40 / 6.40 / 10.7 | 47.02 / 49.76 / 52.31 / 54.4 (aborted) |

Steam Deck (measured), frame ms mean / p95 / p99 / max:

| Load | Legacy | Sprinkles pooled | Sprinkles per-burst |
|---|---|---|---|
| idle | 7.40 / 7.88 / 8.11 / 11.6 | 7.47 / 7.97 / 8.25 / 11.7 | 7.45 / 7.94 / 8.25 / 10.4 |
| 10 impacts/s | 7.59 / 8.17 / 8.48 / 11.4 | 7.93 / 8.44 / 8.76 / 11.4 | 9.98 / 10.61 / 11.53 / 12.5 |
| 50 impacts/s | 8.06 / 8.63 / 8.95 / 10.9 | 8.67 / 9.19 / 9.78 / 11.6 | 25.87 / 27.03 / 27.51 / 28.5 |
| 200 impacts/s | 9.86 / 10.33 / 10.58 / 13.5 | 11.40 / 12.04 / 12.34 / 13.9 | 89.06 / 92.58 / 93.74 / 99.3 (aborted) |
| 8 lasers | 7.45 / 7.96 / 8.26 / 11.1 | 7.59 / 8.09 / 8.45 / 13.2 | 7.61 / 8.08 / 8.34 / 10.1 |
| 8 arcs/s | 7.53 / 8.08 / 8.41 / 11.1 | 7.95 / 8.50 / 8.86 / 11.6 | 7.65 / 8.14 / 8.39 / 10.8 |
| 50/s + lasers + arcs | 8.09 / 8.71 / 9.24 / 11.5 | 9.06 / 10.49 / 11.28 / 14.0 | 26.57 / 27.79 / 28.38 / 46.6 |
| 200/s + lasers + arcs | 9.93 / 10.48 / 10.82 / 12.5 | 11.95 / 12.58 / 12.86 / 14.3 | 90.80 / 94.44 / 95.59 / 96.5 (aborted) |

Repeats agree within 0.6 ms (desktop pooled 200/s: 4.01 and 4.55; Deck
within 0.15 ms). This
session's Deck idle is 0.3 ms slower than the earlier one, so compare
within a table, not across tables.

At 200 impacts/s (ms per frame, means):

| | Desktop legacy | Desktop pooled | Deck legacy | Deck pooled | Deck per-burst |
|---|---|---|---|---|---|
| main world CPU | 2.00 | 1.81 | 5.37 | 4.72 | 6.79 |
| extract | 0.27 | 0.40 | 0.72 | 1.17 | 2.05 |
| render world CPU | 3.76 | 3.81 | 8.98 | 10.04 | 86.78 |
| ...prepare | 0.61 | 1.09 | 1.24 | 2.25 | 53.03 |
| ...graph | 1.22 | 1.98 | 3.30 | 5.00 | 28.57 |
| GPU, all passes | 0.56 | 2.19 | 7.42 | 10.02 | 33.04 |
| ...particle compute + sort | – | 1.41 | – | 2.50 | 3.42 |
| pools (draws per view) | – | 9 | – | 9 | 492 emitters |

**Memory.** Still flat within every run. Pools persist at their high-water
mark: wgpu's allocated memory at 200/s is 211 MB pooled vs 205 MB
per-burst and 204 MB legacy, 228 MB for the full load. The arc's pool is
270 particles × 32 slots (+14 MB alone).

**Visual parity and determinism.** Step 3's captures with pooling:

- Arc: pixel-identical (frames 110–120).
- Stone: scene noise only (2–17 pixels a frame, before the hit as well).
- Laser: the bolt frames (38, 49) and the frames after them are identical.
  Frames 40–47 differ by up to 37 pixels, all inside the box x 740–782,
  y 323–354, where the bolt lands and the legacy Metal impact sparks.
  Cause not found.
- Two pooled runs are pixel-identical (laser frames 36–60, arc 110–120),
  so same seed, same pixels holds.
- A busy stress frame (50/s + lasers + arcs) looks like legacy's
  (`stressframes_*` in the scratchpad).

A first pooled build drew a finished bolt for one extra frame: a pool whose
last burst ended was still drawn that frame but no longer sorted. Pools are
now sorted in the frame a slot is freed too.

**Verdict.** Pooling makes sprinkles viable against legacy on the Deck.
- At 200 impacts/s the Deck runs 11.4 ms a frame against legacy's 9.9
  (+1.5 ms, inside a 16.7 ms frame), instead of 89 ms.
- At 50/s it is +0.6 ms; at 10/s +0.3 ms.
- On the desktop pooled and legacy are within 0.25 ms.

**What is left.**
- **GPU dispatches.** Each burst still has its own compute dispatch. That
  is 2.5 ms of Deck GPU at 200/s (1.4 ms desktop), most of the remaining
  gap. Bursts writing one buffer may also be serialized by barriers. Next
  step: one dispatch per pool, with per-burst parameters in a storage
  buffer.
- **Instances drawn.** A pool draws all its slots' instances, live or not.
  Small for the stone impact (12–18 particles a slot), not for the arc (270
  a slot, 8,640 instances in its first pool): the Deck's arc-only load is
  +0.4 ms over legacy. Drawing up to the highest used slot, or smaller
  pools for big emitters, would fix it.
- **Never freed.** Pools are never freed or shrunk, and live edits to a
  pooled emitter's mesh, material or particle count don't reach existing
  pools. Fine for compiled-in effect files; an editor would need it.
- **Not pooled.** Local-space, trail and sub-emitter emitters keep the
  per-burst path and its cost. None of today's three effects use them.
- **Earlier risks.** The burst-loss risk and the Deck reuse slowdown (above)
  are unchanged, but the per-burst path they concern no longer carries
  Kestrel's effects.

### Step 5: web build on WebGPU (2026-10-27)

**What changed.**
- Root `Cargo.toml`: Bevy's `webgl2` feature is now `webgpu`.
  `cargo metadata --offline` updated `Cargo.lock`. The GL backend crates
  drop out (`glow` 0.17, `glutin_wgl_sys` 0.6, `wgpu-core-deps-wasm`, and
  `wgpu-hal`'s GL, EGL and web deps). `wgpu` gains `naga` and
  `wasm-bindgen-futures` for its browser backend. Native `wgpu-hal`
  loses its GLES backend too, which nothing used (Vulkan, DX12, Metal).
- `MODULE.bazel`: removed the `crate.annotation_select` that forced
  `wgpu-hal`'s `gles` feature on wasm32. It existed only for WebGL and
  would now pull a `glow` that isn't in the lockfile. Bazel then dropped
  the same three crates from `MODULE.bazel.lock` (3 lines), and the
  crate-universe side resolves for every platform triple.
- `app/src/scene.rs`: the camera gets SSAO and TAA on the web too (the
  WebGL2 exception is gone). `app/src/main.rs`: `SprinklesPlugin` is added
  on every rendered build, wasm included.
- Fork, `stats.rs`: the step 4 timers used `std::time::Instant`, which
  panics on `wasm32-unknown-unknown` ("time not implemented on this
  platform"). The first web run aborted at startup on it. They now use
  `web_time::Instant` on wasm32, as `runtime.rs` already did
  (`UPSTREAM.md`). `web-time` was already a wasm-only dependency of the
  fork, so no build change.
- Nothing in `app/web/` or `bazel/web/` assumed WebGL2 (no wasm-bindgen
  flags, canvas context or HTML checks), so nothing changed there.
- `app/README.md`: the web build needs a WebGPU browser (Chrome or Edge
  113+ on Windows, macOS and ChromeOS, Firefox 141+ on Windows, Safari 26;
  Linux Chrome may need the flag). There is no WebGL2 fallback.
- Left as is: other `wasm32` cfgs are for audio, threads, files and
  menus, not WebGL. The stress harness stays native-only (it uses
  `std::time` and writes files). On wasm it compiles but isn't added,
  which shows up as dead-code warnings.

**Verified in a browser (how).** The Claude-in-Chrome extension wasn't
available. Instead I started the installed Chrome 156 with a scratch
profile and `--remote-debugging-port` and drove it over the DevTools
protocol from PowerShell (`step5/cdp.ps1` in the scratchpad: navigate,
evaluate, click, key, screenshot, screencast). The page was served by
`bazel run --config=remote //kestrel/app:web`. Windows 11, RTX 5080.
- `navigator.gpu` exists. `requestAdapter()` returns vendor `nvidia`,
  architecture `blackwell`, `isFallbackAdapter: false`. Bevy logs
  `backend: BrowserWebGpu`. Adapter limits:
  `maxStorageBuffersPerShaderStage` 16, `maxComputeWorkgroupSizeX` 1024,
  `maxDynamicUniformBuffersPerPipelineLayout` 10. The particle compute pass
  binds 5 storage buffers and one dynamic uniform, inside WebGPU's
  defaults.
- Console after the `stats.rs` fix: no panics, no wgpu or WebGPU
  validation errors, no uncaptured errors over the whole session (menu,
  five scenes, about 15 effects). Chrome's own log
  (`chrome_debug.log`) agrees. The only warnings are Chrome's
  `powerPreference` notice and bevy_egui's "TEXTURE_BINDING_ARRAY is not
  supported", both harmless.
- Effects with Effects → VFX backend set to Sprinkles in the debug menu
  (`--vfx` can't be passed on the web):
  - **Stone impact** (Material Lab, sword on the ore rock): flash at the
    contact, velocity-aligned spark bars, sparks bouncing on the floor.
    A/B in the same page: a legacy hit raised the entity count from 1096
    to 1119 (CPU particles); a sprinkles hit left it at 1096 and still
    drew the sparks, so they come from the GPU path.
  - **Laser bolt** (Blackout Hall): a screencast caught the red
    velocity-aligned bar crossing the hall over consecutive frames, then
    the legacy Metal impact on cover.
  - **Arc** (Red Clearing, arc coil on `V`, against zombies): chained arcs
    of blue cubes between the player and several zombies, on two shots.
- The scene renders with SSAO, TAA, bloom and shadows at 60 fps (the
  display rate). No ghosting showed on the bolt, but I didn't compare
  TAA closely against native.
- Screenshots are in the scratchpad's `step5/`: `08_material_lab.png`,
  `10_strike_a_zoom.png`, `14_ab.png` (legacy | sprinkles), `17_bolt_frames.png`,
  `19_arc_frames.png`, and the raw screencasts in `step5/cast/`.

**Not verified.**
- Only the pooled path. `KESTREL_SPRINKLES_NO_POOL` is an environment
  variable, which the browser doesn't have, so the web always pools and
  the unpooled path couldn't be compared. No pooled-path problems showed.
- Other browsers and platforms: only Chrome on Windows with an NVIDIA GPU
  (todo: "Check the web build in Firefox, Safari and on Linux").
- GPU timings on the web (whether `RenderDiagnosticsPlugin` gets timestamp
  queries there) and web frame times under load.

**Native.** `bazel build --config=remote //kestrel/app:demo` passes.
The 9 tests pass with `bazel test --config=remote`: `combat_arena_test`,
`combat_replay_test`, `combat_weapons_test`, `control_protocol_test`,
`forest_motion_test`, `movement_test`, `steamdeck_test` (cached),
`bevy_sprinkles_test`, `parse_test`. This is every test in
`//kestrel/app/...` and `//kestrel/vfx/...` except the two GPU-video
flicker tests. Offscreen captures with `--vfx sprinkles` (refreshed
`demo.exe`, step 3's scripts), against step 4's pooled runs:
- Arc: 1 pixel a frame over frames 108–122.
- Laser: bolt frames (38, 49) identical. Frames 40–47 differ by up to 37
  pixels in the impact box (x 756–773, y 335–352). Today's pooled run
  matches step 4's per-burst run (`v2laser_sprinkles`) exactly, and
  today's `KESTREL_SPRINKLES_NO_POOL=1` run shows that box's difference
  instead. So the step 4 "pooled laser difference" follows the build, not
  pooling (added to that todo).

**Problems found.**
- The step 4 `stats.rs` timers panicked on wasm (fixed above). Anything
  else in the fork or the app that calls `std::time` on wasm will do the
  same. A grep finds only the stress harness, which isn't added there.
- `--vfx` and the other flags can't be given on the web (todo: "Pass
  options to the web build from the URL").
- A synthetic DevTools click on Play Solo highlighted it but didn't start
  the game; Enter did. I didn't look into why (likely press and release in
  the same frame). It doesn't affect real users.
