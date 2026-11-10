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

// Bounded paths (M8.1 of the depths design, `kestrel/SACRED_TREE_DEPTHS.md`):
// a 3D plan expands at most `nav::PATH_BUDGET` nodes, and a plan that
// can't reach its goal can still go as close as it got.

/// Plans that exist across the Tunnel Lab, the longest it has (round the
/// mountain to the bridge's stairs and up), are found well within the
/// budget.
#[test]
fn paths_across_the_tunnel_lab_fit_in_the_budget() {
    let sim = tunnel_lab();
    let terrain = sim.world().terrain();
    let plans = [
        ((0.0, 8.0, 0), (28.0, -8.0, 0)),
        ((0.0, 0.0, 0), (-11.0, 13.0, 5)),
        ((28.0, -8.0, 0), (-11.0, 13.0, 5)),
        ((-28.0, -28.0, 0), (28.0, 28.0, 0)),
        ((-11.0, 13.0, 5), (-11.0, 13.0, 0)),
    ];
    let mut most = 0;
    for ((sx, sz, sh), (gx, gz, gh)) in plans {
        let (start, goal) = (meters(&sim, sx, sz), meters(&sim, gx, gz));
        let plan = nav::path_toward_at(terrain, start, Fx::from_int(sh), goal, Some(Fx::from_int(gh)), RADIUS, &[]);
        eprintln!("({sx}, {sz}, {sh}) to ({gx}, {gz}, {gh}): {} nodes", plan.expanded);
        assert!(plan.reached, "({sx}, {sz}) to ({gx}, {gz}) at {gh} m");
        assert_eq!(plan.route.last(), Some(&(goal, Fx::from_int(gh))));
        most = most.max(plan.expanded);
        assert_eq!(plan.route, nav::path_at(terrain, start, Fx::from_int(sh), goal, Some(Fx::from_int(gh)), RADIUS, &[]));
    }
    assert!(most < nav::PATH_BUDGET * 3 / 4, "{most} nodes");
}

/// The mountain's top, 8 m up, has no way up (its tiers are 1 m walls):
/// the goal's flood runs out of nodes on the top before the search gets
/// there, so a plan to it from the spawn is settled in a few hundred
/// nodes instead of searching every floor of the map. `path_at` gives
/// nothing; the partial plan goes as close as it gets, under the top on
/// the tunnel's floor, and stops when it comes no nearer.
#[test]
fn a_plan_to_the_mountains_top_is_settled_by_the_goals_flood() {
    let sim = tunnel_lab();
    let terrain = sim.world().terrain();
    let (start, goal) = (meters(&sim, 0.0, 0.0), meters(&sim, 14.0, 0.0));
    let top = Some(Fx::from_int(8));
    assert_eq!(terrain.footprint_near(goal, Fx::from_int(8), RADIUS), Some(Fx::from_int(8)), "a floor up there");
    let full = nav::plan_at(terrain, start, Fx::ZERO, goal, top, RADIUS, &[], nav::PATH_BUDGET, false);
    eprintln!("to the top: {} nodes", full.expanded);
    assert!(full.route.is_empty() && !full.reached);
    assert!(full.expanded <= 2 * nav::GOAL_FLOOD, "settled by the flood: {} nodes", full.expanded);
    assert!(nav::path_at(terrain, start, Fx::ZERO, goal, top, RADIUS, &[]).is_empty());
    let toward = nav::path_toward_at(terrain, start, Fx::ZERO, goal, top, RADIUS, &[]);
    eprintln!("toward the top: {} nodes", toward.expanded);
    assert!(!toward.reached);
    assert!(toward.expanded <= nav::PATH_BUDGET / 4, "{} nodes", toward.expanded);
    let &(end, feet) = toward.route.last().expect("a partial route");
    assert_eq!(walked(terrain, start, Fx::ZERO, &toward.route), Fx::ZERO);
    assert_eq!(feet, Fx::ZERO, "on the tunnel's floor");
    assert!(nav::distance(end, goal) <= Fx::HALF, "under the top: {:?}", end - terrain.center);
    // The same ground gives the same plan, and a clone of it too.
    assert_eq!(toward, nav::path_toward_at(terrain, start, Fx::ZERO, goal, top, RADIUS, &[]));
    assert_eq!(toward, nav::path_toward_at(&terrain.clone(), start, Fx::ZERO, goal, top, RADIUS, &[]));
}

/// A wall across the map, 4 m high, with no way through: the goal's side
/// is too big for its flood, so the search goes on alone and stops at the
/// budget (or when its own side runs out), with no route; a partial one
/// ends at the wall.
#[test]
fn a_plan_past_a_wall_stops_at_the_budget() {
    let sim = small("exosuit", &[(true, (3, -16), (6, 16), 0, 8)], Fx::ZERO);
    let terrain = sim.world().terrain();
    let (start, goal) = (meters(&sim, 0.0, 4.0), meters(&sim, 9.0, -4.0));
    let ground = Some(Fx::ZERO);
    for budget in [500, 2000] {
        let plan = nav::plan_at(terrain, start, Fx::ZERO, goal, ground, RADIUS, &[], budget, false);
        assert!(plan.route.is_empty() && !plan.reached);
        assert_eq!(plan.expanded, budget, "stopped at the budget");
    }
    let whole = nav::plan_at(terrain, start, Fx::ZERO, goal, ground, RADIUS, &[], nav::PATH_BUDGET, false);
    eprintln!("past the wall: {} nodes", whole.expanded);
    assert!(whole.route.is_empty() && whole.expanded <= nav::PATH_BUDGET);
    let toward = nav::plan_at(terrain, start, Fx::ZERO, goal, ground, RADIUS, &[], 2000, true);
    assert!(!toward.reached && toward.expanded == 2000);
    walked(terrain, start, Fx::ZERO, &toward.route);
    let at = toward.route.last().unwrap().0 - terrain.center;
    assert!(at.x >= Fx::from_int(2) && at.x < Fx::from_ratio(5, 2), "by the wall: {at:?}");
}

/// A pit 2 m deep with sheer sides: a plan into it from outside has no
/// way, so the partial plan ends at the pit's edge, on the ground nearest
/// the goal (a walker waits there for a player in the pit). A plan that
/// runs out of budget before a goal it could reach is partial too.
#[test]
fn a_partial_plan_ends_at_the_reachable_point_nearest_the_goal() {
    let sim = small("exosuit", &[(false, (5, -3), (8, 3), -4, 0)], Fx::ZERO);
    let terrain = sim.world().terrain();
    let (start, goal) = (meters(&sim, -2.0, 4.0), meters(&sim, 7.0, 2.0));
    let down = Some(Fx::from_int(-2));
    assert!(nav::path_at(terrain, start, Fx::ZERO, goal, down, RADIUS, &[]).is_empty());
    let plan = nav::path_toward_at(terrain, start, Fx::ZERO, goal, down, RADIUS, &[]);
    assert!(!plan.reached);
    assert!(plan.expanded < nav::PATH_BUDGET, "the window has fewer nodes: {}", plan.expanded);
    assert_eq!(walked(terrain, start, Fx::ZERO, &plan.route), Fx::ZERO, "on the ground");
    // The pit's nearest side to the goal is its south one, 1.5 m off at
    // 3.5 m; the body's radius keeps it a little further.
    let end = plan.route.last().unwrap().0;
    let gap = nav::distance(end, goal);
    assert!(gap >= Fx::from_ratio(3, 2) && gap <= Fx::from_ratio(5, 2), "at the pit's edge by the goal: {:?}", end - terrain.center);
    // Out of budget on the way to a goal it could reach: no route from
    // `path_at`'s search, a partial one toward it.
    let tunnel = tunnel_lab();
    let terrain = tunnel.world().terrain();
    let (start, goal) = (meters(&tunnel, 0.0, 8.0), meters(&tunnel, 28.0, -8.0));
    let short = nav::plan_at(terrain, start, Fx::ZERO, goal, Some(Fx::ZERO), RADIUS, &[], 40, true);
    assert!(!short.reached && short.expanded == 40);
    assert!(!short.route.is_empty());
    walked(terrain, start, Fx::ZERO, &short.route);
    assert!(nav::distance(short.route.last().unwrap().0, goal) < nav::distance(start, goal), "closer to the goal");
    assert!(nav::plan_at(terrain, start, Fx::ZERO, goal, Some(Fx::ZERO), RADIUS, &[], 40, false).route.is_empty());
}

/// The walker can't reach a player on the mountain's top: it walks as
/// close as it gets (under the top, in the tunnel) and plans every
/// `FAILED_REPLAN_TICKS`, not every `REPLAN_TICKS`.
#[test]
fn the_walker_backs_off_when_it_cant_reach_the_player() {
    use super::follow::{FAILED_REPLAN_TICKS, REPLAN_TICKS};
    let mut sim = tunnel_lab_with_walker();
    super::tunnel_lab_tests::place(&mut sim, 14.0, 0.0, Fx::from_int(8));
    let retry = |sim: &Simulation| {
        let s = sim.world().settlement().unwrap();
        s.agents.iter().find(|a| a.id == "walker_0").unwrap().sprite().unwrap().retry_at
    };
    let (mut plans, mut last) = (Vec::new(), retry(&sim));
    for _ in 0..content::millis_to_ticks(20_000) {
        let tick = sim.world().tick();
        sim.step(&[]);
        if retry(&sim) != last {
            last = retry(&sim);
            plans.push((tick, last - tick));
        }
    }
    assert_eq!(player(&sim).1, Fx::from_int(8), "the player stays on the top");
    assert!(plans.len() >= 3 && plans.iter().all(|&(_, wait)| wait == FAILED_REPLAN_TICKS), "every plan failed and backed off: {plans:?}");
    assert!(plans.len() as u64 <= content::millis_to_ticks(20_000) / FAILED_REPLAN_TICKS + 1);
    assert!(FAILED_REPLAN_TICKS > REPLAN_TICKS);
    let (w, z) = walker(&sim);
    assert_eq!(z, Fx::ZERO, "on the tunnel's floor");
    assert!(nav::distance(w, player(&sim).0) <= Fx::ONE, "under the player: {:?}", w - sim.world().terrain().center);
    sim.world().settlement().unwrap().validate().unwrap();
}
