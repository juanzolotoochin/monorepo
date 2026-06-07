# GPUI hello-world Example Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Rust example that uses GPUI 0.2.2 to open a window showing "Hello, World!", building hermetically under `rules_rs` and running locally.

**Architecture:** Add `gpui = "0.2.2"` to the root `Cargo.toml`, regenerate `Cargo.lock` with the hermetic toolchain, and add `examples/rust/gpui_hello_world` (a `rust_binary` depending on `@crates//:gpui`). Add `crate.annotation` entries only when a concrete build error requires one.

**Tech Stack:** Bazel 8.5.1 (bzlmod), `rules_rs` 0.0.61, `crate.from_cargo`, GPUI 0.2.2 (Zed's GPU UI framework), zig/lld hermetic toolchain.

---

> **Commit convention:** This repo uses Graphite. Stage with `git add`, commit with
> `gt modify -c -m "..."`. Do NOT use `git commit`. End messages with the
> `Co-Authored-By` trailer.

> **Branch:** Work happens on `gpui-hello-world-example` (already checked out,
> stacked on `migrate-rules_rust-to-rules_rs`). Do NOT switch branches.

## File structure

- **Modify** `Cargo.toml` — add the `gpui` dependency
- **Regenerate** `Cargo.lock` — via hermetic cargo
- **Create** `examples/rust/gpui_hello_world/src/main.rs` — the GPUI app
- **Create** `examples/rust/gpui_hello_world/BUILD.bazel` — the `rust_binary` target
- **(conditional) Modify** `MODULE.bazel` — `crate.annotation` only if a build error requires it
- `MODULE.bazel.lock` updates automatically

---

### Task 1: Add gpui to Cargo.toml and regenerate the lockfile

**Files:**
- Modify: `Cargo.toml`
- Regenerate: `Cargo.lock`

- [ ] **Step 1: Add the dependency**

Edit `Cargo.toml`, adding `gpui` to `[dependencies]` (keep existing entries):

```toml
[dependencies]
ferris-says = "0.3.1"
piston_window = "0.131.0"
gpui = "0.2.2"
```

- [ ] **Step 2: Regenerate Cargo.lock with the hermetic cargo**

Do NOT use `/usr/bin/cargo`. Run:

```bash
bazel fetch @default_rust_toolchains//... 2>&1 | tail -3 || true
CARGO=$(find "$(bazel info output_base)/external" -type f -name cargo -path '*rust*' 2>/dev/null | grep -E 'bin/cargo$' | head -1)
echo "Using hermetic cargo: $CARGO"
case "$CARGO" in /usr/*) echo "REFUSING host cargo"; exit 1;; esac
"$CARGO" generate-lockfile --manifest-path Cargo.toml
```

- [ ] **Step 3: Verify gpui is locked**

Run: `grep -c 'name = "gpui"' Cargo.lock`
Expected: `1`. Also `grep -E 'name = "(blade-graphics|cosmic-text|x11rb|wayland-client)"' Cargo.lock | head` should show gpui's Linux deps were resolved.

- [ ] **Step 4: Verify from_cargo can analyze the new crate**

Run: `bazel build --nobuild @crates//:gpui 2>&1 | tail -5`
Expected: analysis succeeds (target exists). If it errors that `@crates//:gpui` is not found, the lockfile/`from_cargo` did not pick up gpui — re-check Steps 1-2.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock MODULE.bazel.lock
gt modify -c -m "build(rust): add gpui 0.2.2 dependency

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Create the GPUI hello-world example

**Files:**
- Create: `examples/rust/gpui_hello_world/src/main.rs`
- Create: `examples/rust/gpui_hello_world/BUILD.bazel`

- [ ] **Step 1: Write `src/main.rs`**

Create `examples/rust/gpui_hello_world/src/main.rs` (API matches gpui 0.2.2's
`examples/hello_world.rs`):

```rust
use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, SharedString, Window,
    WindowBounds, WindowOptions,
};

struct HelloWorld {
    text: SharedString,
}

impl Render for HelloWorld {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .bg(rgb(0x2e2e2e))
            .size_full()
            .justify_center()
            .items_center()
            .text_xl()
            .text_color(rgb(0xffffff))
            .child(format!("Hello, {}!", &self.text))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(300.), px(300.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| HelloWorld { text: "World".into() }),
        )
        .unwrap();
        cx.activate(true);
    });
}
```

- [ ] **Step 2: Write `BUILD.bazel`**

Create `examples/rust/gpui_hello_world/BUILD.bazel`:

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

- [ ] **Step 3: Commit**

```bash
git add examples/rust/gpui_hello_world
gt modify -c -m "feat(rust): add GPUI hello-world example

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Build the example hermetically

**Files:** none (build/verify; conditional `MODULE.bazel` edits only on error)

- [ ] **Step 1: Build**

Run: `bazel build //examples/rust/gpui_hello_world:hello_world 2>&1 | tail -30`
Expected: `Build completed successfully`.

- [ ] **Step 2: If the build fails, diagnose and add a targeted annotation**

Read the specific error and apply the matching remedy in `MODULE.bazel` (add to
the existing `crate` extension block, then rebuild). Apply only what the error
requires — do not add annotations speculatively.

- Missing gpui cargo feature (e.g. a windowing backend not enabled) →
  ```starlark
  crate.annotation(
      crate = "gpui",
      crate_features = ["<feature-from-error>"],
  )
  ```
- A transitive `-sys` crate's build script needs an env var or data file →
  ```starlark
  crate.annotation(
      crate = "<offending-sys-crate>",
      build_script_env = {"<VAR>": "<value>"},
      # or build_script_data = ["<label>"],
  )
  ```
- A crate needs a source patch → add `patches = ["//bazel/patches:<name>.patch"]`
  + `patch_args = ["-p1"]` to its `crate.annotation`.

After each change: re-run Step 1. Repeat until the build succeeds. If after three
distinct, well-understood attempts the build still fails on native/toolchain
grounds, STOP and report the blocker (this is the documented build risk).

- [ ] **Step 3: Verify hermeticity**

Run:
```bash
bazel aquery 'mnemonic("Rustc", //examples/rust/gpui_hello_world:hello_world)' 2>/dev/null | grep -m1 -oE '[^ ]*bin/rustc'
```
Expected: a path under the Bazel cache (`…default_rust_toolchains…/bin/rustc`),
NOT `/usr/bin/rustc`.

- [ ] **Step 4: Commit (only if MODULE.bazel changed in Step 2)**

```bash
git add MODULE.bazel MODULE.bazel.lock
gt modify -c -m "build(rust): annotate gpui deps for hermetic build

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

If `MODULE.bazel` was not modified, skip this commit.

---

### Task 4: Run the example locally

**Files:** none (runtime verification)

- [ ] **Step 1: Check runtime prerequisites**

Run:
```bash
echo "DISPLAY=$DISPLAY WAYLAND_DISPLAY=$WAYLAND_DISPLAY"
ldconfig -p 2>/dev/null | grep -iE 'libvulkan|libwayland-client|libxkbcommon|libfontconfig|libX11' | head
```
Note which runtime libraries and display are present. GPUI needs a GPU/Vulkan
driver and an X11 or Wayland display at runtime (dlopen'd).

- [ ] **Step 2: Run the window**

Run: `bazel run //examples/rust/gpui_hello_world:hello_world 2>&1 | tail -30`
Expected: a 300x300 window opens showing "Hello, World!".

- [ ] **Step 3: Record the outcome**

- If the window opens: success — note it.
- If it fails at runtime (e.g. "failed to find a Vulkan device", "no display",
  missing `libvulkan.so.1`): report the exact missing library/condition. This is
  an environment limitation, not a build failure — the build (Task 3) is the
  hermetic deliverable; the run depends on the host GPU/display.

No commit (verification only).

---

## Self-review notes

- **Spec coverage:** gpui dep + hermetic lock (Task 1), example binary (Task 2),
  hermetic build + conditional annotations (Task 3), local run + runtime reporting
  (Task 4). All spec goals covered.
- **API:** `main.rs` mirrors gpui 0.2.2's `examples/hello_world.rs`
  (`Application`/`App`, `Bounds::centered`, `open_window`, `WindowOptions`,
  `Render::render(&mut self, &mut Window, &mut Context<Self>)`).
- **No speculative annotations:** Task 3 adds `crate.annotation`s only in response
  to concrete errors, matching the spec's "iterate on errors" approach.
