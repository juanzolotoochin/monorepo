# Sprite Studio Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust GPUI desktop app that opens a sprite *project* directory, imports per-frame PNGs into actions, plays each action as a looping animation, and exports a packed spritesheet PNG + JSON atlas.

**Architecture:** All logic lives in a pure `sprite_studio_core` library (no GPUI) covered by unit tests: project I/O, a filename→action resolver behind a trait, import, spritesheet packing, and a UI-state machine. A thin `sprite_studio_gui` GPUI library renders that state (sidebar + playback stage + frame strip + toolbar). A `sprite_studio` binary wires them together. Image work uses the `image` crate; spritesheet layout mirrors the Go `spritegen` packer (one row per action, frames left-to-right, uniform cells).

**Tech Stack:** Rust 2021, GPUI 0.2.2, `image`, `serde`/`serde_json`, `anyhow`, `tempfile` (tests); built with Bazel `rules_rs`; committed with Graphite (`gt`).

---

## Repo conventions (read once before starting)

- **Commits use Graphite, not `git commit`.** Stage with `git add <paths>`, then create a commit on the current branch with:
  ```bash
  gt modify -c -m "type(scope): subject

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
  ```
  Never run `git commit` or `git branch` directly.
- **Hermetic cargo only** for `Cargo.lock` — never `/usr/bin/cargo` (see Task 1).
- All work happens on the current branch (`sprite-studio-spec`, stacked on `gpui-hello-world-example`). The build setup that makes gpui compile hermetically is on the parent branch.
- The `image` crate version resolved into `Cargo.lock` is `0.25.x`; its `image::imageops::overlay(bottom, top, x, y)` takes `x`/`y` as `i64`. If a build error reports a different signature, match the resolved version.

## File structure

```
salsa/tools/sprite_studio/
  BUILD.bazel                         rust_binary "sprite_studio"
  README.md
  src/main.rs                         window bootstrap → gui root view
  core/
    BUILD.bazel                       rust_library "sprite_studio_core" + rust_test "core_test"
    src/
      lib.rs                          module declarations + re-exports
      resolver.rs                     ActionResolver trait + HeuristicResolver
      project.rs                      Project/Action model + load/open/save
      import.rs                       import_directory + ImportSummary
      sheet.rs                        pack + export + AtlasEntry
      state.rs                        AppState UI-state machine
  gui/
    BUILD.bazel                       rust_library "sprite_studio_gui"
    src/
      lib.rs                          SpriteStudio root view + module decls
      toolbar.rs                      toolbar render + Open/Import/Export actions
      sidebar.rs                      action list render
      stage.rs                        animated playback render + timer
      frame_strip.rs                  frame thumbnails + scrub
```

---

### Task 1: Add core dependencies and regenerate Cargo.lock

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock` (generated)

- [ ] **Step 1: Add dependencies**

Edit `Cargo.toml`, adding to `[dependencies]` (keep existing `ferris-says`, `piston_window`, `gpui` entries):

```toml
image = "0.25"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
tempfile = "3"
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

- [ ] **Step 3: Verify the new crates are locked**

Run: `grep -E 'name = "(image|serde|serde_json|anyhow|tempfile)"' Cargo.lock | sort -u`
Expected: one line per crate.

- [ ] **Step 4: Verify from_cargo can analyze them**

Run: `bazel build --nobuild @crates//:image @crates//:serde @crates//:serde_json @crates//:anyhow @crates//:tempfile 2>&1 | tail -5`
Expected: analysis succeeds (targets exist). If a target is "not found", the lockfile/`from_cargo` did not pick it up — recheck Steps 1-2.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock MODULE.bazel.lock
gt modify -c -m "build(rust): add image/serde/anyhow deps for sprite_studio

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Scaffold the core library and prove the build/test toolchain

**Files:**
- Create: `salsa/tools/sprite_studio/core/src/lib.rs`
- Create: `salsa/tools/sprite_studio/core/src/resolver.rs`
- Create: `salsa/tools/sprite_studio/core/src/project.rs`
- Create: `salsa/tools/sprite_studio/core/src/import.rs`
- Create: `salsa/tools/sprite_studio/core/src/sheet.rs`
- Create: `salsa/tools/sprite_studio/core/src/state.rs`
- Create: `salsa/tools/sprite_studio/core/BUILD.bazel`

- [ ] **Step 1: Create empty module files**

Create these five files, each containing only a doc comment so the crate compiles:

`salsa/tools/sprite_studio/core/src/resolver.rs`:
```rust
//! Maps source filenames to the action/frame they represent.
```
`salsa/tools/sprite_studio/core/src/project.rs`:
```rust
//! Project model and on-disk format.
```
`salsa/tools/sprite_studio/core/src/import.rs`:
```rust
//! Importing a directory of per-frame PNGs into a project.
```
`salsa/tools/sprite_studio/core/src/sheet.rs`:
```rust
//! Spritesheet packing and export.
```
`salsa/tools/sprite_studio/core/src/state.rs`:
```rust
//! Pure UI-state machine driven by the GUI layer.
```

- [ ] **Step 2: Create `lib.rs` with module declarations and a smoke test**

`salsa/tools/sprite_studio/core/src/lib.rs`:
```rust
pub mod import;
pub mod project;
pub mod resolver;
pub mod sheet;
pub mod state;

#[cfg(test)]
mod smoke_tests {
    #[test]
    fn builds_and_runs() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 3: Create the BUILD file**

`salsa/tools/sprite_studio/core/BUILD.bazel`:
```starlark
load("@rules_rs//rs:rust_library.bzl", "rust_library")
load("@rules_rs//rs:rust_test.bzl", "rust_test")

rust_library(
    name = "sprite_studio_core",
    srcs = glob(["src/**/*.rs"]),
    edition = "2021",
    visibility = ["//salsa/tools/sprite_studio:__subpackages__"],
    deps = [
        "@crates//:anyhow",
        "@crates//:image",
        "@crates//:serde",
        "@crates//:serde_json",
    ],
)

rust_test(
    name = "core_test",
    crate = ":sprite_studio_core",
    edition = "2021",
    deps = ["@crates//:tempfile"],
)
```

- [ ] **Step 4: Build and test**

Run: `bazel test //salsa/tools/sprite_studio/core:core_test --test_output=errors`
Expected: PASS (`smoke_tests::builds_and_runs`).

- [ ] **Step 5: Commit**

```bash
git add salsa/tools/sprite_studio/core
gt modify -c -m "feat(rust): scaffold sprite_studio_core library

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Action resolver (filename → action/frame)

**Files:**
- Modify: `salsa/tools/sprite_studio/core/src/resolver.rs`
- Test: same file (`#[cfg(test)]` module)

- [ ] **Step 1: Write the failing tests**

Replace the contents of `resolver.rs` with the trait/types plus tests (no implementation body yet — `parse_one` returns `None` so tests fail):

```rust
//! Maps source filenames to the action/frame they represent.

use std::path::Path;

/// Maps a list of source filenames to the action/frame each one represents.
pub trait ActionResolver {
    fn resolve(&self, filenames: &[String]) -> ResolveResult;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameRef {
    pub action: String,
    pub frame: u64,
    pub source: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ResolveResult {
    pub frames: Vec<FrameRef>,
    pub unmatched: Vec<String>,
}

/// Resolves actions/frames from filenames using a trailing-integer heuristic.
pub struct HeuristicResolver;

impl HeuristicResolver {
    fn parse_one(_filename: &str) -> Option<FrameRef> {
        None
    }
}

impl ActionResolver for HeuristicResolver {
    fn resolve(&self, filenames: &[String]) -> ResolveResult {
        let mut result = ResolveResult::default();
        for f in filenames {
            match Self::parse_one(f) {
                Some(fr) => result.frames.push(fr),
                None => result.unmatched.push(f.clone()),
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fr(action: &str, frame: u64, source: &str) -> FrameRef {
        FrameRef { action: action.into(), frame, source: source.into() }
    }

    #[test]
    fn parses_common_patterns() {
        let r = HeuristicResolver;
        let out = r.resolve(&[
            "Attack (1).png".into(),
            "JumpAttack (10).png".into(),
            "attack_1.png".into(),
            "run-01.png".into(),
            "walk2.png".into(),
        ]);
        assert_eq!(out.frames, vec![
            fr("Attack", 1, "Attack (1).png"),
            fr("JumpAttack", 10, "JumpAttack (10).png"),
            fr("attack", 1, "attack_1.png"),
            fr("run", 1, "run-01.png"),
            fr("walk", 2, "walk2.png"),
        ]);
        assert!(out.unmatched.is_empty());
    }

    #[test]
    fn reports_unmatched_files_with_no_trailing_number() {
        let r = HeuristicResolver;
        let out = r.resolve(&["background.png".into(), "123.png".into()]);
        // "background" has no trailing digits; "123" has digits but no action name.
        assert!(out.frames.is_empty());
        assert_eq!(out.unmatched, vec!["background.png".to_string(), "123.png".to_string()]);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `bazel test //salsa/tools/sprite_studio/core:core_test --test_output=errors`
Expected: FAIL — `parses_common_patterns` asserts non-empty frames but `parse_one` returns `None`.

- [ ] **Step 3: Implement `parse_one`**

Replace the `parse_one` body:

```rust
    fn parse_one(filename: &str) -> Option<FrameRef> {
        let stem = Path::new(filename).file_stem()?.to_str()?;
        // Drop a trailing ')' / spaces so "Name (12)" exposes its digits.
        let tail = stem.trim_end_matches([')', ' ']);
        let bytes = tail.as_bytes();
        let mut start = bytes.len();
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if start == bytes.len() {
            return None; // no trailing integer
        }
        let frame: u64 = tail[start..].parse().ok()?;
        let action = tail[..start]
            .trim_end_matches(['(', ' ', '_', '-'])
            .trim()
            .to_string();
        if action.is_empty() {
            return None; // digits only, no action name
        }
        Some(FrameRef { action, frame, source: filename.to_string() })
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `bazel test //salsa/tools/sprite_studio/core:core_test --test_output=errors`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add salsa/tools/sprite_studio/core/src/resolver.rs
gt modify -c -m "feat(rust): add heuristic action/frame resolver

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Project model and on-disk format

**Files:**
- Modify: `salsa/tools/sprite_studio/core/src/project.rs`
- Test: same file

- [ ] **Step 1: Write the failing tests**

Replace `project.rs` with the model plus tests; leave method bodies `todo!()`:

```rust
//! Project model and on-disk format.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PROJECT_FILE: &str = "project.json";
pub const DEFAULT_FPS: u32 = 12;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub version: u32,
    pub name: String,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Action {
    pub name: String,
    pub fps: u32,
    /// Frame image paths, relative to the project root.
    pub frames: Vec<PathBuf>,
}

impl Project {
    pub fn create_empty(name: impl Into<String>) -> Project {
        todo!()
    }

    /// Loads `project.json` from `dir`. Errors if missing or malformed.
    pub fn load(dir: &Path) -> Result<Project> {
        todo!()
    }

    /// Opens `dir` as a project: loads `project.json` if present, otherwise
    /// returns an empty project named after the directory.
    pub fn open(dir: &Path) -> Result<Project> {
        todo!()
    }

    /// Writes `project.json` into `dir` (creating `dir` if needed).
    pub fn save(&self, dir: &Path) -> Result<()> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn round_trips_through_disk() {
        let dir = tempdir().unwrap();
        let project = Project {
            version: 1,
            name: "freeknight".into(),
            actions: vec![Action {
                name: "Attack".into(),
                fps: 12,
                frames: vec![PathBuf::from("sprites/Attack/000.png")],
            }],
        };
        project.save(dir.path()).unwrap();
        let loaded = Project::load(dir.path()).unwrap();
        assert_eq!(loaded, project);
    }

    #[test]
    fn open_empty_dir_yields_empty_project_named_after_dir() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("freeknight");
        std::fs::create_dir(&sub).unwrap();
        let project = Project::open(&sub).unwrap();
        assert_eq!(project.name, "freeknight");
        assert!(project.actions.is_empty());
    }

    #[test]
    fn load_missing_file_is_error() {
        let dir = tempdir().unwrap();
        assert!(Project::load(dir.path()).is_err());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `bazel test //salsa/tools/sprite_studio/core:core_test --test_output=errors`
Expected: FAIL — `todo!()` panics.

- [ ] **Step 3: Implement the methods**

Replace the four method bodies:

```rust
    pub fn create_empty(name: impl Into<String>) -> Project {
        Project { version: 1, name: name.into(), actions: Vec::new() }
    }

    pub fn load(dir: &Path) -> Result<Project> {
        let path = dir.join(PROJECT_FILE);
        let data = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let project = serde_json::from_str(&data)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(project)
    }

    pub fn open(dir: &Path) -> Result<Project> {
        if dir.join(PROJECT_FILE).exists() {
            Project::load(dir)
        } else {
            let name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("untitled");
            Ok(Project::create_empty(name))
        }
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        let path = dir.join(PROJECT_FILE);
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, data)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
```

- [ ] **Step 4: Run to verify pass**

Run: `bazel test //salsa/tools/sprite_studio/core:core_test --test_output=errors`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add salsa/tools/sprite_studio/core/src/project.rs
gt modify -c -m "feat(rust): add project model and json format

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Import a directory of per-frame PNGs

**Files:**
- Modify: `salsa/tools/sprite_studio/core/src/import.rs`
- Test: same file

- [ ] **Step 1: Write the failing test**

Replace `import.rs`:

```rust
//! Importing a directory of per-frame PNGs into a project.

use crate::project::{Action, Project, DEFAULT_FPS};
use crate::resolver::ActionResolver;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub actions_added: Vec<String>,
    pub frames_copied: usize,
    pub unmatched: Vec<String>,
}

/// Imports every `*.png` in `source_dir` into `project_dir`, grouping frames
/// into actions via `resolver` and copying them to `sprites/<Action>/<NNN>.png`.
pub fn import_directory(
    project_dir: &Path,
    source_dir: &Path,
    resolver: &dyn ActionResolver,
) -> Result<ImportSummary> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::HeuristicResolver;
    use tempfile::tempdir;

    /// Writes a 1x1 PNG so the file is a real decodable image.
    fn write_png(path: &Path) {
        let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0]));
        img.save(path).unwrap();
    }

    #[test]
    fn imports_grouped_and_numerically_ordered_frames() {
        let src = tempdir().unwrap();
        // Two actions; "Run" frames out of order and with a two-digit frame.
        for name in ["Run (2).png", "Run (10).png", "Run (1).png", "Idle (1).png", "notes.txt"] {
            let p = src.path().join(name);
            if name.ends_with(".png") { write_png(&p); } else { std::fs::write(&p, b"x").unwrap(); }
        }
        let proj = tempdir().unwrap();

        let summary = import_directory(proj.path(), src.path(), &HeuristicResolver).unwrap();

        // project.json written with both actions, frames numerically ordered.
        let project = Project::load(proj.path()).unwrap();
        let run = project.actions.iter().find(|a| a.name == "Run").unwrap();
        assert_eq!(run.frames, vec![
            PathBuf::from("sprites/Run/000.png"),
            PathBuf::from("sprites/Run/001.png"),
            PathBuf::from("sprites/Run/002.png"),
        ]);
        assert!(proj.path().join("sprites/Run/002.png").exists());
        assert_eq!(summary.frames_copied, 4);
        assert!(summary.actions_added.contains(&"Run".to_string()));
        // Non-png is ignored entirely (not even unmatched).
        assert!(summary.unmatched.is_empty());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `bazel test //salsa/tools/sprite_studio/core:core_test --test_output=errors`
Expected: FAIL — `todo!()` panics.

- [ ] **Step 3: Implement `import_directory`**

```rust
pub fn import_directory(
    project_dir: &Path,
    source_dir: &Path,
    resolver: &dyn ActionResolver,
) -> Result<ImportSummary> {
    // 1. Collect *.png filenames.
    let mut filenames = Vec::new();
    for entry in std::fs::read_dir(source_dir)
        .with_context(|| format!("reading {}", source_dir.display()))?
    {
        let path = entry?.path();
        let is_png = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("png"))
            .unwrap_or(false);
        if is_png {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                filenames.push(name.to_string());
            }
        }
    }
    filenames.sort();

    // 2. Resolve to action/frame.
    let resolved = resolver.resolve(&filenames);

    // 3. Group by action, sort each by frame number.
    let mut by_action: BTreeMap<String, Vec<(u64, String)>> = BTreeMap::new();
    for fr in resolved.frames {
        by_action.entry(fr.action).or_default().push((fr.frame, fr.source));
    }
    for frames in by_action.values_mut() {
        frames.sort_by_key(|(n, _)| *n);
    }

    // 4. Open or create the project, then copy frames in.
    let mut project = Project::open(project_dir)?;
    let mut summary = ImportSummary { unmatched: resolved.unmatched, ..Default::default() };

    for (action_name, frames) in by_action {
        let action_dir = project_dir.join("sprites").join(&action_name);
        std::fs::create_dir_all(&action_dir)
            .with_context(|| format!("creating {}", action_dir.display()))?;

        let mut rel_frames = Vec::new();
        for (idx, (_n, source)) in frames.iter().enumerate() {
            let rel = PathBuf::from("sprites").join(&action_name).join(format!("{idx:03}.png"));
            std::fs::copy(source_dir.join(source), project_dir.join(&rel))
                .with_context(|| format!("copying {source}"))?;
            rel_frames.push(rel);
            summary.frames_copied += 1;
        }

        match project.actions.iter_mut().find(|a| a.name == action_name) {
            Some(existing) => existing.frames = rel_frames,
            None => {
                project.actions.push(Action {
                    name: action_name.clone(),
                    fps: DEFAULT_FPS,
                    frames: rel_frames,
                });
                summary.actions_added.push(action_name);
            }
        }
    }

    project.actions.sort_by(|a, b| a.name.cmp(&b.name));
    project.save(project_dir)?;
    Ok(summary)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `bazel test //salsa/tools/sprite_studio/core:core_test --test_output=errors`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add salsa/tools/sprite_studio/core/src/import.rs
gt modify -c -m "feat(rust): import per-frame PNG dirs into a project

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Spritesheet packing and export

**Files:**
- Modify: `salsa/tools/sprite_studio/core/src/sheet.rs`
- Test: same file

- [ ] **Step 1: Write the failing tests**

Replace `sheet.rs`:

```rust
//! Spritesheet packing and export.

use crate::project::Project;
use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView, RgbaImage};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AtlasEntry {
    pub action: String,
    pub frame: usize,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub struct PackedSheet {
    pub image: RgbaImage,
    pub atlas: Vec<AtlasEntry>,
}

/// Packs every frame into one sheet: one row per action, frames left-to-right,
/// uniform cells (max width × max height), each frame centered horizontally and
/// bottom-aligned vertically, with `padding` px between cells and at the edges.
pub fn pack(project: &Project, project_dir: &Path, padding: u32) -> Result<PackedSheet> {
    todo!()
}

/// Packs and writes `sheet.png` + `sheet.json` (the atlas) into `out_dir`.
pub fn export(project: &Project, project_dir: &Path, out_dir: &Path, padding: u32) -> Result<()> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Action;
    use std::path::PathBuf;
    use tempfile::tempdir;

    /// Writes a solid-color PNG of the given size and returns its relative path.
    fn write_frame(root: &Path, rel: &str, w: u32, h: u32, color: [u8; 4]) -> PathBuf {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        image::RgbaImage::from_pixel(w, h, image::Rgba(color)).save(&p).unwrap();
        PathBuf::from(rel)
    }

    #[test]
    fn packs_one_row_per_action_with_uniform_cells() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Action A: two 4x4 red frames. Action B: one 2x2 blue frame.
        let a0 = write_frame(root, "sprites/A/000.png", 4, 4, [255, 0, 0, 255]);
        let a1 = write_frame(root, "sprites/A/001.png", 4, 4, [255, 0, 0, 255]);
        let b0 = write_frame(root, "sprites/B/000.png", 2, 2, [0, 0, 255, 255]);
        let project = Project {
            version: 1,
            name: "t".into(),
            actions: vec![
                Action { name: "A".into(), fps: 12, frames: vec![a0, a1] },
                Action { name: "B".into(), fps: 12, frames: vec![b0] },
            ],
        };

        let packed = pack(&project, root, 1).unwrap();

        // cell = 4x4, 2 cols, 2 rows, padding 1:
        // width  = 1 + 2*(4+1) = 11 ; height = 1 + 2*(4+1) = 11
        assert_eq!(packed.image.dimensions(), (11, 11));
        assert_eq!(packed.atlas.len(), 3);

        // A frame 0 sits at the top-left cell, filling it (4x4).
        let a0e = &packed.atlas[0];
        assert_eq!((a0e.action.as_str(), a0e.frame, a0e.x, a0e.y, a0e.w, a0e.h), ("A", 0, 1, 1, 4, 4));

        // B frame 0 is 2x2: centered in x, bottom-aligned in its 4x4 cell on row 1.
        // cell_x = 1, cell_y = 1 + (4+1) = 6 ; x = 1 + (4-2)/2 = 2 ; y = 6 + (4-2) = 8
        let b0e = packed.atlas.iter().find(|e| e.action == "B").unwrap();
        assert_eq!((b0e.x, b0e.y, b0e.w, b0e.h), (2, 8, 2, 2));
        // The blue pixel actually landed there.
        assert_eq!(packed.image.get_pixel(2, 8), &image::Rgba([0, 0, 255, 255]));
    }

    #[test]
    fn export_writes_png_and_json() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let a0 = write_frame(root, "sprites/A/000.png", 2, 2, [255, 0, 0, 255]);
        let project = Project {
            version: 1,
            name: "t".into(),
            actions: vec![Action { name: "A".into(), fps: 12, frames: vec![a0] }],
        };
        let out = tempdir().unwrap();

        export(&project, root, out.path(), 1).unwrap();

        assert!(out.path().join("sheet.png").exists());
        let atlas_json = std::fs::read_to_string(out.path().join("sheet.json")).unwrap();
        assert!(atlas_json.contains("\"action\""));
        assert!(atlas_json.contains("\"A\""));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `bazel test //salsa/tools/sprite_studio/core:core_test --test_output=errors`
Expected: FAIL — `todo!()` panics.

- [ ] **Step 3: Implement `pack` and `export`**

```rust
pub fn pack(project: &Project, project_dir: &Path, padding: u32) -> Result<PackedSheet> {
    // Load all frames, grouped by action, preserving project order.
    let mut rows: Vec<(String, Vec<DynamicImage>)> = Vec::new();
    for action in &project.actions {
        let mut frames = Vec::new();
        for rel in &action.frames {
            let img = image::open(project_dir.join(rel))
                .with_context(|| format!("opening {}", rel.display()))?;
            frames.push(img);
        }
        rows.push((action.name.clone(), frames));
    }

    let all = || rows.iter().flat_map(|(_, fs)| fs.iter());
    let cell_w = all().map(|f| f.width()).max().unwrap_or(0);
    let cell_h = all().map(|f| f.height()).max().unwrap_or(0);
    let max_cols = rows.iter().map(|(_, fs)| fs.len()).max().unwrap_or(0) as u32;
    let n_rows = rows.len() as u32;

    let sheet_w = (padding + max_cols * (cell_w + padding)).max(1);
    let sheet_h = (padding + n_rows * (cell_h + padding)).max(1);
    let mut sheet = RgbaImage::new(sheet_w, sheet_h);

    let mut atlas = Vec::new();
    for (r, (action, frames)) in rows.iter().enumerate() {
        let cell_y = padding + r as u32 * (cell_h + padding);
        for (c, frame) in frames.iter().enumerate() {
            let cell_x = padding + c as u32 * (cell_w + padding);
            let (fw, fh) = (frame.width(), frame.height());
            let x = cell_x + (cell_w - fw) / 2; // center horizontally
            let y = cell_y + (cell_h - fh); // bottom-align vertically
            image::imageops::overlay(&mut sheet, frame, x as i64, y as i64);
            atlas.push(AtlasEntry { action: action.clone(), frame: c, x, y, w: fw, h: fh });
        }
    }

    Ok(PackedSheet { image: sheet, atlas })
}

pub fn export(project: &Project, project_dir: &Path, out_dir: &Path, padding: u32) -> Result<()> {
    let packed = pack(project, project_dir, padding)?;
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("creating {}", out_dir.display()))?;
    packed
        .image
        .save(out_dir.join("sheet.png"))
        .context("writing sheet.png")?;
    let json = serde_json::to_string_pretty(&packed.atlas)?;
    std::fs::write(out_dir.join("sheet.json"), json).context("writing sheet.json")?;
    Ok(())
}
```

- [ ] **Step 4: Run to verify pass**

Run: `bazel test //salsa/tools/sprite_studio/core:core_test --test_output=errors`
Expected: PASS. If the build errors on `overlay`'s argument types, match the resolved `image` version's signature (0.25 uses `i64`).

- [ ] **Step 5: Commit**

```bash
git add salsa/tools/sprite_studio/core/src/sheet.rs
gt modify -c -m "feat(rust): pack frames into a spritesheet + atlas

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: UI-state machine

**Files:**
- Modify: `salsa/tools/sprite_studio/core/src/state.rs`
- Test: same file

- [ ] **Step 1: Write the failing tests**

Replace `state.rs`:

```rust
//! Pure UI-state machine driven by the GUI layer.

use crate::project::{Action, Project, DEFAULT_FPS};
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct AppState {
    pub project: Option<Project>,
    pub project_dir: Option<PathBuf>,
    pub selected_action: Option<usize>,
    pub current_frame: usize,
    pub playing: bool,
}

impl AppState {
    /// Installs a freshly opened/imported project: selects the first action
    /// (if any), resets to frame 0, and starts playing when non-empty.
    pub fn set_project(&mut self, project: Project, dir: PathBuf) {
        todo!()
    }

    pub fn selected(&self) -> Option<&Action> {
        todo!()
    }

    pub fn select_action(&mut self, index: usize) {
        todo!()
    }

    pub fn toggle_play(&mut self) {
        todo!()
    }

    /// Advances to the next frame, wrapping within the selected action.
    pub fn advance_frame(&mut self) {
        todo!()
    }

    /// Pauses and jumps to a specific frame.
    pub fn scrub_to(&mut self, frame: usize) {
        todo!()
    }

    pub fn current_fps(&self) -> u32 {
        todo!()
    }

    /// Sets the selected action's FPS (in memory, clamped to >= 1).
    pub fn set_selected_fps(&mut self, fps: u32) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_with(frames_per_action: &[usize]) -> Project {
        let actions = frames_per_action
            .iter()
            .enumerate()
            .map(|(i, &n)| Action {
                name: format!("A{i}"),
                fps: 10 + i as u32,
                frames: (0..n).map(|f| PathBuf::from(format!("sprites/A{i}/{f:03}.png"))).collect(),
            })
            .collect();
        Project { version: 1, name: "t".into(), actions }
    }

    #[test]
    fn set_project_selects_first_action_and_plays() {
        let mut s = AppState::default();
        s.set_project(project_with(&[3, 2]), PathBuf::from("/p"));
        assert_eq!(s.selected_action, Some(0));
        assert_eq!(s.current_frame, 0);
        assert!(s.playing);
        assert_eq!(s.current_fps(), 10);
    }

    #[test]
    fn set_empty_project_selects_nothing_and_pauses() {
        let mut s = AppState::default();
        s.set_project(project_with(&[]), PathBuf::from("/p"));
        assert_eq!(s.selected_action, None);
        assert!(!s.playing);
        assert_eq!(s.current_fps(), DEFAULT_FPS);
    }

    #[test]
    fn select_action_resets_frame_and_updates_fps() {
        let mut s = AppState::default();
        s.set_project(project_with(&[3, 2]), PathBuf::from("/p"));
        s.current_frame = 2;
        s.select_action(1);
        assert_eq!(s.selected_action, Some(1));
        assert_eq!(s.current_frame, 0);
        assert_eq!(s.current_fps(), 11);
    }

    #[test]
    fn advance_frame_wraps_within_action() {
        let mut s = AppState::default();
        s.set_project(project_with(&[3]), PathBuf::from("/p"));
        s.advance_frame();
        assert_eq!(s.current_frame, 1);
        s.advance_frame();
        s.advance_frame();
        assert_eq!(s.current_frame, 0); // wrapped 2 -> 0
    }

    #[test]
    fn scrub_pauses_and_sets_frame() {
        let mut s = AppState::default();
        s.set_project(project_with(&[3]), PathBuf::from("/p"));
        assert!(s.playing);
        s.scrub_to(2);
        assert!(!s.playing);
        assert_eq!(s.current_frame, 2);
    }

    #[test]
    fn toggle_play_flips_state() {
        let mut s = AppState::default();
        s.set_project(project_with(&[3]), PathBuf::from("/p"));
        s.toggle_play();
        assert!(!s.playing);
        s.toggle_play();
        assert!(s.playing);
    }

    #[test]
    fn set_selected_fps_updates_and_clamps() {
        let mut s = AppState::default();
        s.set_project(project_with(&[3]), PathBuf::from("/p"));
        s.set_selected_fps(24);
        assert_eq!(s.current_fps(), 24);
        s.set_selected_fps(0);
        assert_eq!(s.current_fps(), 1); // clamped to >= 1
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `bazel test //salsa/tools/sprite_studio/core:core_test --test_output=errors`
Expected: FAIL — `todo!()` panics.

- [ ] **Step 3: Implement the methods**

```rust
    pub fn set_project(&mut self, project: Project, dir: PathBuf) {
        let has_actions = !project.actions.is_empty();
        self.selected_action = has_actions.then_some(0);
        self.current_frame = 0;
        self.playing = has_actions;
        self.project = Some(project);
        self.project_dir = Some(dir);
    }

    pub fn selected(&self) -> Option<&Action> {
        let project = self.project.as_ref()?;
        project.actions.get(self.selected_action?)
    }

    pub fn select_action(&mut self, index: usize) {
        self.selected_action = Some(index);
        self.current_frame = 0;
    }

    pub fn toggle_play(&mut self) {
        self.playing = !self.playing;
    }

    pub fn advance_frame(&mut self) {
        let n = self.selected().map(|a| a.frames.len()).unwrap_or(0);
        if n > 0 {
            self.current_frame = (self.current_frame + 1) % n;
        }
    }

    pub fn scrub_to(&mut self, frame: usize) {
        self.playing = false;
        self.current_frame = frame;
    }

    pub fn current_fps(&self) -> u32 {
        self.selected().map(|a| a.fps).unwrap_or(DEFAULT_FPS)
    }

    pub fn set_selected_fps(&mut self, fps: u32) {
        if let (Some(project), Some(i)) = (self.project.as_mut(), self.selected_action) {
            if let Some(action) = project.actions.get_mut(i) {
                action.fps = fps.max(1);
            }
        }
    }
```

- [ ] **Step 4: Run to verify pass**

Run: `bazel test //salsa/tools/sprite_studio/core:core_test --test_output=errors`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add salsa/tools/sprite_studio/core/src/state.rs
gt modify -c -m "feat(rust): add sprite_studio UI-state machine

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: GPUI API spike — confirm image / timer / file-dialog APIs

This task de-risks the GUI by confirming the exact GPUI 0.2.2 APIs **before** building the full UI. Its deliverable is a tiny runnable window plus a notes file the later tasks rely on. No production code from this spike ships; the notes do.

**Files:**
- Create: `salsa/tools/sprite_studio/gui/src/lib.rs` (temporary spike body, replaced in Task 9)
- Create: `salsa/tools/sprite_studio/gui/BUILD.bazel`
- Create: `salsa/tools/sprite_studio/BUILD.bazel`
- Create: `salsa/tools/sprite_studio/src/main.rs`
- Create: `salsa/tools/sprite_studio/SPIKE_NOTES.md`

- [ ] **Step 1: Create the gui crate BUILD file**

`salsa/tools/sprite_studio/gui/BUILD.bazel`:
```starlark
load("@rules_rs//rs:rust_library.bzl", "rust_library")

rust_library(
    name = "sprite_studio_gui",
    srcs = glob(["src/**/*.rs"]),
    edition = "2021",
    visibility = ["//salsa/tools/sprite_studio:__subpackages__"],
    deps = [
        "//salsa/tools/sprite_studio/core:sprite_studio_core",
        "@crates//:anyhow",
        "@crates//:gpui",
    ],
)
```

- [ ] **Step 2: Create the binary BUILD file**

`salsa/tools/sprite_studio/BUILD.bazel`:
```starlark
load("@rules_rs//rs:rust_binary.bzl", "rust_binary")

rust_binary(
    name = "sprite_studio",
    srcs = ["src/main.rs"],
    edition = "2021",
    visibility = ["//visibility:public"],
    deps = [
        "//salsa/tools/sprite_studio/gui:sprite_studio_gui",
        "@crates//:gpui",
    ],
)
```

- [ ] **Step 3: Write a spike that exercises the three uncertain APIs**

`salsa/tools/sprite_studio/gui/src/lib.rs` (spike — exercises an image element, a redraw timer, and a directory picker). Start from the patterns in `examples/rust/gpui_hello_world/src/main.rs` and the gpui 0.2.2 docs; adjust names until it compiles:

```rust
use gpui::{
    div, img, prelude::*, px, rgb, App, Context, SharedString, Timer, Window,
};
use std::time::Duration;

pub struct Spike {
    pub tick: usize,
    pub picked: SharedString,
    pub image_path: SharedString,
}

impl Spike {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Redraw timer: confirm how to re-render on an interval.
        cx.spawn(async move |this, cx| loop {
            Timer::after(Duration::from_millis(500)).await;
            if this.update(cx, |this, cx| {
                this.tick += 1;
                cx.notify();
            }).is_err() {
                break;
            }
        })
        .detach();

        Self {
            tick: 0,
            picked: "(none)".into(),
            image_path: "/home/juanique/Downloads/freeknight/png/Idle (1).png".into(),
        }
    }
}

impl Render for Spike {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .bg(rgb(0x2e2e2e))
            .size_full()
            .text_color(rgb(0xffffff))
            .child(format!("tick: {}", self.tick))
            .child(format!("picked: {}", self.picked))
            // Load a PNG from an absolute path:
            .child(img(std::path::PathBuf::from(self.image_path.to_string())).w(px(200.)).h(px(200.)))
            // Directory picker → record the chosen path:
            .child(
                div()
                    .id("pick")
                    .child("Pick a directory")
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
                            files: false,
                            directories: true,
                            multiple: false,
                        });
                        cx.spawn(async move |this, cx| {
                            if let Ok(Ok(Some(paths))) = paths.await {
                                if let Some(p) = paths.first() {
                                    let p = p.display().to_string();
                                    let _ = this.update(cx, |this, cx| {
                                        this.picked = p.into();
                                        cx.notify();
                                    });
                                }
                            }
                        })
                        .detach();
                    })),
            )
    }
}
```

`salsa/tools/sprite_studio/src/main.rs`:
```rust
use gpui::{px, size, App, Application, Bounds, WindowBounds, WindowOptions};
use sprite_studio_gui::Spike;

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(640.), px(480.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| Spike::new(cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}
```

- [ ] **Step 4: Build, fixing API names until it compiles**

Run: `bazel build //salsa/tools/sprite_studio:sprite_studio 2>&1 | tail -30`
The exact names of `Timer`, `cx.spawn`'s closure shape, `img(...)` source type, `PathPromptOptions`, and `prompt_for_paths`'s return type may differ in 0.2.2. Adjust imports/signatures based on the compiler errors and the gpui source (under `$(bazel info output_base)/external/*gpui*`) until the target builds. Add `crate.annotation` entries in `MODULE.bazel` only if a concrete build error requires one.

- [ ] **Step 5: Run and verify the three behaviors**

Run: `bazel run //salsa/tools/sprite_studio:sprite_studio`
Confirm in the window: (a) the `tick` counter increments (timer + redraw works), (b) the freeknight PNG renders (image-from-path works), (c) clicking "Pick a directory" opens a native dialog and updates `picked` (directory picker works).

- [ ] **Step 6: Record the confirmed APIs**

Create `salsa/tools/sprite_studio/SPIKE_NOTES.md` documenting the exact, working signatures for: the redraw timer (type + closure shape), loading an image from a `PathBuf`, sizing an image element, and the directory picker (option struct + return type). Tasks 9-13 use these notes verbatim. If any name below in later tasks differs from what compiled, the notes win — adjust the later code to match.

- [ ] **Step 7: Commit**

```bash
git add salsa/tools/sprite_studio
gt modify -c -m "feat(rust): spike gpui image/timer/dialog APIs for sprite_studio

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: GUI root view + toolbar (Open / Import)

Replaces the spike with the real root view. The root owns an `AppState` and renders toolbar + sidebar + stage + frame strip. This task wires Open and Import; sidebar/stage/strip are filled in Tasks 10-12 (use empty placeholder `div()`s for them until then).

**Files:**
- Modify: `salsa/tools/sprite_studio/gui/src/lib.rs`
- Create: `salsa/tools/sprite_studio/gui/src/toolbar.rs`
- Create: `salsa/tools/sprite_studio/gui/src/sidebar.rs` (placeholder)
- Create: `salsa/tools/sprite_studio/gui/src/stage.rs` (placeholder)
- Create: `salsa/tools/sprite_studio/gui/src/frame_strip.rs` (placeholder)
- Modify: `salsa/tools/sprite_studio/src/main.rs`

- [ ] **Step 1: Create placeholder modules**

Each of `sidebar.rs`, `stage.rs`, `frame_strip.rs` starts as:
```rust
//! Filled in a later task.
use crate::SpriteStudio;
use gpui::{div, prelude::*, Context, Window};

pub fn render(_app: &mut SpriteStudio, _window: &mut Window, _cx: &mut Context<SpriteStudio>) -> impl IntoElement {
    div()
}
```
(Use the matching filename's purpose in the doc comment.)

- [ ] **Step 2: Write the root view (`lib.rs`)**

Replace `gui/src/lib.rs`:
```rust
//! Sprite Studio GPUI front-end.

mod frame_strip;
mod sidebar;
mod stage;
mod toolbar;

use gpui::{div, prelude::*, rgb, Context, Window};
use sprite_studio_core::state::AppState;

pub struct SpriteStudio {
    pub state: AppState,
}

impl SpriteStudio {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self { state: AppState::default() }
    }
}

impl Render for SpriteStudio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xffffff))
            .child(toolbar::render(self, window, cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar::render(self, window, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .child(stage::render(self, window, cx))
                            .child(frame_strip::render(self, window, cx)),
                    ),
            )
    }
}
```

- [ ] **Step 3: Write the toolbar with Open and Import**

`salsa/tools/sprite_studio/gui/src/toolbar.rs` (use the directory-picker pattern confirmed in `SPIKE_NOTES.md`):
```rust
//! Top toolbar: Open · Import · Export · Play/Pause · FPS.

use crate::SpriteStudio;
use gpui::{div, prelude::*, px, rgb, Context, Window};
use sprite_studio_core::{import::import_directory, project::Project, resolver::HeuristicResolver};
use std::path::PathBuf;

fn button(id: &'static str, label: &str) -> impl IntoElement {
    div()
        .id(id)
        .px_3()
        .py_1()
        .mr_2()
        .rounded_md()
        .bg(rgb(0x3a3a3a))
        .cursor_pointer()
        .child(label.to_string())
}

pub fn render(app: &mut SpriteStudio, _window: &mut Window, cx: &mut Context<SpriteStudio>) -> impl IntoElement {
    let playing = app.state.playing;
    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(40.))
        .px_2()
        .bg(rgb(0x252525))
        .child(button("open", "Open").on_click(cx.listener(|_, _, _, cx| open_project(cx))))
        .child(button("import", "Import").on_click(cx.listener(|_, _, _, cx| import(cx))))
        .child(button("export", "Export")) // wired in Task 13
        .child(
            button("play", if playing { "Pause" } else { "Play" })
                .on_click(cx.listener(|app, _, _, cx| {
                    app.state.toggle_play();
                    cx.notify();
                })),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .ml_4()
                .child(
                    div().id("fps_down").px_2().cursor_pointer().child("-").on_click(
                        cx.listener(|app, _, _, cx| {
                            let fps = app.state.current_fps().saturating_sub(1);
                            app.state.set_selected_fps(fps);
                            cx.notify();
                        }),
                    ),
                )
                .child(div().px_2().child(format!("FPS: {}", app.state.current_fps())))
                .child(
                    div().id("fps_up").px_2().cursor_pointer().child("+").on_click(
                        cx.listener(|app, _, _, cx| {
                            let fps = app.state.current_fps() + 1;
                            app.state.set_selected_fps(fps);
                            cx.notify();
                        }),
                    ),
                ),
        )
}

/// Prompts for a directory and opens it as a project.
fn open_project(cx: &mut Context<SpriteStudio>) {
    let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
    });
    cx.spawn(async move |this, cx| {
        if let Ok(Ok(Some(dirs))) = paths.await {
            if let Some(dir) = dirs.into_iter().next() {
                if let Ok(project) = Project::open(&dir) {
                    let _ = this.update(cx, |this, cx| {
                        this.state.set_project(project, dir);
                        cx.notify();
                    });
                }
            }
        }
    })
    .detach();
}

/// Prompts for a source directory and imports it into the current project dir.
fn import(cx: &mut Context<SpriteStudio>) {
    let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
    });
    cx.spawn(async move |this, cx| {
        if let Ok(Ok(Some(dirs))) = paths.await {
            if let Some(source) = dirs.into_iter().next() {
                // Determine the project dir: the open project's dir, else the source itself.
                let project_dir: Option<PathBuf> =
                    this.read_with(cx, |this, _| this.state.project_dir.clone()).ok().flatten();
                let project_dir = project_dir.unwrap_or_else(|| source.clone());
                if import_directory(&project_dir, &source, &HeuristicResolver).is_ok() {
                    if let Ok(project) = Project::open(&project_dir) {
                        let _ = this.update(cx, |this, cx| {
                            this.state.set_project(project, project_dir);
                            cx.notify();
                        });
                    }
                }
            }
        }
    })
    .detach();
}
```

- [ ] **Step 4: Point `main.rs` at the real root view**

Replace `salsa/tools/sprite_studio/src/main.rs`:
```rust
use gpui::{px, size, App, Application, Bounds, WindowBounds, WindowOptions};
use sprite_studio_gui::SpriteStudio;

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(960.), px(640.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| SpriteStudio::new(cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}
```

- [ ] **Step 5: Build**

Run: `bazel build //salsa/tools/sprite_studio:sprite_studio 2>&1 | tail -20`
Expected: builds. Fix any API drift against `SPIKE_NOTES.md`.

- [ ] **Step 6: Manual check**

Run: `bazel run //salsa/tools/sprite_studio:sprite_studio`
Click **Open**, choose an existing project dir (or any dir) — no crash. Click **Import**, choose `/home/juanique/Downloads/freeknight/png` — afterward `project.json` and `sprites/` appear in the chosen project dir. (Sidebar/stage are still blank — next tasks.)

- [ ] **Step 7: Commit**

```bash
git add salsa/tools/sprite_studio
gt modify -c -m "feat(rust): root view + toolbar with open/import

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 10: Sidebar action list

**Files:**
- Modify: `salsa/tools/sprite_studio/gui/src/sidebar.rs`

- [ ] **Step 1: Implement the sidebar**

Replace `sidebar.rs`:
```rust
//! Left sidebar: the list of actions in the open project.

use crate::SpriteStudio;
use gpui::{div, prelude::*, px, rgb, Context, Window};

pub fn render(app: &mut SpriteStudio, _window: &mut Window, cx: &mut Context<SpriteStudio>) -> impl IntoElement {
    let selected = app.state.selected_action;
    let names: Vec<String> = app
        .state
        .project
        .as_ref()
        .map(|p| p.actions.iter().map(|a| a.name.clone()).collect())
        .unwrap_or_default();

    let mut list = div()
        .flex()
        .flex_col()
        .w(px(180.))
        .h_full()
        .bg(rgb(0x252525))
        .py_2();

    for (i, name) in names.into_iter().enumerate() {
        let is_selected = selected == Some(i);
        list = list.child(
            div()
                .id(("action", i))
                .px_3()
                .py_1()
                .cursor_pointer()
                .when(is_selected, |d| d.bg(rgb(0x3a6ea5)))
                .child(name)
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.state.select_action(i);
                    cx.notify();
                })),
        );
    }

    list
}
```

- [ ] **Step 2: Build**

Run: `bazel build //salsa/tools/sprite_studio:sprite_studio 2>&1 | tail -20`
Expected: builds. If `.when(...)` or `.id((..))` differ, adjust per gpui API / SPIKE_NOTES.

- [ ] **Step 3: Manual check**

Run: `bazel run //salsa/tools/sprite_studio:sprite_studio`, import freeknight, confirm the 7 action names appear and clicking one highlights it.

- [ ] **Step 4: Commit**

```bash
git add salsa/tools/sprite_studio/gui/src/sidebar.rs
gt modify -c -m "feat(rust): action list sidebar

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 11: Animated playback stage

**Files:**
- Modify: `salsa/tools/sprite_studio/gui/src/stage.rs`
- Modify: `salsa/tools/sprite_studio/gui/src/lib.rs` (start the playback timer in `new`)

- [ ] **Step 1: Start a redraw/advance timer in the root view**

In `gui/src/lib.rs`, replace `SpriteStudio::new` to drive playback (use the timer pattern confirmed in `SPIKE_NOTES.md`):
```rust
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| loop {
            // Re-evaluate the delay each tick so FPS/selection changes take effect.
            let fps = this.read_with(cx, |this, _| this.state.current_fps()).unwrap_or(12).max(1);
            gpui::Timer::after(std::time::Duration::from_millis(1000 / fps as u64)).await;
            if this
                .update(cx, |this, cx| {
                    if this.state.playing {
                        this.state.advance_frame();
                        cx.notify();
                    }
                })
                .is_err()
            {
                break;
            }
        })
        .detach();

        Self { state: AppState::default() }
    }
```

- [ ] **Step 2: Implement the stage**

Replace `stage.rs` (resolve the current frame's absolute path and render it):
```rust
//! Center stage: renders the current frame of the selected action.

use crate::SpriteStudio;
use gpui::{div, img, prelude::*, rgb, Context, Window};

pub fn render(app: &mut SpriteStudio, _window: &mut Window, _cx: &mut Context<SpriteStudio>) -> impl IntoElement {
    let frame_path = current_frame_path(app);

    let mut stage = div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .bg(rgb(0x1a1a1a));

    if let Some(path) = frame_path {
        stage = stage.child(img(path).max_w_full().max_h_full());
    } else {
        stage = stage.child("Open or import a project to begin");
    }

    stage
}

fn current_frame_path(app: &SpriteStudio) -> Option<std::path::PathBuf> {
    let dir = app.state.project_dir.as_ref()?;
    let action = app.state.selected()?;
    let rel = action.frames.get(app.state.current_frame)?;
    Some(dir.join(rel))
}
```

- [ ] **Step 3: Build**

Run: `bazel build //salsa/tools/sprite_studio:sprite_studio 2>&1 | tail -20`
Expected: builds. Fix `img(...)` / `max_w_full` against SPIKE_NOTES if names differ.

- [ ] **Step 4: Manual check**

Run: `bazel run //salsa/tools/sprite_studio:sprite_studio`, import freeknight, select **Run** — the animation loops. **Pause**/**Play** in the toolbar stops/starts it, and the FPS **+/-** stepper visibly speeds up / slows down playback.

- [ ] **Step 5: Commit**

```bash
git add salsa/tools/sprite_studio/gui/src/stage.rs salsa/tools/sprite_studio/gui/src/lib.rs
gt modify -c -m "feat(rust): animated playback stage + timer

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 12: Frame strip with scrubbing

**Files:**
- Modify: `salsa/tools/sprite_studio/gui/src/frame_strip.rs`

- [ ] **Step 1: Implement the frame strip**

Replace `frame_strip.rs`:
```rust
//! Bottom strip: thumbnails of the selected action's frames; click to scrub.

use crate::SpriteStudio;
use gpui::{div, img, prelude::*, px, rgb, Context, Window};

pub fn render(app: &mut SpriteStudio, _window: &mut Window, cx: &mut Context<SpriteStudio>) -> impl IntoElement {
    let current = app.state.current_frame;
    let dir = app.state.project_dir.clone();
    let frames: Vec<std::path::PathBuf> = app
        .state
        .selected()
        .map(|a| a.frames.clone())
        .unwrap_or_default();

    let mut strip = div()
        .flex()
        .flex_row()
        .h(px(96.))
        .gap_2()
        .px_2()
        .items_center()
        .overflow_x_scroll()
        .bg(rgb(0x202020));

    for (i, rel) in frames.into_iter().enumerate() {
        let abs = dir.as_ref().map(|d| d.join(&rel));
        let is_current = i == current;
        let mut cell = div()
            .id(("frame", i))
            .w(px(72.))
            .h(px(80.))
            .flex_none()
            .cursor_pointer()
            .border_2()
            .border_color(if is_current { rgb(0x3a6ea5) } else { rgb(0x202020) })
            .on_click(cx.listener(move |app, _, _, cx| {
                app.state.scrub_to(i);
                cx.notify();
            }));
        if let Some(abs) = abs {
            cell = cell.child(img(abs).max_w_full().max_h_full());
        }
        strip = strip.child(cell);
    }

    strip
}
```

- [ ] **Step 2: Build**

Run: `bazel build //salsa/tools/sprite_studio:sprite_studio 2>&1 | tail -20`
Expected: builds. Fix `overflow_x_scroll` / `border_2` against the gpui API if names differ.

- [ ] **Step 3: Manual check**

Run, import freeknight, select an action: thumbnails appear; the playing frame is outlined; clicking a thumbnail pauses and shows that frame on the stage.

- [ ] **Step 4: Commit**

```bash
git add salsa/tools/sprite_studio/gui/src/frame_strip.rs
gt modify -c -m "feat(rust): frame strip with click-to-scrub

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 13: Wire Export

**Files:**
- Modify: `salsa/tools/sprite_studio/gui/src/toolbar.rs`

- [ ] **Step 1: Add the export handler and attach it to the button**

In `toolbar.rs`, add the imports and handler, and replace the placeholder Export button:

```rust
use sprite_studio_core::sheet::export as export_sheet;
```
```rust
        .child(button("export", "Export").on_click(cx.listener(|_, _, _, cx| export(cx))))
```
```rust
/// Prompts for an output directory and writes sheet.png + sheet.json there.
fn export(cx: &mut Context<SpriteStudio>) {
    let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
    });
    cx.spawn(async move |this, cx| {
        if let Ok(Ok(Some(dirs))) = paths.await {
            if let Some(out_dir) = dirs.into_iter().next() {
                let data = this
                    .read_with(cx, |this, _| {
                        this.state
                            .project
                            .clone()
                            .zip(this.state.project_dir.clone())
                    })
                    .ok()
                    .flatten();
                if let Some((project, project_dir)) = data {
                    let _ = export_sheet(&project, &project_dir, &out_dir, 4);
                }
            }
        }
    })
    .detach();
}
```

- [ ] **Step 2: Build**

Run: `bazel build //salsa/tools/sprite_studio:sprite_studio 2>&1 | tail -20`
Expected: builds.

- [ ] **Step 3: Manual check**

Run, import freeknight, click **Export**, choose an output dir. Confirm `sheet.png` (7 rows of 10 frames) and `sheet.json` (atlas entries) are written and the PNG opens correctly in an image viewer.

- [ ] **Step 4: Commit**

```bash
git add salsa/tools/sprite_studio/gui/src/toolbar.rs
gt modify -c -m "feat(rust): wire spritesheet export from toolbar

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 14: README, cleanup, and end-to-end verification

**Files:**
- Create: `salsa/tools/sprite_studio/README.md`
- Delete: `salsa/tools/sprite_studio/SPIKE_NOTES.md` (fold anything still useful into the README)

- [ ] **Step 1: Write the README**

`salsa/tools/sprite_studio/README.md` documenting: what the app does; the project format (`project.json` + `sprites/<Action>/<NNN>.png`); the four flows (Open, Import, View, Export); the import filename heuristic and the `ActionResolver` seam for a future LLM resolver; and run/test commands:
```
bazel run //salsa/tools/sprite_studio:sprite_studio
bazel test //salsa/tools/sprite_studio/core:core_test
```

- [ ] **Step 2: Full test + build sweep**

Run:
```bash
bazel test //salsa/tools/sprite_studio/core:core_test --test_output=errors
bazel build //salsa/tools/sprite_studio/... 2>&1 | tail -10
```
Expected: tests PASS, all targets build.

- [ ] **Step 3: End-to-end manual verification against freeknight**

Run `bazel run //salsa/tools/sprite_studio:sprite_studio` and verify the whole flow in one session:
1. **Open** an empty dir → empty project, no crash.
2. **Import** `/home/juanique/Downloads/freeknight/png` → 7 actions appear in the sidebar; `sprites/<Action>/000.png…` written.
3. **View**: select each action; it loops; Pause/Play works; clicking a thumbnail scrubs.
4. **Export** to a dir → `sheet.png` (7 rows × 10 frames) + `sheet.json` written and valid.

- [ ] **Step 4: Remove the spike notes**

```bash
git rm salsa/tools/sprite_studio/SPIKE_NOTES.md
```

- [ ] **Step 5: Commit**

```bash
git add salsa/tools/sprite_studio/README.md
gt modify -c -m "docs(rust): sprite_studio README; remove spike notes

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Definition of done

- `bazel test //salsa/tools/sprite_studio/core:core_test` passes (resolver, project, import, sheet, state all covered).
- `bazel run //salsa/tools/sprite_studio:sprite_studio` opens a window where the four flows work against the freeknight sample.
- All logic lives in `sprite_studio_core` (no gpui dependency); the gui layer only renders state and translates events.
- The `ActionResolver` trait is in place as the seam for a future LLM resolver (no LLM implementation in this MVP).
