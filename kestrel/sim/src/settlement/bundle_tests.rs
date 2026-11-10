//! Bundles: the Sacred Tree's elder carries one item at a time, up to 5,
//! topping up from a building's output and the tree's piles, and emptying
//! into the pile that takes its item (D6, A4). Every test checks every
//! invariant on every tick.

use content::scenario::Scenario;
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::avatar::{BundleRefusal, Intent};
use super::{AgentIx, DepotIx, Direction, Event, Hold, InventoryRef, ItemIx, Kinds, Settlement, SpriteJob};
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");
const FOREST: &str = include_str!("../../../app/assets/levels/forest_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
const ASH: AgentIx = 1;
const WOOD: ItemIx = 0;
const STONE: ItemIx = 1;
const TREE_WOOD: DepotIx = 0;
const TREE_STONE: DepotIx = 1;
const OUTPUT: InventoryRef = InventoryRef::Output(0);

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

fn bundle(other: InventoryRef, direction: Direction) -> Command {
    command(CommandKind::Bundle { agent: ELDER, other, direction })
}

/// Steps one tick with `turn`, checking every invariant after it.
fn step(sim: &mut Simulation, turn: &[Command]) {
    sim.step(turn);
    settlement(sim).validate().unwrap();
}

/// Steps until `done`, at most `ticks` ticks; returns how many it took.
fn until(sim: &mut Simulation, ticks: u64, done: impl Fn(&Simulation) -> bool) -> u64 {
    for n in 0..ticks {
        if done(sim) {
            return n;
        }
        step(sim, &[]);
    }
    panic!("not done after {ticks} ticks");
}

fn cargo(sim: &Simulation, item: ItemIx) -> u32 {
    settlement(sim).agents[ELDER as usize].carry.inventory.count(item)
}

fn output(sim: &Simulation) -> u32 {
    settlement(sim).buildings[0].output.count(WOOD)
}

fn pile(sim: &Simulation, depot: DepotIx, item: ItemIx) -> u32 {
    settlement(sim).depots[depot as usize].inventory.count(item)
}

/// The elder's carry job and the units in the air are done.
fn settled(sim: &Simulation) -> bool {
    let s = settlement(sim);
    let carry = &s.agents[ELDER as usize].carry;
    carry.job.is_none() && s.transfers.is_empty()
}

/// The bundle refusal the last tick reported, if any.
fn refusal(sim: &Simulation) -> Option<BundleRefusal> {
    settlement(sim).events.iter().find_map(|e| match *e {
        Event::BundleRefused { agent: ELDER, reason } => Some(reason),
        _ => None,
    })
}

/// Walks the elder to `target` and waits until it stands there.
fn walk_to(sim: &mut Simulation, target: FxVec2) {
    step(sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target, run: true } })]);
    until(sim, 20 * HZ, |sim| {
        let a = &settlement(sim).agents[ELDER as usize];
        a.body.route.is_empty() && a.avatar().unwrap().intent == Some(Intent::Stop)
    });
}

/// A spot in front of the output pile, away from its building, within a
/// dock's reach of it.
fn by_output(sim: &Simulation) -> FxVec2 {
    let b = &settlement(sim).buildings[0];
    b.output_position + (b.output_position - b.position).normalize_or_zero() * Fx::from_ratio(2, 5)
}

fn by_pile(sim: &Simulation, depot: DepotIx) -> FxVec2 {
    settlement(sim).depots[depot as usize].position
}

/// Sends Ash to work the Wood Gatherer and waits until its output holds
/// `units`.
fn produce(sim: &mut Simulation, units: u32) {
    if settlement(sim).agents[ASH as usize].sprite().unwrap().holds.is_none() {
        step(sim, &[command(CommandKind::SetSpriteJob { agent: ASH, job: SpriteJob::Staff(0) })]);
    }
    until(sim, (units as u64 * 5 + 20) * HZ, |sim| output(sim) >= units);
}

/// Sends a bundle command and waits until its units have landed.
fn carry(sim: &mut Simulation, other: InventoryRef, direction: Direction) {
    step(sim, &[bundle(other, direction)]);
    assert_eq!(refusal(sim), None);
    until(sim, 10 * HZ, settled);
}

#[test]
fn the_elder_carries_a_bundle_of_five_and_the_tree_piles_take_their_item() {
    let sim = sacred_tree();
    let s = settlement(&sim);
    let cargo = &s.agents[ELDER as usize].carry.inventory;
    assert_eq!((cargo.capacity, cargo.kinds), (5, Kinds::OneKind));
    let kinds: Vec<_> = s.depots.iter().map(|d| d.inventory.kinds).collect();
    assert_eq!(kinds, [Kinds::Only(WOOD), Kinds::Only(STONE), Kinds::Only(STONE + 1)], "wood, stone and amber");
    // Other scenes' cargo and depots hold any mix, as before.
    let forest = World::new_settlement(1, &Content::builtin(), &Scenario::parse(FOREST).unwrap(), &[]).unwrap();
    let forest = forest.settlement().unwrap();
    assert!(forest.agents.iter().all(|a| a.carry.inventory.kinds == Kinds::AnyMix));
    assert!(forest.depots.iter().all(|d| d.inventory.kinds == Kinds::AnyMix));
}

#[test]
fn a_pickup_tops_up_the_bundle_and_stops_at_five() {
    let mut sim = sacred_tree();
    produce(&mut sim, 3);
    let spot = by_output(&sim);
    walk_to(&mut sim, spot);
    // As many as there are: all three, held at once on both sides.
    step(&mut sim, &[bundle(OUTPUT, Direction::Pickup)]);
    let room = |sim: &Simulation| settlement(sim).agents[ELDER as usize].carry.inventory.reserved_in[WOOD as usize];
    let job = settlement(&sim).agents[ELDER as usize].carry.job.clone().expect("a pickup");
    assert_eq!((job.other, job.item, job.direction), (OUTPUT, WOOD, Direction::Pickup));
    // Standing there, the first unit flew at once; the room is held for all.
    assert_eq!((job.remaining, room(&sim)), (2, 3));
    until(&mut sim, 10 * HZ, settled);
    assert_eq!(cargo(&sim, WOOD), 3);
    // Two more fit: the second pickup takes two and leaves the rest.
    produce(&mut sim, 4);
    step(&mut sim, &[bundle(OUTPUT, Direction::Pickup)]);
    assert_eq!(room(&sim), 2);
    until(&mut sim, 10 * HZ, settled);
    assert_eq!(cargo(&sim, WOOD), 5);
    assert!(output(&sim) >= 2, "the rest stays in the output");
    // Full: refused, and nothing changes.
    produce(&mut sim, 1);
    step(&mut sim, &[bundle(OUTPUT, Direction::Pickup)]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::BundleFull));
    assert!(settlement(&sim).agents[ELDER as usize].carry.job.is_none());
    assert_eq!(cargo(&sim, WOOD), 5);
    // An output takes nothing back.
    step(&mut sim, &[bundle(OUTPUT, Direction::Deposit)]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::PickupOnly));
}

#[test]
fn a_bundle_of_one_item_refuses_another() {
    let mut sim = sacred_tree();
    step(&mut sim, &[command(CommandKind::AddStock { depot: TREE_STONE, item: STONE, units: 3 })]);
    produce(&mut sim, 2);
    let stone = by_pile(&sim, TREE_STONE);
    walk_to(&mut sim, stone);
    carry(&mut sim, InventoryRef::Depot(TREE_STONE), Direction::Pickup);
    assert_eq!((cargo(&sim, STONE), pile(&sim, TREE_STONE, STONE)), (3, 0));
    // Carrying stone: wood from the output or the wood pile is refused.
    let spot = by_output(&sim);
    walk_to(&mut sim, spot);
    step(&mut sim, &[bundle(OUTPUT, Direction::Pickup)]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::OtherItem));
    step(&mut sim, &[bundle(InventoryRef::Depot(TREE_WOOD), Direction::Pickup)]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::OtherItem));
    // Stone doesn't go to the wood pile; the stone pile takes it back.
    step(&mut sim, &[bundle(InventoryRef::Depot(TREE_WOOD), Direction::Deposit)]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::WrongPile));
    assert_eq!(cargo(&sim, STONE), 3);
    walk_to(&mut sim, stone);
    carry(&mut sim, InventoryRef::Depot(TREE_STONE), Direction::Deposit);
    assert_eq!((cargo(&sim, STONE), pile(&sim, TREE_STONE, STONE)), (0, 3));
    // Empty-handed now: nothing to deposit, nothing in the wood pile, and
    // a depot transfer of the wrong item is refused as the bundle is.
    step(&mut sim, &[bundle(InventoryRef::Depot(TREE_STONE), Direction::Deposit)]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::NothingCarried));
    step(&mut sim, &[bundle(InventoryRef::Depot(TREE_WOOD), Direction::Pickup)]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::NothingToTake));
    assert_eq!(settlement(&sim).transfer_limit(ELDER as usize, TREE_WOOD as usize, STONE as usize, false), 0);
    // Only depots and outputs hold bundles.
    step(&mut sim, &[bundle(InventoryRef::Agent(ASH), Direction::Pickup), bundle(InventoryRef::Output(7), Direction::Pickup)]);
    let reasons: Vec<_> = settlement(&sim).events.iter().filter_map(|e| match *e {
        Event::BundleRefused { reason, .. } => Some(reason),
        _ => None,
    }).collect();
    assert_eq!(reasons, [BundleRefusal::NotAHolder, BundleRefusal::NotAHolder]);
}

#[test]
fn a_deposit_raises_the_tree_pile_only_as_units_land() {
    let mut sim = sacred_tree();
    produce(&mut sim, 4);
    let spot = by_output(&sim);
    walk_to(&mut sim, spot);
    carry(&mut sim, OUTPUT, Direction::Pickup);
    let carried = cargo(&sim, WOOD);
    assert!(carried >= 4);
    // The order alone moves nothing: the pile holds room for the bundle.
    step(&mut sim, &[bundle(InventoryRef::Depot(TREE_WOOD), Direction::Deposit)]);
    let d = &settlement(&sim).depots[TREE_WOOD as usize].inventory;
    assert_eq!((d.count(WOOD), d.reserved_in[WOOD as usize]), (0, carried));
    // Walking there, then a unit at a time: the pile rises only by the
    // units that landed (A4), and the balance holds throughout.
    let tree = by_pile(&sim, TREE_WOOD);
    step(&mut sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target: tree, run: true } })]);
    until(&mut sim, 20 * HZ, |sim| {
        let s = settlement(sim);
        let in_air = s.transfers.values().filter(|t| t.to == InventoryRef::Depot(TREE_WOOD)).count() as u32;
        let left = cargo(sim, WOOD);
        assert_eq!(pile(sim, TREE_WOOD, WOOD) + in_air + left, carried);
        assert!(s.balances().values().all(|b| b.conserved));
        settled(sim)
    });
    assert_eq!((pile(&sim, TREE_WOOD, WOOD), cargo(&sim, WOOD)), (carried, 0));
}

#[test]
fn a_withdrawal_fills_the_bundle_with_the_pile_item() {
    let mut sim = sacred_tree();
    step(&mut sim, &[command(CommandKind::AddStock { depot: TREE_WOOD, item: WOOD, units: 8 })]);
    // A pile takes only its own item, even from a script.
    step(&mut sim, &[command(CommandKind::AddStock { depot: TREE_WOOD, item: STONE, units: 1 })]);
    assert_eq!(pile(&sim, TREE_WOOD, STONE), 0);
    let tree = by_pile(&sim, TREE_WOOD);
    walk_to(&mut sim, tree);
    carry(&mut sim, InventoryRef::Depot(TREE_WOOD), Direction::Pickup);
    assert_eq!((cargo(&sim, WOOD), pile(&sim, TREE_WOOD, WOOD)), (5, 3));
    step(&mut sim, &[bundle(InventoryRef::Depot(TREE_WOOD), Direction::Pickup)]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::BundleFull));
    // Deposit two of five back with a transfer, then top up again: two.
    step(&mut sim, &[command(CommandKind::Transfer { agent: ELDER, depot: TREE_WOOD, item: WOOD, units: 2, deposit: true })]);
    until(&mut sim, 10 * HZ, settled);
    carry(&mut sim, InventoryRef::Depot(TREE_WOOD), Direction::Pickup);
    assert_eq!((cargo(&sim, WOOD), pile(&sim, TREE_WOOD, WOOD)), (5, 3));
}

/// The gatherer fills its output to its cap while the elder picks up: it
/// pauses while units are only reserved (they are still in the output),
/// starts again once they leave, and never holds more than its cap.
#[test]
fn production_keeps_its_cap_while_units_are_picked_up() {
    let mut sim = sacred_tree();
    let cap = settlement(&sim).building_kind(0).cap;
    produce(&mut sim, cap);
    let spot = by_output(&sim);
    walk_to(&mut sim, spot);
    let held = |sim: &Simulation| {
        let o = &settlement(sim).buildings[0].output;
        o.total() + o.total_in()
    };
    assert_eq!(held(&sim), cap);
    step(&mut sim, &[bundle(OUTPUT, Direction::Pickup)]);
    let mut produced = 0;
    for _ in 0..40 * HZ {
        step(&mut sim, &[]);
        let s = settlement(&sim);
        assert!(held(&sim) <= cap, "over the cap");
        let o = &s.buildings[0].output;
        // Reserved units are counted until they launch.
        assert!(o.reserved_out[WOOD as usize] <= o.count(WOOD));
        produced += s.events.iter().filter(|e| matches!(e, Event::BuildingProduced { .. })).count() as u32;
    }
    // It took five, and made five more to fill up again.
    assert_eq!(cargo(&sim, WOOD), 5);
    assert_eq!((produced, output(&sim)), (5, cap));
}

/// The slice the milestone plays: a sprite works the Wood Gatherer and the
/// elder carries its wood to the tree's wood pile, twice.
#[test]
fn a_sprite_gathers_and_the_elder_carries_wood_to_the_tree() {
    let mut sim = sacred_tree();
    produce(&mut sim, 5);
    assert_eq!(settlement(&sim).agents[ASH as usize].sprite().unwrap().holds, Some(Hold::Inside(0)));
    for round in 1..=2u32 {
        let spot = by_output(&sim);
        walk_to(&mut sim, spot);
        carry(&mut sim, OUTPUT, Direction::Pickup);
        assert_eq!(cargo(&sim, WOOD), 5, "round {round}");
        let tree = by_pile(&sim, TREE_WOOD);
        walk_to(&mut sim, tree);
        carry(&mut sim, InventoryRef::Depot(TREE_WOOD), Direction::Deposit);
        assert_eq!((cargo(&sim, WOOD), pile(&sim, TREE_WOOD, WOOD)), (0, 5 * round));
        produce(&mut sim, 5);
    }
    let s = settlement(&sim);
    let wood = &s.balances()["wood"];
    assert!(wood.conserved);
    assert_eq!(s.accounts[WOOD as usize].produced, wood.stored + wood.carried + wood.in_transit);
}
