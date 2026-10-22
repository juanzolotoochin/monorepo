# Kestrel // 07

A voxel sci-fi action prototype in Bevy 0.18.1, built with Bazel. An armored
salvager carries an industrial cleaver across a suspended mining deck. Cuboid
geometry, materials, effects and sound are generated locally; no art downloads
or runtime asset paths are required.

Asset provenance: models and effects are authored in this example's Rust code;
the swing, clang and footsteps are original procedural synthesis in `sound.c`
(noise, low tones and inharmonic ringing modes). Miniaudio provides playback,
not sound samples. The HUD uses Bevy's bundled default font.

```sh
bazel run //examples/rust/bevy_isometric:demo
bazel test //examples/rust/bevy_isometric:movement_test
```

| Control | Action |
| --- | --- |
| Right click / hold | Move / steer toward the cursor; find a route around obstacles |
| WASD / arrows | Camera-relative movement; cancel the click route |
| Shift | Run |
| Shift + left click | Face the cursor and commit to one heavy sword strike |
| Mouse wheel | Smooth camera zoom |
| R | Reset character position and actions |
| M | Mute / unmute |
| Esc | Quit |

Move within about two world units of a rock and strike toward it. The blade
has a deliberate wind-up, fast cut and long recovery. Contact stops the blade,
briefly freezes the character, then kicks the sword back. Rock strikes create
hot voxel sparks, bouncing chips, a brief light, a small camera kick and a
procedural metallic clang. One cut produces at most one contact. Click again
near the end of recovery to buffer the next strike. The HUD shows the actual
weapon state and contact count. Rocks and props are solid, indestructible
surfaces; there are no enemies, health or loot yet.

Movement uses acceleration, braking, smoothed facing, animated legs and
footsteps. Click routes use a half-unit A* grid with body clearance, diagonal
collision checks and line-of-sight smoothing. Local collision uses substeps
and sliding. Attack collision sweeps the blade through angle space, so a fast
cut cannot skip a rock between rendered frames. Impact particles and lights
expire automatically.

The lighting combines warm shadows, cool fill, ambient occlusion, temporal
antialiasing and restrained bloom. Run on a desktop with a working Vulkan
(Linux) or Metal (macOS) driver. Linux uses X11 / XWayland and a PulseAudio or
PipeWire audio service; audio is optional if no output device is available.
Audio uses the repository's pinned miniaudio, without host development headers.

For a repeatable approach-and-strike preview with rendered screenshots:

```sh
bazel run //examples/rust/bevy_isometric:demo -- \
  --showcase --capture-dir /tmp/kestrel
```

This writes `overview.png` and `impact.png`, then exits. `--showcase` alone
plays the short sequence and leaves the window open for further interaction.

Code is split into `scene.rs` (voxel art and HUD), `gameplay.rs` (controls and
combat), `navigation.rs` (paths and clearance), `effects.rs` (impact feedback),
`sound.rs` / `sound.c` (procedural foley), and `preview.rs` (visual QA).
Headless tests cover obstacle routing, slide collision, blade reach and sweep
consistency, attack timing, one-contact recoil, hit-stop and effect cleanup.

Third-party versions come from the root `Cargo.toml` / `Cargo.lock`.
The targets expose that manifest as a compile-time input because Bevy's
derive macros read it to find the `bevy` crate path. First-party code builds
with Bazel, following the other Rust examples in the repository.
