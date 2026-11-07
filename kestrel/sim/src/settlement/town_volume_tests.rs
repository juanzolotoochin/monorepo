//! The Sacred Tree's town as a volume (M8.3 of the depths design,
//! `SACRED_TREE_DEPTHS.md` T1, T2, T4 and T6): the generated terraces and
//! the hill become solid cells at load with the surface where it was (a
//! quarter-meter terrace step rounded down to its layer), 16 layers deep
//! and a layer of bedrock; stone veins go on down, amber lies from 4 m down
//! and in the mountain; a mountain spur north of the town has a cave to
//! walk into; nobody digs the hill, the tree, buildings, sites or piles at
//! any depth; the elder digs a stair down 8 m and no deeper.

use content::scenario::Scenario;
use fx::{Fx, FxVec2};

use super::dig_tests::facing;
use super::tunnel_lab_tests::{command, feet, me, place};
use super::{AgentIx, Settlement};
use crate::mine::{AMBER, AMBER_DEPTH, STONE};
use crate::nav;
use crate::terrain::{DigRefusal, Terrain, LAYER};
use crate::volume::{height, DigDirection, Volume, VolumeBox};
use crate::{CommandKind, Simulation};

const SUPPLY: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");
const ELDER: AgentIx = 0;

/// The town, no waves.
fn town() -> Simulation {
    let mut scenario = Scenario::parse(SUPPLY).unwrap();
    scenario.waves.clear();
    super::hill_tests::sacred_tree_map(scenario)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn volume(sim: &Simulation) -> &Volume {
    sim.world().terrain().volume.as_ref().expect("the town is a volume")
}

/// The tile `x`, `y` tiles from the tree's.
fn tile(sim: &Simulation, x: i32, y: i32) -> (i32, i32) {
    let tree = Terrain::pit_tile(sim.world().terrain().center);
    (tree.0 + x, tree.1 + y)
}

/// The mountain's footprint: the tiles its solid boxes raise, from the
/// tree's.
fn mountain_tiles() -> Vec<(i32, i32)> {
    let mut tiles = std::collections::BTreeSet::new();
    for b in super::hill_tests::mountain().boxes {
        if let VolumeBox::Solid { min, max, .. } = b {
            for x in min[0]..=max[0] {
                for y in min[1]..=max[1] {
                    tiles.insert((x, y));
                }
            }
        }
    }
    tiles.into_iter().collect()
}

/// Every tile's surface is the old heightfield's, tile by tile, off the
/// mountain: exactly where the terraces' steps are even (the plateau, the
/// foot round it, every other step), a quarter meter lower where they are
/// odd. The hill's datum stays zero.
#[test]
fn the_surface_matches_the_old_heightfield() {
    let sim = town();
    let terrain = sim.world().terrain();
    let mine = terrain.mine().expect("the mine stays");
    let mountain: std::collections::BTreeSet<(i32, i32)> = mountain_tiles().into_iter().map(|(x, y)| tile(&sim, x, y)).collect();
    let (min, size) = (mine.spec.min, mine.spec.size as i32);
    let (mut exact, mut lower, mut tiles) = (0, 0, 0);
    for x in min.0..min.0 + size {
        for y in min.1..min.1 + size {
            if mountain.contains(&(x, y)) {
                continue;
            }
            tiles += 1;
            let old = mine.height((x, y)).unwrap();
            let now = terrain.tile_height((x, y)).unwrap();
            if now == old {
                exact += 1;
            } else {
                assert_eq!(old - now, Fx::from_ratio(1, 4), "({x}, {y}): {old:?} became {now:?}");
                lower += 1;
            }
            assert_eq!(now, LAYER * (old / LAYER).floor(), "({x}, {y}) is its height in whole layers");
        }
    }
    assert_eq!(tiles, size * size - mountain.len() as i32);
    assert!(exact > lower, "most of the ground is where it was: {exact} exact, {lower} a quarter meter lower");
    let hill = mine.spec.hill.unwrap();
    for x in -(hill.plateau as i32)..=hill.plateau as i32 {
        assert_eq!(terrain.tile_height(tile(&sim, x, 0)), Some(Fx::ZERO), "the plateau at ({x}, 0)");
    }
    eprintln!("surface: {exact} tiles exact, {lower} a quarter meter lower, of {tiles}");
}

/// Under every tile the ground goes 16 layers (8 m) down and one of
/// bedrock: one span, from the surface down, outside the mountain.
#[test]
fn the_ground_is_sixteen_layers_deep() {
    let sim = town();
    let terrain = sim.world().terrain();
    assert_eq!(terrain.max_layers, 16);
    let mine = terrain.mine().unwrap();
    let (min, size) = (mine.spec.min, mine.spec.size as i32);
    let mountain: std::collections::BTreeSet<(i32, i32)> = mountain_tiles().into_iter().map(|(x, y)| tile(&sim, x, y)).collect();
    for x in (min.0..min.0 + size).step_by(7) {
        for y in (min.1..min.1 + size).step_by(5) {
            let top = mine.volume_top((x, y)).unwrap();
            let spans = volume(&sim).spans((x, y));
            assert_eq!(spans.first().unwrap().bottom, top - 17, "({x}, {y}): 16 layers and the bedrock");
            if !mountain.contains(&(x, y)) {
                assert_eq!(spans, [crate::volume::Span { bottom: top - 17, top }], "({x}, {y})");
            }
        }
    }
    assert_eq!(volume(&sim).columns().count() as i32, size * size, "every tile of the map");
}

/// Stone as on the heightfield in the top six layers, going on down
/// richer; amber only 4 m or more under the generated surface, or in the
/// mountain's rock, and some of each; most of the ground still earth.
#[test]
fn veins_go_down_and_amber_lies_deep_and_in_the_mountain() {
    let sim = town();
    let terrain = sim.world().terrain();
    let mine = terrain.mine().unwrap();
    let mountain: std::collections::BTreeSet<(i32, i32)> = mountain_tiles().into_iter().map(|(x, y)| tile(&sim, x, y)).collect();
    let mut stone = [0u32; 16];
    let (mut deep_amber, mut mountain_amber, mut cells) = (0, 0, 0);
    for ((t, layer), mineral) in volume(&sim).ore_cells() {
        let top = mine.volume_top(t).unwrap();
        let depth = top - 1 - layer;
        if mineral == AMBER {
            if mountain.contains(&t) {
                mountain_amber += 1;
            } else {
                assert!(depth >= AMBER_DEPTH as i32, "amber at {t:?} layer {layer} is {depth} layers down");
                deep_amber += 1;
            }
            continue;
        }
        assert_eq!(mineral, STONE, "a town holds stone and amber");
        if !mountain.contains(&t) {
            stone[depth as usize] += 1;
            // The top six layers as the heightfield had them.
            if depth < 6 {
                assert_eq!(mine.ore(t, depth as u8), Some(STONE));
            }
        }
    }
    for (t, _) in volume(&sim).columns() {
        if !mountain.contains(&t) {
            cells += 16;
        }
    }
    assert!(stone[..9].windows(2).all(|w| w[0] < w[1]), "richer deeper: {stone:?}");
    assert!(stone.iter().sum::<u32>() * 2 < cells, "most of the ground is earth: {stone:?} of {cells}");
    assert!(deep_amber > 0 && deep_amber * 50 < cells, "amber is deep and rare: {deep_amber} of {cells}");
    assert!(mountain_amber >= 20, "the mountain holds amber: {mountain_amber}");
    // The cave's west wall by the mouth shows it.
    assert_eq!(volume(&sim).ore(tile(&sim, -9, -48), 0), Some(AMBER));
    eprintln!("stone by depth: {stone:?}; amber {deep_amber} deep, {mountain_amber} in the mountain, of {cells} cells");
}

/// The mountain spur: 8 to 10 m over the ground round its foot, its foot
/// 40 to 60 m from the tree, north (-z) of the town, away from the wave
/// lanes along x (their rings at x ±28, z 0).
#[test]
fn the_mountain_stands_north_of_the_town_8_to_10_m_high() {
    let sim = town();
    let terrain = sim.world().terrain();
    let mine = terrain.mine().unwrap();
    let tiles = mountain_tiles();
    let top = tiles.iter().map(|&(x, y)| volume(&sim).top(tile(&sim, x, y)).unwrap()).max().unwrap();
    let foot = tiles.iter().map(|&(x, y)| mine.volume_top(tile(&sim, x, y)).unwrap()).max().unwrap();
    let high = height(top) - height(foot);
    assert!(high >= Fx::from_int(8) && high <= Fx::from_int(10), "{high:?} high");
    let near = tiles.iter().map(|&(x, y)| Fx::from_int(x * x + y * y).sqrt()).min().unwrap();
    assert!(near >= Fx::from_int(40) && near <= Fx::from_int(60), "its foot {near:?} from the tree");
    assert!(tiles.iter().all(|&(_, y)| y < -40), "north of the town");
    for wave in Scenario::parse(SUPPLY).unwrap().waves {
        assert!(wave.position[1].abs() < Fx::from_int(2) && wave.position[0].abs() > Fx::from_int(20), "{} is a lane along x", wave.id);
    }
}

/// The cave mouth: from the open ground in front of the mountain a path
/// walks in on foot to the back of the hall, under its roof with 3 m over
/// the floor, and on into the passage west.
#[test]
fn the_caves_mouth_walks_in_from_the_surface() {
    let sim = town();
    let terrain = sim.world().terrain();
    let r = Fx::from_ratio(8, 25);
    let at = |x: i32, y: i32| Terrain::pit_center(tile(&sim, x, y));
    let outside = at(-6, -40);
    let z = terrain.ground(outside).unwrap();
    assert!(terrain.ceiling_at(outside, z, r).is_none(), "outside under the sky");
    for (goal, name) in [(at(-6, -52), "the back of the hall"), (at(-10, -51), "the passage")] {
        let route = nav::path_at(terrain, outside, z, goal, None, r, &[]);
        assert!(!route.is_empty(), "a walk to {name}");
        let &(end, feet) = route.last().unwrap();
        assert_eq!((end, feet), (goal, -Fx::ONE), "{name}'s floor is at -1 m");
        assert!(terrain.ceiling_at(end, feet, r).is_some(), "{name} is under the mountain");
        let mut a = (outside, z);
        for &(b, feet) in &route {
            assert_eq!(terrain.walk_at(a.0, b, a.1, r), Some(feet), "every leg to {name} walks");
            a = (b, feet);
        }
    }
    assert_eq!(volume(&sim).headroom(tile(&sim, -6, -50), -2), Some(6), "3 m high");
}

/// The elder's would-be level dig from a shaft dug straight down beside
/// `target` (tiles from the tree), at every floor of the shaft down to
/// 8 m: what each refuses, as the app asks (`dig_volume_of`).
fn shaft_refusals(sim: &mut Simulation, shaft: (i32, i32), target: (i32, i32)) -> Vec<Result<((i32, i32), i32), DigRefusal>> {
    let (s, t) = (tile(sim, shaft.0, shaft.1), tile(sim, target.0, target.1));
    let mut out = Vec::new();
    loop {
        let top = sim.world().terrain().volume.as_ref().unwrap().top(s).unwrap();
        let generated = sim.world().terrain().generated_top(s).unwrap();
        if top <= generated - 16 {
            break;
        }
        assert!(sim.world_mut().settlement_and_terrain_mut().1.clear_cell(s, top - 1));
        let center = Terrain::pit_center(s) - sim.world().terrain().center;
        place(sim, center.x.to_f64(), center.y.to_f64(), height(top - 1));
        let heading = facing(t.0 - s.0, t.1 - s.1);
        sim.world_mut().settlement_mut().agents[ELDER as usize].body.heading = heading;
        let (st, terrain) = (settlement(sim), sim.world().terrain());
        out.push(st.dig_volume_of(ELDER, sim.world().tick(), terrain, DigDirection::Level).unwrap());
    }
    out
}

/// Nobody digs under the town (T6): from a shaft beside each, the hill's
/// plateau and a ramp, the tree, a building's site and a pile on the ground
/// refuse a dig at every depth down to 8 m, while the ground on the
/// shaft's other side digs (but by the tree, the plateau all round).
#[test]
fn protected_columns_refuse_digs_at_every_depth() {
    let mut sim = town();
    let hill = sim.world().terrain().mine.unwrap().hill.unwrap();
    let p = hill.plateau as i32;
    // A site at the north foot, and stone spilled on the ground west of it.
    let site_at = sim.world().terrain().center + FxVec2::new(Fx::ZERO, Fx::from_int(-21));
    command(&mut sim, CommandKind::PlaceBlueprint { kind: 0, position: site_at });
    assert_eq!(settlement(&sim).buildings.len(), 2, "the site is placed");
    let stone = settlement(&sim).item_index(crate::mine::STONE_ID).unwrap();
    let pile_at = sim.world().terrain().center + FxVec2::new(Fx::from_int(-8), Fx::from_int(-21));
    {
        let (s, terrain) = sim.world_mut().settlement_and_terrain_mut();
        let terrain = terrain.clone();
        s.spill(stone, 3, pile_at, Fx::ZERO, &terrain);
    }
    let pile = settlement(&sim).piles.as_ref().unwrap().piles.values().find(|pile| pile.item() == Some(stone)).unwrap().position;
    let pile = Terrain::pit_tile(pile - sim.world().terrain().center);
    let cases = [
        ((p + 1, 8), (p, 8), DigRefusal::Undiggable, "the plateau"),
        ((p + 4, 4), (p + 4, 3), DigRefusal::Undiggable, "the east ramp"),
        ((2, 0), (1, 0), DigRefusal::Undiggable, "the tree"),
        ((2, -21), (1, -21), DigRefusal::Blocked, "the site"),
        ((pile.0 + 1, pile.1), pile, DigRefusal::Pile, "the pile"),
    ];
    for (shaft, target, refusal, name) in cases {
        let refused = shaft_refusals(&mut sim, shaft, target);
        assert_eq!(refused.len(), 16, "{name}: 16 floors down the shaft");
        assert!(refused.iter().all(|r| *r == Err(refusal)), "{name}: {refused:?}");
        // The shaft's other side digs, off the plateau.
        let other = (2 * shaft.0 - target.0, 2 * shaft.1 - target.1);
        let s = tile(&sim, shaft.0, shaft.1);
        if sim.world().terrain().is_undiggable(tile(&sim, other.0, other.1)) {
            continue;
        }
        sim.world_mut().settlement_mut().agents[ELDER as usize].body.heading = facing(other.0 - shaft.0, other.1 - shaft.1);
        let ok = settlement(&sim).dig_volume_of(ELDER, sim.world().tick(), sim.world().terrain(), DigDirection::Level).unwrap();
        assert_eq!(ok.map(|(t, _)| t), Ok(tile(&sim, other.0, other.1)), "{name}: the far side of the shaft at {s:?} digs");
    }
}

/// The elder digs a stair down from the north foot (Ctrl+F, a step down
/// each), walking down it a tile at a time, to 8 m under the surface; the
/// next step down is refused at the bedrock. Each dig of stone or amber
/// yields its item.
#[test]
fn the_elder_digs_a_stair_down_8_m_but_no_deeper() {
    let mut sim = town();
    let r = me(&sim).body.base_radius;
    let (x0, y) = (-8, -22);
    let surface = sim.world().terrain().tile_height(tile(&sim, x0, y)).unwrap();
    place(&mut sim, x0 as f64, y as f64, surface);
    for step in 1..=17 {
        sim.world_mut().settlement_mut().agents[ELDER as usize].body.heading = facing(1, 0);
        let down = || CommandKind::DigFront { agent: ELDER, direction: DigDirection::Down };
        let would = settlement(&sim).dig_volume_of(ELDER, sim.world().tick(), sim.world().terrain(), DigDirection::Down).unwrap();
        if step == 17 {
            assert_eq!(would, Err(DigRefusal::Bottom), "no deeper than 8 m");
            break;
        }
        assert!(would.is_ok(), "step {step}: {would:?}");
        command(&mut sim, down());
        while me(&sim).avatar().unwrap().dig.is_some() {
            sim.step(&[]);
        }
        settlement(&sim).validate().unwrap();
        // Walk onto the new step: down a layer.
        let (from, z) = (me(&sim).body.position, feet(&sim));
        let to = Terrain::pit_center(tile(&sim, x0 + step, y));
        let landed = sim.world().terrain().walk_at(from, to, z, r);
        assert_eq!(landed, Some(surface - LAYER.mul_int(step as i64)), "step {step} walks down");
        place(&mut sim, (x0 + step) as f64, y as f64, landed.unwrap());
    }
    assert_eq!(feet(&sim), surface - Fx::from_int(8), "8 m down");
    let s = settlement(&sim);
    let dug: u64 = [crate::mine::STONE_ID, crate::mine::AMBER_ID].iter().map(|id| s.accounts[s.item_index(id).unwrap() as usize].generated).sum();
    eprintln!("a stair 8 m down yielded {dug} stone and amber");
}

/// Haulers and sprites start on the top floor where they stand (the
/// plateau's), not the floor nearest 0 m.
#[test]
fn bodies_start_on_the_top_floor() {
    let sim = town();
    let terrain = sim.world().terrain();
    let s = settlement(&sim);
    let mut bodies = 0;
    for a in &s.agents {
        if a.sprite().is_some() || a.hauler().is_some() {
            assert_eq!(a.height(), terrain.ground(a.body.position).unwrap());
            bodies += 1;
        }
    }
    assert!(bodies >= 4);
    assert_eq!(super::follow::start_height(terrain, Terrain::pit_center(tile(&sim, -6, -56)), Fx::from_ratio(1, 4)), height(16), "on the mountain, its top");
}
