//! Miner sprites (M6.5 of the mining design, M9): a miner digs the nearest
//! exposed stone it can reach from the tree, 4.5 times slower than the
//! elder, and carries the drop to the tree's stone pile; two miners never
//! dig the same tile; with nothing to dig, a miner waits beside the stone
//! pile until the ground changes. Every test checks every invariant on
//! every tick (`dig_tests::step`).

use std::collections::BTreeSet;

use content::scenario::{ticks_from_60hz, Scenario};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::dig_tests::{command, dig, facing, radius, stand, step, walk_to};
use super::miners;
use super::{AgentIx, SceneSetup, Settlement, SpriteJob, Step};
use crate::mine::STONE_ID;
use crate::nav;
use crate::terrain::{Disc, Terrain};
use crate::{CommandKind, Simulation, World};

const SUPPLY: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
const MOSS: AgentIx = 3;
const REED: AgentIx = 4;

/// The sprite's body radius.
fn sprite_radius() -> Fx {
    Fx::from_ratio(11, 50)
}

/// The Sacred Tree's map (its hill and exposed veins), with no waves.
fn map() -> Simulation {
    let mut scenario = Scenario::parse(SUPPLY).unwrap();
    scenario.waves.clear();
    super::hill_tests::sacred_tree_map(scenario)
}

/// The Sacred Tree's supply on a 12 m sloped, six-layer mine from `seed`,
/// with no hill, no waves and no prebuilt Wood Gatherer (it would stand
/// past the edge): little enough ground that it may hold no stone on top.
fn small_town(seed: u64) -> Simulation {
    let mut scenario = Scenario::parse(SUPPLY).unwrap();
    scenario.waves.clear();
    scenario.buildings.clear();
    scenario.extent = Fx::from_int(6);
    let setup = SceneSetup {
        scenario: Some(scenario),
        player: "elder".into(),
        obstacles: vec![Disc { center: FxVec2::ZERO, radius: Fx::ONE }],
        diggable: true,
        sloped: true,
        dig_layers: Some(6),
        mine: Some(seed),
        spawn: [Fx::ZERO, Fx::ZERO, Fx::from_ratio(17, 5)],
        ..SceneSetup::default()
    };
    Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn mine(sim: &mut Simulation, sprite: AgentIx) {
    step(sim, &[command(CommandKind::SetSpriteJob { agent: sprite, job: SpriteJob::Mine })]);
}

/// The stone at the tree's stone pile.
fn delivered(sim: &Simulation) -> u32 {
    settlement(sim).stock("tree_stone", "stone")
}

/// The stone dug out of the ground so far.
fn dug_stone(sim: &Simulation) -> u64 {
    let s = settlement(sim);
    s.accounts[s.item_index(STONE_ID).unwrap() as usize].generated
}

/// The tree's stone pile.
fn stone_pile(sim: &Simulation) -> FxVec2 {
    let s = settlement(sim);
    s.depots[s.stone_depot().unwrap().1 as usize].position
}

/// A miner on the map walks down a ramp to the nearest exposed stone, digs
/// a layer in 9 s facing it, picks up the drop and carries it up to the
/// tree's stone pile.
#[test]
fn a_miner_digs_exposed_stone_and_carries_it_to_the_tree() {
    let mut sim = map();
    let targets = miners::search(sim.world().terrain(), Terrain::pit_tile(stone_pile(&sim)), sprite_radius());
    assert!(!targets.is_empty(), "the map has exposed stone in reach");
    mine(&mut sim, REED);
    let mut first = None;
    for _ in 0..240 * HZ {
        step(&mut sim, &[]);
        let s = settlement(&sim);
        let reed = s.agents[REED as usize].sprite().unwrap();
        if first.is_none() {
            first = reed.dig.clone();
        }
        if delivered(&sim) > 0 && s.agents[REED as usize].carry.job.is_none() && s.transfers.is_empty() {
            break;
        }
    }
    let dig = first.expect("it dug");
    assert_eq!(dig.until - dig.started, ticks_from_60hz(540), "9 s a layer, 4.5 times the elder's 2 s");
    assert_eq!(dig.tile, targets[0].tile, "the nearest stone from the tree");
    let terrain = sim.world().terrain();
    assert!(terrain.pit(dig.tile) >= 1);
    assert!(terrain.mine().unwrap().ore(dig.tile, 0).is_some(), "it dug stone");
    assert!(delivered(&sim) >= 3, "a dig's 3 to 6 stone at the tree: {}", delivered(&sim));
    assert!(delivered(&sim) as u64 <= dug_stone(&sim));
    eprintln!("first stone at the tree at {} s", sim.world().tick() / HZ);
}

/// Miners are a steady but modest income (M6.7's tuning): two miners on
/// the map, left alone for five minutes, bring stone to the tree every
/// minute, but far less than the elder digs (a layer in 2 s, a vein's ten
/// stone layers in a minute or so): enough to keep a ballista or two
/// stocked between waves, not to stock them mid-wave.
#[test]
fn two_miners_bring_a_steady_modest_income() {
    let mut sim = map();
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: MOSS, job: SpriteJob::Mine }), command(CommandKind::SetSpriteJob { agent: REED, job: SpriteJob::Mine })]);
    let mut minutes = Vec::new();
    for _ in 0..5 {
        let before = delivered(&sim);
        for _ in 0..60 * HZ {
            step(&mut sim, &[]);
        }
        minutes.push(delivered(&sim) - before);
    }
    let total: u32 = minutes.iter().sum();
    eprintln!("two miners, stone at the tree each minute: {minutes:?}, {total} in 5 min");
    assert!(minutes[1..].iter().all(|&m| m > 0), "stone comes every minute after the first: {minutes:?}");
    assert!((MINER_INCOME.0..=MINER_INCOME.1).contains(&total), "{total} stone in 5 min: {minutes:?}");
}

/// What two miners bring the tree in five minutes, at least and at most.
const MINER_INCOME: (u32, u32) = (20, 120);

/// Two miners never hold or dig the same tile: the nearest goes to one,
/// and the other digs the next.
#[test]
fn two_miners_never_share_a_tile() {
    let mut sim = map();
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: MOSS, job: SpriteJob::Mine }), command(CommandKind::SetSpriteJob { agent: REED, job: SpriteJob::Mine })]);
    let mut dug: [BTreeSet<(i32, i32)>; 2] = Default::default();
    let mut together = false;
    for _ in 0..240 * HZ {
        step(&mut sim, &[]);
        let s = settlement(&sim);
        let [moss, reed] = [MOSS, REED].map(|a| s.agents[a as usize].sprite().unwrap());
        if let (Some(a), Some(b)) = (moss.dig_claim, reed.dig_claim) {
            assert_ne!(a, b, "one claim a tile, at tick {}", sim.world().tick());
            together = true;
        }
        if let (Some(a), Some(b)) = (&moss.dig, &reed.dig) {
            assert_ne!(a.tile, b.tile);
        }
        for (i, sprite) in [moss, reed].into_iter().enumerate() {
            if let Some(d) = &sprite.dig {
                dug[i].insert(d.tile);
            }
        }
        if dug.iter().all(|d| !d.is_empty()) && together {
            break;
        }
    }
    assert!(together, "both held a claim at once");
    assert!(dug.iter().all(|d| !d.is_empty()), "both dug: {dug:?}");
}

/// A seed whose small town has no stone a miner can dig, and a tile
/// `from` east of the tree from which the elder digs `a`, earth over
/// stone; standing in that pit it digs `b` past it, earth too. Then the
/// stone under `a` lies level with `b`'s floor, where a miner can dig it.
fn buried_vein() -> (u64, (i32, i32)) {
    for seed in 1..2000 {
        let sim = small_town(seed);
        let (s, terrain) = (settlement(&sim), sim.world().terrain());
        let view = s.view(ELDER, 0, terrain);
        let targets = miners::search(terrain, Terrain::pit_tile(stone_pile(&sim)), sprite_radius());
        if !targets.iter().all(|t| view.dig_blocked(t.tile).is_some()) {
            continue;
        }
        let mine = terrain.mine().unwrap();
        let tree = Terrain::pit_tile(terrain.center);
        // Clear of the trunk, which the elder walks past to face east.
        for x in 2..=3 {
            for z in -5..=-1 {
                let from = (tree.0 + x, tree.1 + z);
                let (a, b) = ((from.0 + 1, from.1), (from.0 + 2, from.1));
                if mine.ore(a, 0).is_none() && mine.ore(a, 1).is_some() && mine.ore(b, 0).is_none() && view.dig_in_front(Terrain::pit_center(from), facing(1, 0), radius()) == Ok(a) {
                    return (seed, from);
                }
            }
        }
    }
    panic!("no seed buries its only stone");
}

/// With no stone to dig, a miner waits beside the tree's stone pile. Once
/// the elder digs down to a vein (an earth layer over it, and the tile
/// beside it to stand in), the changed ground has the miner plan again:
/// it digs the stone and brings it to the tree.
#[test]
fn a_miner_waits_for_stone_and_digs_what_the_elder_uncovers() {
    let (seed, from) = buried_vein();
    let mut sim = small_town(seed);
    let (a, b) = ((from.0 + 1, from.1), (from.0 + 2, from.1));
    mine(&mut sim, REED);
    for _ in 0..10 * HZ {
        step(&mut sim, &[]);
    }
    let waiting = |sim: &Simulation| {
        let reed = &settlement(sim).agents[REED as usize];
        let sprite = reed.sprite().unwrap();
        sprite.current() == Some(Step::Wait) && sprite.dig_claim.is_none() && nav::distance(reed.body.position, stone_pile(sim)) < Fx::from_int(2)
    };
    assert!(waiting(&sim), "it waits at the stone pile: {:?}", settlement(&sim).agents[REED as usize].sprite().unwrap().plan);
    // Earth over the stone: the stone is a layer below every tile around.
    stand(&mut sim, from, (1, 0));
    assert_eq!(dig(&mut sim), Some(a));
    for _ in 0..2 * HZ {
        step(&mut sim, &[]);
    }
    assert!(waiting(&sim), "too deep to dig from the ground beside it");
    // The tile past it, from the pit: its floor is level with the stone.
    stand(&mut sim, a, (1, 0));
    assert_eq!(dig(&mut sim), Some(b));
    // Out of the pits and out of the way.
    let tree = Terrain::pit_tile(sim.world().terrain().center);
    let out = (from.0, if from.1 - 1 >= tree.1 - 4 { from.1 - 1 } else { from.1 + 1 });
    walk_to(&mut sim, Terrain::pit_center(from));
    walk_to(&mut sim, Terrain::pit_center(out));
    assert_eq!(dug_stone(&sim), 0, "the elder dug earth");
    let mut claimed = BTreeSet::new();
    let uncovered = sim.world().tick();
    for _ in 0..120 * HZ {
        step(&mut sim, &[]);
        let reed = &settlement(&sim).agents[REED as usize];
        claimed.extend(reed.sprite().unwrap().dig_claim);
        if delivered(&sim) > 0 && reed.carry.job.is_none() && settlement(&sim).transfers.is_empty() {
            break;
        }
    }
    let reed = &settlement(&sim).agents[REED as usize];
    let piles: Vec<_> = settlement(&sim).piles.as_ref().unwrap().piles.values().map(|p| (Terrain::pit_tile(p.position), p.units())).collect();
    let detail = format!(
        "seed {seed}, a {a:?}, b {b:?}, claims {claimed:?}, plan {:?}, at {:?}, dug {}, piles {piles:?}, gave up {:?}, carrying {}, {} s on",
        reed.sprite().unwrap().plan,
        Terrain::pit_tile(reed.body.position),
        dug_stone(&sim),
        reed.sprite().unwrap().unreachable,
        reed.carry.inventory.total(),
        (sim.world().tick() - uncovered) / HZ
    );
    // The stone the elder uncovered: under `a`, or under `b` if it lies
    // there too.
    assert!(!claimed.is_empty() && claimed.iter().all(|t| [a, b].contains(t)), "it claimed the uncovered stone: {detail}");
    assert!(claimed.iter().any(|&t| sim.world().terrain().pit(t) == 2), "and dug it from beside it: {detail}");
    assert!(delivered(&sim) >= 3, "and brought it to the tree: {detail}");
}

/// Miners play the same on 1 and 4 threads, and on from a save: the cached
/// search isn't saved, and the ground gives it back.
#[test]
fn miners_are_deterministic_across_threads_and_saves() {
    let run = |threads: usize, save_at: Option<u64>| {
        let mut sim = Simulation::from_world(map().into_world(), threads);
        let jobs = [command(CommandKind::SetSpriteJob { agent: MOSS, job: SpriteJob::Mine }), command(CommandKind::SetSpriteJob { agent: REED, job: SpriteJob::Mine })];
        let mut hashes = Vec::new();
        for tick in 0..80 * HZ {
            if Some(tick) == save_at {
                let world = sim.into_world();
                let bytes = postcard::to_allocvec(&world.to_state()).unwrap();
                let rules = world.rules().clone();
                let loaded = World::from_state(postcard::from_bytes(&bytes).unwrap(), rules).unwrap();
                sim = Simulation::from_world(loaded, threads);
            }
            let turn = if tick == 0 { &jobs[..] } else { &[] };
            hashes.push(sim.step(turn).total);
        }
        (hashes, dug_stone(&sim))
    };
    let (one, dug) = run(1, None);
    assert!(dug > 0, "they dug in the time");
    assert_eq!(one, run(4, None).0, "the thread count changed the miners");
    assert_eq!(one, run(2, Some(40 * HZ)).0, "a save mid-dig changed them");
}
