# Kestrel

The playable Kestrel prototype and its game-specific tooling live here:

| Path | Purpose |
|---|---|
| [`app/`](app/README.md) | Bevy executable, scene recipes, assets, demo scripts and gameplay tests |
| [`sim/`](sim/) | Deterministic gameplay simulation |
| [`debug_ui/`](debug_ui/README.md) | Debug overlay (egui) and routed gameplay input |
| [`net/`](net/) | Multiplayer transport and protocol |
| [`tools/assetc/`](tools/assetc/) | Build-time voxel asset compiler |
| [`tools/demo/`](tools/demo/) | Controlled demo runner, client and trace assertions |
| [`build/voxel_assets/`](build/voxel_assets/README.md) | Bazel voxel asset rules |
| [`build/game_demo/`](build/game_demo/README.md) | Bazel video demo and simulation test rules |
| [`docs/`](docs/README.md) | Current system documentation |

Run the game with `bazel run --config=full_local -c opt //kestrel/app:demo` and its tests with
`bazel test --config=full_local -c opt //kestrel/...`.
To build and launch it on a Steam Deck in Desktop Mode over SSH, use
[`//kestrel/app:steam_deck`](app/README.md#run-on-a-steam-deck-over-the-network).

Browse the design docs with `bazel run //kestrel/docs` and open the printed
local URL.

Build the Linux targets on remote workers from any client platform with
`bazelisk build //kestrel/... --config=remote`. The remote configuration selects
the pinned Linux execution image and a Linux action environment. Windows
linker options belong to the Windows platform and do not apply to this build.

`bazelisk run //kestrel/app:demo --config=remote` builds for the client OS and CPU,
using the Linux workers for compilation. On Windows this cross-compiles a
Windows executable, downloads the executable and its runfiles, and launches it
locally. The `host_platform` setting describes build tools; the `run` target
platform is selected separately from the actual client machine.

The manual video demo targets still require local GPU drivers; FFmpeg is
fetched from pinned archives on supported hosts. They are excluded from the
recursive build above. For example, build a video locally with
`bazelisk build //kestrel/app:combat_weapons_demo` on Windows or Linux.
