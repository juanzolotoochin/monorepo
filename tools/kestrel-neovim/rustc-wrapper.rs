//! Match Bazel's dependency-only lint/cfg policy for local patched Cargo sources.
use std::{env, fs, process::{Command, exit}};

fn main() {
    let mut args = env::args_os().skip(1);
    let rustc = args.next().expect("Cargo must provide the real rustc executable");
    let mut command = Command::new(rustc);
    command.args(args);
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap_or_default().replace('\\', "/").to_lowercase();
    let policy = env::current_exe().unwrap().with_file_name("rustc-policy.tsv");
    for line in fs::read_to_string(policy).expect("Missing Kestrel rustc policy; rerun prepare").lines() {
        let mut fields = line.split('\t');
        if fields.next() == Some(manifest.as_str()) {
            command.args(fields);
            break;
        }
    }
    let status = command.status().expect("Could not launch pinned rustc");
    exit(status.code().unwrap_or(1));
}
