# Bevy isometric garden

A minimal Bevy 0.18.1 scene built with Bazel and `rules_rs`. All meshes and
materials are generated in Rust: no downloaded art or runtime asset paths.

```sh
bazel run //examples/rust/bevy_isometric:demo
bazel test //examples/rust/bevy_isometric:movement_test
```

Use **WASD** or **arrow keys** to walk in screen directions, hold **Shift** to
run, **R** to return to the center, and **Esc** to quit. The orange character's
nose shows its facing direction. Trees and rocks block movement; the character
slides along obstacles and stays on the island. The orthographic camera stays
fixed above the garden at an isometric angle.

Run on a graphical desktop with a working GPU driver (Vulkan on Linux or Metal
on macOS). The current dependency configuration uses X11 on Linux, including
XWayland on Wayland desktops. Controls are shown in the window title.

`src/main.rs` contains scene construction, camera-relative movement, simple
circle collisions, and headless movement tests. Third-party versions are
resolved by the root `Cargo.toml` / `Cargo.lock`; first-party code builds with
Bazel, following the other Rust examples in this repository.
The targets expose the root manifest as a compile-time input because Bevy's
derive macros read it to find the `bevy` crate path.
