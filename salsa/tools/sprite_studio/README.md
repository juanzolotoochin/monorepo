# Sprite Studio

A native desktop app for managing sprite animation projects, built with [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) (v0.2.2).

## Project format

A project is a directory containing a `project.json` file and a `sprites/` subdirectory:

```
my_project/
  project.json
  sprites/
    Attack/
      000.png
      001.png
      002.png
    Run/
      000.png
      001.png
```

Frame files are named `<NNN>.png` (zero-padded index, 0-based) and are relative paths stored in `project.json`.

Example `project.json`:

```json
{
  "version": 1,
  "name": "freeknight",
  "actions": [
    {
      "name": "Attack",
      "fps": 12,
      "frames": [
        "sprites/Attack/000.png",
        "sprites/Attack/001.png",
        "sprites/Attack/002.png"
      ]
    },
    {
      "name": "Run",
      "fps": 24,
      "frames": [
        "sprites/Run/000.png",
        "sprites/Run/001.png"
      ]
    }
  ]
}
```

## Flows

### Open

Click **Open** in the toolbar and pick a directory.

- If the directory contains a `project.json`, it is loaded.
- If the directory is empty (or has no `project.json`), a fresh project is created named after the directory.

The first action is selected automatically and playback starts.

### Import

Click **Import** and pick a source directory containing per-frame PNGs. The app groups them into actions using the filename heuristic (see below), copies them into `sprites/<Action>/NNN.png`, and saves an updated `project.json`. Re-importing an action replaces its frames (stale files are removed). Files that do not match the heuristic are skipped and reported as unmatched.

**Filename heuristic** — the trailing integer in the stem is the frame number; everything before it (after stripping trailing `(`, `)`, `_`, `-`, spaces) is the action name. Examples:

| Filename | Action | Frame |
|---|---|---|
| `Attack (1).png` | `Attack` | 1 |
| `run_02.png` | `run` | 2 |
| `walk3.png` | `walk` | 3 |
| `JumpAttack (10).png` | `JumpAttack` | 10 |

Files with no trailing integer (e.g. `background.png`) or with digits only and no action name (e.g. `123.png`) are unmatched.

The `ActionResolver` trait is the extension seam for a future LLM-based resolver; the current default is `HeuristicResolver`.

### View

Select an action in the left sidebar to start looping playback at the action's per-action FPS. Controls in the toolbar:

- **Play / Pause** — toggle playback.
- **- / +** — decrement or increment the selected action's FPS (minimum 1).

The bottom frame strip shows thumbnails for all frames of the selected action. Click any thumbnail to pause and jump to that frame (scrub).

### Export

Click **Export** and pick an output directory. Two files are written:

- `sheet.png` — one row per action, frames left-to-right, uniform cells (max frame width × max frame height), each frame centered horizontally and bottom-aligned vertically, with 4 px padding between cells and at the edges.
- `sheet.json` — a JSON atlas array. Each entry has `action`, `frame` (0-based column index), `x`, `y`, `w`, `h` (pixel coordinates of the frame within `sheet.png`).

## Known limitation

The frame strip uses `overflow_x_hidden`. GPUI 0.2.2 does not expose `overflow_x_scroll`, so with many frames the strip clips rather than scrolling.

## Run & test

Requires a GPU and an active display (native window).

```
bazel run //salsa/tools/sprite_studio:sprite_studio
```

Unit tests (19 tests covering core: project, import, resolver, sheet, state):

```
bazel test //salsa/tools/sprite_studio/core:core_test
```
