//! The Sacred Tree's big map (M6.3 of the mining design, M5 and M7): 256 ×
//! 256 m of generated ground to dig, the town on an undiggable plateau 2 m
//! up with cliff sides and two ramps, stone exposed 15 to 40 m from the
//! tree, and ghouls from a ring 40 m out coming up the ramps. The map is
//! the scene's own: its mine line is read from `sacred_tree.ron`.

use std::time::Instant;

use content::scenario::Scenario;
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::{AgentIx, BuildingKindIx, PlacementRefusal, SceneSetup, Settlement};
use crate::mine::{Hill, EXPOSED_VEINS, STONE};
use crate::nav;
use crate::terrain::{DigRefusal, Disc, Terrain, LAYER};
use crate::{Simulation, World};

const SCENE: &str = include_str!("../../../app/assets/levels/sacred_tree.ron");
const SUPPLY: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
const STONE_GATHERER: BuildingKindIx = 1;
/// How wide a wave plans its route (`combat_housekeeping`).
fn ghoul_clearance() -> Fx {
    Fx::from_ratio(1, 5)
}

/// The number after `key: ` on the scene's `mine:` line.
fn mine_number(key: &str) -> u64 {
    let line = SCENE.lines().find(|l| l.starts_with("mine:")).expect("the scene has a mine");
    let at = line.find(&format!("{key}: ")).unwrap_or_else(|| panic!("no {key} in {line}")) + key.len() + 2;
    line[at..].chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap()
}

/// The scene's hill, as `lockstep::scene_setup` reads it.
pub(super) fn hill() -> Hill {
    let n = |key: &str| mine_number(key) as u8;
    Hill { center: (0, 0), plateau: n("plateau"), steps: n("height"), ramp_width: n("ramp_width"), ramp_length: n("ramp_length"), foot: n("foot") }
}

/// The Sacred Tree as the app makes it from its scene: `scenario` on the
/// scene's sloped, six-layer mine with its hill, the trunk blocking the
/// ground as the architecture does.
pub(super) fn sacred_tree_map(scenario: Scenario) -> Simulation {
    let setup = SceneSetup {
        scenario: Some(scenario),
        player: "elder".into(),
        obstacles: vec![Disc { center: FxVec2::ZERO, radius: Fx::ONE }],
        diggable: true,
        sloped: true,
        dig_layers: Some(6),
        mine: Some(mine_number("seed")),
        hill: Some(hill()),
        spawn: [Fx::ZERO, Fx::ZERO, Fx::from_ratio(17, 5)],
        ..SceneSetup::default()
    };
    Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1)
}

fn map() -> Simulation {
    sacred_tree_map(Scenario::parse(SUPPLY).unwrap())
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

/// The tile `x`, `y` tiles from the tree's.
fn tile(terrain: &Terrain, x: i32, y: i32) -> (i32, i32) {
    let tree = Terrain::pit_tile(terrain.center);
    (tree.0 + x, tree.1 + y)
}

/// A point `x`, `y` meters from the tree.
fn at(terrain: &Terrain, x: f64, y: f64) -> FxVec2 {
    let fx = |v: f64| Fx::from_bits((v * 4294967296.0).round() as i64);
    FxVec2::new(terrain.center.x + fx(x), terrain.center.y + fx(y))
}

/// Every leg of `route` from `from` is walkable for a body of `radius`.
fn walks(terrain: &Terrain, from: FxVec2, route: &[FxVec2], radius: Fx) -> bool {
    let mut from = from;
    route.iter().all(|&to| std::mem::replace(&mut from, to) == to || terrain.walkable(from, to, radius))
}

/// Where `route` from `from` crosses the plateau's edge `x` meters from
/// the tree (its -x or +x side), as meters along y from the tree.
fn crossing(terrain: &Terrain, from: FxVec2, route: &[FxVec2], x: Fx) -> Option<Fx> {
    let edge = terrain.center.x + x;
    let mut a = from;
    for &b in route {
        if (a.x - edge) * (b.x - edge) <= Fx::ZERO && a.x != b.x {
            let y = a.y + (b.y - a.y) * ((edge - a.x) / (b.x - a.x));
            return Some(y - terrain.center.y);
        }
        a = b;
    }
    None
}

/// The town's ground is the plateau's top, at zero as before; the ground
/// round it lies 2 m below. Its sides are cliffs too steep to walk, and
/// its two ramps, a quarter meter a tile, walk straight down to the foot.
#[test]
fn the_tree_stands_on_a_plateau_with_cliffs_and_two_ramps() {
    let sim = map();
    let terrain = sim.world().terrain();
    let hill = terrain.mine.unwrap().hill.expect("the scene's hill");
    assert_eq!(hill.center, Terrain::pit_tile(terrain.center), "centered on the tree");
    let (p, w, l) = (hill.plateau as i32, (hill.ramp_width / 2) as i32, hill.ramp_length as i32);
    let height = -LAYER.mul_int(hill.steps as i64 / 2);
    assert_eq!(height, -Fx::from_int(2), "2 m high");
    assert!(p >= 14, "wide enough for the 26 m town (plateau {p})");
    assert!(hill.ramp_width >= 5 && l >= 4, "a few tiles wide, at least 4 long");
    // The plateau: level, and gentle up to the ring next to its edge.
    for x in -p..=p {
        for y in -p..=p {
            assert_eq!(terrain.tile_height(tile(terrain, x, y)), Some(Fx::ZERO));
            if x.abs() < p && y.abs() < p {
                assert!(!terrain.steep(tile(terrain, x, y)), "({x}, {y}) on the plateau is walkable");
            }
        }
    }
    // Its sides drop 2 m to the foot, too steep to walk.
    let r = Fx::from_ratio(8, 25);
    for (x, y) in [(0, p + 1), (0, -p - 1), (p + 1, p + 1), (-p - 1, w + 1), (p + 1, -w - 1)] {
        assert_eq!(terrain.tile_height(tile(terrain, x, y)), Some(height), "the foot at ({x}, {y})");
    }
    for (x, y) in [(0, p), (3, -p), (p, 5), (-p, -5)] {
        assert!(terrain.steep(tile(terrain, x, y)), "the cliff at ({x}, {y})");
    }
    assert!(!terrain.walkable(at(terrain, 0.0, -10.0), at(terrain, 0.0, -20.0), r), "no way down the north cliff");
    assert!(!terrain.walkable(at(terrain, 5.0, 20.0), at(terrain, 5.0, 10.0), r), "nor up the south one");
    // The ramps fall evenly to the foot, and walk straight up and down.
    for side in [-1, 1] {
        for k in 1..=l {
            let h = terrain.tile_height(tile(terrain, side * (p + k), 0)).unwrap();
            assert_eq!(h, height * Fx::from_int(k) / Fx::from_int(l), "ramp tile {k} on side {side}");
        }
        for y in -(w - 1)..=(w - 1) {
            for k in 0..=l {
                assert!(!terrain.steep(tile(terrain, side * (p + k), y)), "ramp tile ({k}, {y}) on side {side}");
            }
        }
        let (top, foot) = (at(terrain, (side * 8) as f64, 0.0), at(terrain, (side * (p + l + 6)) as f64, 0.0));
        assert!(terrain.walkable(top, foot, r) && terrain.walkable(foot, top, r), "down and up the ramp on side {side}");
        let (top, foot) = (at(terrain, (side * 8) as f64, (w - 1) as f64), at(terrain, (side * (p + l + 6)) as f64, (w - 1) as f64));
        assert!(terrain.walkable(top, foot, r), "along its edge on side {side}");
    }
    // Round the foot the ground stays flat, then the terraces rise.
    for (x, y) in [(0, p + hill.foot as i32), (-p - hill.foot as i32, 0), (p + 5, p + 5)] {
        assert_eq!(terrain.tile_height(tile(terrain, x, y)), Some(height), "flat at ({x}, {y})");
    }
    let highest = (-128..128).flat_map(|x| (-128..128).map(move |y| (x, y))).filter_map(|(x, y)| terrain.tile_height(tile(terrain, x, y))).max().unwrap();
    assert!(highest > Fx::ZERO, "terraces far off rise past the plateau ({highest:?})");
    assert_eq!(terrain.extent, Fx::from_int(128), "256 × 256 m");
}

/// Nobody digs the plateau or its ramps; the ground at the foot digs as
/// any other.
#[test]
fn nobody_digs_the_hill() {
    let sim = map();
    let terrain = sim.world().terrain();
    let hill = terrain.mine.unwrap().hill.unwrap();
    let (p, l) = (hill.plateau as i32, hill.ramp_length as i32);
    let r = Fx::from_ratio(8, 25);
    let refusal = |own: (i32, i32), to: (i32, i32)| terrain.dig_refusal(tile(terrain, own.0, own.1), tile(terrain, to.0, to.1), r);
    assert_eq!(refusal((5, 5), (6, 5)), Some(DigRefusal::Undiggable), "the plateau");
    assert_eq!(refusal((p, 0), (p - 1, 0)), Some(DigRefusal::Undiggable), "its edge");
    assert_eq!(refusal((-p - 3, 0), (-p - 4, 0)), Some(DigRefusal::Undiggable), "the west ramp");
    assert_eq!(refusal((p + l + 1, 1), (p + l, 1)), Some(DigRefusal::Undiggable), "the east ramp's foot");
    assert_eq!(refusal((0, p + 4), (0, p + 3)), None, "the ground at the cliff's foot");
    assert_eq!(refusal((p + l + 2, 0), (p + l + 1, 0)), None, "the ground at a ramp's foot");
    assert!(terrain.undiggable.len() == 3, "{:?}", terrain.undiggable);
}

/// The generator puts stone at the surface 15 to 40 m from the tree, off
/// the hill, where the elder can dig it from the tile beside it, so the
/// first stone needs no earthwork; the seed moves the veins.
#[test]
fn stone_lies_exposed_15_to_40_m_from_the_tree() {
    let sim = map();
    let terrain = sim.world().terrain();
    let mine = terrain.mine().unwrap();
    let hill = mine.spec.hill.unwrap();
    let veins = hill.exposed_veins(mine.spec.seed);
    assert_eq!(veins.len(), EXPOSED_VEINS);
    let tree = terrain.center;
    let r = Fx::from_ratio(8, 25);
    for &(x, y) in &veins {
        let away = nav::distance(tree, Terrain::pit_center((x, y)));
        assert!(away >= Fx::from_int(15) && away <= Fx::from_int(40), "{:?} is {away:?} from the tree", (x, y));
        for t in [(x, y), (x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
            assert_eq!(mine.ore(t, 0), Some(STONE), "stone on top at {t:?}");
            assert_eq!(mine.ore(t, 1), Some(STONE), "and below it at {t:?}");
            assert!(!terrain.is_undiggable(t), "{t:?} is off the hill");
        }
        // Dug from the tile beside it, along x away from the tree.
        let side = if x > tree.x.round().floor_to_int() as i32 { 1 } else { -1 };
        assert_eq!(terrain.dig_refusal((x + 2 * side, y), (x + side, y), r), None, "the vein at {:?} digs", (x, y));
    }
    let quarters: std::collections::BTreeSet<(bool, bool)> = veins.iter().map(|&(x, y)| (Fx::from_int(x) > tree.x, Fx::from_int(y) > tree.y)).collect();
    assert_eq!(quarters.len(), 4, "one in each quarter: {veins:?}");
    assert_ne!(hill.exposed_veins(mine.spec.seed + 1), veins, "the seed moves them");
    let exposed = (-40..=40)
        .flat_map(|x| (-40..=40).map(move |y| (x, y)))
        .map(|(x, y)| tile(terrain, x, y))
        .filter(|&t| {
            let away = nav::distance(tree, Terrain::pit_center(t));
            away >= Fx::from_int(15) && away <= Fx::from_int(40) && mine.ore(t, 0).is_some()
        })
        .count();
    assert!(exposed >= 5 * EXPOSED_VEINS, "{exposed} stone tiles at the surface 15 to 40 m out");
    eprintln!("stone at the surface 15 to 40 m from the tree: {exposed} tiles; veins at {veins:?}");
}

/// Every lane's ghouls start on the ring 40 m out, one side per ramp, and
/// their route comes up that side's ramp: the plateau's edge is crossed
/// on the ramp, and every leg is walkable. Prints what a route costs.
#[test]
fn ghouls_come_from_the_ring_up_the_ramps() {
    let sim = map();
    let terrain = sim.world().terrain();
    let hill = terrain.mine.unwrap().hill.unwrap();
    let (p, w) = (Fx::from_int(hill.plateau as i32) + Fx::HALF, Fx::from_int((hill.ramp_width / 2) as i32) + Fx::HALF);
    let s = settlement(&sim);
    let mut sides = std::collections::BTreeSet::new();
    for wave in &s.waves {
        let ring = nav::distance(terrain.center, wave.position);
        assert!((Fx::from_int(38)..=Fx::from_int(42)).contains(&ring), "{} starts {ring:?} out", wave.id);
        let started = Instant::now();
        let route = nav::path(terrain, wave.position, wave.target, ghoul_clearance());
        let took = started.elapsed();
        assert!(!route.is_empty(), "{} has a route", wave.id);
        assert!(walks(terrain, wave.position, &route, ghoul_clearance()), "{}'s route is walkable: {route:?}", wave.id);
        let side = if wave.position.x < terrain.center.x { -p } else { p };
        let y = crossing(terrain, wave.position, &route, side).unwrap_or_else(|| panic!("{} never climbs the hill: {route:?}", wave.id));
        assert!(y.abs() < w, "{} comes up the ramp, at y {y:?}", wave.id);
        sides.insert(side > Fx::ZERO);
        eprintln!("{}: route of {} legs in {took:?}", wave.id, route.len());
    }
    assert_eq!(sides.len(), 2, "both ramps have lanes");
}

/// The town stands on the plateau as it did on flat ground: its prebuilt
/// Wood Gatherer and its open ground to the east take buildings. A
/// building can't go over a pit, nor on the cliff's edge.
#[test]
fn buildings_stand_on_the_plateau_but_not_over_pits_or_cliffs() {
    let mut sim = map();
    let s = settlement(&sim);
    let terrain = sim.world().terrain();
    assert_eq!(s.buildings.len(), 1, "the Wood Gatherer");
    let check = |sim: &Simulation, x: f64, y: f64| {
        let terrain = sim.world().terrain();
        settlement(sim).placement_check(STONE_GATHERER, at(terrain, x, y), terrain)
    };
    assert!(check(&sim, 5.0, 0.0).is_ok(), "open ground to the east");
    let hill = terrain.mine.unwrap().hill.unwrap();
    let edge = hill.plateau as f64;
    assert_eq!(check(&sim, 4.0, edge - 0.2), Err(PlacementRefusal::Uneven), "the cliff's edge");
    // A pit at the foot, south of the hill.
    let foot = (0, hill.plateau as i32 + 5);
    let pit = tile(terrain, foot.0, foot.1);
    assert!(check(&sim, 0.5, foot.1 as f64).is_ok(), "flat ground at the foot");
    sim.world_mut().settlement_and_terrain_mut().1.deepen(pit);
    assert_eq!(check(&sim, 0.5, foot.1 as f64), Err(PlacementRefusal::Uneven), "over the pit");
    assert_eq!(PlacementRefusal::Uneven.message(), "UNEVEN GROUND");
}

/// The elder walks from the tree down the east ramp to the foot, and a
/// sprite's errand on the plateau plans as before: on a 256 m map nav
/// searches a window round the start and goal, so routes stay cheap.
/// Prints what they cost.
#[test]
fn the_elder_walks_down_a_ramp_to_the_foot_and_back() {
    let mut sim = map();
    let terrain = sim.world().terrain();
    let hill = terrain.mine.unwrap().hill.unwrap();
    let foot = at(terrain, (hill.plateau + hill.ramp_length + 4) as f64, 4.0);
    let from = settlement(&sim).agents[ELDER as usize].body.position;
    let r = settlement(&sim).agents[ELDER as usize].radius();
    let started = Instant::now();
    let down = nav::path(terrain, from, foot, r);
    eprintln!("tree to the east foot: {} legs in {:?}", down.len(), started.elapsed());
    assert!(!down.is_empty() && walks(terrain, from, &down, r));
    let started = Instant::now();
    let errand = nav::path(terrain, at(terrain, -1.7, 2.4), at(terrain, 6.0, -3.0), r);
    eprintln!("an errand on the plateau: {} legs in {:?}", errand.len(), started.elapsed());
    assert!(!errand.is_empty());
    // The worst case: a goal with no way to it (a pit two layers deep at
    // the foot) searches the whole window, the hill and its ramps among it.
    let mut walled = terrain.clone();
    let pit = tile(terrain, 0, hill.plateau as i32 + 6);
    walled.deepen(pit);
    walled.deepen(pit);
    let started = Instant::now();
    assert!(nav::path(&walled, from, Terrain::pit_center(pit), r).is_empty(), "no way into the pit");
    eprintln!("no way into a pit at the foot: searched in {:?}", started.elapsed());
    let go = |target| crate::Command {
        player: crate::PlayerId(0),
        kind: crate::CommandKind::MoveAvatar { agent: ELDER, intent: super::avatar::Intent::GoTo { target, run: true } },
    };
    sim.step(&[go(foot)]);
    for _ in 0..40 * HZ {
        sim.step(&[]);
        settlement(&sim).validate().unwrap();
    }
    let a = &settlement(&sim).agents[ELDER as usize];
    assert!(nav::distance(a.body.position, foot) < Fx::from_ratio(1, 5), "at the foot: {:?}", a.body.position);
    assert_eq!(a.avatar().unwrap().height, -Fx::from_int(2), "2 m down");
    sim.step(&[go(from)]);
    for _ in 0..40 * HZ {
        sim.step(&[]);
    }
    let a = &settlement(&sim).agents[ELDER as usize];
    assert!(nav::distance(a.body.position, from) < Fx::from_ratio(1, 5), "back by the tree: {:?}", a.body.position);
    assert_eq!(a.avatar().unwrap().height, Fx::ZERO);
}
