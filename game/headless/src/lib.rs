//! The headless runner: runs a save plus an input log without a window and
//! writes one hash per tick. Every determinism test is a comparison of two
//! hash files that must be identical.

use std::time::Instant;

use content::Content;
use save::{InputLog, LoadError};
use sim::scenario::Scenario;
use sim::{Simulation, TickHash};

/// A save at the start tick and the inputs that follow it.
pub struct Recording {
    pub save: Vec<u8>,
    pub inputs: InputLog,
    /// The hashes from running the scenario directly, without a save in between.
    pub hashes: Vec<TickHash>,
}

/// Runs `scenario` for `ticks` ticks, saving at tick 0 and logging every turn.
pub fn record(scenario: &Scenario, ticks: u64, threads: usize, content: &Content) -> Recording {
    let mut sim = Simulation::new(scenario.config.clone(), content.rules.clone(), threads);
    let save = save::write(sim.world(), content);
    let mut inputs = InputLog::new(sim.world().tick());
    let mut hashes = Vec::with_capacity(ticks as usize);
    for _ in 0..ticks {
        let turn = scenario.turn(&sim);
        hashes.push(sim.step(&turn));
        inputs.push(turn);
    }
    Recording { save, inputs, hashes }
}

/// Loads `save` and runs every turn in `inputs`, returning each tick's hash.
pub fn replay(save: &[u8], inputs: &InputLog, threads: usize, content: &Content) -> Result<Vec<TickHash>, LoadError> {
    let world = save::read(save, content)?;
    if world.tick() != inputs.start_tick {
        return Err(LoadError::Invalid(format!(
            "save is at tick {} but the input log starts at {}",
            world.tick(),
            inputs.start_tick
        )));
    }
    let mut sim = Simulation::from_world(world, threads);
    Ok((inputs.start_tick..inputs.end_tick()).map(|tick| sim.step(inputs.turn(tick))).collect())
}

/// One line per tick: the tick and its total hash.
pub fn format_hashes(hashes: &[TickHash]) -> String {
    hashes.iter().map(|h| format!("{} {:016x}\n", h.tick, h.total)).collect()
}

/// The first line where two hash files differ, if any. Line endings don't matter.
pub fn first_difference(expected: &str, actual: &str) -> Option<String> {
    let mut expected_lines = expected.lines();
    let mut actual_lines = actual.lines();
    for line in 1.. {
        match (expected_lines.next(), actual_lines.next()) {
            (None, None) => return None,
            (a, b) if a == b => continue,
            (a, b) => {
                return Some(format!("line {line}: expected {:?}, got {:?}", a.unwrap_or("<end>"), b.unwrap_or("<end>")))
            }
        }
    }
    unreachable!()
}

/// The scenarios with golden hash files in `game/testdata/`.
pub struct Golden {
    pub name: &'static str,
    pub scenario: fn() -> Scenario,
    pub ticks: u64,
}

pub const GOLDEN: &[Golden] = &[
    Golden { name: "small", scenario: Scenario::small, ticks: 300 },
    Golden { name: "large", scenario: Scenario::large, ticks: 15 },
];

/// The hash file for a golden scenario, produced through the save and input-log path.
pub fn golden_hashes(golden: &Golden, threads: usize, content: &Content) -> String {
    let recording = record(&(golden.scenario)(), golden.ticks, threads, content);
    let replayed = replay(&recording.save, &recording.inputs, threads, content).expect("own recording loads");
    let direct = format_hashes(&recording.hashes);
    let through_save = format_hashes(&replayed);
    if let Some(difference) = first_difference(&direct, &through_save) {
        panic!("{}: replay from the save differs from the direct run at {difference}", golden.name);
    }
    through_save
}

#[derive(Debug)]
pub struct BenchResult {
    pub movers: usize,
    pub threads: usize,
    pub mean_ms: f64,
    pub max_ms: f64,
}

/// Times `ticks` ticks of `scenario` after `warmup` ticks.
pub fn bench(scenario: &Scenario, warmup: u64, ticks: u64, threads: usize, content: &Content) -> BenchResult {
    let mut sim = Simulation::new(scenario.config.clone(), content.rules.clone(), threads);
    for _ in 0..warmup {
        let turn = scenario.turn(&sim);
        sim.step(&turn);
    }
    let mut total = 0.0;
    let mut max: f64 = 0.0;
    for _ in 0..ticks {
        let turn = scenario.turn(&sim);
        let start = Instant::now();
        sim.step(&turn);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        total += ms;
        max = max.max(ms);
    }
    BenchResult { movers: sim.world().mover_count(), threads, mean_ms: total / ticks as f64, max_ms: max }
}

pub fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

#[cfg(test)]
mod tests;
