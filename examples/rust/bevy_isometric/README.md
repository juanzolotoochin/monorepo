# Kestrel // 07

A voxel sci-fi action prototype in Bevy 0.18.1, built with Bazel. An armored
salvager carries an industrial cleaver across a suspended mining deck. Cuboid
geometry, materials, effects and sound are generated locally; no art downloads
or runtime asset paths are required.

Asset provenance: models and effects are authored in this example's Rust code
and RON recipes;
the swing, clang and footsteps are original procedural synthesis in `sound.c`
(noise, low tones and inharmonic ringing modes). Miniaudio provides playback,
not sound samples. The HUD uses Bevy's bundled default font.

```sh
bazel run //examples/rust/bevy_isometric:demo
bazel test //examples/rust/bevy_isometric:movement_test
```

To cross-compile a Windows x64 release from Linux or macOS, use the MSVC
target platform. The compiler and build scripts run on the host; Rust and the
native audio code target Windows through hermetic Clang/LLD, the Microsoft
runtime and Windows SDK. No host Visual Studio installation is needed.

The upstream toolchain requires explicit acceptance of both Microsoft licenses.
After accepting the [runtime and SDK license terms](https://github.com/hermeticbuild/hermetic-llvm/tree/v0.8.24#windows), run:

```sh
bazel build -c opt \
  --platforms=//platforms:rust_windows_x86_64 \
  --repo_env=BAZEL_MSVC_RUNTIME_VISUAL_STUDIO_EULA=1 \
  --repo_env=BAZEL_WINDOWS_SDK_EULA=1 \
  //examples/rust/bevy_isometric:demo
```

Copy `bazel-bin/examples/rust/bevy_isometric/demo.exe` to Windows. All scene
assets and sound synthesis are embedded in the executable. The default MSVC
configuration links the runtime dynamically: install the current
[Visual C++ v14 x64 Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist)
if `VCRUNTIME140.dll` is unavailable. A compatible GPU driver is also needed.
Cross-compilation verifies the build; rendering, input and audio still need a
smoke test on Windows. Use `bazel build`, rather than `bazel run` or `bazel test`,
for Windows targets on a Linux/macOS host. To compile the headless test executable,
use the same flags with `//examples/rust/bevy_isometric:movement_test`.

Windows integration notes: BLAKE3 uses intrinsics to avoid the host-only MASM
assembler; Bazel drives the MSVC link action and supplies the windowing/pipe
SDK libraries. `Cargo.lock` intentionally resolves `gpu-allocator`'s broad
Windows API dependency to `windows 0.58.0`, matching `wgpu-hal`'s DirectX types.
Other dependencies still use `windows 0.57.0`; preserve that separate resolution
when updating the lockfile.

Audio initialization and shutdown run on a dedicated thread. On Windows this
keeps miniaudio's multithreaded COM apartment separate from winit's window-thread
OLE apartment. The Windows test executable includes a regression for this
startup sequence; run it on Windows with:

```powershell
.\movement_test.exe sound::tests::audio_startup_leaves_the_window_thread_compatible_with_ole --exact --nocapture
```

| Control | Action |
| --- | --- |
| 1 / 2 | Load the mining deck / boulder lab |
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

Code is split into `assets.rs` (compiled scenes and prefab caching),
`scene.rs` (shared deck, character art and HUD), `gameplay.rs` (controls and
combat), `navigation.rs` (paths and clearance), `effects.rs` (impact feedback),
`sound.rs` / `sound.c` (procedural foley), and `preview.rs` (visual QA).
Headless tests cover obstacle routing, slide collision, blade reach and sweep
consistency, attack timing, one-contact recoil, hit-stop and effect cleanup.

Third-party versions come from the root `Cargo.toml` / `Cargo.lock`.
The targets expose that manifest as a compile-time input because Bevy's
derive macros read it to find the `bevy` crate path. First-party code builds
with Bazel, following the other Rust examples in the repository.


Boulders now use the [Bazel voxel asset pipeline](../../../bazel/voxel_assets/README.md).
Edit recipes in `assets/prefabs/`, material values in `assets/palette.ron`, and
placements in `assets/levels/`. `assets/BUILD.bazel` declares reusable prefab
and scene targets. The generator loops live in `//tools/assetc/core`; changing
a recipe rebuilds its mesh during the Bazel build. The deck's other props and
character parts are still authored in `scene.rs`.

Press **1** or **2** to load the mining deck or boulder lab. Both use the same
deck stage; the lab hides the industrial dressing. Baked assets are shared
between the two layouts. The lab also shows runtime seed/size overrides and
repeated instances that reuse one generated mesh. The HUD's `DYNAMIC BUILDS`
count stays unchanged on repeat visits because geometry is cached. Scene
changes reset movement, attacks and contact feedback.

```sh
bazel run //examples/rust/bevy_isometric:demo -- --scene boulder-lab
bazel build //examples/rust/bevy_isometric/assets:ore_small
```

For visual QA of either scene, add `--scene mining-deck` or `--scene boulder-lab`
to the existing `--showcase --capture-dir ...` command. All packs are embedded,
including in the Windows `.exe`.

The lab includes labeled **metal**, **crystal**, **polymer**, and **mixed** samples.
Material palettes (`assets/{metal,crystal,polymer,mixed}.ron`) assign an `impact`
response independently of their visual color, emission, and roughness. Stone
chips, metal throws hot sparks, crystal pulses blue and chimes, and polymer
absorbs the blow with a muffled knock and greater displacement.

The mixed sample uses four material quadrants: strike different sides to try
all four reactions on one asset. The HUD reports the last response under `HIT`.
Existing ore boulders also mix stone and metal
reactions. Exact sword-segment/mesh contact selects the struck material after
broad-phase collision checks. Only that material slot on that instance flashes;
authored emission is restored afterward. Meshes and CPU contact triangles are
cached; reactive material handles belong to each instance and are removed when
switching scenes. The contact query currently scans nearby targets' triangles;
it has no spatial acceleration structure for very large models.
