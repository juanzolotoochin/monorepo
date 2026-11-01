//! Burst (D3): an elder possessing a built building adds the kind's Burst
//! units to its output at once, up to the possessed cap, then waits out the
//! cooldown. The cooldown is the elder's, so leaving, boarding another
//! building or respawning doesn't reset it. Every test checks every
//! invariant, conservation among them, on every tick.

use content::scenario::{Building, Scenario};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::{AgentIx, BuildingIx, BurstRefusal, Event, Seat, Settlement};
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

/// The scene, with `edit` applied to its scenario first.
fn sim_with(edit: impl FnOnce(&mut Scenario)) -> Simulation {
    let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
    edit(&mut scenario);
    // The tree's trunk, as the scene's architecture blocks it.
    let world = World::new_settlement(1, &Content::builtin(), &scenario, &[Disc { center: FxVec2::ZERO, radius: Fx::ONE }]).unwrap();
    Simulation::from_world(world, 1)
}

/// The scene with room in the possessed output for 30 s of an elder's
/// work and two Bursts, so a cooldown can run out before the output fills.
fn roomy() -> Simulation {
    sim_with(|s| s.building_kinds.iter_mut().for_each(|k| k.possessed_cap = 60))
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn command(kind: CommandKind) -> Command {
    Command { player: PlayerId(0), kind }
}

fn board() -> Command {
    command(CommandKind::Board { agent: ELDER })
}

fn burst() -> Command {
    command(CommandKind::Burst { agent: ELDER })
}

/// Steps one tick with `turn`, checking every invariant after it.
fn step(sim: &mut Simulation, turn: &[Command]) {
    sim.step(turn);
    settlement(sim).validate().unwrap();
}

fn steps(sim: &mut Simulation, ticks: u64) {
    for _ in 0..ticks {
        step(sim, &[]);
    }
}

fn wood(sim: &Simulation, building: BuildingIx) -> u32 {
    settlement(sim).buildings[building as usize].output.count(0)
}

fn burst_ready(sim: &Simulation) -> u64 {
    settlement(sim).agents[ELDER as usize].avatar().unwrap().burst_ready
}

/// What the last tick's Burst did: the units it added, or why it didn't.
fn bursted(sim: &Simulation) -> Option<Result<u32, BurstRefusal>> {
    settlement(sim).events.iter().find_map(|e| match *e {
        Event::BuildingBurst { units, .. } => Some(Ok(units)),
        Event::BurstRefused { agent, reason } if agent == ELDER => Some(Err(reason)),
        _ => None,
    })
}

/// Units the last tick's production (not a Burst) put in building `b`.
fn produced(sim: &Simulation, b: BuildingIx) -> u32 {
    settlement(sim).events.iter().filter(|e| **e == Event::BuildingProduced { building: b }).count() as u32
}

/// Walks the elder beside building `b` and boards it.
fn possess(sim: &mut Simulation, b: BuildingIx) {
    let s = settlement(sim);
    let to = s.leave_spot(b, s.agents[ELDER as usize].radius(), sim.world().terrain()).unwrap();
    step(sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target: to, run: true } })]);
    for _ in 0..20 * HZ {
        let a = &settlement(sim).agents[ELDER as usize];
        if a.body.route.is_empty() && a.avatar().unwrap().intent == Some(Intent::Stop) {
            break;
        }
        step(sim, &[]);
    }
    step(sim, &[board()]);
    assert_eq!(settlement(sim).seat(ELDER as usize), Some(Seat::Building(b)));
}

/// Building `b`'s Burst units, as its kind sets them (content).
fn units(sim: &Simulation, b: BuildingIx) -> u32 {
    settlement(sim).building_kind(b).burst
}

/// Bursts once, checking it added `units` on top of the tick's production.
fn burst_adds(sim: &mut Simulation, b: BuildingIx, units: u32) {
    let before = wood(sim, b);
    let made = settlement(sim).accounts[0].produced;
    step(sim, &[burst()]);
    assert_eq!(bursted(sim), Some(Ok(units)));
    assert_eq!(wood(sim, b), before + units + produced(sim, b));
    assert_eq!(settlement(sim).accounts[0].produced, made + (units + produced(sim, b)) as u64, "counted as produced");
}

#[test]
fn a_burst_adds_its_units_at_once_then_waits_out_its_cooldown() {
    let mut sim = roomy();
    possess(&mut sim, 0);
    let cooldown = settlement(&sim).building_kind(0).burst_cooldown;
    assert_eq!(cooldown, 30 * HZ);
    let units_0 = units(&sim, 0);
    burst_adds(&mut sim, 0, units_0);
    let ready = burst_ready(&sim);
    assert_eq!(ready, sim.world().tick() - 1 + cooldown);
    // A second Burst right after is refused, in whole seconds left.
    step(&mut sim, &[burst()]);
    let Some(Err(reason)) = bursted(&sim) else { panic!("refused") };
    assert_eq!(reason.message(), "Burst ready in 30 s");
    // Still refused on the last tick of the cooldown, with no units and no
    // new cooldown.
    let wait = ready - sim.world().tick() - 1;
    steps(&mut sim, wait);
    let before = wood(&sim, 0);
    step(&mut sim, &[burst()]);
    assert_eq!(bursted(&sim), Some(Err(BurstRefusal::Cooling { ticks: 1 })));
    assert_eq!(BurstRefusal::Cooling { ticks: 1 }.message(), "Burst ready in 1 s");
    assert_eq!(wood(&sim, 0), before + produced(&sim, 0));
    assert_eq!(burst_ready(&sim), ready);
    // Ready on its tick.
    assert_eq!(settlement(&sim).burst_check(ELDER as usize, sim.world().tick()), Ok(units(&sim, 0)));
    let units_0 = units(&sim, 0);
    burst_adds(&mut sim, 0, units_0);
    assert_eq!(burst_ready(&sim), ready + cooldown);
}

#[test]
fn a_burst_is_refused_unseated_and_on_a_site_and_spends_no_cooldown() {
    let mut sim = sim_with(|_| {});
    step(&mut sim, &[burst()]);
    assert_eq!(bursted(&sim), Some(Err(BurstRefusal::NotPossessing)));
    assert_eq!(BurstRefusal::NotPossessing.message(), "Possess a building to Burst");
    // Beside a site, which is no seat, likewise.
    let center = sim.world().terrain().center;
    step(&mut sim, &[command(CommandKind::PlaceBlueprint { kind: 0, position: center + FxVec2::new(fx(4.6), fx(-2.2)) })]);
    assert_eq!(settlement(&sim).buildings.len(), 2, "the site went down");
    let s = settlement(&sim);
    let to = s.leave_spot(1, s.agents[ELDER as usize].radius(), sim.world().terrain()).unwrap();
    step(&mut sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target: to, run: true } })]);
    steps(&mut sim, 10 * HZ);
    step(&mut sim, &[board()]);
    assert_eq!(settlement(&sim).seat(ELDER as usize), None);
    step(&mut sim, &[burst()]);
    assert_eq!(bursted(&sim), Some(Err(BurstRefusal::NotPossessing)));
    assert_eq!(burst_ready(&sim), 0, "no cooldown spent");
}

#[test]
fn a_burst_fills_the_output_up_to_the_possessed_cap_and_not_past_it() {
    let mut sim = sim_with(|_| {});
    possess(&mut sim, 0);
    let cap = settlement(&sim).building_kind(0).possessed_cap;
    // One short of the cap, a Burst adds the one unit that fits.
    for _ in 0..60 * HZ {
        if wood(&sim, 0) == cap - 1 {
            break;
        }
        step(&mut sim, &[]);
    }
    assert_eq!(wood(&sim, 0), cap - 1);
    burst_adds(&mut sim, 0, 1);
    assert_eq!(wood(&sim, 0), cap);
    let ready = burst_ready(&sim);
    // Full, a Burst after the cooldown is refused and spends none.
    let wait = ready - sim.world().tick();
    steps(&mut sim, wait);
    step(&mut sim, &[burst()]);
    assert_eq!(bursted(&sim), Some(Err(BurstRefusal::OutputFull)));
    assert_eq!(BurstRefusal::OutputFull.message(), "Output full");
    assert_eq!((wood(&sim, 0), burst_ready(&sim)), (cap, ready));
}

#[test]
fn the_cooldown_outlasts_leaving_another_building_and_a_respawn() {
    // A second gatherer, with room for the 30 s it is possessed.
    let mut sim = sim_with(|s| {
        s.buildings.push(Building { kind: "wood_gatherer".into(), position: [fx(4.6), fx(-2.2)] });
        s.building_kinds.iter_mut().for_each(|k| k.possessed_cap = 60);
    });
    possess(&mut sim, 0);
    let units_0 = units(&sim, 0);
    burst_adds(&mut sim, 0, units_0);
    let ready = burst_ready(&sim);
    // Leaving and possessing the other gatherer doesn't reset it.
    step(&mut sim, &[board()]);
    assert_eq!(settlement(&sim).seat(ELDER as usize), None);
    possess(&mut sim, 1);
    step(&mut sim, &[burst()]);
    assert!(matches!(bursted(&sim), Some(Err(BurstRefusal::Cooling { .. }))));
    assert_eq!(burst_ready(&sim), ready);
    // Nor does respawning from the seat.
    step(&mut sim, &[command(CommandKind::Respawn { agent: ELDER })]);
    assert_eq!((settlement(&sim).seat(ELDER as usize), burst_ready(&sim)), (None, ready));
    // Once it runs out, the other gatherer bursts.
    possess(&mut sim, 1);
    let now = sim.world().tick();
    assert!(now < ready, "back in before the cooldown ran out");
    step(&mut sim, &[burst()]);
    assert!(matches!(bursted(&sim), Some(Err(BurstRefusal::Cooling { .. }))));
    let wait = ready - sim.world().tick();
    steps(&mut sim, wait);
    let units_1 = units(&sim, 1);
    burst_adds(&mut sim, 1, units_1);
}
