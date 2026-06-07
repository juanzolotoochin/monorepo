# Sprite Studio — GPUI sprite project app

**Date:** 2026-06-06
**Status:** Approved

## Summary

A Rust [GPUI](https://gpui.rs) desktop app for working with sprite **projects**.
A project is a directory containing a `project.json` metadata file and a
`sprites/` directory holding one PNG per frame, organized by action. The MVP
supports four flows:

1. **Open** a project directory.
2. **Import** a directory of per-frame PNGs (e.g. `/home/juanique/Downloads/freeknight/png`),
   grouping frames into actions and copying them into the project.
3. **View** actions as looping animations with a per-frame strip.
4. **Export** a packed spritesheet PNG plus a JSON atlas.

The image analysis (packing, sizing) is **reimplemented in Rust** with the
`image` crate rather than calling the existing Go `spritegen` tool, keeping the
app single-language with no runtime Go dependency.

## Goals

- Open/import/view/export work end-to-end against the `freeknight` sample
  (7 actions × 10 frames, 587×707 RGBA).
- All non-UI logic (import, packing, project I/O, UI state transitions) lives in
  a pure library with no GPUI dependency and is covered by unit tests.
- `bazel run` launches the window and the app opens/imports/views/exports.

## Non-goals (MVP)

- Background removal, trimming, or normalization on import (freeknight frames are
  already uniform-size transparent PNGs). The packer handles non-uniform frames
  generically, but import does not alter pixels.
- LLM / vision-based action detection. Action resolution is heuristic
  filename parsing, behind a trait so an LLM resolver can be added later.
- Editing sprites (drawing, reordering frames, renaming actions) beyond what
  import produces.
- Headless/CI execution of the GUI (needs GPU + display).

## Context

- **GPUI** is consumed via the existing `rules_rs` `crate.from_cargo` pipeline
  (`gpui = "0.2.2"` in `//:Cargo.toml`), the same path as the
  `examples/rust/gpui_hello_world` binary. New crates (`image`, `serde`,
  `serde_json`, `anyhow`) are added the same way and may need `crate.annotation`
  entries only if concrete build errors require them.
- **spritegen** (`salsa/tools/spritegen`, Go) already implements sheet packing
  (`spritesheet.Pack`: one row per action, frames left-to-right, uniform cells,
  centered horizontally / bottom-aligned vertically) and label prettifying via
  Claude. We mirror the pack layout in Rust; we do not call the Go code.
- The `freeknight` sample uses the filename convention `<Action> (<N>).png`,
  which is also spritegen's `pack` input format.

## Approach

**Separate pure logic from the GPUI shell.** All interesting behavior lives in a
`core` library with no `gpui` dependency, so it is testable without a window. The
`gui` library is a thin GPUI rendering layer over `core`; `main.rs` only wires
them together and opens the window.

(Rejected: shelling out to the Go `spritegen` binary — clean reuse but adds a
runtime binary dependency and a process boundary; the user chose a Rust
reimplementation. Rejected: CGo/FFI — fragile under the hermetic zig build.)

**Validate the risky GPUI APIs first.** Before building the full UI, spike the
exact GPUI 0.2.2 APIs for (a) loading a PNG from a file path into a renderable
element, (b) advancing frames on a timer with re-render, and (c) native
file/directory pickers for Open/Import/Export. Adjust the gui layer to whatever
the real API provides rather than designing around assumed names.

## Design

### Location & crate layout

Proposed home: `salsa/tools/sprite_studio/` (Rust, alongside the Go `spritegen`).

```
salsa/tools/sprite_studio/
  BUILD.bazel
  core/                         rust_library "sprite_studio_core"  (no gpui)
    src/
      lib.rs
      project.rs                project model + load/save project.json
      resolver.rs               ActionResolver trait + HeuristicResolver
      import.rs                 scan dir, resolve, copy frames, write project
      sheet.rs                  spritesheet packing + atlas JSON
      state.rs                  pure UI-state machine
  gui/                          rust_library "sprite_studio_gui"  (core + gpui)
    src/
      lib.rs                    SpriteStudio root view
      sidebar.rs  stage.rs  frame_strip.rs  toolbar.rs
  src/
    main.rs                     rust_binary "sprite_studio"
```

Each `.rs` file has one clear responsibility; the file boundaries match the
testable units described below.

### 1. Project format (`core/project.rs`)

A project is a directory:

```
myproject/
  project.json
  sprites/
    Attack/  000.png 001.png … 009.png
    Idle/    000.png …
```

`project.json`:
```json
{
  "version": 1,
  "name": "freeknight",
  "actions": [
    { "name": "Attack", "fps": 12, "frames": ["sprites/Attack/000.png", "..."] }
  ]
}
```

- Frame paths are stored **relative to the project root** so projects are
  movable.
- One file per frame, zero-padded (`%03d`) in playback order.
- `project.json` is the source of truth for the action list, frame order, and
  per-action FPS (default 12).

Rust types (serde):
```rust
struct Project { version: u32, name: String, actions: Vec<Action> }
struct Action  { name: String, fps: u32, frames: Vec<PathBuf> }  // relative paths
```

API: `Project::load(dir) -> Result<Project>`, `Project::save(&self, dir)`,
`Project::create_empty(dir, name)`. `Project::load` errors only on a malformed
`project.json`. **Open** behavior: if the directory has a `project.json`, load
it; if it is empty (or has no `project.json`), open an empty in-memory project
rooted there — `project.json` is written on first import or save. This is the
"empty project you can import into" flow.

### 2. Action resolution (`core/resolver.rs`)

```rust
/// Maps a source filename to which action/frame it represents.
trait ActionResolver {
    fn resolve(&self, filenames: &[String]) -> ResolveResult;
}
struct FrameRef { action: String, frame: u64, source: String }
struct ResolveResult { frames: Vec<FrameRef>, unmatched: Vec<String> }
```

`HeuristicResolver` (default): strip the extension, find the **trailing integer**
(allowing it to be wrapped in parens/spaces/`_`/`-`), use it as the frame number,
and take the remaining leading text — trimmed of trailing separators/parens — as
the action name. Handles `Attack (1)`, `attack_1`, `attack-01`, `attack1`. Files
with no trailing integer go to `unmatched` and are reported, not dropped.

This trait is the seam for a future `LlmResolver` (prettify names, or
vision-based classification) — no MVP implementation.

### 3. Import (`core/import.rs`)

`import_directory(project_dir, source_dir, &dyn ActionResolver) -> Result<ImportSummary>`:

1. Scan `source_dir` for `*.png`.
2. Resolve filenames → `FrameRef`s via the resolver.
3. Group by action; sort frames numerically by frame number (so `(10)` follows
   `(9)`).
4. For each frame, copy the source PNG to
   `sprites/<Action>/<NNN>.png` (NNN = zero-padded sequential index).
5. Merge into `project.json` (create it if absent) and save.
6. Return `ImportSummary { actions_added, frames_copied, unmatched: Vec<String> }`.

Import does not modify pixels.

### 4. UI state (`core/state.rs`)

A pure, GPUI-free state machine the gui layer renders and drives:

```rust
struct AppState {
    project: Option<Project>,
    project_dir: Option<PathBuf>,
    selected_action: Option<usize>,
    current_frame: usize,
    playing: bool,
}
impl AppState {
    fn select_action(&mut self, i: usize);   // resets current_frame, keeps playing
    fn toggle_play(&mut self);
    fn advance_frame(&mut self);              // wraps within the selected action
    fn scrub_to(&mut self, frame: usize);     // pauses + sets current_frame
    fn current_fps(&self) -> u32;
}
```

All transitions (selection resets frame, advance wraps, scrub pauses) are unit
tested here, independent of rendering.

### 5. Spritesheet export (`core/sheet.rs`)

`pack(project, &project_dir, padding) -> Result<PackedSheet>` where
`PackedSheet { image: RgbaImage, atlas: Vec<AtlasEntry> }` and
`AtlasEntry { action, frame, x, y, w, h }`.

Layout (mirrors spritegen): uniform cell = `max(frame width) × max(frame height)`
over all frames; **one row per action**; frames left-to-right; each frame
centered horizontally and bottom-aligned vertically in its cell; `padding` px
between cells and around the edges (fixed default, e.g. 4).

`export(project, project_dir, out_dir)` writes `sheet.png` and `sheet.json`
(the atlas) to a user-chosen location.

### 6. GUI layer (`gui/`)

Layout A (approved):

- **`toolbar.rs`** — Open Project · Import · Export · Play/Pause · FPS control.
  Open/Import use a native directory picker; Export uses a directory/save picker.
- **`sidebar.rs`** — list of `project.actions`; click sets `selected_action`.
- **`stage.rs`** — renders the current frame
  (`actions[selected].frames[current_frame]`) of the selected action, scaled to
  fit. A timer fires at `current_fps()`, calls `advance_frame()` + `cx.notify()`
  while `playing`.
- **`frame_strip.rs`** — thumbnails of all frames of the selected action; current
  highlighted; clicking calls `scrub_to`.

The gui layer holds the `AppState` and translates GPUI events into `AppState`
method calls. Exact image-loading, timer, and file-dialog APIs are pinned by the
spike in Approach.

## Build

- Add `image`, `serde`, `serde_json`, `anyhow` to `//:Cargo.toml`; regenerate
  `Cargo.lock`; add `crate.annotation`s only if build errors require them.
- `salsa/tools/sprite_studio/BUILD.bazel` defines `sprite_studio_core`
  (`rust_library`), `sprite_studio_gui` (`rust_library`, deps core + `@crates//:gpui`),
  and `sprite_studio` (`rust_binary`). Test targets for the core library use
  `rust_test`.

## Testing

- **`resolver`**: parsing across `(N)`, `_N`, `-N`, `N` patterns; numeric vs
  lexicographic ordering; unmatched files reported.
- **`project`**: `project.json` round-trip; relative-path stability; load errors.
- **`import`**: import synthetic PNGs from a tmpdir → correct subdir layout, file
  count, frame ordering, summary; re-import merges.
- **`sheet`**: packed dimensions; atlas rects; a frame's pixels land in the right
  cell (uniform and non-uniform frame sizes); padding honored.
- **`state`**: select resets frame; advance wraps; scrub pauses; fps lookup.
- Fixtures are tiny PNGs generated in-test. Manual/e2e verification via
  `bazel run` against the `freeknight` sample.
