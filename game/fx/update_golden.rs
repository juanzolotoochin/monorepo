//! Rewrites `game/fx/testdata/golden.txt`. Run with `bazel run //game/fx:update_golden`.

use std::path::PathBuf;

fn main() {
    let workspace = std::env::var("BUILD_WORKSPACE_DIRECTORY")
        .expect("run this with `bazel run`, which sets BUILD_WORKSPACE_DIRECTORY");
    let path: PathBuf = [workspace.as_str(), "game", "fx", "testdata", "golden.txt"].iter().collect();
    std::fs::write(&path, fx::golden::report()).expect("write golden file");
    println!("wrote {}", path.display());
}
