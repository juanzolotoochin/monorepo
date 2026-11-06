//! Ghouls, miners and the dug ground on the Sacred Tree's map (M6.7): a
//! pit dug in a ghoul's way re-routes it, a lane cut by a moat sends its
//! ghouls round by the other ramp, and with both ramps cut they walk as
//! close as they can and wait there. A miner off the hill runs home from
//! a ghoul and goes back to work once it has gone. Every tick checks every
//! invariant.

use content::scenario::{Scenario, Wave};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::avatar::Dig;
use super::chase::Goal;
use super::dig_tests::{command, step};
use super::{AgentIx, Event, Settlement, SpriteJob, Step, Threat};
use crate::nav::distance;
use crate::terrain::Terrain;
use crate::{CommandKind, Simulation};

const SUPPLY: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
const REED: AgentIx = 4;

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

/// The map with no waves but `waves`, each one ghoul from scenario point
/// `from` to beside the trunk on its side, at `first` seconds.
fn map(waves: &[([f64; 2], f64)]) -> Simulation {
    let mut scenario = Scenario::parse(SUPPLY).unwrap();
    scenario.waves.clear();
    for (i, &([x, y], first)) in waves.iter().enumerate() {
        scenario.waves.push(Wave {
            id: format!("lane_{i}"),
            position: [fx(x), fx(y)],
            target: [fx(1.8f64.copysign(x)), fx(-0.6)],
            character: "ghoul_sacred_tree".into(),
            first: fx(first),
            every: fx(90.0),
            count: 1,
            nights: Vec::new(),
        });
    }
    super::hill_tests::sacred_tree_map(scenario)
}

/// The tile `x`, `y` tiles from the tree's.
fn tile(sim: &Simulation, x: i32, y: i32) -> (i32, i32) {
    let tree = Terrain::pit_tile(sim.world().terrain().center);
    (tree.0 + x, tree.1 + y)
}

/// Every leg of `route` from `from` is walkable for a ghoul.
fn walks(terrain: &Terrain, from: FxVec2, route: &[FxVec2]) -> bool {
    let mut a = from;
    route.iter().all(|&b| {
        let ok = a == b || super::chase::walk_clear(terrain, a, b);
        a = b;
        ok
    })
}

/// Whether a walk from `from` along `route` comes within `reach` of the
/// middle of any of `tiles`.
fn passes(from: FxVec2, route: &[FxVec2], tiles: &[(i32, i32)], reach: Fx) -> bool {
    let mut a = from;
    route.iter().any(|&b| {
        let near = tiles.iter().any(|&t| {
            let c = Terrain::pit_center(t);
            distance(a, c) <= reach || crate::terrain::segment_disc(a, b, c, reach).is_some()
        });
        a = b;
        near
    })
}

/// A moat two layers deep and two tiles wide round the foot of the ramp on
/// side `side` (1 east, -1 west), from cliff to cliff: across the ramp's
/// foot and along both its sides up to the plateau.
fn moat(sim: &Simulation, side: i32) -> Vec<(i32, i32)> {
    let hill = sim.world().terrain().mine.unwrap().hill.unwrap();
    let (p, w, l) = (hill.plateau as i32, (hill.ramp_width / 2) as i32, hill.ramp_length as i32);
    let mut tiles = Vec::new();
    for out in p + 1..=p + l + 2 {
        for y in -w - 2..=w + 2 {
            let across = out > p + l;
            let along = y.abs() > w;
            if across || along {
                tiles.push(tile(sim, side * out, y));
            }
        }
    }
    tiles
}

/// Takes two layers out of each of `tiles`, straight in the ground.
fn dig_out(sim: &mut Simulation, tiles: &[(i32, i32)]) {
    let terrain = sim.world_mut().settlement_and_terrain_mut().1;
    for &t in tiles {
        assert!(!terrain.is_undiggable(t), "{t:?} can be dug");
        assert!(terrain.deepen(t) && terrain.deepen(t));
    }
}

/// Steps until the first ghoul is out.
fn until_spawned(sim: &mut Simulation) {
    while settlement(sim).threats.is_empty() {
        step(sim, &[]);
    }
}

/// A pit dug in a ghoul's way after it planned its route turns it round
/// the pit: the dig replans the routes that pass near it, and the ghoul
/// still reaches the tree. One that walked its old route would cross the
/// pit's steep walls.
#[test]
fn a_pit_dug_in_a_ghouls_way_turns_it_round_the_pit() {
    let mut sim = map(&[([40.0, 0.0], 1.0)]);
    until_spawned(&mut sim);
    let ghoul = settlement(&sim).threats[0].clone();
    // A 3 x 3 pit two layers deep on its route, at the foot of the ramp.
    let pit: Vec<(i32, i32)> = (27..=29).flat_map(|x| (-1..=1).map(move |y| (x, y))).map(|(x, y)| tile(&sim, x, y)).collect();
    assert!(passes(ghoul.position, &ghoul.route, &pit, Fx::HALF), "the route crosses the pit's ground: {:?}", ghoul.route);
    // All but the last layer straight in the ground; the elder digs that.
    let (last, rest) = pit.split_last().unwrap();
    dig_out(&mut sim, rest);
    sim.world_mut().settlement_and_terrain_mut().1.deepen(*last);
    let tick = sim.world().tick();
    sim.world_mut().settlement_mut().agents[ELDER as usize].avatar_mut().unwrap().dig = Some(Dig { tile: *last, started: tick, until: tick + 1, layer: None });
    step(&mut sim, &[]);
    let terrain = sim.world().terrain();
    assert_eq!(terrain.pit(*last), 2, "the elder's dig went in");
    assert!(!walks(terrain, ghoul.position, &ghoul.route), "the old route crosses the pit");
    let t = &settlement(&sim).threats[0];
    assert!(walks(terrain, t.position, &t.route), "the new one walks round it: {:?}", t.route);
    assert!(!t.stranded);
    let mut breached = false;
    for _ in 0..120 * HZ {
        step(&mut sim, &[]);
        breached |= settlement(&sim).events.iter().any(|e| matches!(e, Event::Breach { .. }));
        if breached {
            break;
        }
    }
    assert!(breached, "it reaches the tree");
}

/// A moat round the east ramp's foot cuts that lane: the east ghoul goes
/// round the hill and up the west ramp, and still breaches.
#[test]
fn a_moat_round_one_ramp_sends_its_ghouls_round_by_the_other() {
    let mut sim = map(&[([40.0, 0.0], 1.0)]);
    let east = moat(&sim, 1);
    dig_out(&mut sim, &east);
    until_spawned(&mut sim);
    let s = settlement(&sim);
    let t = &s.threats[0];
    assert!(!t.stranded, "the west ramp is still open");
    let center = sim.world().terrain().center;
    let hill = sim.world().terrain().mine.unwrap().hill.unwrap();
    let west_edge = center.x - Fx::from_int(hill.plateau as i32);
    assert!(t.route.iter().any(|p| p.x < west_edge), "it comes up the west ramp: {:?}", t.route);
    let mut breached = false;
    for _ in 0..240 * HZ {
        step(&mut sim, &[]);
        breached |= settlement(&sim).events.iter().any(|e| matches!(e, Event::Breach { .. }));
        if breached {
            break;
        }
    }
    assert!(breached, "it reaches the tree the long way round");
}

/// A night's lane keeps sending, so a moat dug by day round the east ramp
/// after its first column came sends the next night's ghoul round by the
/// west ramp: the dig drops the lane's planned route as it does a timed
/// wave's still sending. On a short day (20 s days, 9 s nights) with only
/// the east lane, one ghoul a night.
#[test]
fn a_moat_dug_by_day_sends_the_next_nights_column_round_by_the_other_ramp() {
    let mut scenario = Scenario::parse(SUPPLY).unwrap();
    scenario.day_cycle = Some(content::scenario::DayCycle { day: Fx::from_int(20), night: Fx::from_int(9) });
    scenario.waves.truncate(1);
    scenario.waves[0].nights = vec![1];
    let mut sim = super::hill_tests::sacred_tree_map(scenario);
    until_spawned(&mut sim);
    let center = sim.world().terrain().center;
    let hill = sim.world().terrain().mine.unwrap().hill.unwrap();
    let west_edge = center.x - Fx::from_int(hill.plateau as i32);
    let first = settlement(&sim).threats[0].clone();
    assert!(first.route.iter().all(|p| p.x > center.x), "the first comes up the east ramp: {:?}", first.route);
    // The next day, after the sunrise burned it away.
    let clock = settlement(&sim).clock.unwrap();
    while sim.world().tick() < clock.cycle() + 5 * HZ {
        step(&mut sim, &[]);
    }
    assert!(settlement(&sim).threats.is_empty(), "burned away at dawn");
    let east = moat(&sim, 1);
    dig_out(&mut sim, &east);
    {
        let (s, terrain) = sim.world_mut().settlement_and_terrain_mut();
        let terrain = terrain.clone();
        s.reroute_round(&east, &terrain);
    }
    assert!(settlement(&sim).waves[0].route.is_none(), "the lane plans again");
    until_spawned(&mut sim);
    let t = &settlement(&sim).threats[0];
    assert_eq!(clock.night(sim.world().tick()), 2);
    assert!(!t.stranded && t.route.iter().any(|p| p.x < west_edge), "the second comes up the west ramp: {:?}", t.route);
}

/// With both ramps cut, no ghoul reaches the tree: one already on its way
/// and one sent afterwards each walk to the reachable point nearest the
/// tree and wait there, stranded, without breaching. Digging both moats
/// takes the elder many minutes; whether it should make the tree safe is a
/// playtest question (todo).
#[test]
fn with_both_ramps_cut_ghouls_wait_as_close_as_they_can() {
    let mut sim = map(&[([40.0, 0.0], 1.0), ([-40.0, 0.0], 20.0)]);
    until_spawned(&mut sim);
    let mut both = moat(&sim, 1);
    both.extend(moat(&sim, -1));
    dig_out(&mut sim, &both);
    {
        let (s, terrain) = sim.world_mut().settlement_and_terrain_mut();
        let terrain = terrain.clone();
        s.reroute_round(&both, &terrain);
    }
    assert!(settlement(&sim).threats[0].stranded, "the ghoul on its way is cut off");
    let mut breaches = 0;
    for _ in 0..150 * HZ {
        step(&mut sim, &[]);
        breaches += settlement(&sim).events.iter().filter(|e| matches!(e, Event::Breach { .. })).count();
    }
    assert_eq!(breaches, 0, "nobody reaches the tree");
    let s = settlement(&sim);
    assert_eq!(s.threats.len(), 2, "both wait");
    let terrain = sim.world().terrain();
    for t in &s.threats {
        assert!(t.stranded && t.goal == Goal::Tree, "{t:?}");
        assert!(t.route.len() == 1 && distance(t.route[0], t.position) < Fx::ONE, "it waits where it stands: {:?} at {:?}", t.route, t.position);
        let gap = distance(terrain.center, t.position);
        assert!(gap < Fx::from_int(25), "it waits near the hill, {gap:?} from the tree");
        eprintln!("stranded ghoul {} waits {gap:?} m from the tree", t.id);
    }
}

/// A miner off the hill runs home when a ghoul comes within its detect
/// radius, dropping its dig, keeps clear of it, and goes back to mining
/// once the ghoul is gone.
#[test]
fn a_miner_flees_a_ghoul_home_and_goes_back_to_work() {
    let mut sim = map(&[]);
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: REED, job: SpriteJob::Mine })]);
    let reed = |sim: &Simulation| settlement(sim).agents[REED as usize].clone();
    while reed(&sim).sprite().unwrap().dig.is_none() {
        step(&mut sim, &[]);
        assert!(sim.world().tick() < 120 * HZ, "Reed digs");
    }
    let terrain = sim.world().terrain();
    let hill = terrain.mine.unwrap().hill.unwrap();
    let at = reed(&sim).body.position;
    assert!(!hill.on_plateau(Terrain::pit_tile(at)), "it digs off the hill");
    // A ghoul 2.5 m from Reed, on the side away from the tree, walking to
    // the tree.
    let away = (at - terrain.center).normalize_or_zero();
    let position = at + away * fx(2.5);
    let route = super::chase::route_home(terrain, position, terrain.center + FxVec2::new(fx(1.8), fx(-0.6))).0;
    assert!(!route.is_empty());
    let character = Content::builtin().names.character_index("ghoul_sacred_tree").unwrap();
    let s = sim.world_mut().settlement_mut();
    let id = s.next_id;
    s.next_id += 1;
    s.threats.push(Threat {
        id,
        wave: 0,
        character,
        position,
        heading: Fx::ZERO,
        travelled: Fx::ZERO,
        hp: 2,
        armor: 0,
        pierced: false,
        shocked_until: 0,
        route,
        stagger_until: 0,
        knockback: FxVec2::ZERO,
        windup: None,
        ready_at: 0,
        burn: None,
        goal: Goal::Tree,
        stranded: false,
        height: Default::default(),
        chase: Vec::new(),
        backoff: Default::default(),
    });
    for _ in 0..2 * HZ {
        step(&mut sim, &[]);
    }
    let a = reed(&sim);
    let sprite = a.sprite().unwrap();
    assert_eq!(sprite.plan, vec![Step::GoTo(a.body.home), Step::Wait], "Reed flees home");
    assert!(sprite.dig.is_none() && sprite.dig_claim.is_none(), "its dig is dropped");
    for _ in 0..40 * HZ {
        step(&mut sim, &[]);
        assert!(reed(&sim).body.alive(), "Reed outruns the ghoul");
        if hill.on_plateau(Terrain::pit_tile(reed(&sim).body.position)) {
            break;
        }
    }
    assert!(hill.on_plateau(Terrain::pit_tile(reed(&sim).body.position)), "Reed is up on the hill");
    sim.world_mut().settlement_mut().threats.clear();
    let mut back = false;
    for _ in 0..60 * HZ {
        step(&mut sim, &[]);
        back |= reed(&sim).sprite().unwrap().dig.is_some();
        if back {
            break;
        }
    }
    assert!(back, "Reed goes back to digging");
}
