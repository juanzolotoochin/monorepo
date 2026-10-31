//! Building seats (D3, A3): the elder boards a built building's slot, which
//! puts out the sprite working there; the sprite plans again, and takes the
//! slot back once the elder leaves. While seated the elder produces at the
//! multiplier up to the possessed cap, and moving or dropping does nothing.
//! Every test checks every invariant on every tick.

use content::scenario::{Building, Scenario};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::avatar::{BundleRefusal, Intent};
use super::haulers::{footprint, RETRY_TICKS};
use super::{AgentIx, BuildingIx, Event, Hold, InteractTarget, Interaction, Occupant, Seat, Settlement, Slot, SpriteJob, Step};
use crate::nav::distance;
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
const ASH: AgentIx = 1;
const FERN: AgentIx = 2;
const WOOD_GATHERER: SpriteJob = SpriteJob::Staff(0);

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

fn sim_of(scenario: &Scenario) -> Simulation {
    // The tree's trunk, as the scene's architecture blocks it.
    let world = World::new_settlement(1, &Content::builtin(), scenario, &[Disc { center: FxVec2::ZERO, radius: Fx::ONE }]).unwrap();
    Simulation::from_world(world, 1)
}

fn sacred_tree() -> Simulation {
    sim_of(&Scenario::parse(SACRED_TREE).unwrap())
}

/// The scene with a second Wood Gatherer on the clearing's east side.
fn two_gatherers() -> Simulation {
    let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
    scenario.buildings.push(Building { kind: "wood_gatherer".into(), position: [fx(4.6), fx(-2.2)] });
    sim_of(&scenario)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn command(kind: CommandKind) -> Command {
    Command { player: PlayerId(0), kind }
}

fn board(agent: AgentIx) -> Command {
    command(CommandKind::Board { agent })
}

fn job(agent: AgentIx, job: SpriteJob) -> Command {
    command(CommandKind::SetSpriteJob { agent, job })
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

/// Steps until `done`, at most `ticks` ticks; returns how many it took.
fn until(sim: &mut Simulation, ticks: u64, done: impl Fn(&Simulation) -> bool) -> u64 {
    for n in 0..ticks {
        if done(sim) {
            return n;
        }
        step(sim, &[]);
    }
    let s = settlement(sim);
    let elder = &s.agents[ELDER as usize];
    let ash = &s.agents[ASH as usize];
    panic!("not done after {ticks} ticks: elder at {:?} route {:?} intent {:?} seat {:?}; ash at {:?} {:?} {:?}", elder.body.position, elder.body.route, elder.avatar().unwrap().intent, elder.avatar().unwrap().seat, ash.body.position, ash.sprite().unwrap().holds, ash.sprite().unwrap().plan);
}

fn seat(sim: &Simulation, agent: AgentIx) -> Option<Seat> {
    settlement(sim).seat(agent as usize)
}

fn slot(sim: &Simulation, building: BuildingIx) -> Slot {
    settlement(sim).buildings[building as usize].slot
}

fn holds(sim: &Simulation, sprite: AgentIx) -> Option<Hold> {
    settlement(sim).agents[sprite as usize].sprite().unwrap().holds
}

fn position(sim: &Simulation, agent: AgentIx) -> FxVec2 {
    settlement(sim).agents[agent as usize].body.position
}

fn wood(sim: &Simulation, building: BuildingIx) -> u32 {
    settlement(sim).buildings[building as usize].output.count(0)
}

fn slot_spot(sim: &Simulation, building: BuildingIx) -> FxVec2 {
    let s = settlement(sim);
    s.buildings[building as usize].slot_position(s.building_kind(building))
}

/// Where `agent` stands on leaving `building`.
fn leave_spot(sim: &Simulation, agent: AgentIx, building: BuildingIx) -> FxVec2 {
    let s = settlement(sim);
    s.leave_spot(building, s.agents[agent as usize].radius(), sim.world().terrain()).expect("a clear spot beside it")
}

/// The refusal the last tick reported for `agent`, if any.
fn refusal(sim: &Simulation, agent: AgentIx) -> Option<BundleRefusal> {
    settlement(sim).events.iter().find_map(|e| match *e {
        Event::BundleRefused { agent: a, reason } if a == agent => Some(reason),
        _ => None,
    })
}

fn walk_to(sim: &mut Simulation, agent: AgentIx, player: PlayerId, target: FxVec2) {
    step(sim, &[Command { player, kind: CommandKind::MoveAvatar { agent, intent: Intent::GoTo { target, run: true } } }]);
    until(sim, 20 * HZ, |sim| {
        let a = &settlement(sim).agents[agent as usize];
        a.body.route.is_empty() && a.avatar().unwrap().intent == Some(Intent::Stop)
    });
}

/// Sends Ash to the Wood Gatherer and waits until it works there.
fn staff(sim: &mut Simulation) {
    step(sim, &[job(ASH, WOOD_GATHERER)]);
    until(sim, 20 * HZ, |sim| holds(sim, ASH) == Some(Hold::Inside(0)));
}

#[test]
fn boarding_puts_the_sprite_out_and_it_takes_the_slot_back_when_the_elder_leaves() {
    let mut sim = sacred_tree();
    staff(&mut sim);
    let leave = leave_spot(&sim, ELDER, 0);
    walk_to(&mut sim, ELDER, PlayerId(0), leave);
    step(&mut sim, &[board(ELDER)]);
    // Seated in the slot the tick the command came, and the sprite out
    // beside the building, clear of the slot spot and the elder's way out.
    assert_eq!(seat(&sim, ELDER), Some(Seat::Building(0)));
    assert_eq!(slot(&sim, 0), Slot::Occupied(Occupant::Elder(ELDER)));
    assert_eq!(holds(&sim, ASH), None);
    let (ash, spot) = (position(&sim, ASH), slot_spot(&sim, 0));
    let apart = Fx::from_ratio(1, 2);
    assert!(distance(ash, spot) > apart && distance(ash, leave) > apart, "Ash at {ash:?}, slot {spot:?}, leave {leave:?}");
    assert!(distance(ash, settlement(&sim).buildings[0].position) < Fx::from_int(2), "beside the building");
    assert!(settlement(&sim).agents[ASH as usize].body.route.is_empty());
    // The elder stands at the slot, which others walk past.
    assert_eq!(position(&sim, ELDER), spot);
    assert!(!footprint(&settlement(&sim).agents[ELDER as usize]).solid);
    // With no other Wood Gatherer, Ash waits where it stands, its job kept.
    step(&mut sim, &[]);
    let sprite = settlement(&sim).agents[ASH as usize].sprite().unwrap();
    assert_eq!((sprite.job, sprite.current()), (WOOD_GATHERER, Some(Step::Wait)));
    steps(&mut sim, 5 * HZ);
    assert!(distance(position(&sim, ASH), ash) < Fx::from_ratio(1, 10), "it waited where it was put");

    // Leaving puts the elder at its spot and frees the slot; Ash takes it
    // on its next retry, with no new job.
    step(&mut sim, &[board(ELDER)]);
    assert_eq!(seat(&sim, ELDER), None);
    assert_eq!(position(&sim, ELDER), leave);
    assert_eq!(slot(&sim, 0), Slot::Empty);
    let waited = until(&mut sim, RETRY_TICKS + 2, |sim| holds(sim, ASH).is_some());
    assert!(waited <= RETRY_TICKS + 1);
    until(&mut sim, 10 * HZ, |sim| holds(sim, ASH) == Some(Hold::Inside(0)));
    assert_eq!(settlement(&sim).agents[ASH as usize].sprite().unwrap().job, WOOD_GATHERER);
}

#[test]
fn a_put_out_sprite_takes_another_free_building_of_its_kind() {
    let mut sim = two_gatherers();
    staff(&mut sim);
    let to = leave_spot(&sim, ELDER, 0);
    walk_to(&mut sim, ELDER, PlayerId(0), to);
    step(&mut sim, &[board(ELDER)]);
    assert_eq!(seat(&sim, ELDER), Some(Seat::Building(0)));
    // Ash plans again, for the free gatherer, and goes to work there.
    step(&mut sim, &[]);
    assert_eq!(holds(&sim, ASH), Some(Hold::Claimed(1)));
    until(&mut sim, 30 * HZ, |sim| holds(sim, ASH) == Some(Hold::Inside(1)));
    // Leaving, the elder's slot stays empty: Ash has no loyalty to it.
    step(&mut sim, &[board(ELDER)]);
    steps(&mut sim, RETRY_TICKS * 2);
    assert_eq!((slot(&sim, 0), holds(&sim, ASH)), (Slot::Empty, Some(Hold::Inside(1))));
}

#[test]
fn a_sprite_on_its_way_loses_its_claim() {
    let mut sim = sacred_tree();
    let to = leave_spot(&sim, ELDER, 0);
    walk_to(&mut sim, ELDER, PlayerId(0), to);
    step(&mut sim, &[job(ASH, WOOD_GATHERER)]);
    steps(&mut sim, HZ);
    assert_eq!(holds(&sim, ASH), Some(Hold::Claimed(0)));
    let ash = position(&sim, ASH);
    step(&mut sim, &[board(ELDER)]);
    assert_eq!((slot(&sim, 0), holds(&sim, ASH)), (Slot::Occupied(Occupant::Elder(ELDER)), None));
    assert!(settlement(&sim).agents[ASH as usize].body.route.is_empty(), "it stops");
    assert!(distance(position(&sim, ASH), ash) < Fx::from_ratio(1, 5), "where it was, not put beside the building");
    step(&mut sim, &[]);
    assert_eq!(settlement(&sim).agents[ASH as usize].sprite().unwrap().current(), Some(Step::Wait));
}

#[test]
fn seated_the_elder_works_at_the_multiplier_up_to_the_possessed_cap_and_back() {
    let mut sim = sacred_tree();
    staff(&mut sim);
    let to = leave_spot(&sim, ELDER, 0);
    walk_to(&mut sim, ELDER, PlayerId(0), to);
    let kind = settlement(&sim).building_kind(0).clone();
    step(&mut sim, &[board(ELDER)]);
    // A sprite's period is 5 s: possessed, a unit every 1.25 s.
    let start = wood(&sim, 0);
    steps(&mut sim, 10 * HZ);
    let made = wood(&sim, 0) - start;
    assert!((8..=9).contains(&made), "{made} units in 10 s");
    assert_eq!(kind.cap_for(slot(&sim, 0).occupant()), 20);
    until(&mut sim, 30 * HZ, |sim| wood(sim, 0) == 20);
    steps(&mut sim, 5 * HZ);
    assert_eq!(wood(&sim, 0), 20, "the possessed cap");
    // Leaving, Ash goes back in, back to its own cap: nothing more comes
    // until units leave.
    step(&mut sim, &[board(ELDER)]);
    until(&mut sim, 10 * HZ, |sim| holds(sim, ASH) == Some(Hold::Inside(0)));
    assert_eq!(kind.cap_for(slot(&sim, 0).occupant()), 10);
    steps(&mut sim, 10 * HZ);
    assert_eq!(wood(&sim, 0), 20);
}

#[test]
fn a_sprite_works_at_its_own_rate_again_after_the_elder_leaves() {
    let mut sim = sacred_tree();
    staff(&mut sim);
    let to = leave_spot(&sim, ELDER, 0);
    walk_to(&mut sim, ELDER, PlayerId(0), to);
    step(&mut sim, &[board(ELDER)]);
    steps(&mut sim, 3 * HZ);
    step(&mut sim, &[board(ELDER)]);
    until(&mut sim, 10 * HZ, |sim| holds(sim, ASH) == Some(Hold::Inside(0)));
    let start = wood(&sim, 0);
    steps(&mut sim, 10 * HZ);
    let made = wood(&sim, 0) - start;
    assert!((2..=3).contains(&made), "{made} units in 10 s");
}

#[test]
fn boarding_is_refused_while_carrying_for_a_site_and_out_of_reach() {
    let mut sim = sacred_tree();
    // Out of reach, the key does nothing, and an interact is refused.
    step(&mut sim, &[board(ELDER)]);
    assert_eq!((seat(&sim, ELDER), refusal(&sim, ELDER)), (None, None));
    step(&mut sim, &[command(CommandKind::Interact { agent: ELDER, target: InteractTarget::Slot(0) })]);
    assert_eq!((seat(&sim, ELDER), refusal(&sim, ELDER)), (None, Some(BundleRefusal::OutOfReach)));

    // Carrying: refused, pointing to the drop key.
    step(&mut sim, &[command(CommandKind::AddStock { depot: 0, item: 0, units: 3 })]);
    let dock = settlement(&sim).depots[0].position;
    walk_to(&mut sim, ELDER, PlayerId(0), dock);
    step(&mut sim, &[command(CommandKind::Interact { agent: ELDER, target: InteractTarget::Depot(0) })]);
    until(&mut sim, 5 * HZ, |sim| settlement(sim).agents[ELDER as usize].carry.inventory.count(0) == 3 && settlement(sim).transfers.is_empty());
    let to = leave_spot(&sim, ELDER, 0);
    walk_to(&mut sim, ELDER, PlayerId(0), to);
    step(&mut sim, &[board(ELDER)]);
    assert_eq!((seat(&sim, ELDER), refusal(&sim, ELDER)), (None, Some(BundleRefusal::Carrying)));
    assert_eq!(BundleRefusal::Carrying.message(), "Drop the bundle first (Q)");
    // Interact offers no slot while carrying; a stale one is refused too.
    let world = sim.world();
    assert!(settlement(&sim).interact_target(ELDER, world.tick(), world.terrain()).is_none_or(|(t, _)| t != InteractTarget::Slot(0)));
    step(&mut sim, &[command(CommandKind::Interact { agent: ELDER, target: InteractTarget::Slot(0) })]);
    assert_eq!((seat(&sim, ELDER), refusal(&sim, ELDER)), (None, Some(BundleRefusal::Carrying)));
    // Dropped in the open, it boards.
    let center = sim.world().terrain().center;
    let to = center + FxVec2::new(Fx::from_int(3), -Fx::from_int(4));
    walk_to(&mut sim, ELDER, PlayerId(0), to);
    step(&mut sim, &[command(CommandKind::Drop { agent: ELDER })]);
    until(&mut sim, 5 * HZ, |sim| settlement(sim).agents[ELDER as usize].carry.inventory.total() == 0 && settlement(sim).transfers.is_empty());
    let to = leave_spot(&sim, ELDER, 0);
    walk_to(&mut sim, ELDER, PlayerId(0), to);
    step(&mut sim, &[board(ELDER)]);
    assert_eq!(seat(&sim, ELDER), Some(Seat::Building(0)));
}

#[test]
fn a_site_is_no_seat() {
    let mut sim = sacred_tree();
    let site = FxVec2::new(fx(4.6), fx(-2.2));
    let center = sim.world().terrain().center;
    step(&mut sim, &[command(CommandKind::PlaceBlueprint { kind: 1, position: center + site })]);
    assert_eq!(settlement(&sim).buildings.len(), 2, "the site went down");
    let to = leave_spot(&sim, ELDER, 1);
    walk_to(&mut sim, ELDER, PlayerId(0), to);
    assert!(distance(position(&sim, ELDER), slot_spot(&sim, 1)) < Fx::from_int(1));
    step(&mut sim, &[board(ELDER)]);
    assert_eq!((seat(&sim, ELDER), slot(&sim, 1)), (None, Slot::Empty));
    let world = sim.world();
    assert!(settlement(&sim).interact_target(ELDER, world.tick(), world.terrain()).is_none_or(|(t, _)| t != InteractTarget::Slot(1)));
}

#[test]
fn interact_boards_and_leaves_and_a_sprite_inside_keeps_its_menu() {
    let mut sim = sacred_tree();
    let target = |sim: &Simulation| {
        let world = sim.world();
        settlement(sim).interact_target(ELDER, world.tick(), world.terrain())
    };
    let to = leave_spot(&sim, ELDER, 0);
    walk_to(&mut sim, ELDER, PlayerId(0), to);
    // An empty building's slot is the nearest target: interacting boards.
    assert_eq!(target(&sim), Some((InteractTarget::Slot(0), Interaction::Board { building: 0 })));
    step(&mut sim, &[command(CommandKind::Interact { agent: ELDER, target: InteractTarget::Slot(0) })]);
    assert_eq!(seat(&sim, ELDER), Some(Seat::Building(0)));
    // Seated, the slot is the one target, and interacting leaves it.
    assert_eq!(target(&sim), Some((InteractTarget::Slot(0), Interaction::Leave { building: 0 })));
    step(&mut sim, &[command(CommandKind::Interact { agent: ELDER, target: InteractTarget::Output(0) })]);
    assert_eq!(seat(&sim, ELDER), Some(Seat::Building(0)), "another target does nothing while seated");
    step(&mut sim, &[command(CommandKind::Interact { agent: ELDER, target: InteractTarget::Slot(0) })]);
    assert_eq!(seat(&sim, ELDER), None);
    step(&mut sim, &[]);
    assert_eq!(slot(&sim, 0), Slot::Empty);
    // A sprite inside stands on the slot spot: the tie goes to the sprite's
    // job menu (the lower kind), and the boarding key still boards.
    staff(&mut sim);
    assert_eq!(target(&sim), Some((InteractTarget::Sprite(ASH), Interaction::JobMenu)));
    step(&mut sim, &[board(ELDER)]);
    assert_eq!((seat(&sim, ELDER), holds(&sim, ASH)), (Some(Seat::Building(0)), None));
}

#[test]
fn seated_movement_and_dropping_do_nothing() {
    let mut sim = sacred_tree();
    let to = leave_spot(&sim, ELDER, 0);
    walk_to(&mut sim, ELDER, PlayerId(0), to);
    step(&mut sim, &[board(ELDER)]);
    let spot = slot_spot(&sim, 0);
    let walk = |direction| command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::Walk { direction, run: true } });
    step(&mut sim, &[walk(FxVec2::new(Fx::ONE, Fx::ZERO)), command(CommandKind::Drop { agent: ELDER }), command(CommandKind::Jump { agent: ELDER })]);
    steps(&mut sim, HZ);
    step(&mut sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target: FxVec2::ZERO, run: false } })]);
    steps(&mut sim, HZ);
    assert_eq!((position(&sim, ELDER), seat(&sim, ELDER)), (spot, Some(Seat::Building(0))));
    assert!(settlement(&sim).agents[ELDER as usize].body.route.is_empty());
    assert_eq!(refusal(&sim, ELDER), None);
}

/// Two elders reaching for one slot on a tick: boarding goes in command
/// order, which runs players in order (`World::apply_commands`), so the
/// lower player wins whichever way the turn lists them, and the other is
/// told the building is possessed.
#[test]
fn two_elders_racing_for_one_slot_go_in_command_order() {
    for reversed in [false, true] {
        let mut sim = sacred_tree();
        step(&mut sim, &[Command { player: PlayerId(1), kind: CommandKind::Join }]);
        let other = settlement(&sim).avatar_of(PlayerId(1)).unwrap();
        let to = leave_spot(&sim, ELDER, 0);
        walk_to(&mut sim, ELDER, PlayerId(0), to);
        // The second stands just outside the slot spot.
        let spot = slot_spot(&sim, 0);
        let across = spot + (spot - settlement(&sim).buildings[0].position).normalize_or_zero() * Fx::from_ratio(1, 2);
        walk_to(&mut sim, other, PlayerId(1), across);
        for a in [ELDER, other] {
            assert!(distance(position(&sim, a), spot) <= super::interact::INTERACT_REACH, "{a} at {:?}, the slot at {spot:?}", position(&sim, a));
        }
        let mut turn = vec![board(ELDER), Command { player: PlayerId(1), kind: CommandKind::Board { agent: other } }];
        if reversed {
            turn.reverse();
        }
        step(&mut sim, &turn);
        assert_eq!(slot(&sim, 0), Slot::Occupied(Occupant::Elder(ELDER)));
        assert_eq!((seat(&sim, ELDER), seat(&sim, other)), (Some(Seat::Building(0)), None));
        assert_eq!(refusal(&sim, other), Some(BundleRefusal::Possessed));
        // Once the first leaves, the second gets in.
        step(&mut sim, &[board(ELDER)]);
        step(&mut sim, &[Command { player: PlayerId(1), kind: CommandKind::Board { agent: other } }]);
        assert_eq!(slot(&sim, 0), Slot::Occupied(Occupant::Elder(other)));
    }
}

#[test]
fn leaving_the_game_or_respawning_while_seated_frees_the_slot() {
    let mut sim = sacred_tree();
    step(&mut sim, &[Command { player: PlayerId(1), kind: CommandKind::Join }]);
    let other = settlement(&sim).avatar_of(PlayerId(1)).unwrap();
    let to = leave_spot(&sim, other, 0);
    walk_to(&mut sim, other, PlayerId(1), to);
    step(&mut sim, &[Command { player: PlayerId(1), kind: CommandKind::Board { agent: other } }]);
    assert_eq!(slot(&sim, 0), Slot::Occupied(Occupant::Elder(other)));
    step(&mut sim, &[Command { player: PlayerId(1), kind: CommandKind::Leave }]);
    assert_eq!(slot(&sim, 0), Slot::Empty);
    // Respawning, the slot empties in housekeeping.
    let to = leave_spot(&sim, ELDER, 0);
    walk_to(&mut sim, ELDER, PlayerId(0), to);
    step(&mut sim, &[board(ELDER)]);
    assert_eq!(slot(&sim, 0), Slot::Occupied(Occupant::Elder(ELDER)));
    step(&mut sim, &[command(CommandKind::Respawn { agent: ELDER })]);
    assert_eq!((seat(&sim, ELDER), slot(&sim, 0)), (None, Slot::Empty));
}

/// Boarding and leaving over and over while two sprites work two
/// gatherers: every invariant holds on every tick (A8), and at the end both
/// sprites are back at work.
#[test]
fn repeated_board_and_leave_cycles_stay_valid() {
    let mut sim = two_gatherers();
    step(&mut sim, &[job(ASH, WOOD_GATHERER), job(FERN, WOOD_GATHERER)]);
    until(&mut sim, 30 * HZ, |sim| holds(sim, ASH).is_some_and(|h| matches!(h, Hold::Inside(_))) && holds(sim, FERN).is_some_and(|h| matches!(h, Hold::Inside(_))));
    let mut boarded = 0;
    for cycle in 0..12u64 {
        let building = (cycle % 2) as BuildingIx;
        let to = leave_spot(&sim, ELDER, building);
        walk_to(&mut sim, ELDER, PlayerId(0), to);
        step(&mut sim, &[board(ELDER)]);
        boarded += usize::from(seat(&sim, ELDER) == Some(Seat::Building(building)));
        steps(&mut sim, (cycle % 3 + 1) * HZ);
        step(&mut sim, &[board(ELDER)]);
        assert_eq!(seat(&sim, ELDER), None, "cycle {cycle}: left");
        steps(&mut sim, (cycle % 4) * HZ / 2);
    }
    assert_eq!(boarded, 12);
    until(&mut sim, 30 * HZ, |sim| {
        [ASH, FERN].iter().all(|&s| matches!(holds(sim, s), Some(Hold::Inside(_))))
    });
}
