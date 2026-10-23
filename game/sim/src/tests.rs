use std::time::Instant;

use crate::id::Ids;
use crate::rng::{stream, Rng};
use crate::scenario::{golden_report, Scenario};
use crate::timer::TimerWheel;
use crate::{Mover, Simulation, TickHash};

const TICKS: u64 = 300;

fn assert_same(a: &[TickHash], b: &[TickHash], context: &str) {
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(b) {
        if let Some(difference) = x.first_difference(y) {
            panic!("{context}: first difference at {difference}");
        }
    }
}

#[test]
fn thread_count_does_not_change_results() {
    let scenario = Scenario::small();
    let one = scenario.run(TICKS, 1);
    for threads in [2, 16] {
        assert_same(&one, &scenario.run(TICKS, threads), &format!("1 vs {threads} threads"));
    }
}

#[test]
fn replay_is_identical_and_seed_matters() {
    let scenario = Scenario::small();
    let a = scenario.run(100, 4);
    assert_same(&a, &scenario.run(100, 4), "replay");

    let mut other = Scenario::small();
    other.config.seed += 1;
    assert_ne!(a.last().unwrap().total, other.run(100, 4).last().unwrap().total);
}

/// Runs the scenario checking bookkeeping every tick, and checks that every
/// mechanism actually fired so the determinism tests cover them.
#[test]
fn invariants_hold_and_every_stage_has_work() {
    let scenario = Scenario::small();
    let mut sim = scenario.start(4);
    let spawned = scenario.per_cluster * scenario.clusters * scenario.clusters;
    let (mut max_sleeping, mut min_movers, mut max_movers) = (0, usize::MAX, 0);
    let mut eaten = false;
    for _ in 0..TICKS {
        let turn = scenario.turn(&sim);
        sim.step(&turn);
        let world = sim.world();
        world.check_invariants();
        let count = world.mover_count();
        min_movers = min_movers.min(count);
        max_movers = max_movers.max(count);
        max_sleeping = max_sleeping.max(world.sleeping_timers());
        eaten |= world.chunks().iter().any(|c| c.food < 90);
    }
    assert!(max_movers >= spawned as usize, "movers were not all spawned");
    assert!(min_movers < spawned as usize, "no mover died");
    assert!(max_sleeping > 0, "no mover slept");
    assert!(eaten, "no food was eaten");
}

#[test]
fn quiet_chunks_are_skipped() {
    // A single mover in a large world: every other chunk stays asleep and
    // keeps its hash from tick 0.
    let mut config = Scenario::small().config;
    config.max_movers = 1;
    let mut sim = Simulation::new(config, content::Content::builtin().rules, 2);
    let first = sim.step(&[crate::Command {
        player: crate::PlayerId(0),
        kind: crate::CommandKind::SpawnMovers { center: fx::FxVec2::new(fx::Fx::from_int(16), fx::Fx::from_int(16)), count: 1 },
    }]);
    let mut last = first.clone();
    for _ in 0..50 {
        last = sim.step(&[]);
    }
    let changed = first.chunks.iter().zip(&last.chunks).filter(|(a, b)| a != b).count();
    assert!(changed <= 2, "{changed} chunks changed");
}

#[test]
fn chunk_order_is_fair() {
    let mut first = [0u32; 4];
    for tick in 0..4000 {
        let mut order = [0u32, 1, 2, 3];
        Rng::new(1, tick, 0, stream::CHUNK_ORDER).shuffle(&mut order);
        first[order[0] as usize] += 1;
    }
    for count in first {
        assert!((900..1100).contains(&count), "{first:?}");
    }
}

#[test]
fn stale_ids_do_not_resolve() {
    let mut ids = Ids::<Mover>::default();
    let a = ids.alloc();
    ids.free(a);
    let b = ids.alloc();
    assert_eq!(a.index(), b.index());
    assert!(!ids.is_allocated(a));
    assert!(ids.is_allocated(b));
}

#[test]
fn timer_wheel_wakes_on_time() {
    let mut wheel = TimerWheel::default();
    wheel.schedule(5, 'a');
    wheel.schedule(5 + 256, 'b');
    wheel.schedule(5, 'c');
    let mut due = Vec::new();
    wheel.drain_due(5, &mut due);
    assert_eq!(due, ['a', 'c']);
    due.clear();
    wheel.drain_due(5 + 256, &mut due);
    assert_eq!(due, ['b']);
    assert!(wheel.is_empty());
}

#[test]
fn golden_report_matches() {
    let expected = include_str!("../testdata/golden.txt");
    let actual = golden_report();
    // Compare by line: `lines` drops a trailing CR, in case a checkout converted line endings.
    assert!(
        actual.lines().eq(expected.lines()),
        "Simulation results changed. If this is intended, run `bazel run //game/sim:update_golden`.\n\
         expected:\n{expected}\nactual:\n{actual}"
    );
}

/// Not a pass/fail benchmark (that lives in the headless runner); prints the
/// cost per tick for a quick check. Run with `--test_arg=--ignored -c opt`.
#[test]
#[ignore]
fn bench_large() {
    let scenario = Scenario::large();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut sim = scenario.start(threads);
    for _ in 0..10 {
        let turn = scenario.turn(&sim);
        sim.step(&turn);
    }
    let start = Instant::now();
    let ticks = 100;
    for _ in 0..ticks {
        let turn = scenario.turn(&sim);
        sim.step(&turn);
    }
    let per_tick = start.elapsed().as_secs_f64() * 1000.0 / ticks as f64;
    println!("{} movers, {threads} threads: {per_tick:.2} ms per tick", sim.world().mover_count());
}

#[test]
fn dig_commands_change_terrain_and_the_hash() {
    let config = Scenario::small().config;
    let rules = content::Content::builtin().rules;
    let mut world = crate::World::new(config.clone(), rules.clone());
    let mut terrain = world.terrain().clone();
    terrain.add_rock((40, 40));
    world.set_terrain(terrain);
    let mut other = crate::World::new(config, rules);
    other.set_terrain(world.terrain().clone());

    let dig = crate::Command { player: crate::PlayerId(0), kind: crate::CommandKind::Dig { tile: (40, 40) } };
    let dug = world.step(&[dig]);
    let kept = other.step(&[]);
    assert!(!world.terrain().is_rock((40, 40)));
    assert!(other.terrain().is_rock((40, 40)));
    assert_eq!(dug.first_difference(&kept).unwrap(), "tick 0: subsystem terrain");
}
