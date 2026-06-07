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
