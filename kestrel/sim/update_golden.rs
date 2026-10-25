//! Rewrites `kestrel/sim/testdata/golden.txt`. Run with `bazel run //kestrel/sim:update_golden`.

use std::path::PathBuf;

fn main() {
    let workspace = std::env::var("BUILD_WORKSPACE_DIRECTORY")
        .expect("run this with `bazel run`, which sets BUILD_WORKSPACE_DIRECTORY");
    let path: PathBuf = [workspace.as_str(), "game", "sim", "testdata", "golden.txt"].iter().collect();
    std::fs::write(&path, sim::scenario::golden_report()).expect("write golden file");
    println!("wrote {}", path.display());
}
