//! Miner sprites (M6.5 of the mining design, M9): on a heightfield a
//! miner digs the nearest exposed stone it can reach from the tree, 4.5
//! times slower than the elder, and carries the drop to the tree's stone
//! pile; two miners never dig the same tile; with nothing to dig, a miner
//! waits beside the stone pile until the ground changes. In the town's
//! volume (M8.4 of the depths design) a miner mines walls: a level dig
//! into the tile beside it from its floor, never its floor or ceiling,
//! never a protected column, walking to the wall in 3D; amber goes to the
//! amber pile. Every test checks every invariant on every tick
//! (`dig_tests::step`).

use std::collections::BTreeSet;

use content::scenario::{ticks_from_60hz, Scenario};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::dig_tests::{command, dig, facing, radius, stand, step, walk_to};
use super::miners;
use super::{AgentIx, SceneSetup, Settlement, SpriteJob, Step};
use crate::mine::{AMBER, AMBER_ID, STONE_ID};
use crate::nav;
use crate::terrain::{Disc, Terrain};
use crate::volume::{height, HEADROOM};
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
pub(super) fn map() -> Simulation {
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

pub(super) fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

pub(super) fn mine(sim: &mut Simulation, sprite: AgentIx) {
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

/// The amber at the tree's amber pile.
fn delivered_amber(sim: &Simulation) -> u32 {
    settlement(sim).stock("tree_amber", "amber")
}

/// The amber dug out of the ground so far.
fn dug_amber(sim: &Simulation) -> u64 {
    let s = settlement(sim);
    s.accounts[s.item_index(AMBER_ID).unwrap() as usize].generated
}

/// The tile `x`, `z` tiles from the tree's.
pub(super) fn tile(sim: &Simulation, x: i32, z: i32) -> (i32, i32) {
    let tree = Terrain::pit_tile(sim.world().terrain().center);
    (tree.0 + x, tree.1 + z)
}

/// Takes the ore out of every cell of the town's volume for which `strip`
/// holds (its tile from the tree's).
pub(super) fn strip_ore(sim: &mut Simulation, strip: impl Fn((i32, i32)) -> bool) {
    let tree = Terrain::pit_tile(sim.world().terrain().center);
    let terrain = sim.world_mut().settlement_and_terrain_mut().1;
    let volume = terrain.volume.as_mut().unwrap();
    let cells: Vec<_> = volume.ore_cells().map(|(cell, _)| cell).filter(|&(t, _)| strip((t.0 - tree.0, t.1 - tree.1))).collect();
    for (t, layer) in cells {
        volume.set_ore(t, layer, None);
    }
    terrain.revision += 1;
}

/// A tunnel at the hill's north foot, dug straight into the cells: a
/// trench from the surface at x 8, z -26 (from the tree), walled on its
/// sides, down a layer a tile eastward for six tiles, then a roofed tunnel five tiles long at
/// that floor, ending in a wall at x 19. The wall's four cells over the
/// floor are amber, and so are its cell under them (the floor's) and over
/// them (the ceiling's). Returns the wall's tile, the tunnel's last tile (where a
/// miner stands to dig it) and the floor's layer.
pub(super) fn tunnel(sim: &mut Simulation) -> ((i32, i32), (i32, i32), i32) {
    let (x0, z) = (8, -26);
    let [start, wall, stand] = [x0, x0 + 11, x0 + 10].map(|x| tile(sim, x, z));
    let terrain = sim.world_mut().settlement_and_terrain_mut().1;
    let volume = terrain.volume.as_mut().unwrap();
    let top = volume.top(start).unwrap();
    let floor = top - 6;
    for i in 0..6 {
        volume.clear_box((start.0 + i, start.1), (start.0 + i, start.1), top - 1 - i, top + 8);
    }
    // Walled 2 m high from a tile before it, so a route only walks in and
    // out along it (a string-pulled route cut diagonally from a sloped
    // step onto the ground beside it is one a walker can't follow:
    // `todo/sacred-tree.md`, M8.8).
    for x in start.0 - 1..start.0 + 6 {
        for side in [start.1 - 1, start.1 + 1] {
            let ground = volume.top((x, side)).unwrap();
            volume.fill_box((x, side), (x, side), ground, ground + HEADROOM);
        }
    }
    for x in start.0 + 6..=stand.0 {
        assert!(volume.top((x, start.1)).unwrap() > floor + HEADROOM, "the tunnel at x {x} has a roof");
        volume.clear_box((x, start.1), (x, start.1), floor, floor + HEADROOM);
    }
    assert!(volume.top(wall).unwrap() > floor + HEADROOM);
    for layer in floor - 1..=floor + HEADROOM {
        assert!(volume.set_ore(wall, layer, Some(AMBER)), "the wall is solid at layer {layer}");
    }
    terrain.revision += 1;
    (wall, stand, floor)
}

/// What `who` is up to, and the ground piles: for a failed check.
pub(super) fn story(sim: &Simulation, who: AgentIx) -> String {
    let s = settlement(sim);
    let a = &s.agents[who as usize];
    let sprite = a.sprite().unwrap();
    let piles: Vec<_> = s.piles.as_ref().unwrap().piles.values().map(|p| (Terrain::pit_tile(p.position), p.item(), p.units(), p.height)).collect();
    format!(
        "plan {:?} step {}, at {:?} {:?} height {:?}, carrying {} {:?}, gave up {:?}, route {:?}, piles {piles:?}, {} s",
        sprite.plan,
        sprite.step,
        Terrain::pit_tile(a.body.position),
        a.body.position,
        sprite.height,
        a.carry.inventory.total(),
        a.carry.inventory.kind(),
        sprite.unreachable,
        a.body.route.iter().take(4).collect::<Vec<_>>(),
        sim.world().tick() / HZ
    )
}

/// Runs until `who`'s first dig has started and ended, returning it.
pub(super) fn first_dig(sim: &mut Simulation, who: AgentIx, seconds: u64) -> super::avatar::Dig {
    let mut first = None;
    for _ in 0..seconds * HZ {
        step(sim, &[]);
        let sprite = settlement(sim).agents[who as usize].sprite().unwrap();
        match (&first, &sprite.dig) {
            (None, Some(dig)) => first = Some(dig.clone()),
            (Some(_), None) => break,
            _ => {}
        }
    }
    first.unwrap_or_else(|| panic!("it dug: {}", story(sim, who)))
}

/// With ore only in the tunnel's end wall, a miner walks down the trench
/// into the tunnel and digs the wall in front of it level, in 9 s: the four cells over its floor, never the
/// floor's cell under them nor the ceiling's over them, nor its own tile.
/// Then it carries the amber up to the tree's amber pile.
#[test]
fn a_miner_digs_a_tunnel_wall_beside_it_never_its_floor_or_ceiling() {
    let mut sim = map();
    let (wall, stand, floor) = tunnel(&mut sim);
    // A lantern lights the tunnel: miners work only in light (M8.7).
    let lantern_at = tile(&sim, 16, -26);
    super::light_tests::light_up(&mut sim, lantern_at, floor);
    let wall_from_tree = (wall.0 - tile(&sim, 0, 0).0, wall.1 - tile(&sim, 0, 0).1);
    strip_ore(&mut sim, |t| t != wall_from_tree);
    let targets = miners::search(sim.world().terrain(), &settlement(&sim).light, Terrain::pit_tile(stone_pile(&sim)), sprite_radius());
    eprintln!("targets: {targets:?}");
    assert_eq!((targets[0].tile, targets[0].stand, targets[0].wall), (wall, stand, Some(floor)), "the amber wall first");
    assert!(targets.iter().all(|t| t.wall.is_some()), "walls only, in a volume");
    let own = sim.world().terrain().volume.as_ref().unwrap().spans(stand).to_vec();
    mine(&mut sim, REED);
    let dig = first_dig(&mut sim, REED, 240);
    assert_eq!((dig.tile, dig.layer), (wall, Some(floor)), "level, from its floor");
    assert_eq!(dig.until - dig.started, ticks_from_60hz(540), "9 s, as a pit's layer");
    let reed = &settlement(&sim).agents[REED as usize];
    assert_eq!(Terrain::pit_tile(reed.body.position), stand, "dug from the tunnel's end");
    assert!((reed.sprite().unwrap().height - height(floor)).abs() < Fx::HALF, "on the tunnel's floor");
    let volume = sim.world().terrain().volume.as_ref().unwrap();
    assert!((floor..floor + HEADROOM).all(|layer| !volume.is_solid(wall, layer)), "the wall's cells are dug");
    for layer in [floor - 1, floor + HEADROOM] {
        assert!(volume.is_solid(wall, layer) && volume.ore(wall, layer) == Some(AMBER), "the cell at layer {layer} stays");
    }
    assert_eq!(volume.spans(stand), &own[..], "its own tile is untouched");
    assert_eq!(dug_amber(&sim), (floor..floor + HEADROOM).map(|layer| crate::mine::cell_yield(wall, layer) as u64).sum::<u64>());
    for _ in 0..240 * HZ {
        step(&mut sim, &[]);
        if delivered_amber(&sim) as u64 == dug_amber(&sim) {
            break;
        }
    }
    assert_eq!(delivered_amber(&sim) as u64, dug_amber(&sim), "it carried the amber up to the tree: {}", story(&sim, REED));
    eprintln!("the tunnel's amber at the tree at {} s", sim.world().tick() / HZ);
}

/// With ore only in the mountain, a miner walks from the tree round the
/// hill, in at the cave's mouth, and digs amber from the hall's wall on
/// its floor (-1 m), then brings it to the tree.
#[test]
fn a_miner_walks_into_the_mountain_cave_and_mines_amber() {
    let mut sim = map();
    let mountain = |t: (i32, i32)| (-16..=4).contains(&t.0) && (-68..=-47).contains(&t.1);
    strip_ore(&mut sim, |t| !mountain(t));
    // A lantern in the hall's middle lights it (M8.7: miners work in light).
    let lantern_at = tile(&sim, -6, -50);
    super::light_tests::light_up(&mut sim, lantern_at, -2);
    mine(&mut sim, REED);
    let dig = first_dig(&mut sim, REED, 300);
    let (x, z) = (dig.tile.0 - tile(&sim, 0, 0).0, dig.tile.1 - tile(&sim, 0, 0).1);
    assert!((-9..=-3).contains(&x) && (-54..=-47).contains(&z), "a wall of the cave's hall: {x}, {z}");
    assert_eq!(dig.layer, Some(-2), "from the hall's floor");
    assert!((settlement(&sim).agents[REED as usize].sprite().unwrap().height - height(-2)).abs() < Fx::HALF);
    assert!(dug_amber(&sim) > 0, "it dug amber");
    for _ in 0..300 * HZ {
        step(&mut sim, &[]);
        if delivered_amber(&sim) > 0 {
            break;
        }
    }
    assert!(delivered_amber(&sim) > 0, "and brought it out of the mountain to the tree: {}", story(&sim, REED));
    eprintln!("the cave's amber at the tree at {} s", sim.world().tick() / HZ);
}

/// Miners never dig a protected column: amber in the hill's cliffs is no
/// target, and the amber tunnel's wall with a pile on it (its column, at
/// the surface) is left alone while the miners dig other walls.
#[test]
fn a_miner_never_digs_a_protected_column() {
    let mut sim = map();
    let (wall, _, floor) = tunnel(&mut sim);
    // Lit, so the wall would be a target but for the pile (M8.7).
    let lantern_at = tile(&sim, 16, -26);
    super::light_tests::light_up(&mut sim, lantern_at, floor);
    // Amber all through the plateau's north edge and the ground at its foot.
    let cliff: Vec<(i32, i32)> = (-20..=-12).flat_map(|z| (-4..=4).map(move |x| (x, z))).map(|(x, z)| tile(&sim, x, z)).collect();
    {
        let terrain = sim.world_mut().settlement_and_terrain_mut().1;
        let volume = terrain.volume.as_mut().unwrap();
        for &t in &cliff {
            for layer in -20..40 {
                volume.set_ore(t, layer, Some(AMBER));
            }
        }
        terrain.revision += 1;
    }
    let terrain = sim.world().terrain();
    let hill: Vec<_> = cliff.iter().filter(|&&t| terrain.is_undiggable(t)).collect();
    assert!(!hill.is_empty() && hill.len() < cliff.len(), "the plateau's edge and its foot");
    let targets = miners::search(terrain, &settlement(&sim).light, Terrain::pit_tile(stone_pile(&sim)), sprite_radius());
    assert!(targets.iter().all(|t| !terrain.is_undiggable(t.tile)), "no target on the hill: {targets:?}");
    // Wood on the wall's column (miners don't fetch wood).
    let wood = settlement(&sim).item_index("wood").unwrap();
    let above = Terrain::pit_center(wall);
    {
        let (s, terrain) = sim.world_mut().settlement_and_terrain_mut();
        let terrain = terrain.clone();
        s.spill(wood, 3, above, Fx::ZERO, &terrain);
        s.accounts[wood as usize].generated += 3;
    }
    // Moved onto the column's middle (the spill puts it beside).
    let ground = sim.world().terrain().height(above);
    let pile = sim.world_mut().settlement_mut().piles.as_mut().unwrap().piles.values_mut().find(|p| p.item() == Some(wood)).unwrap();
    (pile.position, pile.height) = (above, ground);
    assert_eq!(settlement(&sim).view(ELDER, 0, sim.world().terrain()).dig_blocked(wall), Some(crate::terrain::DigRefusal::Pile));
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: MOSS, job: SpriteJob::Mine }), command(CommandKind::SetSpriteJob { agent: REED, job: SpriteJob::Mine })]);
    let mut digs = BTreeSet::new();
    for _ in 0..120 * HZ {
        step(&mut sim, &[]);
        let s = settlement(&sim);
        let view = s.view(ELDER, sim.world().tick(), sim.world().terrain());
        for a in [MOSS, REED] {
            if let Some(dig) = &s.agents[a as usize].sprite().unwrap().dig {
                assert!(!sim.world().terrain().is_undiggable(dig.tile) && view.dig_blocked(dig.tile).is_none(), "{a} digs a protected column {:?}", dig.tile);
                digs.insert(dig.tile);
            }
        }
    }
    assert!(!digs.is_empty(), "they dug other walls");
    let volume = sim.world().terrain().volume.as_ref().unwrap();
    assert!((floor..floor + HEADROOM).all(|layer| volume.is_solid(wall, layer)), "the wall under the pile stands");
    eprintln!("dug {digs:?}, none of the {} hill tiles", hill.len());
}

/// With no ore in any wall a miner can reach (the town stripped of it),
/// a miner waits beside the tree's stone pile, digging nothing.
#[test]
fn with_no_faces_a_miner_idles_by_the_stone_pile() {
    let mut sim = map();
    strip_ore(&mut sim, |_| true);
    assert!(miners::search(sim.world().terrain(), &settlement(&sim).light, Terrain::pit_tile(stone_pile(&sim)), sprite_radius()).is_empty());
    mine(&mut sim, REED);
    for _ in 0..30 * HZ {
        step(&mut sim, &[]);
        assert!(settlement(&sim).agents[REED as usize].sprite().unwrap().dig.is_none());
    }
    let reed = &settlement(&sim).agents[REED as usize];
    let sprite = reed.sprite().unwrap();
    assert_eq!((sprite.current(), sprite.dig_claim), (Some(Step::Wait), None), "{:?}", sprite.plan);
    assert!(nav::distance(reed.body.position, stone_pile(&sim)) < Fx::from_int(2));
    assert_eq!(dug_stone(&sim) + dug_amber(&sim), 0);
}

/// Miners are a steady but modest income (M6.7's tuning): two miners on
/// the map, left alone for five minutes, bring stone to the tree every
/// minute, but far less than the elder digs (a layer in 2 s, a vein's ten
/// stone layers in a minute or so): enough to keep a ballista or two
/// stocked between waves, not to stock them mid-wave. (Since M8.4 they mine
/// walls; what they dig in the town's volume is reported, amber too.)
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
    eprintln!("two miners, stone at the tree each minute: {minutes:?}, {total} in 5 min; amber {} at the tree, {} dug", delivered_amber(&sim), dug_amber(&sim));
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
        let targets = miners::search(terrain, &settlement(&sim).light, Terrain::pit_tile(stone_pile(&sim)), sprite_radius());
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
/// search isn't saved, and the ground gives it back. In the town's volume
/// the save holds a wall dig in a plan (`Step::DigWall`, format 44).
#[test]
fn miners_are_deterministic_across_threads_and_saves() {
    let run = |threads: usize, save_at: Option<u64>| {
        let mut sim = Simulation::from_world(map().into_world(), threads);
        let jobs = [command(CommandKind::SetSpriteJob { agent: MOSS, job: SpriteJob::Mine }), command(CommandKind::SetSpriteJob { agent: REED, job: SpriteJob::Mine })];
        let mut hashes = Vec::new();
        let mut saved = false;
        for tick in 0..80 * HZ {
            // From `save_at` on, once a miner's plan holds a wall dig.
            let walls = || settlement(&sim).agents.iter().filter_map(|a| a.sprite()).any(|s| s.plan.iter().any(|step| matches!(step, Step::DigWall { .. })));
            if !saved && save_at.is_some_and(|at| tick >= at) && walls() {
                saved = true;
                let world = sim.into_world();
                let bytes = postcard::to_allocvec(&world.to_state()).unwrap();
                let rules = world.rules().clone();
                let loaded = World::from_state(postcard::from_bytes(&bytes).unwrap(), rules).unwrap();
                sim = Simulation::from_world(loaded, threads);
            }
            let turn = if tick == 0 { &jobs[..] } else { &[] };
            hashes.push(sim.step(turn).total);
        }
        (hashes, dug_stone(&sim), saved)
    };
    let (one, dug, _) = run(1, None);
    assert!(dug > 0, "they dug in the time");
    assert_eq!(one, run(4, None).0, "the thread count changed the miners");
    let (saved, _, walls) = run(2, Some(30 * HZ));
    assert!(walls, "saved while a miner planned a wall dig");
    assert_eq!(one, saved, "a save mid-dig changed them");
}
