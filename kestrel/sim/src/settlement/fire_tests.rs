//! The fireball and burning ground (`SACRED_TREE_BALLISTA.md`, B4): the
//! Burst key in a possessed ballista throws a fireball at the reticle, on
//! the elder's own cooldown, for no stone. It hurts the threats where it
//! lands and leaves a fire there for a while; a threat in a fire burns,
//! once a second, until a while after it leaves. Fire touches threats only.
//! Every test checks every invariant, conservation among them, on every
//! tick.

use content::scenario::{Building, Scenario};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::{Ability, AgentIx, BuildingIx, BurstRefusal, Event, FireballDef, FirePatch, Impact, Seat, Settlement, Threat};
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
const ASH: AgentIx = 1;
const STONE: usize = 1;
const GATHERER: BuildingIx = 0;
const BALLISTA: BuildingIx = 1;

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

/// The scene with a built, empty ballista on the clearing's east side.
fn town() -> Simulation {
    let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
    scenario.buildings.push(Building { kind: "ballista".into(), position: [fx(5.0), fx(0.6)] });
    let world = World::new_settlement(1, &Content::builtin(), &scenario, &[Disc { center: FxVec2::ZERO, radius: Fx::ONE }]).unwrap();
    Simulation::from_world(world, 1)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn settlement_mut(sim: &mut Simulation) -> &mut Settlement {
    sim.world_mut().settlement_mut()
}

fn command(kind: CommandKind) -> Command {
    Command { player: PlayerId(0), kind }
}

fn step(sim: &mut Simulation, turn: &[Command]) {
    sim.step(turn);
    settlement(sim).validate().unwrap();
}

fn steps(sim: &mut Simulation, ticks: u64) {
    for _ in 0..ticks {
        step(sim, &[]);
    }
}

fn until(sim: &mut Simulation, ticks: u64, done: impl Fn(&Simulation) -> bool) {
    for _ in 0..ticks {
        if done(sim) {
            return;
        }
        step(sim, &[]);
    }
    panic!("not done after {ticks} ticks");
}

/// The next tick to run.
fn tick(sim: &Simulation) -> u64 {
    sim.world().tick()
}

/// The tick that ran last.
fn ran(sim: &Simulation) -> u64 {
    tick(sim) - 1
}

fn walk_to(sim: &mut Simulation, target: FxVec2) {
    use super::avatar::Intent;
    step(sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target, run: true } })]);
    until(sim, 20 * HZ, |sim| {
        let a = &settlement(sim).agents[ELDER as usize];
        a.body.route.is_empty() && a.avatar().unwrap().intent == Some(Intent::Stop)
    });
}

/// Walks the elder to building `b`'s slot and possesses it.
fn possess(sim: &mut Simulation, b: BuildingIx) {
    let s = settlement(sim);
    let spot = s.buildings[b as usize].slot_position(s.building_kind(b));
    walk_to(sim, spot);
    step(sim, &[command(CommandKind::Board { agent: ELDER })]);
    until(sim, HZ, |sim| settlement(sim).seat(ELDER as usize) == Some(Seat::Building(b)));
}

fn leave(sim: &mut Simulation) {
    step(sim, &[command(CommandKind::Board { agent: ELDER })]);
    assert_eq!(settlement(sim).seat(ELDER as usize), None);
}

fn aim(offset: FxVec2) -> Command {
    command(CommandKind::AimAvatar { agent: ELDER, aim: offset })
}

fn burst() -> Command {
    command(CommandKind::Burst { agent: ELDER })
}

/// The ballista's fireball, as its kind sets it (content).
fn fireball(sim: &Simulation) -> FireballDef {
    settlement(sim).building_kind(BALLISTA).defence.as_ref().unwrap().fireball.unwrap()
}

fn center(sim: &Simulation) -> FxVec2 {
    settlement(sim).buildings[BALLISTA as usize].position
}

/// What the last tick's Burst key did: where a fireball will land, or why
/// nothing happened.
fn thrown(sim: &Simulation) -> Option<Result<FxVec2, BurstRefusal>> {
    settlement(sim).events.iter().find_map(|e| match *e {
        Event::FireballThrown { agent: ELDER, building: BALLISTA, to, .. } => Some(Ok(to)),
        Event::BurstRefused { agent: ELDER, reason } => Some(Err(reason)),
        _ => None,
    })
}

fn fireball_ready(sim: &Simulation) -> u64 {
    settlement(sim).agents[ELDER as usize].avatar().unwrap().fireball_ready
}

/// A ghoul with `hp` that stands still (staggered for good) at `offset`
/// from the ballista.
fn threat(sim: &mut Simulation, id: u64, offset: FxVec2, hp: u32) {
    let position = center(sim) + offset;
    let character = Content::builtin().names.character_index("ghoul_forest").unwrap();
    let s = settlement_mut(sim);
    s.threats.push(Threat {
        id,
        wave: 0,
        character,
        position,
        heading: Fx::ZERO,
        travelled: Fx::ZERO,
        hp,
        armor: 0,
        pierced: false,
        shocked_until: 0,
        route: vec![position],
        stagger_until: u64::MAX,
        knockback: FxVec2::ZERO,
        windup: None,
        ready_at: 0,
        burn: None,
        goal: Default::default(),
    });
    s.threats.sort_by_key(|t| t.id);
    s.next_id = s.next_id.max(id + 1);
}

fn hp(sim: &Simulation, id: u64) -> Option<u32> {
    settlement(sim).threat(id).map(|t| t.hp)
}

/// Fire hits on the last tick, by threat.
fn fire_hits(sim: &Simulation) -> Vec<(u64, bool)> {
    settlement(sim)
        .events
        .iter()
        .filter_map(|e| match *e {
            Event::Hit { target, agent: None, projectile: None, killed, impact: Impact::Fire, .. } => Some((target, killed)),
            _ => None,
        })
        .collect()
}

/// A fire patch of the ballista's kind at `offset` from it, lit now, put
/// there by hand.
fn light(sim: &mut Simulation, offset: FxVec2) {
    let def = fireball(sim);
    let (position, now) = (center(sim) + offset, tick(sim));
    let s = settlement_mut(sim);
    let id = s.next_id;
    s.next_id += 1;
    s.fires.push(FirePatch { id, position, radius: def.fire_radius, born: now, expires: now + def.fire_ticks, burn: def.burn });
}

/// The Burst key in a possessed ballista throws a fireball at the reticle,
/// for no stone, and the elder then waits out its cooldown: refused, in
/// whole seconds, until its tick, and then at most the range away. Not
/// possessing, the key does nothing. The cooldown is the elder's, apart
/// from a gatherer's Burst: the gatherer still Bursts, and leaving and
/// respawning keep it.
#[test]
fn a_fireball_goes_to_the_reticle_then_waits_out_its_own_cooldown() {
    let mut sim = town();
    let def = fireball(&sim);
    assert_eq!(def.cooldown, 20 * HZ);
    step(&mut sim, &[burst()]);
    assert_eq!(thrown(&sim), Some(Err(BurstRefusal::NotPossessing)));

    possess(&mut sim, BALLISTA);
    let offset = FxVec2::new(fx(2.0), fx(-1.0));
    step(&mut sim, &[aim(offset)]);
    let to = center(&sim) + offset;
    assert_eq!(settlement(&sim).burst_check(ELDER as usize, tick(&sim)), Ok(Ability::Fireball { to }));
    step(&mut sim, &[burst()]);
    assert_eq!(thrown(&sim), Some(Ok(to)));
    let ready = ran(&sim) + def.cooldown;
    assert_eq!(fireball_ready(&sim), ready);
    let s = settlement(&sim);
    assert_eq!(s.fireballs.len(), 1);
    assert_eq!((s.fireballs[0].from, s.fireballs[0].to), (center(&sim), to));
    assert_eq!((s.accounts[STONE].spent, s.buildings[BALLISTA as usize].output.total()), (0, 0), "no stone");
    step(&mut sim, &[burst()]);
    let Some(Err(reason)) = thrown(&sim) else { panic!("refused") };
    assert_eq!(reason, BurstRefusal::FireballCooling { ticks: def.cooldown - 1 });
    assert_eq!(reason.message(), "Fireball ready in 20 s");
    // Still refused on its last tick, with no new cooldown.
    let wait = ready - 1 - tick(&sim);
    steps(&mut sim, wait);
    step(&mut sim, &[burst()]);
    assert_eq!(thrown(&sim), Some(Err(BurstRefusal::FireballCooling { ticks: 1 })));
    assert_eq!(fireball_ready(&sim), ready);
    // Ready on its tick; past the range, the reticle is cut to it.
    step(&mut sim, &[aim(FxVec2::new(fx(30.0), Fx::ZERO)), burst()]);
    let to = thrown(&sim).unwrap().unwrap();
    let range = settlement(&sim).building_kind(BALLISTA).defence.as_ref().unwrap().range;
    assert!(((to - center(&sim)).length() - range).abs() < Fx::from_ratio(1, 100), "{to:?}");
    let ready = ran(&sim) + def.cooldown;

    // The gatherer's Burst has its own cooldown.
    leave(&mut sim);
    possess(&mut sim, GATHERER);
    step(&mut sim, &[burst()]);
    assert!(settlement(&sim).events.iter().any(|e| matches!(e, Event::BuildingBurst { building: GATHERER, .. })));
    assert_eq!(fireball_ready(&sim), ready);
    leave(&mut sim);
    step(&mut sim, &[command(CommandKind::Respawn { agent: ELDER })]);
    assert_eq!(fireball_ready(&sim), ready, "respawning keeps it");
}

/// A fireball lands at the reticle after its flight: it hurts every threat
/// within its radius there (killing one, counted as a kill), and leaves a
/// fire of the kind's radius, which burns out after its time.
#[test]
fn a_fireball_hurts_threats_where_it_lands_and_leaves_a_fire() {
    let mut sim = town();
    let def = fireball(&sim);
    possess(&mut sim, BALLISTA);
    let offset = FxVec2::new(fx(3.0), Fx::ZERO);
    threat(&mut sim, 1000, offset, 5);
    threat(&mut sim, 1001, offset + FxVec2::new(Fx::ZERO, fx(0.5)), 2);
    // In the fire's reach but past the blast's.
    threat(&mut sim, 1002, offset + FxVec2::new(Fx::ZERO, fx(-1.2)), 5);
    threat(&mut sim, 1003, FxVec2::new(fx(-3.0), Fx::ZERO), 5);
    let kills = settlement(&sim).combat.kills;
    step(&mut sim, &[aim(offset), burst()]);
    let to = thrown(&sim).unwrap().unwrap();
    let arrival = settlement(&sim).fireballs[0].arrival;
    let flight = content::scenario::ticks(Fx::from_int(3) / def.speed);
    assert_eq!(arrival, ran(&sim) + flight);
    until(&mut sim, 2 * HZ, |sim| ran(sim) == arrival);
    assert_eq!(fire_hits(&sim), [(1000, false), (1001, true)]);
    assert_eq!((hp(&sim, 1000), hp(&sim, 1001), hp(&sim, 1002), hp(&sim, 1003)), (Some(3), None, Some(5), Some(5)));
    assert_eq!(settlement(&sim).combat.kills, kills + 1);
    assert!(settlement(&sim).events.contains(&Event::FireballLanded { position: to }));
    let s = settlement(&sim);
    assert!(s.fireballs.is_empty());
    assert_eq!(s.fires.len(), 1);
    assert_eq!((s.fires[0].position, s.fires[0].radius, s.fires[0].expires), (to, def.fire_radius, arrival + def.fire_ticks));
    // Both in the fire caught; the far one didn't.
    assert!(s.threat(1000).unwrap().burn.is_some() && s.threat(1002).unwrap().burn.is_some());
    assert!(s.threat(1003).unwrap().burn.is_none());
    until(&mut sim, 7 * HZ, |sim| settlement(sim).fires.is_empty());
    assert_eq!(ran(&sim), arrival + def.fire_ticks, "out on its tick");
}

/// A threat that stays in a fire until it burns out takes a hit a period
/// after catching, then one each period, until the linger after the fire
/// went out; then it stops burning.
#[test]
fn a_burn_hits_once_a_period_until_the_linger_runs_out() {
    let mut sim = town();
    let def = fireball(&sim);
    let (period, linger) = (def.burn.period, def.burn.linger);
    assert_eq!((period, linger, def.fire_ticks), (HZ, 3 * HZ, 6 * HZ));
    threat(&mut sim, 1000, FxVec2::new(fx(3.0), Fx::ZERO), 20);
    light(&mut sim, FxVec2::new(fx(3.0), fx(0.5)));
    let lit = tick(&sim);
    let mut hits = Vec::new();
    for _ in 0..12 * HZ {
        step(&mut sim, &[]);
        if !fire_hits(&sim).is_empty() {
            hits.push(ran(&sim) - lit);
        }
    }
    // In the fire for its 6 s (its last tick is 6 s less one after it
    // lit), then 3 s more.
    let last = def.fire_ticks - 1 + linger;
    let expected: Vec<u64> = (1..).map(|k| k * period).take_while(|&t| t <= last).collect();
    assert_eq!(hits, expected);
    assert_eq!(hits.len(), 8);
    assert_eq!(hp(&sim, 1000), Some(12));
    assert!(settlement(&sim).threat(1000).unwrap().burn.is_none());
}

/// Two fires on one threat burn it as one: its burn is refreshed, not
/// stacked. Taken out of the fire, it burns on for the linger and stops.
#[test]
fn a_burn_refreshes_rather_than_stacks_and_ends_after_leaving() {
    let mut sim = town();
    let def = fireball(&sim);
    let period = def.burn.period;
    let spot = FxVec2::new(fx(3.0), Fx::ZERO);
    threat(&mut sim, 1000, spot, 20);
    light(&mut sim, spot);
    light(&mut sim, spot + FxVec2::new(fx(0.4), Fx::ZERO));
    let lit = tick(&sim);
    let mut hits = Vec::new();
    // Out of both fires a period and a half after catching.
    let out = lit + period + period / 2;
    for _ in 0..8 * HZ {
        if tick(&sim) == out {
            let away = center(&sim) + FxVec2::new(fx(-3.0), Fx::ZERO);
            settlement_mut(&mut sim).threats[0].position = away;
        }
        step(&mut sim, &[]);
        for (target, _) in fire_hits(&sim) {
            assert_eq!(target, 1000);
            hits.push(ran(&sim) - lit);
        }
    }
    // Its last tick in a fire was the one before it was moved.
    let last = out - 1 - lit + def.burn.linger;
    let expected: Vec<u64> = (1..).map(|k| k * period).take_while(|&t| t <= last).collect();
    assert_eq!(hits, expected, "one hit a period, however many fires");
    assert_eq!(hits.len(), 4);
    assert_eq!(hp(&sim, 1000), Some(16));
    assert!(settlement(&sim).threat(1000).unwrap().burn.is_none());
}

/// A burn's kill counts as any other.
#[test]
fn a_burn_can_kill() {
    let mut sim = town();
    let spot = FxVec2::new(fx(3.0), Fx::ZERO);
    threat(&mut sim, 1000, spot, 2);
    light(&mut sim, spot);
    let kills = settlement(&sim).combat.kills;
    until(&mut sim, 4 * HZ, |sim| hp(sim, 1000).is_none());
    assert_eq!(fire_hits(&sim), [(1000, true)]);
    assert_eq!(settlement(&sim).combat.kills, kills + 1);
}

/// Fire hurts threats only: the elder and the sprites stand in it unhurt,
/// and a fireball landing on the elder does nothing to it.
#[test]
fn fire_does_not_hurt_the_elder_or_sprites() {
    let mut sim = town();
    let elder = settlement(&sim).agents[ELDER as usize].body.position;
    let ash = settlement(&sim).agents[ASH as usize].body.position;
    let before: Vec<u32> = settlement(&sim).agents.iter().map(|a| a.body.health).collect();
    let middle = center(&sim);
    light(&mut sim, elder - middle);
    light(&mut sim, ash - middle);
    for _ in 0..8 * HZ {
        step(&mut sim, &[]);
        assert!(fire_hits(&sim).is_empty());
    }
    let after: Vec<u32> = settlement(&sim).agents.iter().map(|a| a.body.health).collect();
    assert_eq!(before, after);
    // And a fireball at the elder's own feet.
    possess(&mut sim, BALLISTA);
    let feet = settlement(&sim).agents[ELDER as usize].body.position - center(&sim);
    step(&mut sim, &[aim(feet), burst()]);
    assert!(thrown(&sim).unwrap().is_ok());
    steps(&mut sim, 8 * HZ);
    let after: Vec<u32> = settlement(&sim).agents.iter().map(|a| a.body.health).collect();
    assert_eq!(before, after);
}

/// Two runs of the same turns, with fireballs and burning threats, end in
/// the same state; a client's replay of the elder throwing one matches the
/// simulation's avatar and predicts no event (stage 5 throws it).
#[test]
fn fire_is_deterministic_and_replays_as_the_simulation_does() {
    let run = || {
        let mut sim = town();
        possess(&mut sim, BALLISTA);
        for (i, x) in [2.0, 2.5, 3.0, 3.5].iter().enumerate() {
            threat(&mut sim, 1000 + i as u64, FxVec2::new(fx(*x), fx(0.2 * i as f64)), 4);
        }
        step(&mut sim, &[aim(FxVec2::new(fx(2.8), Fx::ZERO))]);
        let turns = [vec![burst()], vec![], vec![burst()]];
        let mut predicted = Vec::new();
        {
            let world = sim.world();
            let mut replay = settlement(&sim).replay_avatar(ELDER, world.tick(), world.terrain()).unwrap();
            for turn in &turns {
                let kinds: Vec<CommandKind> = turn.iter().map(|c| c.kind.clone()).collect();
                replay.step_with(kinds.iter());
                assert_eq!(replay.events(), []);
                predicted.push(replay.state().avatar().unwrap().aim);
            }
        }
        for (turn, predicted) in turns.iter().zip(predicted) {
            step(&mut sim, turn);
            assert_eq!(settlement(&sim).agents[ELDER as usize].avatar().unwrap().aim, predicted);
        }
        steps(&mut sim, 10 * HZ);
        sim
    };
    let (a, b) = (run(), run());
    assert_eq!(settlement(&a), settlement(&b));
    assert_eq!(settlement(&a).behaviour_digest(), settlement(&b).behaviour_digest());
    assert!(settlement(&a).combat.kills > 0, "the fire killed some");
}
