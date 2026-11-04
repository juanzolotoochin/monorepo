//! The Sacred Tree's waves on the map (M5.6, M7.2): a calm first day, then
//! one wave a night, a column per lane from a ring 40 m out up the hill's
//! ramps, burned away at sunrise. These tests play the map's own nights
//! against a town that ignores them and against one that defends, so the
//! numbers in the supply keep the defence mattering without being needed
//! every second; and they check the nights' rules on a short day. Every
//! tick checks every invariant.

use content::scenario::{Building, DayCycle, Scenario, Stack, Wave};
use content::TICKS_PER_SECOND;
use fx::Fx;

use super::clock::{Clock, Phase};
use super::combat::{lane_starts, COLUMN_GAP, DAWN_BURN};
use super::{AgentIx, BuildingIx, BuildingKindIx, DepotIx, Event, ItemIx, Seat, Settlement, SpriteJob};
use crate::{Command, CommandKind, PlayerId, Simulation};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
const SPRITES: [AgentIx; 4] = [1, 2, 3, 4];
const STONE: ItemIx = 1;
const TREE_STONE: DepotIx = 1;
const BALLISTA_KIND: BuildingKindIx = 1;
/// The first dusk, when the map's first ghouls come: the calm first day.
const CALM: u64 = 150 * HZ;
/// A day and a night.
const CYCLE: u64 = 225 * HZ;
/// The fourth night's sunrise, and its ghouls' last burns out.
const LAST: u64 = 4 * CYCLE + 2 * DAWN_BURN + HZ;

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

/// The map as it ships (the hill in its mine), with `ballistas` built at scenario points and
/// plenty of stone in the tree to stock them from.
fn town(ballistas: &[[f64; 2]]) -> Simulation {
    town_with(ballistas, |_| {})
}

/// `town`, with `change` made to the supply first.
fn town_with(ballistas: &[[f64; 2]], change: impl FnOnce(&mut Scenario)) -> Simulation {
    let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
    for &[x, y] in ballistas {
        scenario.buildings.push(Building { kind: "ballista".into(), position: [fx(x), fx(y)] });
    }
    scenario.depots[TREE_STONE as usize].stock.push(Stack { item: "stone".into(), units: 400 });
    change(&mut scenario);
    super::hill_tests::sacred_tree_map(scenario)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn clock(sim: &Simulation) -> Clock {
    settlement(sim).clock.expect("the Sacred Tree has days and nights")
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
    dispelled: u32,
    broken: u32,
    sprite_deaths: u32,
    shots: u32,
    fell_at: Option<u64>,
    /// Each ghoul sent: the tick it set off on and its lane.
    sent: Vec<(u64, u16)>,
}

/// Steps with `turn`, checking every invariant and the balances, and adds
/// up what happened.
fn step(sim: &mut Simulation, turn: &[Command], tally: &mut Tally) {
    let alive: Vec<bool> = SPRITES.iter().map(|&a| settlement(sim).agents[a as usize].body.alive()).collect();
    let newest = settlement(sim).next_id;
    let tick = sim.world().tick();
    sim.step(turn);
    let s = settlement(sim);
    s.validate().unwrap();
    assert!(s.balances().values().all(|b| b.conserved), "conserved at tick {}", sim.world().tick());
    tally.sent.extend(s.threats.iter().filter(|t| t.id >= newest && t.travelled == Fx::ZERO).map(|t| (tick, t.wave)));
    for e in &s.events {
        match *e {
            Event::Breach { .. } => tally.breaches += 1,
            Event::Hit { killed: true, impact, .. } => {
                tally.kills += 1;
                tally.fire_kills += u32::from(impact == super::combat::Impact::Fire);
            }
            Event::Dispelled { .. } => tally.dispelled += 1,
            Event::BuildingBroken { .. } => tally.broken += 1,
            Event::BuildingShot { .. } => tally.shots += 1,
            Event::TreeFell => tally.fell_at = Some(tick),
            _ => {}
        }
    }
    tally.sprite_deaths += SPRITES.iter().zip(alive).filter(|&(&a, was)| was && !s.agents[a as usize].body.alive()).count() as u32;
}

/// Every ghoul sent breached, died, burned away at sunrise or is still
/// out.
fn accounted(sim: &Simulation, tally: &Tally) {
    let s = settlement(sim);
    let spawned: u32 = s.waves.iter().map(|w| w.spawned).sum();
    assert_eq!(spawned as usize, tally.sent.len(), "{tally:?}");
    assert_eq!(spawned, s.threats.len() as u32 + tally.breaches + tally.kills + tally.dispelled, "every ghoul breached, died, was dispelled or is still out: {tally:?}");
    assert_eq!(s.dispelled, u64::from(tally.dispelled));
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

/// A lane's column: 2.5 s apart, or spread over the night's first third
/// when that is too long.
#[test]
fn a_lanes_ghouls_set_off_in_a_column_within_the_nights_first_third() {
    let night = 75 * HZ;
    assert_eq!(lane_starts(5, night).collect::<Vec<_>>(), (0..5).map(|i| i * COLUMN_GAP).collect::<Vec<_>>());
    assert_eq!(COLUMN_GAP, 5 * HZ / 2);
    // Twelve 2.5 s apart would take 27.5 s, past the first 25 s.
    let crowded: Vec<u64> = lane_starts(12, night).collect();
    assert_eq!(crowded.len(), 12);
    assert!(crowded.windows(2).all(|w| w[1] - w[0] == night / 3 / 12) && *crowded.last().unwrap() < night / 3, "{crowded:?}");
    assert_eq!(lane_starts(0, night).count(), 0);
}

/// On a short day (20 s days, 9 s nights, so a night's first third is 3 s
/// and its columns are squeezed into it), no ghoul comes by day; each
/// night every lane sends its listed count, all within the night's first
/// third; after the last listed night, it repeats. No ghoul gets near the
/// tree in 9 s, so each sunrise burns every one away.
#[test]
fn ghouls_come_only_by_night_as_listed_and_the_last_night_repeats() {
    let mut sim = town_with(&[], |s| s.day_cycle = Some(DayCycle { day: Fx::from_int(20), night: Fx::from_int(9) }));
    let mut tally = Tally::default();
    let c = clock(&sim);
    let nights = 6;
    while sim.world().tick() < nights * c.cycle() + 2 * DAWN_BURN + HZ {
        step(&mut sim, &[], &mut tally);
    }
    let s = settlement(&sim);
    let listed: Vec<Vec<u32>> = s.waves.iter().map(|w| w.nights.clone()).collect();
    assert_eq!(listed, [vec![3, 5, 5, 5], vec![2, 4, 5, 5]], "the supply's nights");
    for &(tick, _) in &tally.sent {
        assert_eq!(c.phase(tick), Phase::Night, "a ghoul came by day, at tick {tick}");
        assert!(c.since_dusk(tick).unwrap() < c.night / 3, "a ghoul came after the night's first third, at tick {tick}");
    }
    for night in 1..=nights {
        for (w, lane) in listed.iter().enumerate() {
            let sent = tally.sent.iter().filter(|&&(tick, wave)| wave as usize == w && c.night(tick) == night).count();
            let expected = lane[(night as usize).min(lane.len()) - 1];
            assert_eq!(sent, expected as usize, "lane {w} on night {night}");
        }
    }
    // Nights 5 and 6 are night 4 again.
    assert_eq!(tally.sent.len() as u32, listed.iter().map(|l| l.iter().sum::<u32>() + 2 * l[3]).sum::<u32>());
    assert!(s.threats.is_empty(), "the last sunrise burned them all");
    assert_eq!((tally.kills, tally.breaches), (0, 0), "{tally:?}");
    assert_eq!(tally.dispelled as usize, tally.sent.len());
    accounted(&sim, &tally);
}

/// At the first sunrise every ghoul still out catches fire, stands where
/// it is and falls within two to four seconds: dispelled, not killed, with
/// no hit, kill or breach; the balances hold throughout (`step`). A first
/// night of eight from the east (the last three are still on the hill at
/// dawn) leaves some out.
#[test]
fn the_sunrise_burns_every_ghoul_away() {
    let mut sim = town_with(&[], |s| s.waves[0].nights = vec![8; 4]);
    let mut tally = Tally::default();
    while sim.world().tick() < CYCLE {
        step(&mut sim, &[], &mut tally);
    }
    assert_eq!(clock(&sim).night(CYCLE - 1), 1);
    let out = settlement(&sim).threats.clone();
    assert!(out.len() >= 2, "ghouls are out at dawn: {tally:?}");
    let before = (tally.breaches, tally.kills, settlement(&sim).combat.kills);
    step(&mut sim, &[], &mut tally);
    let s = settlement(&sim);
    assert!(s.threats.iter().all(|t| t.burn.is_some_and(|b| b.dawn) && t.windup.is_none()), "all alight");
    let mut hits = 0;
    while !settlement(&sim).threats.is_empty() {
        let at: Vec<_> = settlement(&sim).threats.iter().map(|t| (t.id, t.position)).collect();
        step(&mut sim, &[], &mut tally);
        let s = settlement(&sim);
        assert!(s.threats.iter().all(|t| at.contains(&(t.id, t.position))), "a burning ghoul stands still");
        hits += s.events.iter().filter(|e| matches!(e, Event::Hit { .. })).count();
        assert!(sim.world().tick() <= CYCLE + 2 * DAWN_BURN + 1, "all fall within four seconds");
    }
    assert!(sim.world().tick() > CYCLE + DAWN_BURN, "they burn a while first");
    assert_eq!(hits, 0, "the sunrise burn hits nothing");
    assert_eq!((tally.breaches, tally.kills, settlement(&sim).combat.kills), before, "no kill, no breach");
    assert_eq!(tally.dispelled as usize, out.len());
    accounted(&sim, &tally);
    // The day after is calm until the next dusk.
    while sim.world().tick() < CYCLE + CALM {
        step(&mut sim, &[], &mut tally);
        assert!(settlement(&sim).threats.is_empty());
    }
}

/// Left alone, the town meets nothing on the first day, then every lane
/// sends its column each night, and the tree falls on the third night (the
/// first ghouls reach the tree about 63 s after dusk, M6.7): losing is
/// possible when the defence is ignored, but not early.
#[test]
fn an_undefended_town_has_its_calm_day_then_falls_on_the_third_night() {
    let mut sim = town(&[]);
    let mut tally = Tally::default();
    assert_eq!(settlement(&sim).next_wave(0), Some(CALM), "the HUD's next wave is the first dusk");
    for _ in 0..CALM {
        step(&mut sim, &[], &mut tally);
    }
    assert!(tally.sent.is_empty(), "no ghoul on the first day");
    while tally.fell_at.is_none() && sim.world().tick() < LAST {
        step(&mut sim, &[], &mut tally);
    }
    eprintln!("undefended: {tally:?}");
    let fell = tally.fell_at.expect("the tree falls");
    let c = clock(&sim);
    eprintln!("the tree fell on night {} ({} s, {} s after dusk)", c.night(fell), fell / HZ, c.since_dusk(fell).map_or(0, |t| t / HZ));
    assert_eq!((c.night(fell), c.phase(fell)), (3, Phase::Night), "the tree falls on the third night (at {} s)", fell / HZ);
    assert_eq!(settlement(&sim).nights_survived(fell), Some(2));
    accounted(&sim, &tally);
}

/// How long after dusk the restock comes: 5 s before the first ghouls
/// reach the old clearing's edge, 52 s after they set off (M6.7).
const RESTOCK: u64 = 47 * HZ;

/// Plays four of the map's nights against ballistas at `spots` (scenario
/// meters), each manned by a sprite, their stone topped up `RESTOCK` after
/// each dusk, as a player would on one trip from the tree. Returns the
/// tree's health at the end and what happened.
fn defend(spots: &[[f64; 2]]) -> (u32, Tally) {
    let mut sim = town(spots);
    let mut tally = Tally::default();
    let man: Vec<Command> = (0..spots.len()).map(|i| command(CommandKind::SetSpriteJob { agent: SPRITES[i], job: SpriteJob::Staff(BALLISTA_KIND) })).collect();
    step(&mut sim, &man, &mut tally);
    let c = clock(&sim);
    while sim.world().tick() < LAST && tally.fell_at.is_none() {
        if c.since_dusk(sim.world().tick()) == Some(RESTOCK) {
            for b in 1..=spots.len() {
                restock(&mut sim, b as BuildingIx);
            }
        }
        step(&mut sim, &[], &mut tally);
    }
    accounted(&sim, &tally);
    (settlement(&sim).health, tally)
}

/// Beside the tree, off the way of the column up the east ramp, in reach
/// of where it ends.
const EAST_OF_TREE: [f64; 2] = [3.0, -1.4];
/// Beside the tree, off the way of the lanes up the west ramp, in reach of
/// where they end.
const WEST_OF_TREE: [f64; 2] = [-3.0, -1.4];

/// The defence is a question of two entrances (M7). A manned ballista by
/// the east ramp's end, stocked each night, shoots its column (a ghoul
/// needs two bolts, the store holds ten and the column brings 3 to 5), but
/// the west ramp's column walks in and breaches. A second by the west
/// ramp's end holds four nights.
#[test]
fn a_ballista_guards_one_ramp_and_one_by_each_holds_four_nights() {
    let (one, tally) = defend(&[EAST_OF_TREE]);
    eprintln!("one ballista by the east ramp: tree {one} {tally:?}");
    let east = tally.sent.iter().filter(|&&(_, w)| w == 0).count() as u32;
    let west = tally.sent.len() as u32 - east;
    assert!(tally.breaches + 3 >= west, "the west column breaches: {west} came, {tally:?}");
    assert!(one <= 10 || tally.fell_at.is_some(), "tree {one}, {tally:?}");
    let (two, tally) = defend(&[EAST_OF_TREE, WEST_OF_TREE]);
    eprintln!("a ballista by each ramp: tree {two} {tally:?}");
    assert_eq!(tally.fell_at, None);
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
        nights: Vec::new(),
    });
    // An empty ballista east of the column's way: the elder only throws.
    scenario.buildings.push(Building { kind: "ballista".into(), position: [fx(2.6), fx(-4.0)] });
    let mut sim = super::hill_tests::sacred_tree_map(scenario);
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

