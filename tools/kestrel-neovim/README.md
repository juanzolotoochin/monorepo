# Personal Kestrel Neovim editor cache

This editor cache is exclusively for Rust under `kestrel/` (Kestrel). It is now
the approved default for that directory in the user's personal Neovim.
Normal builds remain Bazel. No BUILD files, root Cargo manifests, global
toolchain defaults, VS Code settings, or other editor configuration are changed.
The personal Rust settings module delegates other projects to its unchanged,
byte-for-byte original adapter backup.

From the checkout root in PowerShell 7:

```powershell
python tools/kestrel-neovim/cache.py prepare --all-kestrel
python tools/kestrel-neovim/cache.py fetch # Once if registry entries are missing
./tools/kestrel-neovim/install.ps1 # One-time personal Neovim installation
nvim kestrel/app/src/flow.rs
```

Restart Neovim after installing. Plain `nvim` automatically uses the Cargo
client for this checkout's `kestrel/`; all other Rust projects retain their previous
setup. Cache checks/activation are lazy, so opening another project does not
check the Kestrel cache. Reinstalling updates the shim while retaining the same
original backup. The installed shim and backup paths/hashes are recorded in
`.rules_rust_analyzer/cargo-ide/default-install.json`.

Explicit session fallback and return:

```vim
:KestrelBazel
:KestrelCargo
```

`:KestrelCargoOff` is also a session fallback alias after Cargo activation.
Fallback detaches the Cargo client immediately, restores the old Rust setup,
and retains that choice until Neovim exits. The next normal launch uses the
approved Cargo default again. A startup fallback can skip Cargo activation:

```powershell
$env:KESTREL_RUST_MODE = 'bazel'
nvim kestrel/app/src/flow.rs
$env:KESTREL_RUST_MODE = $null
```

The older `start.ps1` session launcher remains available for using the cache
without installing a default adapter.

An already open Neovim session can activate Cargo explicitly:

```vim
:lua dofile('D:/projects/juanique/monorepo/tools/kestrel-neovim/enable.lua')
```

Prerequisites are Python 3.11+, the repository's normal Bazel environment,
rustup's `1.97.1-x86_64-pc-windows-msvc` toolchain, and a working Windows native
linker environment for Cargo's procedural macros/build scripts. The existing
`tools/setup-rust-analyzer.ps1` must have generated
`.rules_rust_analyzer/editor-settings.json`. The prototype pins its own Cargo,
rustc, rust-analyzer, proc-macro server and rustfmt; it does not change the
global rustup default. CLI metadata/check commands use the existing Cargo
registry cache offline; they report missing cached packages instead of silently
changing the environment.

## What the cache contains

`prepare` queries evaluated Bazel Rust providers and configured attributes.
Kestrel preparation uses `--config=remote`. The editor specifically selects
`--platforms=@native_platform//:host`, matching the repository's remote-run
configuration: local Windows Cargo/proc macros need the native target graph
while build tools still execute on remote Linux workers. It
prepares the real asset outputs and sound library with normal caching and
BuildBuddy enabled. Its preparation invocation requests top-level output
downloads so Cargo can read the real artifacts locally; it does not change
`.bazelrc` or disable remote caching. Each invocation link and duration is kept
in `.rules_rust_analyzer/cargo-ide/runs.jsonl`.

Generated manifests point directly at existing source files. They cover
libraries, binaries, ordinary unit tests, standalone Rust tests, dependency
aliases, generated scene crates and the app's native sound dependency. Cargo
rebuilds procedural macros from the real patched dependency sources. All
generated manifests, a separate lockfile, compiler artifacts and measurements
live under the already ignored `.rules_rust_analyzer/cargo-ide/` directory.

Bazel's resolved feature sets include features unified across the monorepo.
Small generated support crates activate those same Cargo-valid features; they
do not replace application crates. A small rustc wrapper applies Bazel's
dependency-only lint ceiling and synthetic feature cfgs to dependency sources.
The app's own lints remain enabled. Cargo owns its normal package environment,
including manifest locations; custom first-party environment values are
translated explicitly. Unsupported first-party compiler flags/native targets
cause preparation to fail rather than silently disappearing.

Cargo check validates Rust code and records native-link metadata. It does not
validate the final native link or replace any Bazel build/test/run command.
Generated assets and native artifacts remain Bazel outputs; after a Bazel clean
or relevant source/build changes, regenerate the editor cache.

## Freshness and diagnostics

```powershell
python tools/kestrel-neovim/cache.py status
python tools/kestrel-neovim/cache.py check
```

Kestrel startup checks fingerprints of build metadata, dependency manifests, asset
recipes/compiler inputs, generated outputs and the generator. Rerun `prepare
--all-kestrel` and restart the Kestrel Rust client when these inputs change. This is
an explicit cache-refresh workflow; it does not run Bazel for every Rust edit.
Ordinary application source edits are handled directly by rust-analyzer/Cargo.
Stale-cache errors are reported rather than silently selecting another model.
After preparation, `:KestrelCargo` retries activation; `:KestrelBazel` selects
the explicit fallback when needed.

Native diagnostics see unsaved buffers. Compiler checks read saved files.
The client supplies its Cargo project at initialization, avoiding the obsolete
root `rust-project.json`, and advertises/handles diagnostic refresh for Neovim
0.11. Experimental native semantic diagnostics are explicitly disabled, as
chosen by the user. Stable native diagnostics and saved compiler checks remain
enabled. Most type/borrow-check errors still rely on the saved compiler check.

## Reproduce the validation

```powershell
python tools/kestrel-neovim/cache.py metadata
python tools/kestrel-neovim/audit.py
nvim --headless -u NONE -l tools/kestrel-neovim/verify-live.lua
python tools/kestrel-neovim/prepare-overlay.py
nvim --headless -u NONE -l tools/kestrel-neovim/verify-saved.lua
python tools/kestrel-neovim/benchmark.py
nvim --headless -n kestrel/app/src/flow.rs -c "lua dofile('tools/kestrel-neovim/verify-default.lua')"
```

The live test changes only the in-memory actual `flow.rs` buffer and disables
compiler checks. It tests cross-crate definitions, repeated syntax-error
appearance/clearing, Bevy macro expansion, and a semantic probe, recording
whether experimental diagnostics were required. The saved test writes only a
faithful copy of `kestrel/` under `validation-overlay/` and waits for compiler-check
completion, independently of native syntax diagnostics. Overlay results are
reported separately from checks using original source paths.
The copy excludes Bazel BUILD/Starlark files so normal Bazel package discovery
cannot see duplicate Kestrel targets inside the validation cache.

Validation probes are self-contained and anchored in `Flow::take_opened`.
No intentionally broken source file is required; syntax/semantic probes live
only in memory or in the owned validation overlay. `benchmark.py` creates a new empty compiler cache for one cold check
and two repeats; existing caches are preserved. Cold compilation is separate
from initial indexing of a fresh analyzer process with prepared Cargo artifacts.
Its temporary compiler artifacts are removed after successful measurement;
`--keep-artifacts` retains them for investigation. Measurement JSON/logs remain.

Results are saved as `live-results.json`, `saved-results.json`,
`benchmark-results.json` and `fidelity-results.json` inside the ignored cache.
The measured prototype was reviewed before adopting the scoped personal default.
The installed-default test separately checks ordinary startup, stable native
errors and definitions, Alt-G, outside-kestrel/standalone Cargo routing, and the
fallback/return commands. Run it with `KESTREL_RUST_MODE=bazel` to validate the
startup fallback. It changes the actual game buffer only in memory.
