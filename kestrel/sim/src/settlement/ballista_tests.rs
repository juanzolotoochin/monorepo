//! The Sacred Tree's ballista (`SACRED_TREE_BALLISTA.md`, B1, B2): a
//! building that shoots the nearest threat in range while someone is
//! inside and its ammo store has stone, one stone a shot, through the
//! Woodland Ballista's helpers. Players fill the store by interacting while
//! carrying stone, as much as fits; nothing comes back out. Every test
//! checks every invariant, conservation among them, on every tick.

use content::scenario::{Building, Scenario, Stack};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::avatar::{BundleRefusal, Intent};
use super::{AgentIx, BuildingIx, BuildingKindIx, DepotIx, Event, InteractTarget, Interaction, ItemIx, Occupant, Settlement, Slot, SpriteJob, Threat};
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

const HZ: u64 = TICKS_PER_SECOND as u64;
const ELDER: AgentIx = 0;
const ASH: AgentIx = 1;
const STONE: ItemIx = 1;
const TREE_WOOD: DepotIx = 0;
const TREE_STONE: DepotIx = 1;
const BALLISTA_KIND: BuildingKindIx = 2;
/// The ballista these tests put up beside the prebuilt Wood Gatherer.
const BALLISTA: BuildingIx = 1;

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

/// The scene with a built ballista on the clearing's east side, and stone
/// and wood in the tree's piles to stock it from.
fn town() -> Simulation {
    let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
    scenario.buildings.push(Building { kind: "ballista".into(), position: [fx(5.0), fx(0.6)] });
    scenario.depots[TREE_WOOD as usize].stock.push(Stack { item: "wood".into(), units: 10 });
    scenario.depots[TREE_STONE as usize].stock.push(Stack { item: "stone".into(), units: 40 });
    // The tree's trunk, as the scene's architecture blocks it.
    let world = World::new_settlement(1, &Content::builtin(), &scenario, &[Disc { center: FxVec2::ZERO, radius: Fx::ONE }]).unwrap();
    Simulation::from_world(world, 1)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

/// A scenario point (meters from the clearing's center) in world tiles.
fn at(sim: &Simulation, x: f64, y: f64) -> FxVec2 {
    let center = sim.world().terrain().center;
    FxVec2::new(center.x + fx(x), center.y + fx(y))
}

fn command(kind: CommandKind) -> Command {
    Command { player: PlayerId(0), kind }
}

/// Steps one tick with `turn`, checking every invariant after it.
fn step(sim: &mut Simulation, turn: &[Command]) {
    sim.step(turn);
    settlement(sim).validate().unwrap();
}

/// Steps `ticks` ticks, returning the shots the ballista fired meanwhile.
fn steps(sim: &mut Simulation, ticks: u64) -> u32 {
    let mut shots = 0;
    for _ in 0..ticks {
        step(sim, &[]);
        shots += fired(sim);
    }
    shots
}

/// Shots the ballista fired on the last tick.
fn fired(sim: &Simulation) -> u32 {
    settlement(sim).events.iter().filter(|e| **e == Event::BuildingShot { building: BALLISTA }).count() as u32
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

fn ammo(sim: &Simulation) -> u32 {
    settlement(sim).buildings[BALLISTA as usize].output.count(STONE)
}

fn cargo(sim: &Simulation, item: ItemIx) -> u32 {
    settlement(sim).agents[ELDER as usize].carry.inventory.count(item)
}

fn spent(sim: &Simulation) -> u64 {
    settlement(sim).accounts[STONE as usize].spent
}

/// The elder's interact target now, as the app reads it.
fn target(sim: &Simulation) -> Option<(InteractTarget, Interaction)> {
    let world = sim.world();
    settlement(sim).interact_target(ELDER, world.tick(), world.terrain())
}

fn walk_to(sim: &mut Simulation, target: FxVec2) {
    step(sim, &[command(CommandKind::MoveAvatar { agent: ELDER, intent: Intent::GoTo { target, run: true } })]);
    until(sim, 20 * HZ, |sim| {
        let a = &settlement(sim).agents[ELDER as usize];
        a.body.route.is_empty() && a.avatar().unwrap().intent == Some(Intent::Stop)
    });
}

/// Interacts with the elder's target, which must be `expected`, and waits
/// for its units to land.
fn use_target(sim: &mut Simulation, expected: InteractTarget) -> Interaction {
    let (t, interaction) = target(sim).expect("a target in reach");
    assert_eq!(t, expected, "{interaction:?}");
    step(sim, &[command(CommandKind::Interact { agent: ELDER, target: t })]);
    let refused = settlement(sim).events.iter().find_map(|e| match *e {
        Event::BundleRefused { agent: ELDER, reason } => Some(reason),
        _ => None,
    });
    assert_eq!(refused, None);
    until(sim, 10 * HZ, |sim| {
        let s = settlement(sim);
        s.agents[ELDER as usize].carry.job.is_none() && s.transfers.is_empty()
    });
    interaction
}

/// Takes a bundle of `item` out of its tree pile.
fn withdraw(sim: &mut Simulation, depot: DepotIx) {
    let dock = settlement(sim).depots[depot as usize].position;
    walk_to(sim, dock);
    use_target(sim, InteractTarget::Depot(depot));
}

/// Walks the elder to where it stands at the ballista's ammo pile.
fn walk_to_ballista(sim: &mut Simulation) {
    let b = &settlement(sim).buildings[BALLISTA as usize];
    let spot = b.output_position + (b.output_position - b.position).normalize_or_zero() * Fx::from_ratio(2, 5);
    walk_to(sim, spot);
}

/// Carries a bundle of stone from the tree to the ballista and delivers
/// it, returning what the delivery moved.
fn stock(sim: &mut Simulation) -> u32 {
    withdraw(sim, TREE_STONE);
    walk_to_ballista(sim);
    match use_target(sim, InteractTarget::Output(BALLISTA)) {
        Interaction::Bundle(job) => job.remaining,
        other => panic!("not a delivery: {other:?}"),
    }
}

/// Sets Ash to man the ballista and waits until Ash is inside.
fn man(sim: &mut Simulation) {
    step(sim, &[command(CommandKind::SetSpriteJob { agent: ASH, job: SpriteJob::Staff(BALLISTA_KIND) })]);
    until(sim, 30 * HZ, |sim| settlement(sim).buildings[BALLISTA as usize].slot == Slot::Occupied(Occupant::Sprite(ASH)));
}

/// A ghoul with `hp` that stands still (staggered for good) at a scenario
/// point, east of the ballista and away from the elder.
fn threat(sim: &mut Simulation, id: u64, x: f64, y: f64, hp: u32) {
    let position = at(sim, x, y);
    let character = Content::builtin().names.character_index("ghoul_forest").unwrap();
    let s = sim.world_mut().settlement_mut();
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
    });
    s.threats.sort_by_key(|t| t.id);
    s.next_id = s.next_id.max(id + 1);
}

fn hp(sim: &Simulation, id: u64) -> Option<u32> {
    settlement(sim).threat(id).map(|t| t.hp)
}

/// With nobody inside, or with nothing in its store, a ballista never
/// shoots, however near the threat.
#[test]
fn an_unmanned_or_empty_ballista_never_fires() {
    let mut sim = town();
    assert_eq!(settlement(&sim).building_kind(BALLISTA).defence.as_ref().map(|d| (d.ammo, d.ammo_cap)), Some((STONE, 10)));
    threat(&mut sim, 1000, 7.0, 0.6, 3);
    // Stocked but unmanned.
    assert_eq!(stock(&mut sim), 5);
    assert_eq!(steps(&mut sim, 5 * HZ), 0);
    assert_eq!((ammo(&sim), hp(&sim, 1000), settlement(&sim).combat.shots), (5, Some(3), 0));
    // A sprite claiming it, on its way in, doesn't shoot either.
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: ASH, job: SpriteJob::Staff(BALLISTA_KIND) })]);
    until(&mut sim, 2 * HZ, |sim| settlement(sim).buildings[BALLISTA as usize].slot == Slot::Claimed(ASH));
    assert_eq!(fired(&sim), 0);

    // Manned but empty: a second ballista, built and manned, with nothing
    // in its store.
    let mut sim = town();
    threat(&mut sim, 1000, 7.0, 0.6, 3);
    man(&mut sim);
    assert_eq!(steps(&mut sim, 5 * HZ), 0);
    assert_eq!((ammo(&sim), hp(&sim, 1000), spent(&sim)), (0, Some(3), 0));
}

/// Manned and stocked, the ballista shoots the nearest threat in range,
/// at most once a period, one stone a shot (spent, in the balances); once
/// that one is dead it turns to the next, and one out of range is left
/// alone.
#[test]
fn a_manned_stocked_ballista_shoots_the_nearest_threat_one_stone_a_shot() {
    let mut sim = town();
    assert_eq!(stock(&mut sim), 5);
    man(&mut sim);
    let period = settlement(&sim).building_kind(BALLISTA).defence.as_ref().unwrap().period;
    threat(&mut sim, 1001, 7.6, -1.6, 2);
    threat(&mut sim, 1000, 7.0, 0.6, 2);
    threat(&mut sim, 1002, 9.0, 0.6, 2);
    let mut shots = Vec::new();
    for _ in 0..6 * HZ {
        step(&mut sim, &[]);
        if fired(&sim) > 0 {
            let s = settlement(&sim);
            let p = s.projectiles.iter().max_by_key(|p| p.id).unwrap();
            assert_eq!((p.item, p.from, p.defender), (Some(STONE), s.buildings[BALLISTA as usize].position, None));
            shots.push((sim.world().tick(), p.target.unwrap()));
        }
    }
    let targets: Vec<u64> = shots.iter().map(|&(_, t)| t).collect();
    assert_eq!(targets, [1000, 1000, 1001, 1001], "the nearest first, two hits each");
    assert!(shots.windows(2).all(|w| w[1].0 - w[0].0 >= period), "{shots:?}");
    assert_eq!((hp(&sim, 1000), hp(&sim, 1001), hp(&sim, 1002)), (None, None, Some(2)), "the far one is out of range");
    assert_eq!((ammo(&sim), spent(&sim), settlement(&sim).combat.shots), (1, 4, 4));
}

/// A ballista that runs out stops, and starts again once stone comes in.
#[test]
fn a_ballista_out_of_ammo_stops_until_stocked() {
    let mut sim = town();
    assert_eq!(stock(&mut sim), 5);
    man(&mut sim);
    threat(&mut sim, 1000, 7.0, 0.6, 20);
    let shots = steps(&mut sim, 8 * HZ);
    assert_eq!((shots, ammo(&sim), spent(&sim)), (5, 0, 5));
    assert_eq!(steps(&mut sim, 3 * HZ), 0, "no ammo, no shot");
    assert_eq!(hp(&sim, 1000), Some(15));
    assert_eq!(stock(&mut sim), 5);
    assert!(steps(&mut sim, 2 * HZ) > 0, "stocked again");
}

/// A delivery moves as much stone as fits in the store, and the rest stays
/// in the bundle; a full store is no target, nor is one for an empty hand
/// (nothing comes out) or a bundle of wood (it takes stone only).
#[test]
fn a_delivery_fills_the_store_up_to_its_cap() {
    let mut sim = town();
    // Empty-handed, the ammo pile is no target: its slot is the one there.
    walk_to_ballista(&mut sim);
    assert!(target(&sim).is_none_or(|(t, _)| t != InteractTarget::Output(BALLISTA)));
    let view_refusal = {
        let world = sim.world();
        settlement(&sim).bundle_job(ELDER, super::InventoryRef::Output(BALLISTA), super::Direction::Pickup, world.tick(), world.terrain())
    };
    assert_eq!(view_refusal, Err(BundleRefusal::DepositOnly));
    // Wood isn't ammo.
    withdraw(&mut sim, TREE_WOOD);
    walk_to_ballista(&mut sim);
    assert!(target(&sim).is_none_or(|(t, _)| t != InteractTarget::Output(BALLISTA)));
    let dock = settlement(&sim).depots[TREE_WOOD as usize].position;
    walk_to(&mut sim, dock);
    use_target(&mut sim, InteractTarget::Depot(TREE_WOOD));
    // Two bundles fill it.
    assert_eq!((stock(&mut sim), stock(&mut sim)), (5, 5));
    assert_eq!((ammo(&sim), cargo(&sim, STONE)), (10, 0));
    // A third bundle doesn't go in a full store.
    withdraw(&mut sim, TREE_STONE);
    walk_to_ballista(&mut sim);
    assert!(target(&sim).is_none_or(|(t, _)| t != InteractTarget::Output(BALLISTA)));
    // Two shots make room for two; the other three stay in the bundle.
    man(&mut sim);
    threat(&mut sim, 1000, 7.0, 0.6, 20);
    until(&mut sim, 5 * HZ, |sim| ammo(sim) == 8);
    settlement_mut(&mut sim).threats.clear();
    match use_target(&mut sim, InteractTarget::Output(BALLISTA)) {
        Interaction::Bundle(job) => assert_eq!((job.item, job.direction, job.remaining), (STONE, super::Direction::Deposit, 2)),
        other => panic!("not a delivery: {other:?}"),
    }
    assert_eq!((ammo(&sim), cargo(&sim, STONE)), (10, 3));
}

fn settlement_mut(sim: &mut Simulation) -> &mut Settlement {
    sim.world_mut().settlement_mut()
}
