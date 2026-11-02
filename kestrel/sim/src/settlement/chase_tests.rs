//! Enemies chase, then return (`SACRED_TREE_BALLISTA.md`, B5): a Sacred
//! Tree ghoul turns on the nearest elder or sprite it sees within its
//! detect radius, swings at it, and gives up past the leash or out of sight
//! to plan a fresh route to the tree from where it stands. A forest ghoul
//! keeps to its route. Every test checks every invariant on every tick.

use content::character::ChaseDef;
use content::scenario::{Scenario, Wave};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::chase::{in_sight, route_home, stop_distance};
use super::{AgentIx, Event, Goal, Hold, Quarry, Seat, Settlement, SpriteJob, Threat};
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
const ASH: AgentIx = 1;
const CHASER: &str = "ghoul_sacred_tree";
/// The wave south of the tree: one ghoul walking north to the trunk.
const SOUTH: ([f64; 2], [f64; 2]) = ([0.0, -7.5], [0.0, -1.8]);

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

fn point(p: [f64; 2]) -> [Fx; 2] {
    [fx(p[0]), fx(p[1])]
}

/// The scene with one `character` ghoul coming from `from` to `to` after
/// `first` seconds, and extra obstacles (scenario meters) beside the trunk.
fn town(character: &str, (from, to): ([f64; 2], [f64; 2]), first: f64, walls: &[Disc]) -> Simulation {
    let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
    scenario.waves.push(Wave {
        id: "test".into(),
        position: point(from),
        target: point(to),
        character: character.into(),
        first: fx(first),
        every: fx(1.0),
        count: 1,
    });
    let mut obstacles = vec![Disc { center: FxVec2::ZERO, radius: Fx::ONE }];
    obstacles.extend_from_slice(walls);
    let world = World::new_settlement(1, &Content::builtin(), &scenario, &obstacles).unwrap();
    Simulation::from_world(world, 1)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn settlement_mut(sim: &mut Simulation) -> &mut Settlement {
    sim.world_mut().settlement_mut()
}

/// A scenario point in the settlement's coordinates.
fn at(sim: &Simulation, p: [f64; 2]) -> FxVec2 {
    sim.world().terrain().center + FxVec2::new(fx(p[0]), fx(p[1]))
}

fn command(kind: CommandKind) -> Command {
    Command { player: PlayerId(0), kind }
}

fn step(sim: &mut Simulation, turn: &[Command]) {
    sim.step(turn);
    settlement(sim).validate().unwrap();
}

/// Steps until `done`, at most `ticks` ticks.
fn until(sim: &mut Simulation, ticks: u64, mut done: impl FnMut(&Simulation) -> bool) {
    for _ in 0..ticks {
        if done(sim) {
            return;
        }
        step(sim, &[]);
    }
    panic!("not done after {ticks} ticks");
}

/// The tick that ran last.
fn ran(sim: &Simulation) -> u64 {
    sim.world().tick() - 1
}

fn ghoul(sim: &Simulation) -> Option<&Threat> {
    settlement(sim).threats.first()
}

fn goal(sim: &Simulation) -> Option<Goal> {
    ghoul(sim).map(|t| t.goal)
}

fn breaches(sim: &Simulation) -> u64 {
    settlement(sim).combat.breaches
}

fn chase_def(sim: &Simulation) -> ChaseDef {
    let s = settlement(sim);
    s.characters[s.waves[0].character as usize].chase.unwrap()
}

/// Puts sprite `agent` at `p` (scenario meters), at home there, so it
/// stands still.
fn place(sim: &mut Simulation, agent: AgentIx, p: [f64; 2]) {
    let position = at(sim, p);
    let a = &mut settlement_mut(sim).agents[agent as usize];
    a.body.position = position;
    a.body.home = position;
    a.body.route.clear();
    let sprite = a.sprite_mut().unwrap();
    sprite.plan.clear();
    sprite.step = 0;
}

/// The ThreatSwing events of the last tick, by agent: (hit, damage).
fn swings(sim: &Simulation) -> Vec<(AgentIx, bool, u32)> {
    settlement(sim)
        .events
        .iter()
        .filter_map(|e| match *e {
            Event::ThreatSwing { agent, hit, damage, .. } => Some((agent, hit, damage)),
            _ => None,
        })
        .collect()
}

/// A sprite 1.6 m east of the ghoul's way: the ghoul turns on it once it
/// comes within 3 m, on one of its look ticks, leaves its route to walk at
/// it, and swings at it. Sprites have no health yet, so the swing is a hit
/// for no damage.
#[test]
fn a_ghoul_leaves_its_route_to_chase_a_sprite_and_swings_at_it() {
    let mut sim = town(CHASER, SOUTH, 1.0, &[]);
    place(&mut sim, ASH, [1.6, -4.5]);
    let sprite = at(&sim, [1.6, -4.5]);
    until(&mut sim, 3 * HZ, |sim| ghoul(sim).is_some());
    let def = chase_def(&sim);
    let id = ghoul(&sim).unwrap().id;
    // Walking its route, it keeps to the line from the wave to the tree.
    let line = at(&sim, SOUTH.0).x;
    until(&mut sim, 20 * HZ, |sim| {
        let t = ghoul(sim).unwrap();
        if t.goal == Goal::Tree {
            assert_eq!(t.position.x, line, "on its route");
        }
        t.goal != Goal::Tree
    });
    assert_eq!(goal(&sim), Some(Goal::Chase(Quarry::Agent(ASH))));
    // It decided in stage 3 of the tick that ran, one of its look ticks,
    // with the sprite within 3 m and in sight.
    assert_eq!((ran(&sim) + id) % def.look, 0);
    let t = ghoul(&sim).unwrap().clone();
    assert!((t.position - sprite).length() <= def.detect);
    // It steers off its route, straight at the sprite, and swings.
    let mut hit = false;
    until(&mut sim, 10 * HZ, |sim| {
        hit |= swings(sim).contains(&(ASH, true, 0));
        hit
    });
    let t = ghoul(&sim).unwrap();
    assert!(t.position.x - line > fx(0.5), "it left its route: {:?}", t.position);
    let reach = settlement(&sim).characters[t.character as usize].attack.unwrap().reach;
    assert!((t.position - sprite).length() <= reach);
    assert_eq!(goal(&sim), Some(Goal::Chase(Quarry::Agent(ASH))), "it keeps at it");
    assert_eq!(breaches(&sim), 0);
}

/// The same sprite behind a wall is never chased: the ghoul walks on and
/// breaches at the tree.
#[test]
fn a_ghoul_does_not_chase_a_sprite_behind_a_wall() {
    let wall: Vec<Disc> = (0..13)
        .map(|i| Disc { center: FxVec2::new(fx(1.0), fx(-7.5 + 0.5 * i as f64)), radius: fx(0.3) })
        .collect();
    let mut sim = town(CHASER, SOUTH, 1.0, &wall);
    place(&mut sim, ASH, [1.8, -4.5]);
    until(&mut sim, 3 * HZ, |sim| ghoul(sim).is_some());
    let sprite = at(&sim, [1.8, -4.5]);
    let detect = chase_def(&sim).detect;
    let mut near = false;
    until(&mut sim, 30 * HZ, |sim| {
        if let Some(t) = ghoul(sim) {
            assert_eq!(t.goal, Goal::Tree, "never turns on a sprite it can't see");
            near |= (t.position - sprite).length() <= detect;
        }
        breaches(sim) == 1
    });
    assert!(near, "it came within its detect radius");
}

/// A sprite working inside a building, and the elder possessing one, are
/// sheltered (B6): a ghoul passing in sight of the slot never turns on
/// them.
#[test]
fn an_occupant_inside_a_building_is_no_quarry() {
    // Past the Wood Gatherer's slot side, to the tree's west.
    let way = ([-7.5, 2.4], [-2.0, -0.8]);
    for possess in [false, true] {
        let mut sim = town(CHASER, way, 25.0, &[]);
        if possess {
            let s = settlement(&sim);
            let spot = s.buildings[0].slot_position(s.building_kind(0));
            step(&mut sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target: spot, run: true } })]);
            until(&mut sim, 20 * HZ, |sim| {
                let a = &settlement(sim).agents[ELDER as usize];
                a.body.route.is_empty() && a.avatar().unwrap().intent == Some(Intent::Stop)
            });
            step(&mut sim, &[command(CommandKind::Board { agent: ELDER })]);
            until(&mut sim, HZ, |sim| settlement(sim).seat(ELDER as usize) == Some(Seat::Building(0)));
        } else {
            step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: ASH, job: SpriteJob::Staff(0) })]);
            until(&mut sim, 20 * HZ, |sim| settlement(sim).agents[ASH as usize].sprite().unwrap().holds == Some(Hold::Inside(0)));
        }
        until(&mut sim, 30 * HZ, |sim| ghoul(sim).is_some());
        let s = settlement(&sim);
        let occupant = s.agents[if possess { ELDER } else { ASH } as usize].body.position;
        let t = ghoul(&sim).unwrap();
        let def = chase_def(&sim);
        let stop = stop_distance(s.characters[t.character as usize].attack.unwrap().reach);
        // It would see it from where it comes in.
        assert!((t.position - occupant).length() <= def.detect);
        assert!(in_sight(sim.world().terrain(), t.position, occupant, stop));
        until(&mut sim, 30 * HZ, |sim| {
            assert_eq!(goal(sim).unwrap_or(Goal::Tree), Goal::Tree, "no chase (possessing: {possess})");
            breaches(sim) == 1
        });
    }
}

/// The elder walks up to the ghoul's way and is chased; it runs off past
/// the leash, and the ghoul gives up there, plans a fresh route to the tree
/// from where it stands, and breaches. Player combat is off on this map,
/// so its swings at the elder do no harm.
#[test]
fn a_ghoul_gives_up_past_the_leash_and_reaches_the_tree() {
    let mut sim = town(CHASER, SOUTH, 6.0, &[]);
    let near = at(&sim, [1.4, -4.0]);
    step(&mut sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target: near, run: false } })]);
    until(&mut sim, 30 * HZ, |sim| goal(sim).is_some_and(|g| g != Goal::Tree));
    assert_eq!(goal(&sim), Some(Goal::Chase(Quarry::Agent(ELDER))));
    let mut swung = false;
    until(&mut sim, 10 * HZ, |sim| {
        swung |= swings(sim).iter().any(|&(agent, hit, damage)| agent == ELDER && hit && damage == 0);
        swung
    });
    assert_eq!(settlement(&sim).agents[ELDER as usize].body.health, settlement(&sim).agents[ELDER as usize].body.max_health);
    // Off east, past the leash.
    let away = at(&sim, [8.0, -4.0]);
    step(&mut sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target: away, run: true } })]);
    let leash = chase_def(&sim).leash;
    until(&mut sim, 20 * HZ, |sim| goal(sim) == Some(Goal::Tree));
    let s = settlement(&sim);
    let t = ghoul(&sim).unwrap();
    let elder = s.agents[ELDER as usize].body.position;
    assert!((t.position - elder).length() > leash, "gave up only past the leash");
    // Planned in stage 5, after the ghoul moved: from where it stands.
    let target = s.waves[t.wave as usize].target;
    assert_eq!(t.route, route_home(sim.world().terrain(), t.position, target));
    assert_eq!(t.route.last(), Some(&target));
    until(&mut sim, 30 * HZ, |sim| {
        assert_ne!(goal(sim), Some(Goal::Chase(Quarry::Agent(ELDER))), "the elder stays out of reach");
        breaches(sim) == 1
    });
}

/// A sprite that disappears behind a wall, still within the leash, is
/// given up on at once.
#[test]
fn a_ghoul_gives_up_on_a_quarry_out_of_sight() {
    // A short wall east of the ghoul's way, with a pocket behind it.
    let wall: Vec<Disc> = (0..5).map(|i| Disc { center: FxVec2::new(fx(2.2), fx(-5.5 + 0.5 * i as f64)), radius: fx(0.3) }).collect();
    let mut sim = town(CHASER, SOUTH, 1.0, &wall);
    place(&mut sim, ASH, [1.3, -4.5]);
    until(&mut sim, 20 * HZ, |sim| goal(sim) == Some(Goal::Chase(Quarry::Agent(ASH))));
    place(&mut sim, ASH, [3.2, -4.5]);
    let hidden = at(&sim, [3.2, -4.5]);
    step(&mut sim, &[]);
    let t = ghoul(&sim).unwrap();
    assert!((t.position - hidden).length() <= chase_def(&sim).leash);
    assert_eq!(goal(&sim), Some(Goal::Tree));
    until(&mut sim, 30 * HZ, |sim| breaches(sim) == 1);
}

/// The forest's ghoul, with no `chase`, walks past the same sprite on its
/// route and breaches.
#[test]
fn a_ghoul_without_a_chase_keeps_to_its_route() {
    let mut sim = town("ghoul_forest", SOUTH, 1.0, &[]);
    place(&mut sim, ASH, [1.6, -4.5]);
    let line = at(&sim, SOUTH.0).x;
    until(&mut sim, 30 * HZ, |sim| {
        if let Some(t) = ghoul(sim) {
            assert_eq!((t.goal, t.position.x), (Goal::Tree, line));
        }
        assert!(swings(sim).is_empty());
        breaches(sim) == 1
    });
}

/// Two runs of a chase hash the same on every tick.
#[test]
fn a_chase_is_deterministic() {
    let run = || {
        let mut sim = town(CHASER, SOUTH, 1.0, &[]);
        place(&mut sim, ASH, [1.6, -4.5]);
        let mut hashes = Vec::new();
        let mut chased = false;
        for _ in 0..20 * HZ {
            hashes.push(sim.step(&[]).total);
            chased |= goal(&sim).is_some_and(|g| g != Goal::Tree);
        }
        assert!(chased);
        (hashes, settlement(&sim).behaviour_digest())
    };
    assert_eq!(run(), run());
}
