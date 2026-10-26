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

## Optional personal Neovim setup (Windows)

The game crates and normal builds are defined by Bazel. The root `Cargo.toml`
only resolves third-party dependencies. The personal Neovim setup projects
evaluated Bazel metadata into an ignored Cargo editor workspace exclusively
for Kestrel under `kestrel/`. Other editors and normal builds do not use it.

From the repository root, using PowerShell 7:

```powershell
./tools/setup-rust-analyzer.ps1
python tools/kestrel-neovim/cache.py prepare --all-kestrel
python tools/kestrel-neovim/cache.py fetch # Once if registry entries are missing
./tools/kestrel-neovim/install.ps1
nvim kestrel/app/src/flow.rs
```

The installer preserves the entire previous Rust adapter and installs a scoped
router through the existing `user.lsp.settings` loader. Restart Neovim after
installation. Plain `nvim` then uses `kestrel_cargo` for this checkout's `kestrel/`
and retains the original Rust setup for other projects. Alt-G still performs
definition lookup. `:KestrelBazel` selects the old setup for the session;
`:KestrelCargo` switches back. `KESTREL_RUST_MODE=bazel` selects the startup
fallback. Experimental diagnostics stay disabled; saved compiler checks stay on.

Generated manifests, the separate Cargo lockfile and compiler artifacts live
under `.rules_rust_analyzer/cargo-ide/`. First-party sources remain in place;
real generated assets, macros, versions/features, aliases, tests and native
dependency metadata remain represented. Fingerprints detect stale build/asset
inputs. Refresh explicitly with `cache.py prepare --all-kestrel` after relevant
changes, then restart the client. Ordinary Rust edits do not invoke Bazel.

The Cargo model is supplied at initialization, avoiding the obsolete checked-in
`rust-project.json`. Native stable diagnostics see unsaved buffers; compiler
checks read saved files. Validation injects errors only into an in-memory `flow.rs` buffer or an
owned source/data overlay; the real source tree stays unchanged.
Saved compiler clearing was verified in a faithful source/data copy. Startup,
cold compilation and warm checks have separate measurements; timings vary.

See [the personal tooling guide](../tools/kestrel-neovim/README.md) for setup,
fallbacks, cache refresh and reproduction details. Cargo check does not validate
the final native link or replace Bazel builds, tests or execution.

The retained Bazel fallback's checker needs the Windows-path patch in `MODULE.bazel` to
check nested modules such as `kestrel/app/src/flow.rs`; otherwise it passes native
backslashes into a Bazel query and fails before compiling. The patched checker
was verified against an actual syntax error in that module.
