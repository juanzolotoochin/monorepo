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
