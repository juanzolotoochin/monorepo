//! Paths in 3D and the test walker (V.7; V8 of the verticality design,
//! `kestrel/VERTICALITY.md`): `nav::path_at` on the Tunnel Lab's floors,
//! and the walker that follows the player through them.

use content::Content;
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::tunnel_lab_tests::{command, layout, meters, small, tunnel_lab, Cells};
use super::{PileSetup, SceneSetup};
use crate::nav;
use crate::terrain::Terrain;
use crate::{CommandKind, Simulation, World};

/// The Tunnel Lab as the app makes it, with its test walker where the
/// scene puts it (`walkers:`).
pub(super) fn tunnel_lab_with_walker() -> Simulation {
    let layout = layout();
    assert!(super::tunnel_lab_tests::SCENE.contains("walkers: [(-2.0, 0.0, 3.0)]"), "the scene's walker");
    let setup = SceneSetup {
        walkers: vec![[Fx::from_int(-2), Fx::from_int(3)]],
        player: "exosuit".into(),
        extent: Fx::from_int(layout.extent),
        diggable: true,
        sloped: true,
        volume: Some(layout.build()),
        piles: Some(PileSetup { capacity: 12, dispensers: Vec::new(), piles: Vec::new(), collector: Some([Fx::from_int(-3), Fx::from_int(-4)]) }),
        ..SceneSetup::default()
    };
    Simulation::from_world(World::new_scene(1, &Content::builtin(), &setup).unwrap(), 1)
}

const RADIUS: Fx = Fx::from_bits(1_288_490_189); // 0.3 m

/// Every 0.25 m along `route` from `start`, with the feet's height of the
/// waypoint it walks to.
fn samples(start: FxVec2, route: &[(FxVec2, Fx)]) -> Vec<(FxVec2, Fx)> {
    let mut out = Vec::new();
    let mut from = start;
    for &(to, z) in route {
        let steps = ((to - from).length() / Fx::from_ratio(1, 4)).ceil().floor_to_int().max(1);
        out.extend((1..=steps).map(|i| (from + (to - from) * Fx::from_ratio(i, steps), z)));
        from = to;
    }
    out
}

/// Walks `route` from `start` with the feet at `z`, step by step as a body
/// does (`Terrain::walk_at`), returning the feet at its end: every segment
/// is walkable.
fn walked(terrain: &Terrain, start: FxVec2, z: Fx, route: &[(FxVec2, Fx)]) -> Fx {
    let (mut from, mut feet) = (start, z);
    for &(to, h) in route {
        feet = terrain.walk_at(from, to, feet, RADIUS).unwrap_or_else(|| panic!("{from:?} to {to:?} isn't walkable"));
        assert!((feet - h).abs() <= crate::terrain::LAYER, "the waypoint's height {h:?} is where the feet are, {feet:?}");
        from = to;
    }
    feet
}

/// From the open ground north-west of the mountain to the south-east of
/// it, the way is the tunnel: in at the west mouth, out at the east, on its
/// floor at 0 m all the way (the mountain's tiers are walls, and round it
/// is further).
#[test]
fn a_path_runs_through_the_tunnel() {
    let sim = tunnel_lab();
    let terrain = sim.world().terrain();
    let (start, goal) = (meters(&sim, 0.0, 8.0), meters(&sim, 28.0, -8.0));
    let route = nav::path_at(terrain, start, Fx::ZERO, goal, Some(Fx::ZERO), RADIUS, &[]);
    assert_eq!(route.last().map(|&(p, _)| p), Some(goal));
    assert_eq!(walked(terrain, start, Fx::ZERO, &route), Fx::ZERO);
    let center = terrain.center;
    let mut inside = 0;
    for (p, z) in samples(start, &route) {
        assert_eq!(z, Fx::ZERO, "on the ground or the tunnel's floor at {:?}", p - center);
        let at = p - center;
        if at.x > Fx::from_int(3) && at.x < Fx::from_int(25) {
            assert!(at.y.abs() <= Fx::from_ratio(3, 2), "in the tunnel at {at:?}");
            inside += 1;
        }
    }
    assert!(inside > 80, "through the tunnel: {inside} samples");
}

/// From the cave under the bridge to the same spot on top of it, (-11, 13)
/// 5 m up: out of the cave, round to the stairs on the bridge's west side,
/// up them and back over the cave. And back down.
#[test]
fn a_path_climbs_from_under_the_bridge_onto_it() {
    let sim = tunnel_lab();
    let terrain = sim.world().terrain();
    let spot = meters(&sim, -11.0, 13.0);
    let up = nav::path_at(terrain, spot, Fx::ZERO, spot, Some(Fx::from_int(5)), RADIUS, &[]);
    assert_eq!(up.last(), Some(&(spot, Fx::from_int(5))), "on the bridge over the cave");
    assert_eq!(walked(terrain, spot, Fx::ZERO, &up), Fx::from_int(5));
    let center = terrain.center;
    let stairs = samples(spot, &up).iter().filter(|&&(p, _)| (p - center).x < Fx::from_int(-19)).count();
    assert!(stairs > 0, "by the stairs");
    let down = nav::path_at(terrain, spot, Fx::from_int(5), spot, Some(Fx::ZERO), RADIUS, &[]);
    assert_eq!(down.last(), Some(&(spot, Fx::ZERO)), "back in the cave");
    assert_eq!(walked(terrain, spot, Fx::from_int(5), &down), Fx::ZERO);
    // With no floor wanted, the nearest: already there.
    assert_eq!(nav::path_at(terrain, spot, Fx::ZERO, spot, None, RADIUS, &[]), vec![(spot, Fx::ZERO)]);
}

/// A wall across the map with a passage 1.5 m high has no way through; at
/// 2 m it has.
#[test]
fn no_path_through_a_passage_without_headroom() {
    for (top, through) in [(3, false), (4, true)] {
        let wall: &[Cells] = &[(true, (3, -16), (6, 16), 0, 8), (false, (3, -1), (6, 1), 0, top)];
        let sim = small("exosuit", wall, Fx::ZERO);
        let terrain = sim.world().terrain();
        let (start, goal) = (meters(&sim, 0.0, 4.0), meters(&sim, 9.0, -4.0));
        let route = nav::path_at(terrain, start, Fx::ZERO, goal, Some(Fx::ZERO), RADIUS, &[]);
        assert_eq!(!route.is_empty(), through, "a passage {top} layers high");
        if through {
            walked(terrain, start, Fx::ZERO, &route);
        }
    }
}

/// A stair dug a layer a tile down into a pit 2 m deep is a way down; the
/// pit's sheer sides are not, so without it there is no path.
#[test]
fn a_path_goes_down_a_dug_stair() {
    let pit: Cells = (false, (5, -3), (8, 3), -4, 0);
    let stair: [Cells; 3] = [(false, (2, -1), (2, 1), -1, 0), (false, (3, -1), (3, 1), -2, 0), (false, (4, -1), (4, 1), -3, 0)];
    for dug in [true, false] {
        let mut boxes = vec![pit];
        if dug {
            boxes.extend(stair);
        }
        let sim = small("exosuit", &boxes, Fx::ZERO);
        let terrain = sim.world().terrain();
        let (start, goal) = (meters(&sim, -2.0, 4.0), meters(&sim, 7.0, 2.0));
        let route = nav::path_at(terrain, start, Fx::ZERO, goal, None, RADIUS, &[]);
        if dug {
            assert_eq!(route.last(), Some(&(goal, Fx::from_int(-2))), "down in the pit");
            assert_eq!(walked(terrain, start, Fx::ZERO, &route), Fx::from_int(-2));
            // Up again.
            let back = nav::path_at(terrain, goal, Fx::from_int(-2), start, Some(Fx::ZERO), RADIUS, &[]);
            assert_eq!(walked(terrain, goal, Fx::from_int(-2), &back), Fx::ZERO);
        } else {
            assert!(route.is_empty(), "no way down a sheer 2 m: {route:?}");
        }
    }
}

/// The same ground gives the same path, and a clone of it too.
#[test]
fn paths_in_3d_are_deterministic() {
    let sim = tunnel_lab();
    let terrain = sim.world().terrain();
    let (start, goal) = (meters(&sim, 0.0, 8.0), meters(&sim, -16.0, 13.0));
    let route = nav::path_at(terrain, start, Fx::ZERO, goal, None, RADIUS, &[]);
    assert_eq!(route.last(), Some(&(goal, Fx::from_int(5))), "the only floor there is the bridge's");
    assert_eq!(route, nav::path_at(terrain, start, Fx::ZERO, goal, None, RADIUS, &[]));
    assert_eq!(route, nav::path_at(&terrain.clone(), start, Fx::ZERO, goal, None, RADIUS, &[]));
}

fn walker(sim: &Simulation) -> (FxVec2, Fx) {
    let s = sim.world().settlement().unwrap();
    let a = s.agents.iter().find(|a| a.id == "walker_0").unwrap();
    (a.body.position, a.sprite().unwrap().height)
}

fn player(sim: &Simulation) -> (FxVec2, Fx) {
    let s = sim.world().settlement().unwrap();
    let a = &s.agents[s.player().unwrap() as usize];
    (a.body.position, a.avatar().unwrap().height)
}

/// Sends the player to scene meters `x`, `z` (its route a 3D path, V.7)
/// and runs until the walker is beside it on its floor, within `seconds`.
/// Returns the walker's feet every tick.
fn lead(sim: &mut Simulation, x: f64, z: f64, seconds: u64) -> Vec<Fx> {
    let agent = sim.world().settlement().unwrap().player().unwrap();
    let target = meters(sim, x, z);
    command(sim, CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target, run: false } });
    let mut trail = Vec::new();
    for _ in 0..content::millis_to_ticks(seconds * 1000) {
        sim.step(&[]);
        let ((w, wz), (p, pz)) = (walker(sim), player(sim));
        trail.push(wz);
        let arrived = nav::distance(p, target) < Fx::from_ratio(1, 5);
        if arrived && nav::distance(w, p) <= Fx::from_ratio(5, 2) && (wz - pz).abs() <= Fx::ONE {
            return trail;
        }
    }
    panic!("the walker didn't follow to ({x}, {z}): it is at {:?}, the player at {:?}", walker(sim), player(sim));
}

/// The walker follows the player from the spawn into the tunnel, then out,
/// round to the bridge's stairs and up onto the bridge over the cave,
/// keeping about 2 m away, its feet on each floor.
#[test]
fn the_walker_follows_the_player_into_the_tunnel_and_onto_the_bridge() {
    let mut sim = tunnel_lab_with_walker();
    assert_eq!(walker(&sim).1, Fx::ZERO, "on the ground by the spawn");
    let center = sim.world().terrain().center;
    let tunnel = lead(&mut sim, 12.0, 0.0, 30);
    assert!(tunnel.iter().all(|&z| z == Fx::ZERO), "on the tunnel's floor");
    assert!(sim.world().terrain().ground(walker(&sim).0).unwrap() > Fx::from_int(3), "under the mountain");
    lead(&mut sim, -16.0, 13.0, 60);
    let bridge = lead(&mut sim, -11.0, 13.0, 30);
    assert_eq!(player(&sim).1, Fx::from_int(5), "the player's GoTo stays on the bridge");
    assert_eq!(walker(&sim).1, Fx::from_int(5), "the walker is on the bridge, by the cave's spot: {:?}", walker(&sim).0 - center);
    assert!(bridge.iter().all(|&z| z > Fx::from_int(4)));
    assert!(nav::distance(walker(&sim).0, player(&sim).0) >= Fx::from_ratio(3, 2), "keeping its distance");
    sim.world().settlement().unwrap().validate().unwrap();
}

/// Two runs of the walker following the player hash alike every tick.
#[test]
fn the_walker_replays_as_the_simulation_does() {
    let run = || {
        let mut sim = tunnel_lab_with_walker();
        let agent = sim.world().settlement().unwrap().player().unwrap();
        let target = meters(&sim, 12.0, 0.0);
        command(&mut sim, CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target, run: false } });
        (0..600).map(|_| sim.step(&[]).total).collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}
