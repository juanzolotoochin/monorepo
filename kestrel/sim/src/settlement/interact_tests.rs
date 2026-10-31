//! Interact: the elder acts on the nearest usable thing in reach, which the
//! app highlights, and the sim checks the target again when the command
//! lands (D5). Every test checks every invariant on every tick.

use content::scenario::Scenario;
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::avatar::{BundleRefusal, Intent};
use super::interact::{interact_target, INTERACT_REACH};
use super::{AgentIx, DepotIx, Direction, Event, InteractTarget, Interaction, ItemIx, Settlement, SpriteJob};
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
const ASH: AgentIx = 1;
const FERN: AgentIx = 2;
const MOSS: AgentIx = 3;
const WOOD: ItemIx = 0;
const STONE: ItemIx = 1;
const TREE_WOOD: DepotIx = 0;
const TREE_STONE: DepotIx = 1;
const OUTPUT: InteractTarget = InteractTarget::Output(0);

fn sacred_tree() -> Simulation {
    let scenario = Scenario::parse(SACRED_TREE).unwrap();
    // The tree's trunk, as the scene's architecture blocks it.
    let world = World::new_settlement(1, &Content::builtin(), &scenario, &[Disc { center: FxVec2::ZERO, radius: Fx::ONE }]).unwrap();
    Simulation::from_world(world, 1)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

fn command(kind: CommandKind) -> Command {
    Command { player: PlayerId(0), kind }
}

fn interact(target: InteractTarget) -> Command {
    command(CommandKind::Interact { agent: ELDER, target })
}

/// Steps one tick with `turn`, checking every invariant after it.
fn step(sim: &mut Simulation, turn: &[Command]) {
    sim.step(turn);
    settlement(sim).validate().unwrap();
}

/// Steps until `done`, at most `ticks` ticks.
fn until(sim: &mut Simulation, ticks: u64, done: impl Fn(&Simulation) -> bool) {
    for _ in 0..ticks {
        if done(sim) {
            return;
        }
        step(sim, &[]);
    }
    panic!("not done after {ticks} ticks");
}

/// The elder's interact target now, as the app reads it.
fn target(sim: &Simulation) -> Option<(InteractTarget, Interaction)> {
    let world = sim.world();
    settlement(sim).interact_target(ELDER, world.tick(), world.terrain())
}

fn cargo(sim: &Simulation, item: ItemIx) -> u32 {
    settlement(sim).agents[ELDER as usize].carry.inventory.count(item)
}

fn settled(sim: &Simulation) -> bool {
    let s = settlement(sim);
    s.agents[ELDER as usize].carry.job.is_none() && s.transfers.is_empty()
}

/// The bundle refusal the last tick reported, if any.
fn refusal(sim: &Simulation) -> Option<BundleRefusal> {
    settlement(sim).events.iter().find_map(|e| match *e {
        Event::BundleRefused { agent: ELDER, reason } => Some(reason),
        _ => None,
    })
}

fn walk_to(sim: &mut Simulation, target: FxVec2) {
    step(sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target, run: true } })]);
    until(sim, 20 * HZ, |sim| {
        let a = &settlement(sim).agents[ELDER as usize];
        a.body.route.is_empty() && a.avatar().unwrap().intent == Some(Intent::Stop)
    });
}

/// A spot `meters` in front of the output pile, away from its building.
fn before_output(sim: &Simulation, meters: Fx) -> FxVec2 {
    let b = &settlement(sim).buildings[0];
    b.output_position + (b.output_position - b.position).normalize_or_zero() * meters
}

fn dock(sim: &Simulation, depot: DepotIx) -> FxVec2 {
    settlement(sim).depots[depot as usize].position
}

/// Sends Ash to work the Wood Gatherer and waits until its output holds
/// `units`.
fn produce(sim: &mut Simulation, units: u32) {
    if settlement(sim).agents[ASH as usize].sprite().unwrap().holds.is_none() {
        step(sim, &[command(CommandKind::SetSpriteJob { agent: ASH, job: SpriteJob::Staff(0) })]);
    }
    until(sim, (units as u64 * 5 + 20) * HZ, |sim| settlement(sim).buildings[0].output.count(WOOD) >= units);
}

/// The pickup the target offers: what it is, its item and units.
fn offer(sim: &Simulation) -> Option<(InteractTarget, Direction, ItemIx, u32)> {
    match target(sim)? {
        (t, Interaction::Bundle(job)) => Some((t, job.direction, job.item, job.remaining)),
        (_, Interaction::JobMenu) => None,
    }
}

#[test]
fn nothing_is_in_reach_at_the_start() {
    let sim = sacred_tree();
    assert_eq!(target(&sim), None);
}

/// One interact walks in, picks up at the output and deposits at the tree:
/// the key's whole loop, each step the nearest usable target.
#[test]
fn interact_picks_up_at_the_output_and_deposits_at_the_tree() {
    let mut sim = sacred_tree();
    produce(&mut sim, 3);
    // A step short of the dock's reach, but in interact's.
    let near = before_output(&sim, Fx::from_ratio(6, 5));
    walk_to(&mut sim, near);
    assert_eq!(offer(&sim), Some((OUTPUT, Direction::Pickup, WOOD, 3)));
    step(&mut sim, &[interact(OUTPUT)]);
    assert_eq!(refusal(&sim), None);
    // It walks in, and the units fly once it is there.
    until(&mut sim, 10 * HZ, settled);
    assert_eq!(cargo(&sim, WOOD), 3);
    // Carrying wood by the tree: the wood pile takes it, the stone pile not.
    let wood = dock(&sim, TREE_WOOD);
    walk_to(&mut sim, wood);
    assert_eq!(offer(&sim), Some((InteractTarget::Depot(TREE_WOOD), Direction::Deposit, WOOD, 3)));
    step(&mut sim, &[interact(InteractTarget::Depot(TREE_WOOD))]);
    until(&mut sim, 10 * HZ, settled);
    assert_eq!((cargo(&sim, WOOD), settlement(&sim).depots[TREE_WOOD as usize].inventory.count(WOOD)), (0, 3));
    // Empty-handed now, the pile offers its wood back.
    assert_eq!(offer(&sim), Some((InteractTarget::Depot(TREE_WOOD), Direction::Pickup, WOOD, 3)));
}

/// Targets that can't be used are passed over for the next nearest, or for
/// none: an empty output, a bundle of another item, a full bundle.
#[test]
fn interact_skips_what_cannot_be_used() {
    let mut sim = sacred_tree();
    let at_output = before_output(&sim, Fx::from_ratio(2, 5));
    walk_to(&mut sim, at_output);
    assert_eq!(target(&sim), None, "an empty output");
    // Carrying stone: the output's wood is another item.
    step(&mut sim, &[command(CommandKind::AddStock { depot: TREE_STONE, item: STONE, units: 2 })]);
    let stone = dock(&sim, TREE_STONE);
    walk_to(&mut sim, stone);
    assert_eq!(offer(&sim), Some((InteractTarget::Depot(TREE_STONE), Direction::Pickup, STONE, 2)));
    step(&mut sim, &[interact(InteractTarget::Depot(TREE_STONE))]);
    until(&mut sim, 10 * HZ, settled);
    produce(&mut sim, 2);
    walk_to(&mut sim, at_output);
    assert_eq!(target(&sim), None, "a bundle of stone");
    // Back with five wood: a full bundle takes nothing more there.
    let stone = dock(&sim, TREE_STONE);
    walk_to(&mut sim, stone);
    step(&mut sim, &[interact(InteractTarget::Depot(TREE_STONE))]);
    until(&mut sim, 10 * HZ, settled);
    produce(&mut sim, 6);
    walk_to(&mut sim, at_output);
    step(&mut sim, &[interact(OUTPUT)]);
    until(&mut sim, 10 * HZ, settled);
    assert_eq!(cargo(&sim, WOOD), 5);
    assert_eq!(target(&sim), None, "a full bundle");
}

/// Equally near targets go to the lower kind, then the lower index: the
/// lower sprite of two, and a depot before a sprite. Sprites stand where
/// the test says (the view reads no agents), on the wood pile's dock with
/// the elder.
#[test]
fn a_tie_goes_to_the_lower_kind_then_index() {
    let mut sim = sacred_tree();
    let wood = dock(&sim, TREE_WOOD);
    let tied = |sim: &Simulation| {
        let (s, world) = (settlement(sim), sim.world());
        let mut me = s.agents[ELDER as usize].clone();
        me.body.position = wood;
        let sprites = [(MOSS, wood), (FERN, wood)];
        interact_target(&s.view(ELDER, world.tick(), world.terrain()), &me, sprites).map(|(t, _)| t)
    };
    // An empty pile is no use empty-handed: the sprites tie, the lower wins.
    assert_eq!(tied(&sim), Some(InteractTarget::Sprite(FERN)));
    // With wood to withdraw, the pile ties with them and is the lower kind.
    step(&mut sim, &[command(CommandKind::AddStock { depot: TREE_WOOD, item: WOOD, units: 2 })]);
    assert_eq!(tied(&sim), Some(InteractTarget::Depot(TREE_WOOD)));
}

/// The sim checks the highlighted target again: one out of reach by the
/// time the command lands, or no longer usable, is refused with a reason,
/// and nothing moves.
#[test]
fn interact_refuses_a_stale_target() {
    let mut sim = sacred_tree();
    produce(&mut sim, 2);
    // Ash goes home, so the output holds what it holds.
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: ASH, job: SpriteJob::Idle })]);
    let near = before_output(&sim, Fx::from_ratio(2, 5));
    walk_to(&mut sim, near);
    assert_eq!(offer(&sim).map(|o| o.0), Some(OUTPUT));
    // Walked off past the reach: refused.
    let away = before_output(&sim, INTERACT_REACH + Fx::ONE);
    walk_to(&mut sim, away);
    step(&mut sim, &[interact(OUTPUT)]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::OutOfReach));
    assert!(settlement(&sim).agents[ELDER as usize].carry.job.is_none());
    // Back in reach, it takes every unit; the output is no use any more,
    // and a second interact on it gets the bundle's own reason.
    walk_to(&mut sim, near);
    step(&mut sim, &[interact(OUTPUT)]);
    until(&mut sim, 10 * HZ, settled);
    assert!(cargo(&sim, WOOD) >= 2);
    assert_eq!(settlement(&sim).buildings[0].output.count(WOOD), 0);
    assert_eq!(target(&sim), None);
    step(&mut sim, &[interact(OUTPUT)]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::NothingToTake));
    // A target that never was is nowhere, so out of reach.
    step(&mut sim, &[interact(InteractTarget::Output(9)), interact(InteractTarget::Depot(9))]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::OutOfReach));
    assert!(settled(&sim));
}

/// A sprite in reach is a target while empty-handed, and interacting with
/// it changes nothing in the sim: the app's menu sets its job.
#[test]
fn a_sprite_is_a_job_menu_target() {
    let mut sim = sacred_tree();
    let fern = settlement(&sim).agents[FERN as usize].body.position;
    walk_to(&mut sim, fern + FxVec2::new(Fx::ZERO, -Fx::from_ratio(4, 5)));
    assert_eq!(target(&sim), Some((InteractTarget::Sprite(FERN), Interaction::JobMenu)));
    let before = settlement(&sim).agents[FERN as usize].sprite().unwrap().job;
    step(&mut sim, &[interact(InteractTarget::Sprite(FERN))]);
    assert_eq!(refusal(&sim), None);
    let s = settlement(&sim);
    assert_eq!(s.agents[FERN as usize].sprite().unwrap().job, before);
    assert!(s.agents[ELDER as usize].carry.job.is_none());
}
