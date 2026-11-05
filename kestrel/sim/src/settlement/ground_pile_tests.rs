//! Ground piles in a town: the elder drops its whole bundle at its feet,
//! onto a pile of the same item there or a new one, and picks it back up
//! as it would from an output (M2.4). Every test checks every invariant on
//! every tick.

use content::scenario::Scenario;
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::avatar::{BundleRefusal, Intent};
use super::interact::{InteractTarget, Interaction};
use super::piles::{self, DropSpot, PileAction, PileId};
use super::{AgentIx, DepotIx, Event, InventoryRef, ItemIx, Settlement};
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
const WOOD: ItemIx = 0;
const STONE: ItemIx = 1;
const TREE_WOOD: DepotIx = 0;
const TREE_STONE: DepotIx = 1;

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

fn cargo(sim: &Simulation, item: ItemIx) -> u32 {
    settlement(sim).agents[ELDER as usize].carry.inventory.count(item)
}

/// The elder's carry job and the units in the air are done.
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

/// A point `x`, `z` meters from the tree.
fn at(sim: &Simulation, x: i32, z: i32) -> FxVec2 {
    sim.world().terrain().center + FxVec2::new(Fx::from_int(x), Fx::from_int(z))
}

/// Walks the elder to `target` and waits until it stands there.
fn walk_to(sim: &mut Simulation, target: FxVec2) {
    step(sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target, run: true } })]);
    until(sim, 20 * HZ, |sim| {
        let a = &settlement(sim).agents[ELDER as usize];
        a.body.route.is_empty() && a.avatar().unwrap().intent == Some(Intent::Stop)
    });
}

/// Walks the elder to `x`, `z` meters from the tree.
fn walk_at(sim: &mut Simulation, x: i32, z: i32) {
    let target = at(sim, x, z);
    walk_to(sim, target);
}

/// Puts `units` more of `item` in the tree pile `depot` and fills the
/// bundle there.
fn withdraw(sim: &mut Simulation, depot: DepotIx, item: ItemIx, units: u32) {
    step(sim, &[command(CommandKind::AddStock { depot, item, units })]);
    let dock = settlement(sim).depots[depot as usize].position;
    walk_to(sim, dock);
    step(sim, &[command(CommandKind::Interact { agent: ELDER, target: InteractTarget::Depot(depot) })]);
    until(sim, 10 * HZ, settled);
}

/// Drops the bundle and waits until its units have landed.
fn drop_bundle(sim: &mut Simulation) {
    step(sim, &[command(CommandKind::Drop { agent: ELDER })]);
    assert_eq!(refusal(sim), None);
    until(sim, 10 * HZ, settled);
}

/// The ground piles: id, item and units.
fn piles(sim: &Simulation) -> Vec<(PileId, Option<ItemIx>, u32)> {
    settlement(sim).piles.as_ref().unwrap().piles.iter().map(|(&id, p)| (id, p.item(), p.units())).collect()
}

/// A drop makes a pile at the elder's feet, the whole bundle flying onto
/// it as transfers; a second drop of the same item there merges into it.
#[test]
fn a_drop_makes_a_pile_and_a_second_one_merges() {
    let mut sim = sacred_tree();
    assert_eq!(piles(&sim), vec![], "a town starts with no ground piles");
    withdraw(&mut sim, TREE_WOOD, WOOD, 8);
    walk_at(&mut sim, 3, -4);
    step(&mut sim, &[command(CommandKind::Drop { agent: ELDER })]);
    let s = settlement(&sim);
    let job = s.agents[ELDER as usize].carry.job.clone().expect("the drop's carry job");
    assert_eq!((job.other, job.item, job.remaining), (InventoryRef::Pile(0), WOOD, 5));
    let feet = s.agents[ELDER as usize].body.position;
    let pile = &s.piles.as_ref().unwrap().piles[&0];
    assert!(crate::nav::distance(pile.position, feet) <= piles::DROP_GAP + Fx::from_ratio(1, 100), "at its feet");
    until(&mut sim, 10 * HZ, settled);
    assert_eq!((piles(&sim), cargo(&sim, WOOD)), (vec![(0, Some(WOOD), 5)], 0));

    // The other three wood, dropped in the same place: onto the same pile.
    withdraw(&mut sim, TREE_WOOD, WOOD, 0);
    walk_to(&mut sim, feet);
    drop_bundle(&mut sim);
    assert_eq!(piles(&sim), vec![(0, Some(WOOD), 8)]);
    let wood = &settlement(&sim).balances()["wood"];
    assert_eq!((wood.supplied, wood.stored), (8, 8));
}

/// Stone dropped on the wood pile's spot finds no room; a step aside it
/// makes a pile of its own.
#[test]
fn another_item_makes_its_own_pile() {
    let mut sim = sacred_tree();
    withdraw(&mut sim, TREE_WOOD, WOOD, 5);
    walk_at(&mut sim, 3, -4);
    drop_bundle(&mut sim);
    withdraw(&mut sim, TREE_STONE, STONE, 4);
    walk_at(&mut sim, 3, -4);
    step(&mut sim, &[command(CommandKind::Drop { agent: ELDER })]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::NoRoom), "the wood pile is in the way");
    walk_at(&mut sim, 3, -2);
    drop_bundle(&mut sim);
    assert_eq!(piles(&sim), vec![(0, Some(WOOD), 5), (1, Some(STONE), 4)]);
    assert_eq!(cargo(&sim, STONE), 0);
}

/// A ground pile is an interact target like an output: its pickup tops the
/// bundle up with its item, and the pile goes once it is empty and nothing
/// flies to or from it.
#[test]
fn a_pickup_tops_up_from_a_ground_pile_until_it_goes() {
    let mut sim = sacred_tree();
    withdraw(&mut sim, TREE_WOOD, WOOD, 8);
    walk_at(&mut sim, 3, -4);
    drop_bundle(&mut sim);
    withdraw(&mut sim, TREE_WOOD, WOOD, 0);
    walk_at(&mut sim, 3, -4);
    drop_bundle(&mut sim);
    // Two wood in hand, eight on the ground.
    withdraw(&mut sim, TREE_WOOD, WOOD, 2);
    walk_at(&mut sim, 3, -3);
    let s = settlement(&sim);
    let (target, interaction) = s.interact_target(ELDER, sim.world().tick(), sim.world().terrain()).expect("the pile is in reach");
    assert_eq!(target, InteractTarget::Pile(0));
    let Interaction::Bundle(job) = interaction else { panic!("a pickup, not {interaction:?}") };
    assert_eq!((job.item, job.remaining), (WOOD, 3), "tops the bundle up to 5");
    step(&mut sim, &[command(CommandKind::Interact { agent: ELDER, target })]);
    until(&mut sim, 10 * HZ, settled);
    assert_eq!((cargo(&sim, WOOD), piles(&sim)), (5, vec![(0, Some(WOOD), 5)]));

    // Carrying stone, a wood pile is no target.
    let mut stone = sacred_tree();
    withdraw(&mut stone, TREE_WOOD, WOOD, 5);
    walk_at(&mut stone, 3, -4);
    drop_bundle(&mut stone);
    withdraw(&mut stone, TREE_STONE, STONE, 1);
    walk_at(&mut stone, 3, -3);
    let s = settlement(&stone);
    let found = s.interact_target(ELDER, stone.world().tick(), stone.world().terrain()).map(|(t, _)| t);
    assert_ne!(found, Some(InteractTarget::Pile(0)));

    // Deposited, then the rest picked up: the empty pile goes.
    let dock = settlement(&sim).depots[TREE_WOOD as usize].position;
    walk_to(&mut sim, dock);
    step(&mut sim, &[command(CommandKind::Interact { agent: ELDER, target: InteractTarget::Depot(TREE_WOOD) })]);
    until(&mut sim, 10 * HZ, settled);
    walk_at(&mut sim, 3, -3);
    step(&mut sim, &[command(CommandKind::Interact { agent: ELDER, target: InteractTarget::Pile(0) })]);
    // Not while its last units are still in the air.
    until(&mut sim, 10 * HZ, |sim| settlement(sim).agents[ELDER as usize].carry.job.is_none());
    assert!(!settlement(&sim).transfers.is_empty() && piles(&sim).len() == 1, "the pile stays while units fly from it");
    until(&mut sim, 10 * HZ, settled);
    assert_eq!((cargo(&sim, WOOD), piles(&sim)), (5, vec![]));
    let wood = &settlement(&sim).balances()["wood"];
    assert!(wood.conserved, "{wood:?}");
}

/// A drop is refused with a reason: with nothing carried, with the lab's
/// piles at their most, and on ground that isn't clear. A pile haul is no
/// command a town takes.
#[test]
fn a_drop_is_refused_with_its_reason() {
    let mut sim = sacred_tree();
    step(&mut sim, &[command(CommandKind::Drop { agent: ELDER })]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::NothingToDrop));
    withdraw(&mut sim, TREE_WOOD, WOOD, 5);
    // At the tree's wood pile, a drop would hide its dock.
    step(&mut sim, &[command(CommandKind::Drop { agent: ELDER })]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::NoRoom));
    walk_at(&mut sim, 3, -4);
    // With the lab full, there is no new pile.
    let mut full = settlement(&sim).clone();
    let lab = full.piles.as_mut().unwrap();
    lab.max_piles = lab.piles.len();
    let tick = sim.world().tick();
    let a = &full.agents[ELDER as usize];
    assert_eq!(piles::drop_plan(&a.carry, &a.body, &full.view(ELDER, tick, sim.world().terrain())), Err(BundleRefusal::TooManyPiles));
    let s = settlement(&sim);
    let a = &s.agents[ELDER as usize];
    assert!(matches!(piles::drop_plan(&a.carry, &a.body, &s.view(ELDER, tick, sim.world().terrain())), Ok((DropSpot::New(_), _))));
    // Pile hauls are the pile lab's: a town refuses them.
    let spot = at(&sim, 3, -4);
    step(&mut sim, &[command(CommandKind::Pile { agent: ELDER, action: PileAction::Drop(spot), pace: Fx::ZERO, approach: None })]);
    let hauler = settlement(&sim).piles.as_ref().unwrap().hauler(ELDER).unwrap();
    assert_eq!(hauler.last_error.as_deref(), Some("Nothing here is hauled"));
    assert_eq!((cargo(&sim, WOOD), piles(&sim)), (5, vec![]));
}

/// Two drops on one tick at one spot: the first makes the pile, the second
/// merges into it (same item) in command order.
#[test]
fn drops_on_one_tick_resolve_in_command_order() {
    let mut sim = sacred_tree();
    withdraw(&mut sim, TREE_WOOD, WOOD, 5);
    walk_at(&mut sim, 3, -4);
    // The same agent twice: the second finds the bundle busy.
    step(&mut sim, &[command(CommandKind::Drop { agent: ELDER }), command(CommandKind::Drop { agent: ELDER })]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::Busy));
    until(&mut sim, 10 * HZ, settled);
    assert_eq!(piles(&sim), vec![(0, Some(WOOD), 5)]);
}
