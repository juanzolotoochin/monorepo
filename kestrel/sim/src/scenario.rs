//! A fixed test scenario shared by the determinism tests, the golden report
//! and benchmarks.

use std::fmt::Write;

use fx::{Fx, FxVec2};

use content::Content;

use crate::{Command, CommandKind, PlayerId, Simulation, TickHash, WorldConfig};

pub struct Scenario {
    pub config: WorldConfig,
    /// Clusters per side; each cluster spawns `per_cluster` movers on tick 0.
    pub clusters: u32,
    pub per_cluster: u32,
}

impl Scenario {
    /// 8 × 8 chunks (256 × 256 tiles) with about 5000 movers.
    pub fn small() -> Scenario {
        Scenario {
            config: WorldConfig { seed: 42, chunks_x: 8, chunks_y: 8, max_movers: 20_000 },
            clusters: 4,
            per_cluster: 320,
        }
    }

    /// The MVP world, 16 × 16 chunks (512 × 512 tiles), with 100k movers.
    pub fn large() -> Scenario {
        Scenario {
            config: WorldConfig { seed: 7, chunks_x: 16, chunks_y: 16, max_movers: 150_000 },
            clusters: 25,
            per_cluster: 160,
        }
    }

    /// The commands for `tick`, given the world as it stands before the tick.
    pub fn turn(&self, sim: &Simulation) -> Vec<Command> {
        let world = sim.world();
        let size = self.config.size();
        match world.tick() {
            0 => {
                let mut turn = Vec::new();
                for cy in 0..self.clusters {
                    for cx in 0..self.clusters {
                        let at = |i: u32, extent: Fx| extent.mul_int(2 * i as i64 + 1).div_int(2 * self.clusters as i64);
                        turn.push(Command {
                            player: PlayerId(cx % 2),
                            kind: CommandKind::SpawnMovers {
                                center: FxVec2::new(at(cx, size.x), at(cy, size.y)),
                                count: self.per_cluster,
                            },
                        });
                    }
                }
                turn
            }
            // Players push the first mover of some chunks, exercising wake-ups.
            tick if tick % 25 == 10 => world
                .chunks()
                .iter()
                .step_by(7)
                .filter_map(|chunk| chunk.movers.id.first().copied())
                .map(|mover| Command {
                    player: PlayerId(1),
                    kind: CommandKind::Impulse { mover, velocity: FxVec2::new(Fx::from_ratio(1, 5), Fx::ZERO) },
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Runs `ticks` ticks and returns every tick's hash.
    pub fn run(&self, ticks: u64, threads: usize) -> Vec<TickHash> {
        let mut sim = self.start(threads);
        self.run_from(&mut sim, ticks)
    }

    /// A fresh simulation with the builtin content.
    pub fn start(&self, threads: usize) -> Simulation {
        Simulation::new(self.config.clone(), Content::builtin().rules, threads)
    }

    /// Runs `ticks` more ticks of an existing simulation.
    pub fn run_from(&self, sim: &mut Simulation, ticks: u64) -> Vec<TickHash> {
        (0..ticks)
            .map(|_| {
                let turn = self.turn(sim);
                sim.step(&turn)
            })
            .collect()
    }
}

/// The golden report: the small scenario's hashes every 50 ticks, and the
/// subsystem hashes at the end. `testdata/golden.txt` holds the expected
/// report; regenerate it with `bazel run //kestrel/sim:update_golden`.
pub fn golden_report() -> String {
    let hashes = Scenario::small().run(300, 1);
    let mut out = String::new();
    for hash in hashes.iter().filter(|h| h.tick % 50 == 49) {
        writeln!(out, "tick {} {:016x}", hash.tick, hash.total).unwrap();
    }
    let last = hashes.last().unwrap();
    for (name, hash) in &last.subsystems {
        writeln!(out, "{name} {hash:016x}").unwrap();
    }
    // The forest outpost's settlement, 20 seconds in.
    let forest = content::scenario::Scenario::parse(FOREST).expect("forest scenario");
    let world = crate::World::new_settlement(11, Content::builtin().rules, &forest, &[]).expect("forest world");
    let mut sim = Simulation::from_world(world, 1);
    for tick in 0..600 {
        let hash = sim.step(&[]);
        if tick % 150 == 149 {
            writeln!(out, "forest tick {} {:016x}", hash.tick, hash.total).unwrap();
        }
    }
    out
}

const FOREST: &str = include_str!("../../app/assets/levels/forest_supply.ron");
