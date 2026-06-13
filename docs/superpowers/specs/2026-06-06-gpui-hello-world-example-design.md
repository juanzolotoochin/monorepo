# GPUI hello-world Rust UI example

**Date:** 2026-06-06
**Status:** Approved

## Summary

Add a Rust example that uses [GPUI](https://gpui.rs) — Zed's GPU-accelerated UI
framework — to open a window displaying "Hello, World!". GPUI 0.2.2 is published
on crates.io, so it is consumed through the existing `rules_rs`
`crate.from_cargo` pipeline like any other third-party crate. The example must
both build hermetically under Bazel and run locally (opening a real window).

## Goals

- `gpui = "0.2.2"` resolves through `crate.from_cargo` and builds hermetically
  under `rules_rs` + the zig/lld toolchain (no host cargo/rustc, including
  lockfile regeneration).
- A new `examples/rust/gpui_hello_world` binary opens a 300x300 window with
  centered "Hello, World!" text.
- `bazel run` launches the window on this Linux machine.

## Non-goals

- Headless / CI execution of the GUI (running needs a GPU + display).
- A non-trivial UI (no interactivity beyond displaying text).
- Pinning gpui features up front (start with defaults; see Approach).

## Context

- Rust support uses `rules_rs` 0.0.61 with `crate.from_cargo` reading
  `//:Cargo.toml` + `//:Cargo.lock`. Example binaries load
  `@rules_rs//rs:rust_binary.bzl` and depend on `@crates//:<crate>`.
- The default build platform carries the `@llvm//constraints/libc:gnu.2.28`
  constraint (via `//platforms:rust_linux_x86_64`) so crate targets resolve as
  compatible.
- GPUI 0.2.2 deps on Linux are largely **dlopen-based** (`wayland-backend`
  dlopen, `zed-font-kit` fontconfig-dlopen) and pure-Rust protocol crates
  (`x11rb`, `wayland-client`, `cosmic-text`, `blade-graphics`/`ash`), so most
  system libraries are loaded at runtime rather than linked at build time.

## Approach

**Default features, iterate on errors.** Add `gpui = "0.2.2"` to `Cargo.toml`,
depend on `@crates//:gpui`, and let `from_cargo` resolve gpui's default feature
set (which on Linux pulls the blade/Vulkan + Wayland/X11 windowing backend). Add
`crate.annotation` entries (features, `build_script_env`, `build_script_data`,
patches) **only** when a concrete build error requires it. This avoids guessing
feature/annotation names before observing real failures.

(Rejected: pinning gpui `crate_features` up front — more control but speculative,
since the correct feature names aren't known until a build is attempted.)

## Design

### 1. `//:Cargo.toml`
Add to `[dependencies]`:
```toml
gpui = "0.2.2"
```
Regenerate `//:Cargo.lock` with the Bazel-managed hermetic cargo (the same
`find`-the-toolchain-cargo flow used to create the lockfile originally), never
`/usr/bin/cargo`.

### 2. `examples/rust/gpui_hello_world/`
- `src/main.rs` — a GPUI application that opens a centered 300x300 window whose
  root view renders a flex container with a background color and centered
  "Hello, World!" text. The exact GPUI API (`Application`/`App`, `open_window`,
  `WindowOptions`, `Bounds`, `div()`, `Render`) is verified against the gpui
  0.2.2 docs/source during implementation, since GPUI's API shifts between
  releases.
- `BUILD.bazel`:
  ```starlark
  load("@rules_rs//rs:rust_binary.bzl", "rust_binary")

  rust_binary(
      name = "hello_world",
      srcs = ["src/main.rs"],
      edition = "2021",
      visibility = ["//visibility:public"],
      deps = ["@crates//:gpui"],
  )
  ```

### 3. Annotations (only if needed)
If the build fails, add `crate.annotation(...)` entries in `MODULE.bazel` for the
offending crate (gpui or a transitive `-sys` dep) — e.g. `crate_features`,
`build_script_env`, `build_script_data`, or a `patches` entry. Each addition is
driven by a specific observed error, not added speculatively.

## Verification

- `bazel build //examples/rust/gpui_hello_world:hello_world` succeeds.
- Hermeticity: compilation uses the `rules_rs` toolchain rustc (under the Bazel
  cache), not `/usr/bin/rustc`; the lockfile was regenerated with the hermetic
  cargo.
- `bazel run //examples/rust/gpui_hello_world:hello_world` opens a window
  showing "Hello, World!" on this machine. If the host lacks Vulkan or a display,
  report exactly which runtime library/condition is missing rather than claiming
  success.

## Risks

- **Build-time native deps:** a transitive `-sys` crate may need a build-script
  tweak (cc flags, headers, env) under the zig toolchain. Resolved per-error via
  `crate.annotation`. Most gpui Linux deps avoid this via dlopen/pure-Rust, so
  the surface is expected to be small but is not zero.
- **gpui API drift:** the hello-world source must match gpui 0.2.2's API exactly;
  verify against that version's `examples/hello_world.rs` and docs.
- **Runtime environment:** `bazel run` success depends on host
  `libvulkan`/`libxkbcommon`/`libwayland-client` (or X11) / `libfontconfig` and a
  working GPU + display. This is expected (local run, not hermetic) and is a
  reporting concern, not a build blocker.
- **Lockfile growth:** gpui drags in a large dependency tree; `Cargo.lock` and
  `MODULE.bazel.lock` will grow substantially.
