# Game

The playable Kestrel prototype and its game-specific tooling live here:

| Path | Purpose |
|---|---|
| [`app/`](app/README.md) | Bevy executable, scene recipes, assets, demo scripts and gameplay tests |
| [`core/`](core/README.md) | Prototype gameplay model and navigation |
| [`tools/assetc/`](tools/assetc/) | Build-time voxel asset compiler |
| [`tools/demo/`](tools/demo/) | Controlled demo runner, client and trace assertions |
| [`build/voxel_assets/`](build/voxel_assets/README.md) | Bazel voxel asset rules |
| [`build/game_demo/`](build/game_demo/README.md) | Bazel video demo and simulation test rules |
| [`docs/`](docs/architecture.md) | Planned deterministic game architecture |

Run the game with `bazel run //game/app:demo` and its tests with
`bazel test --config=full_local //game/...`.

Build the Linux targets on remote workers from any client platform with
`bazelisk build //game/... --config=remote`. The remote configuration selects
the pinned Linux execution image and a Linux action environment. Windows
linker options belong to the Windows platform and do not apply to this build.

`bazelisk run //game/app:demo --config=remote` builds for the client OS and CPU,
using the Linux workers for compilation. On Windows this cross-compiles a
Windows executable, downloads the executable and its runfiles, and launches it
locally. The `host_platform` setting describes build tools; the `run` target
platform is selected separately from the actual client machine.

The manual video demo targets still require local GPU drivers; FFmpeg is
fetched from pinned archives on supported hosts. They are excluded from the
recursive build above. For example, build a video locally with
`bazelisk build //game/app:combat_weapons_demo` on Windows or Linux.
