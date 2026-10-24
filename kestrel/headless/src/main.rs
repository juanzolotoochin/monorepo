//! `headless`: run the simulation without a window.
//!
//! ```text
//! headless record <small|large> <ticks> <out.gsav> <out.glog>
//! headless replay <save.gsav> <inputs.glog> [--threads N] [--hashes out.txt]
//! headless bench [small|large] [--ticks N] [--threads N] [--max-ms MS]
//! headless dump <save.gsav>
//! headless golden            (with `bazel run`: rewrites kestrel/testdata/*.hashes)
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use content::Content;
use headless::{bench, default_threads, format_hashes, golden_hashes, record, replay, GOLDEN};
use save::InputLog;
use sim::scenario::Scenario;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let content = Content::builtin();
    let (command, rest) = args.split_first().ok_or_else(usage)?;
    let mut positional = Vec::new();
    let mut threads = default_threads();
    let mut ticks = None;
    let mut max_ms = None;
    let mut hashes_out = None;
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        let mut value = || iter.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--threads" => threads = parse(&value()?)?,
            "--ticks" => ticks = Some(parse(&value()?)?),
            "--max-ms" => max_ms = Some(parse::<f64>(&value()?)?),
            "--hashes" => hashes_out = Some(value()?),
            _ => positional.push(arg.clone()),
        }
    }

    match (command.as_str(), positional.as_slice()) {
        ("record", [scenario, count, save_path, inputs_path]) => {
            let recording = record(&scenario_named(scenario)?, parse(count)?, threads, &content);
            write(save_path, &recording.save)?;
            write(inputs_path, &recording.inputs.to_bytes())
        }
        ("replay", [save_path, inputs_path]) => {
            let inputs = InputLog::from_bytes(&read(inputs_path)?).map_err(|e| e.to_string())?;
            let hashes = replay(&read(save_path)?, &inputs, threads, &content).map_err(|e| e.to_string())?;
            let text = format_hashes(&hashes);
            match hashes_out {
                Some(path) => write(&path, text.as_bytes()),
                None => {
                    print!("{text}");
                    Ok(())
                }
            }
        }
        ("bench", names) => {
            let scenario = scenario_named(names.first().map_or("large", String::as_str))?;
            let result = bench(&scenario, 10, ticks.unwrap_or(100), threads, &content);
            println!(
                "{} movers, {} threads: {:.2} ms per tick (max {:.2})",
                result.movers, result.threads, result.mean_ms, result.max_ms
            );
            match max_ms {
                Some(limit) if result.mean_ms > limit => Err(format!("mean {:.2} ms exceeds {limit} ms", result.mean_ms)),
                _ => Ok(()),
            }
        }
        ("dump", [save_path]) => {
            print!("{}", save::dump_ron(&read(save_path)?).map_err(|e| e.to_string())?);
            Ok(())
        }
        ("golden", []) => {
            let workspace = std::env::var("BUILD_WORKSPACE_DIRECTORY")
                .map_err(|_| "run this with `bazel run //kestrel/headless -- golden`".to_string())?;
            for golden in GOLDEN {
                let path: PathBuf = [workspace.as_str(), "game", "testdata", &format!("{}.hashes", golden.name)].iter().collect();
                write(&path.to_string_lossy(), golden_hashes(golden, threads, &content).as_bytes())?;
                println!("wrote {}", path.display());
            }
            Ok(())
        }
        _ => Err(usage()),
    }
}

fn scenario_named(name: &str) -> Result<Scenario, String> {
    match name {
        "small" => Ok(Scenario::small()),
        "large" => Ok(Scenario::large()),
        _ => Err(format!("unknown scenario {name}; expected small or large")),
    }
}

fn parse<T: std::str::FromStr>(text: &str) -> Result<T, String> {
    text.parse().map_err(|_| format!("invalid number {text}"))
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn write(path: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{path}: {e}"))
}

fn usage() -> String {
    "usage: headless record|replay|bench|dump|golden ... (see kestrel/headless/src/main.rs)".to_string()
}
