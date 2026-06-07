//! Spritesheet packing and export.

use crate::project::Project;
use anyhow::{Context, Result};
use image::{DynamicImage, RgbaImage};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AtlasEntry {
    pub action: String,
    /// Column index of this frame within its action's row (0-based).
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

/// Packs and writes `sheet.png` + `sheet.json` (the atlas) into `out_dir`.
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
    fn pack_empty_project_produces_empty_atlas_without_panicking() {
        let dir = tempdir().unwrap();
        let project = Project { version: 1, name: "t".into(), actions: vec![] };
        let packed = pack(&project, dir.path(), 4).unwrap();
        assert!(packed.atlas.is_empty());
        // Degenerate but valid image dimensions (no panic).
        let (w, h) = packed.image.dimensions();
        assert!(w >= 1 && h >= 1);
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
