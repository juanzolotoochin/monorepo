# Migrate Rust support from `rules_rust` to `rules_rs`

**Date:** 2026-06-06
**Status:** Approved

## Summary

The monorepo already has Rust support built on
[`rules_rust`](https://github.com/bazelbuild/rules_rust) (v0.56.0), with four
examples under `examples/rust/`. This work migrates that setup to
[`rules_rs`](https://github.com/hermeticbuild/rules_rs) (v0.0.61) — a next-gen
ruleset built on top of `rules_rust` that provides a reimplemented
`crate_universe`, optimized hermetic toolchains, and first-class Windows
support.

All four existing examples are kept and must continue to build. No host Rust
toolchain may be used — the build, including one-time `Cargo.lock` generation,
must go through the Bazel-managed hermetic toolchain.

## Goals

- Replace `rules_rust` (direct dep) with `rules_rs` for Rust builds.
- Keep all four examples building: `helloworld`, `formatting_lib_demo`,
  `third_party_demo`, `piston_window_demo`.
- Fully hermetic: no dependency on `/usr/bin/cargo` or `/usr/bin/rustc`, not
  even for lockfile generation.

## Non-goals

- Adding new Rust examples or features.
- Migrating any non-Rust part of the build.
- Bumping Rust source editions (examples stay on edition 2021).

## Current state

- `MODULE.bazel` (lines ~226–249):
  - `bazel_dep(name = "rules_rust", version = "0.56.0")`
  - `rust` toolchain extension (edition 2021), `register_toolchains("@rust_toolchains//:all")`
  - `crate` extension using **`crate.spec`** for `ferris-says` (0.3.1) and
    `piston_window` (0.131.0), plus a `crate.annotation` patching `khronos_api`.
- No `Cargo.toml` / `Cargo.lock` exist (spec-based resolution).
- `//bazel/patches/khronos_api.patch` patches `khronos_api`'s `build.rs` to
  resolve `api_webgl/extensions` via `CARGO_MANIFEST_DIR` (pulled in transitively
  by `piston_window`).
- Root `rust-project.json` is committed; it hard-codes `rules_rust++rust+...`
  paths from the local Bazel cache.
- Five BUILD files load from `@rules_rust//rust:defs.bzl`:
  - `examples/rust/helloworld/BUILD.bazel` (`rust_binary`)
  - `examples/rust/formatting_lib_demo/BUILD.bazel` (`rust_binary`, dep on the lib)
  - `examples/rust/formatting_lib_demo/formatting/BUILD.bazel` (`rust_library`)
  - `examples/rust/third_party_demo/BUILD.bazel` (`rust_binary`, `@crates//:ferris-says`)
  - `examples/rust/piston_window_demo/BUILD.bazel` (`rust_binary`, `@crates//:piston_window`)

## Approach

**Single root Cargo manifest.** One `//:Cargo.toml` + `//:Cargo.lock` listing
only the third-party crates. Bazel remains the build system of record; cargo only
resolves versions. This mirrors the current `crate.spec` setup with minimal
churn. (Rejected alternatives: a full cargo workspace with per-example
manifests — more idiomatic but ~6 extra files for crates that are never
cargo-built; keeping both rulesets side-by-side — not a migration.)

`rules_rs`'s `from_cargo` uses the `Cargo.lock` directly (no splicing, no
Bazel-specific lockfile) and supports `crate.annotation` with `patches` /
`patch_args` / `patch_tool`, so the existing `khronos_api` patch transfers
almost verbatim.

## Design

### 1. `MODULE.bazel`

Remove:
- `bazel_dep(name = "rules_rust", version = "0.56.0")` (stays available
  transitively via `rules_rs`; no direct dep needed).
- The `rust` toolchain extension block and `register_toolchains("@rust_toolchains//:all")`.
- The `crate` extension `crate.spec(...)` calls and `crate.from_specs()`.

Add:
```starlark
bazel_dep(name = "rules_rs", version = "0.0.61")

toolchains = use_extension("@rules_rs//rs/toolchains:module_extension.bzl", "toolchains")
toolchains.toolchain(
    edition = "2021",
    version = "1.92.0",  # pinned hermetic rust; confirm availability in rules_rs 0.0.61
)
use_repo(toolchains, "default_rust_toolchains")
register_toolchains("@default_rust_toolchains//:all")

crate = use_extension("@rules_rs//rs:extensions.bzl", "crate")
crate.from_cargo(
    name = "crates",
    cargo_lock = "//:Cargo.lock",
    cargo_toml = "//:Cargo.toml",
    platform_triples = [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
    ],
)
crate.annotation(
    crate = "khronos_api",
    patch_args = ["-p1"],
    patches = ["//bazel/patches:khronos_api.patch"],
)
use_repo(crate, "crates")
```

Notes:
- Keep the existing macOS LLVM `register_toolchains` lines untouched.
- Exact toolchain extension symbol/version string to be confirmed against
  `rules_rs` 0.0.61 source; the README example uses
  `@rules_rs//rs/toolchains:module_extension.bzl` with `toolchains.toolchain(...)`.

### 2. `//:Cargo.toml` and `//:Cargo.lock`

`Cargo.toml` — a minimal manifest depending on the two external crates:
```toml
[package]
name = "monorepo-crates"
version = "0.0.0"
edition = "2021"

[dependencies]
ferris-says = "0.3.1"
piston_window = "0.131.0"
```
(`[lib]`/`[[bin]]` may be added with a placeholder src if cargo requires a target
to resolve; prefer a virtual setup if `from_cargo` accepts a manifest with no
crate targets.)

`Cargo.lock` — generated **through the hermetic toolchain**, not host cargo.
Mechanism (confirm against 0.0.61 source during implementation, in priority
order):
1. A `rules_rs`/`rules_rust` repin path (`CARGO_BAZEL_REPIN=1 bazel sync
   --only=crates`, which runs `cargo update` using the toolchain's cargo), or
2. `bazel run` of the toolchain-provided cargo binary with `generate-lockfile`.

The chosen command is recorded in the implementation plan and verified to resolve
to a Bazel-downloaded cargo (not `/usr/bin/cargo`).

### 3. BUILD files

Rewrite the load statements in the five BUILD files:
- `@rules_rust//rust:defs.bzl` `rust_binary` → `@rules_rs//rs:rust_binary.bzl` `rust_binary`
- `@rules_rust//rust:defs.bzl` `rust_library` → `@rules_rs//rs:rust_library.bzl` `rust_library`

Use the upstream helper `scripts/rewrite_rules_rust_loads.sh` if convenient, or
edit by hand (only five files). Rule attributes and `@crates//:...` dep labels
are unchanged.

### 4. `rust-project.json`

Regenerate via the `rules_rs` rust-analyzer target (the current file hard-codes
`rules_rust++` cache paths). Confirm the target name in 0.0.61.

## Verification

- `bazel build //examples/rust/...` succeeds for all four examples (including
  `piston_window_demo`, the patched/heaviest one).
- `bazel run //examples/rust/helloworld:hello_world` and
  `bazel run //examples/rust/third_party_demo:third_party_demo` run and produce
  expected output. `piston_window_demo` is GUI/headless — build-only.
- Hermeticity check: the resolved Rust toolchain points at a Bazel-downloaded
  rustc/cargo under the Bazel cache, not `/usr/bin/rustc`; lockfile generation
  used no host cargo.
- `MODULE.bazel.lock` and `rust-project.json` updated and committed.

## Risks

- **Toolchain symbol / version drift.** The exact toolchain extension label and a
  valid pinned Rust version for `rules_rs` 0.0.61 must be confirmed from source;
  the README is thin.
- **`from_cargo` manifest shape.** Whether `from_cargo` accepts a manifest with
  no crate target (virtual) vs. requiring a placeholder lib/bin — resolve during
  implementation.
- **`khronos_api` patch context.** The patch (`-p1`) must still apply against the
  same crate version resolved by `from_cargo`; the `build.rs` env handling under
  `rules_rs`'s reimplemented build-script execution may differ from `rules_rust`.
  This is the most likely friction point even though the annotation API exists.

## Out of scope / follow-ups

- Deleting `//bazel/patches/khronos_api.patch` if it turns out unneeded under
  `rules_rs` — keep it unless verification proves otherwise.
