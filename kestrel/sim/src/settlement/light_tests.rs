//! Lanterns and the light grid (M8.6 of the Sacred Tree depths, L1, L2,
//! L6): the grid stops at rock, at the lantern's range and at its level; it
//! is recomputed when the ground or the lanterns change; a lantern's
//! blueprint goes down on a tunnel's floor (and not where there is none),
//! miners bring its stone and amber down and build it; and a save gives the
//! same grid back.

use content::TICKS_PER_SECOND;
use fx::{Fx, FxVec2};

use super::dig_tests::{command, step};
use super::light::{self, Lamp, LightGrid, FULL};
use super::miner_tests::{map, mine, settlement, story, strip_ore, tile, tunnel};
use super::{AgentIx, BuildingIx, BuildingKindIx, PlacementRefusal, Stage};
use crate::terrain::Terrain;
use crate::volume::{height, Volume, HEADROOM};
use crate::{CommandKind, Simulation, World};

const HZ: u64 = TICKS_PER_SECOND as u64;
const MOSS: AgentIx = 3;
const REED: AgentIx = 4;
const WOOD_GATHERER: BuildingKindIx = 0;

/// A room 25 tiles square round tile (0, 0): ground up to layer 0 (its
/// floor), a roof from `HEADROOM` up to 8, and open sky over that.
fn room() -> Volume {
    let mut volume = Volume::default();
    volume.fill_box((-12, -12), (12, 12), -8, 0);
    volume.fill_box((-12, -12), (12, 12), HEADROOM, 8);
    volume
}

fn lamp(tile: (i32, i32), floor: i32) -> Lamp {
    Lamp { tile, floor, range: Fx::from_int(6) }
}

fn level(cells: &std::collections::BTreeMap<((i32, i32), i32), u8>, tile: (i32, i32), layer: i32) -> u8 {
    cells.get(&(tile, layer)).copied().unwrap_or(0)
}

#[test]
fn the_grid_stops_at_its_range_and_falls_off_softly() {
    let cells = light::compute(&room(), &[lamp((0, 0), 0)]);
    assert_eq!(level(&cells, (0, 0), 0), FULL, "full at the lantern");
    // 1 - (d / 6)²: 3 m out three quarters, 5 m out under a third.
    assert_eq!(level(&cells, (3, 0), 0), 191);
    assert_eq!(level(&cells, (0, -5), 0), 77);
    assert!(level(&cells, (4, 4), 0) > 0, "5.66 m out, inside the range");
    assert_eq!(level(&cells, (6, 0), 0), 0, "6 m out, the range");
    assert_eq!(level(&cells, (5, 4), 0), 0, "6.4 m out");
    let steps: Vec<u8> = (0..6).map(|x| level(&cells, (x, 0), 0)).collect();
    assert!(steps.windows(2).all(|w| w[0] > w[1]), "it falls off with distance: {steps:?}");
    assert!(cells.keys().all(|&(t, layer)| layer == 0 && t.0.abs() < 6 && t.1.abs() < 6), "nothing else is lit");
    // Symmetric, and deterministic: the same lamps give the same grid.
    assert_eq!(level(&cells, (-3, 0), 0), 191);
    assert_eq!(level(&cells, (0, 3), 0), 191);
    assert_eq!(cells, light::compute(&room(), &[lamp((0, 0), 0)]));
}

#[test]
fn the_grid_stops_at_rock() {
    let mut volume = room();
    // A wall three tiles wide, two tiles east of the lantern, floor to roof.
    volume.fill_box((2, -1), (2, 1), 0, HEADROOM);
    let cells = light::compute(&volume, &[lamp((0, 0), 0)]);
    assert!(level(&cells, (1, 0), 0) > 0, "in front of the wall");
    for x in 2..6 {
        assert_eq!(level(&cells, (x, 0), 0), 0, "the wall and behind it, x {x}");
    }
    assert_eq!(level(&cells, (4, 0), 0), 0);
    assert!(level(&cells, (3, 3), 0) > 0, "round the wall's end, in sight");
    // A pillar a tile high only (to layer 2) doesn't stop it: the light
    // goes over at the head's height, 1 to 2 m up.
    let mut low = room();
    low.fill_box((2, 0), (2, 0), 0, 2);
    let cells = light::compute(&low, &[lamp((0, 0), 0)]);
    assert!(level(&cells, (4, 0), 0) > 0, "over a low pillar");
    // A one-tile tunnel lights along itself only.
    let mut rock = Volume::default();
    rock.fill_box((-12, -12), (12, 12), -8, 12);
    rock.clear_box((-8, 0), (8, 0), 0, HEADROOM);
    let cells = light::compute(&rock, &[lamp((0, 0), 0)]);
    assert!((-5..=5).all(|x| level(&cells, (x, 0), 0) > 0), "the tunnel, both ways");
    assert!(cells.keys().all(|&(t, _)| t.1 == 0), "never into the rock beside it");
}

#[test]
fn the_grid_stays_on_its_level() {
    // Two rooms over one another: a lower floor at layer 0 under a roof
    // from 4 to 8, an upper floor at layer 8 under the sky.
    let volume = room();
    let below = light::compute(&volume, &[lamp((0, 0), 0)]);
    assert!(below.keys().all(|&(_, layer)| layer == 0), "a lantern below lights its floor only");
    assert_eq!(level(&below, (0, 0), 8), 0, "not the floor over it");
    let above = light::compute(&volume, &[lamp((0, 0), 8)]);
    assert!(above.keys().all(|&(_, layer)| layer == 8), "a lantern above lights its floor only");
    assert_eq!(level(&above, (0, 0), 0), 0, "not the floor under it");
    assert_eq!(level(&above, (3, 0), 8), 191);
    // A step of a layer up or down is the same level: the light climbs a
    // ramp, onto the tile's own floor.
    let mut ramp = Volume::default();
    ramp.fill_box((-12, -12), (12, 12), -8, 0);
    ramp.fill_box((-12, -12), (12, 12), 6, 8);
    ramp.fill_box((2, -12), (12, 12), 0, 1);
    let cells = light::compute(&ramp, &[lamp((0, 0), 0)]);
    assert_eq!(level(&cells, (3, 0), 1), 191, "a layer up");
    assert_eq!(level(&cells, (3, 0), 0), 0);
}

#[test]
fn ground_under_open_sky_counts_as_lit() {
    let mut terrain = Terrain::flat(FxVec2::ZERO, Fx::from_int(12));
    let grid = LightGrid::default();
    assert_eq!(grid.at(&terrain, (0, 0), 0), Fx::ONE, "no volume: all surface");
    assert!(grid.lit(&terrain, FxVec2::ZERO, Fx::ZERO));
    terrain.volume = Some(room());
    assert_eq!(grid.at(&terrain, (0, 0), 0), Fx::ZERO, "roofed and unlit");
    assert_eq!(grid.at(&terrain, (0, 0), 8), Fx::ONE, "the roof's top is under the sky");
    assert!(!grid.lit(&terrain, FxVec2::ZERO, Fx::ZERO));
    assert!(grid.lit(&terrain, FxVec2::ZERO, height(8)));
}

/// The lantern kind's index.
fn lantern(sim: &Simulation) -> BuildingKindIx {
    settlement(sim).building_kinds.iter().position(|k| k.is_lantern()).expect("the town has lanterns") as BuildingKindIx
}

/// Puts the elder in the middle of `on`, its feet at `feet`, standing.
fn put_elder(sim: &mut Simulation, on: (i32, i32), feet: Fx) {
    let s = sim.world_mut().settlement_mut();
    let player = s.player().unwrap() as usize;
    let a = &mut s.agents[player];
    a.body.position = Terrain::pit_center(on);
    a.body.velocity = FxVec2::ZERO;
    a.body.route.clear();
    let avatar = a.avatar_mut().unwrap();
    avatar.height = feet;
    avatar.flight.support = feet;
}

/// Puts `units` of the town's stone and amber in the tree's piles.
fn stock(sim: &mut Simulation, units: u32) {
    let s = settlement(sim);
    let (stone, amber) = (s.stone_depot().unwrap(), s.amber_depot().unwrap());
    let turn: Vec<_> = [stone, amber].into_iter().map(|(item, depot)| command(CommandKind::AddStock { depot, item, units })).collect();
    step(sim, &turn);
}

/// Runs until building `b` is built, at most `seconds`.
fn until_built(sim: &mut Simulation, b: BuildingIx, seconds: u64) {
    for _ in 0..seconds * HZ {
        step(sim, &[]);
        if settlement(sim).buildings[b as usize].built() {
            return;
        }
    }
    panic!("never built: {:?}, moss {}, reed {}", settlement(sim).buildings[b as usize].stage, story(sim, MOSS), story(sim, REED));
}

/// In the town's tunnel (`miner_tests::tunnel`, at the hill's north foot):
/// the elder, on the tunnel's floor, puts a lantern's blueprint down two
/// tiles back from its end; it stands on that floor, not the hill's top
/// over it. A spot in the rock beside it, or over the trench's drop from
/// the ground above, has no floor; another kind still refuses the hollow
/// column. The two miners bring the tree's stone and amber down to it and
/// build it; it lights its stretch of tunnel and not the ground over it,
/// and lights a wall dug out within its range on the next tick. A save
/// gives the same grid back, and the world steps on the same.
#[test]
fn a_lantern_blueprint_underground_gets_its_stone_and_amber_and_is_built() {
    let mut sim = map();
    let (wall, stand, floor) = tunnel(&mut sim);
    // No ore round the tunnel, so the miners dig elsewhere: the light there
    // changes only when the test digs.
    strip_ore(&mut sim, |t| (t.0 - 16).abs() <= 10 && (t.1 + 26).abs() <= 10);
    let at = tile(&sim, 16, -26);
    let kind = lantern(&sim);
    put_elder(&mut sim, stand, height(floor));
    step(&mut sim, &[]);
    let feet = Some(height(floor));
    let s = settlement(&sim);
    let terrain = sim.world().terrain();
    let spot = s.placement_check_at(kind, Terrain::pit_center(at), feet, terrain).expect("a lantern goes on the tunnel's floor");
    assert!((spot.height - height(floor)).abs() <= Fx::from_ratio(1, 100), "on the tunnel's floor: {:?}", spot.height);
    let beside = (at.0, at.1 + 1);
    assert_eq!(s.placement_check_at(kind, Terrain::pit_center(beside), feet, terrain), Err(PlacementRefusal::NoFloor), "in the rock beside it");
    let top = terrain.volume.as_ref().unwrap().ground(Terrain::pit_center(beside)).unwrap();
    let trench = tile(&sim, 13, -26);
    assert_eq!(s.placement_check_at(kind, Terrain::pit_center(trench), Some(top), terrain), Err(PlacementRefusal::NoFloor), "over the trench's drop");
    assert_eq!(s.placement_check(WOOD_GATHERER, Terrain::pit_center(at), terrain), Err(PlacementRefusal::Uneven), "other kinds keep off tunnels");

    stock(&mut sim, 1);
    step(&mut sim, &[command(CommandKind::PlaceBlueprint { kind, position: Terrain::pit_center(at) })]);
    let b = (settlement(&sim).buildings.len() - 1) as BuildingIx;
    mine(&mut sim, MOSS);
    mine(&mut sim, REED);
    let site = &settlement(&sim).buildings[b as usize];
    assert!(site.site().is_some() && settlement(&sim).building_kind(b).is_lantern());
    assert!((site.height - height(floor)).abs() <= Fx::from_ratio(1, 100), "the site is on the tunnel's floor");
    assert_eq!(settlement(&sim).light.cells().count(), 0, "a site gives no light");
    until_built(&mut sim, b, 240);
    eprintln!("the lantern built at {} s", sim.world().tick() / HZ);
    let s = settlement(&sim);
    assert_eq!(s.buildings[b as usize].stage, Stage::Built);
    for (item, _) in [s.stone_depot().unwrap(), s.amber_depot().unwrap()] {
        assert_eq!(s.accounts[item as usize].consumed, 1, "its stone and its amber went in");
    }

    // Its light: its stretch of tunnel, on its floor only.
    let terrain = sim.world().terrain();
    let volume = terrain.volume.as_ref().unwrap();
    assert_eq!(s.light.level(at, floor), FULL);
    assert_eq!(s.light_at(terrain, at, floor), Fx::ONE);
    assert!(s.lit(terrain, Terrain::pit_center(stand), height(floor)), "the tunnel's end, 2 m on");
    assert!(s.light.level(stand, floor) > 0);
    assert_eq!(s.light.level(wall, floor), 0, "not the rock at its end");
    let over = volume.top(at).unwrap();
    assert_eq!(s.light.level(at, over), 0, "not the ground over it");
    assert!(s.light.cells().all(|((t, layer), _)| t.1 == at.1 && (layer - floor).abs() <= 1), "along the tunnel, on its level");
    let revision = s.light.revision;
    assert!(revision > 0);

    // The ground changes: a dig far off changes no light, and the grid's
    // revision stays; the end wall dug out is lit on the next tick.
    {
        let terrain = sim.world_mut().settlement_and_terrain_mut().1;
        terrain.revision += 1;
    }
    step(&mut sim, &[]);
    assert_eq!(settlement(&sim).light.revision, revision, "no light changed");
    {
        let terrain = sim.world_mut().settlement_and_terrain_mut().1;
        terrain.volume.as_mut().unwrap().clear_box(wall, wall, floor, floor + HEADROOM);
        terrain.revision += 1;
    }
    step(&mut sim, &[]);
    assert_eq!(settlement(&sim).light.revision, revision + 1);
    assert!(settlement(&sim).light.level(wall, floor) > 0, "the dug wall is lit");

    // A save round trip: the grid is recomputed on load, the same, and the
    // world steps on the same.
    let grid = settlement(&sim).light.clone();
    let world = sim.into_world();
    let bytes = postcard::to_allocvec(&world.to_state()).unwrap();
    let rules = world.rules().clone();
    let loaded = World::from_state(postcard::from_bytes(&bytes).unwrap(), rules).unwrap();
    assert_eq!(loaded.settlement().unwrap().light, grid, "the same grid after a load");
    assert!(loaded.settlement().unwrap().light.cells().count() > 0);
    let mut a = Simulation::from_world(world, 1);
    let mut b = Simulation::from_world(loaded, 2);
    for _ in 0..2 * HZ {
        assert_eq!(a.step(&[]).total, b.step(&[]).total, "a loaded world steps the same");
    }
}

/// A lantern on the hill's top, beside the tree's plaza: it goes down on
/// the top floor, gives the ground there no extra light in the query (the
/// sky lights it), and ghouls and the elder's interact leave it be: no slot
/// to possess, nothing to break.
#[test]
fn a_lantern_on_the_surface_lights_its_ground_and_has_no_slot() {
    let mut sim = map();
    let kind = lantern(&sim);
    stock(&mut sim, 1);
    let at = tile(&sim, 4, 4);
    step(&mut sim, &[command(CommandKind::PlaceBlueprint { kind, position: Terrain::pit_center(at) })]);
    let b = (settlement(&sim).buildings.len() - 1) as BuildingIx;
    mine(&mut sim, MOSS);
    assert!(settlement(&sim).buildings[b as usize].site().is_some(), "it went down: {:?}", settlement(&sim).events);
    until_built(&mut sim, b, 120);
    let s = settlement(&sim);
    let terrain = sim.world().terrain();
    let top = terrain.volume.as_ref().unwrap().top(at).unwrap();
    assert_eq!(s.light.level(at, top), FULL, "the grid lights the top too");
    assert_eq!(s.light_at(terrain, tile(&sim, 30, 30), top), Fx::ONE, "but the surface is lit anyway");
    assert!(!s.breakable(b), "ghouls leave it be");
    let lantern = &s.buildings[b as usize];
    assert_eq!(lantern.slot_position(s.building_kind(b)), lantern.position, "no slot");
    assert!(!terrain.obstacles.iter().any(|o| o.center == lantern.position), "no obstacle");
}
