//! The Sacred Tree's waves on the map (M5.6): a calm start, then ghouls on
//! a timer from the clearing's edges. These tests play the map's own
//! waves against a town that ignores them and against one that defends,
//! so the numbers in the supply keep the defence mattering without being
//! needed every second. Every tick checks every invariant.

use content::scenario::{Building, Scenario, Stack, Wave};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::{AgentIx, BuildingIx, BuildingKindIx, DepotIx, Event, ItemIx, Seat, Settlement, SpriteJob};
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
const SPRITES: [AgentIx; 4] = [1, 2, 3, 4];
const STONE: ItemIx = 1;
const TREE_STONE: DepotIx = 1;
const BALLISTA_KIND: BuildingKindIx = 2;
/// When the map's first ghoul comes: the calm start (the supply's first
/// wave).
const CALM: u64 = 210 * HZ;
/// Every lane has sent all its ghouls by then, and the last have arrived.
const LAST: u64 = 720 * HZ;

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

/// The map as it ships, with `ballistas` built at scenario points and
/// plenty of stone in the tree to stock them from.
fn town(ballistas: &[[f64; 2]]) -> Simulation {
    let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
    for &[x, y] in ballistas {
        scenario.buildings.push(Building { kind: "ballista".into(), position: [fx(x), fx(y)] });
    }
    scenario.depots[TREE_STONE as usize].stock.push(Stack { item: "stone".into(), units: 400 });
    let world = World::new_settlement(1, &Content::builtin(), &scenario, &[Disc { center: FxVec2::ZERO, radius: Fx::ONE }]).unwrap();
    Simulation::from_world(world, 1)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn command(kind: CommandKind) -> Command {
    Command { player: PlayerId(0), kind }
}

/// What a run saw.
#[derive(Debug, Default)]
struct Tally {
    breaches: u32,
    kills: u32,
    fire_kills: u32,
    broken: u32,
    sprite_deaths: u32,
    shots: u32,
    fell_at: Option<u64>,
}

/// Steps with `turn`, checking every invariant and the balances, and adds
/// up what happened.
fn step(sim: &mut Simulation, turn: &[Command], tally: &mut Tally) {
    let alive: Vec<bool> = SPRITES.iter().map(|&a| settlement(sim).agents[a as usize].body.alive()).collect();
    sim.step(turn);
    let s = settlement(sim);
    s.validate().unwrap();
    assert!(s.balances().values().all(|b| b.conserved), "conserved at tick {}", sim.world().tick());
    for e in &s.events {
        match *e {
            Event::Breach { .. } => tally.breaches += 1,
            Event::Hit { killed: true, impact, .. } => {
                tally.kills += 1;
                tally.fire_kills += u32::from(impact == super::combat::Impact::Fire);
            }
            Event::BuildingBroken { .. } => tally.broken += 1,
            Event::BuildingShot { .. } => tally.shots += 1,
            Event::TreeFell => tally.fell_at = Some(sim.world().tick() - 1),
            _ => {}
        }
    }
    tally.sprite_deaths += SPRITES.iter().zip(alive).filter(|&(&a, was)| was && !s.agents[a as usize].body.alive()).count() as u32;
}

/// Tops ballista `b`'s ammo up, if it stands, from the tree's stone pile, as a player
/// carrying stone over would: the units leave the pile and go in the
/// store, so the balances still hold.
fn restock(sim: &mut Simulation, b: BuildingIx) {
    let s = sim.world_mut().settlement_mut();
    if !s.buildings[b as usize].built() {
        return;
    }
    let units = s.buildings[b as usize].output.free().min(s.depots[TREE_STONE as usize].inventory.available(STONE));
    s.depots[TREE_STONE as usize].inventory.remove(STONE, units);
    assert!(s.buildings[b as usize].output.add(STONE, units));
}

/// The ticks the map's waves start on: each lane's first ghoul and every
/// `every` after, merged, a wave being the lanes that start within 20 s.
fn wave_starts(s: &Settlement) -> Vec<u64> {
    let mut starts: Vec<u64> = s.waves.iter().flat_map(|w| (0..w.count as u64).map(move |n| w.first + n * w.every)).collect();
    starts.sort();
    starts.dedup_by(|later, earlier| *later - *earlier < 20 * HZ);
    starts
}

/// Left alone, the town meets nothing during the calm start, then every
/// lane sends its ghouls on the timer, and the waves fell the tree in the
/// fifth wave: losing is possible when the defence is ignored, but not
/// early.
#[test]
fn an_undefended_town_has_its_calm_start_then_falls_to_the_waves() {
    let mut sim = town(&[]);
    let mut tally = Tally::default();
    let starts = wave_starts(settlement(&sim));
    assert_eq!(starts.len(), 6, "six waves: {starts:?}");
    assert_eq!(starts[0], CALM);
    for _ in 0..CALM - 1 {
        step(&mut sim, &[], &mut tally);
    }
    assert!(settlement(&sim).threats.is_empty(), "no ghoul in the calm start");
    while tally.fell_at.is_none() && sim.world().tick() < LAST {
        step(&mut sim, &[], &mut tally);
    }
    let s = settlement(&sim);
    eprintln!("undefended: {tally:?}");
    let fell = tally.fell_at.expect("the tree falls");
    let wave = starts.iter().filter(|&&t| t <= fell).count();
    assert_eq!(wave, 5, "the tree falls in the fifth wave (at {} s)", fell / HZ);
    assert!(s.waves.iter().all(|w| w.spawned > 0), "every lane sent ghouls: {:?}", s.waves.iter().map(|w| w.spawned).collect::<Vec<_>>());
    let spawned: u32 = s.waves.iter().map(|w| w.spawned).sum();
    assert_eq!(spawned, s.threats.len() as u32 + tally.breaches + tally.kills, "every ghoul breached, died or is still out");
}

/// Plays the map's waves against ballistas at `spots` (scenario meters),
/// each manned by a sprite, their stone topped up 5 s before each wave and,
/// with `mid_wave`, once more 12 s into it, as a player would on one trip
/// from the tree. Returns the tree's health at the end and what happened.
fn defend(spots: &[[f64; 2]], mid_wave: bool) -> (u32, Tally) {
    let mut sim = town(spots);
    let mut tally = Tally::default();
    let starts = wave_starts(settlement(&sim));
    let man: Vec<Command> = (0..spots.len()).map(|i| command(CommandKind::SetSpriteJob { agent: SPRITES[i], job: SpriteJob::Staff(BALLISTA_KIND) })).collect();
    step(&mut sim, &man, &mut tally);
    while sim.world().tick() < LAST && tally.fell_at.is_none() {
        let t = sim.world().tick();
        if starts.iter().any(|&start| start == t + 5 * HZ || (mid_wave && start + 12 * HZ == t)) {
            for b in 1..=spots.len() {
                restock(&mut sim, b as BuildingIx);
            }
        }
        step(&mut sim, &[], &mut tally);
    }
    let s = settlement(&sim);
    let spawned: u32 = s.waves.iter().map(|w| w.spawned).sum();
    assert_eq!(spawned, s.threats.len() as u32 + tally.breaches + tally.kills, "every ghoul breached, died or is still out");
    (s.health, tally)
}

/// Beside the tree, off the north column's way.
const WEST_OF_NORTH: [f64; 2] = [-1.4, -3.0];
const EAST_OF_NORTH: [f64; 2] = [1.6, -2.8];

/// Defence matters, but not every second. One manned ballista stocked only
/// between waves runs dry in the bigger ones (a ghoul needs two bolts and
/// the store holds ten), is broken while empty and the tree barely stands.
/// The same ballista reloaded once mid-wave keeps most of the tree's health
/// though some ghouls breach; two stocked between waves stop nearly all.
#[test]
fn one_ballista_reloaded_mid_wave_or_two_keep_the_tree_standing() {
    let (dry, tally) = defend(&[WEST_OF_NORTH], false);
    eprintln!("one ballista, between waves: tree {dry} {tally:?}");
    assert!(dry < 8, "one ballista with no reload mid-wave runs dry: tree {dry}, {tally:?}");
    let (reloaded, tally) = defend(&[WEST_OF_NORTH], true);
    eprintln!("one ballista, reloaded mid-wave: tree {reloaded} {tally:?}");
    assert_eq!(tally.fell_at, None);
    assert!((12..20).contains(&reloaded), "some ghouls breach, most don't: tree {reloaded}, {tally:?}");
    let (two, tally) = defend(&[WEST_OF_NORTH, EAST_OF_NORTH], false);
    eprintln!("two ballistas, between waves: tree {two} {tally:?}");
    assert!(two >= 17, "tree {two}, {tally:?}");
}

/// The fireball thrown at the head of a wave's column hurts it but doesn't
/// clear it: some of the ghouls walk on out of the fire, to be finished by
/// bolts or to breach.
#[test]
fn a_fireball_on_a_column_leaves_survivors() {
    let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
    scenario.waves.clear();
    scenario.waves.push(Wave {
        id: "column".into(),
        position: [fx(0.0), fx(-8.5)],
        target: [fx(0.0), fx(-1.8)],
        character: "ghoul_sacred_tree".into(),
        first: fx(15.0),
        every: fx(2.5),
        count: 6,
    });
    // An empty ballista east of the column's way: the elder only throws.
    scenario.buildings.push(Building { kind: "ballista".into(), position: [fx(2.6), fx(-4.0)] });
    let world = World::new_settlement(1, &Content::builtin(), &scenario, &[Disc { center: FxVec2::ZERO, radius: Fx::ONE }]).unwrap();
    let mut sim = Simulation::from_world(world, 1);
    let mut tally = Tally::default();
    let ballista: BuildingIx = 1;
    let board = |sim: &Simulation| {
        let s = settlement(sim);
        s.buildings[ballista as usize].slot_position(s.building_kind(ballista))
    };
    let spot = board(&sim);
    let go = command(CommandKind::MoveAvatar { agent: ELDER, intent: super::avatar::Intent::GoTo { target: spot, run: true } });
    step(&mut sim, &[go], &mut tally);
    for _ in 0..10 * HZ {
        step(&mut sim, &[], &mut tally);
    }
    step(&mut sim, &[command(CommandKind::Board { agent: ELDER })], &mut tally);
    assert_eq!(settlement(&sim).seat(ELDER as usize), Some(Seat::Building(ballista)));
    // Once the column's head is 3 m from the trunk, throw at it.
    let center = sim.world().terrain().center;
    while settlement(&sim).threats.first().is_none_or(|t| t.position.y < center.y - fx(4.5)) {
        step(&mut sim, &[], &mut tally);
    }
    let head = settlement(&sim).threats[0].position;
    let offset = head - settlement(&sim).buildings[ballista as usize].position;
    step(&mut sim, &[command(CommandKind::AimAvatar { agent: ELDER, aim: offset }), command(CommandKind::Burst { agent: ELDER })], &mut tally);
    assert!(settlement(&sim).events.iter().any(|e| matches!(e, Event::FireballThrown { .. })), "thrown");
    for _ in 0..40 * HZ {
        step(&mut sim, &[], &mut tally);
    }
    eprintln!("fireball on a column of 6: {tally:?}");
    assert!(tally.kills >= 1, "the fireball kills: {tally:?}");
    assert!(tally.kills <= 3, "but leaves most of the column: {tally:?}");
    assert_eq!(tally.kills + tally.breaches, 6, "{tally:?}");
}

