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
        Project { version: 1, name: name.into(), actions: Vec::new() }
    }

    /// Loads `project.json` from `dir`. Errors if missing or malformed.
    pub fn load(dir: &Path) -> Result<Project> {
        let path = dir.join(PROJECT_FILE);
        let data = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let project = serde_json::from_str(&data)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(project)
    }

    /// Opens `dir` as a project: loads `project.json` if present, otherwise
    /// returns an empty project named after the directory.
    pub fn open(dir: &Path) -> Result<Project> {
        if dir.join(PROJECT_FILE).exists() {
            Project::load(dir)
        } else {
            let name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("untitled");
            Ok(Project::create_empty(name))
        }
    }

    /// Writes `project.json` into `dir` (creating `dir` if needed).
    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        let path = dir.join(PROJECT_FILE);
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, data)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
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
