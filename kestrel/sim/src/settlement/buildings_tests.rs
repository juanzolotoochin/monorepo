//! Buildings: the Sacred Tree's prebuilt Wood Gatherer, its footprint
//! checks, and production by who occupies it. The slot is set by hand here;
//! sprites and possession fill it in later steps.

use content::scenario::{Building, Scenario};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::{BuildingIx, Event, Occupant, Settlement, Slot, Stage};
use crate::terrain::Disc;
use crate::{Simulation, World};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

/// The tree's trunk, as the scene's architecture blocks it.
fn trunk() -> Disc {
    Disc { center: FxVec2::ZERO, radius: Fx::ONE }
}

fn load(scenario: &Scenario) -> Result<World, String> {
    World::new_settlement(1, &Content::builtin(), scenario, &[trunk()])
}

fn sacred_tree() -> Simulation {
    Simulation::from_world(load(&Scenario::parse(SACRED_TREE).unwrap()).unwrap(), 1)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

/// The Wood Gatherer's output, in units of wood.
fn output(sim: &Simulation) -> u32 {
    let s = settlement(sim);
    s.buildings[0].output.count(s.item_index("wood").unwrap())
}

fn occupy(sim: &mut Simulation, slot: Slot) {
    sim.world_mut().settlement_mut().buildings[0].slot = slot;
}

/// Steps `ticks` ticks, checking every invariant after each.
fn steps(sim: &mut Simulation, ticks: u64) {
    for _ in 0..ticks {
        sim.step(&[]);
        settlement(sim).validate().unwrap();
    }
}

/// The elder, the only agent until sprites come in: any agent index does
/// for a slot set by hand.
const ELDER: u16 = 0;

#[test]
fn the_sacred_tree_loads_with_one_built_wood_gatherer() {
    let sim = sacred_tree();
    let s = settlement(&sim);
    assert_eq!(s.building_kinds.iter().map(|k| k.id.as_str()).collect::<Vec<_>>(), ["wood_gatherer", "stone_gatherer"]);
    let kind = s.building_kind(0);
    assert_eq!((kind.period, kind.cap, kind.possessed_cap), (5 * TICKS_PER_SECOND as u64, 10, 20));
    assert_eq!((kind.possessed_multiplier, kind.burst, kind.burst_cooldown), (Fx::from_int(4), 2, 30 * TICKS_PER_SECOND as u64));
    assert_eq!(kind.parts.len(), 2);
    assert_eq!(s.buildings.len(), 1);
    let b = &s.buildings[0];
    assert_eq!((b.stage.clone(), b.slot, b.output.total()), (Stage::Built, Slot::Empty, 0));
    let center = sim.world().terrain().center;
    assert_eq!(b.position, FxVec2::new(center.x + fx(-4.6), center.y + fx(-2.2)));
    assert!(sim.world().terrain().obstacles.contains(&Disc { center: b.position, radius: kind.radius }));
    // The output faces the tree: toward the midpoint of its two piles.
    let tree = FxVec2::new(center.x, center.y + fx(1.7));
    let (out, toward) = (b.output_position - b.position, tree - b.position);
    assert!((out.length() - fx(1.2)).abs() < fx(0.001), "{}", out.length().to_f64());
    assert!(out.perp_dot(toward).abs() < fx(0.001) && out.dot(toward) > Fx::ZERO);
    s.validate().unwrap();
}

#[test]
fn an_empty_building_never_produces() {
    let mut sim = sacred_tree();
    steps(&mut sim, 60 * TICKS_PER_SECOND as u64);
    assert_eq!(output(&sim), 0);
    assert_eq!(settlement(&sim).buildings[0].progress, Fx::ZERO);
}

#[test]
fn a_claimed_slot_does_not_produce() {
    let mut sim = sacred_tree();
    occupy(&mut sim, Slot::Claimed(ELDER));
    steps(&mut sim, 30 * TICKS_PER_SECOND as u64);
    assert_eq!(output(&sim), 0);
}

#[test]
fn a_sprite_produces_one_unit_per_period() {
    let mut sim = sacred_tree();
    occupy(&mut sim, Slot::Occupied(Occupant::Sprite(ELDER)));
    let period = 5 * TICKS_PER_SECOND as u64;
    steps(&mut sim, period - 1);
    assert_eq!(output(&sim), 0);
    steps(&mut sim, 1);
    assert_eq!(output(&sim), 1);
    assert!(settlement(&sim).events.contains(&Event::BuildingProduced { building: 0 as BuildingIx }));
    steps(&mut sim, 3 * period);
    assert_eq!(output(&sim), 4);
    let wood = &settlement(&sim).balances()["wood"];
    assert_eq!((wood.supplied, wood.stored), (4, 4));
    assert!(wood.conserved);
}

#[test]
fn production_pauses_at_the_cap_and_resumes_when_units_leave() {
    let mut sim = sacred_tree();
    occupy(&mut sim, Slot::Occupied(Occupant::Sprite(ELDER)));
    let period = 5 * TICKS_PER_SECOND as u64;
    steps(&mut sim, 10 * period);
    assert_eq!(output(&sim), 10);
    steps(&mut sim, 5 * period);
    assert_eq!(output(&sim), 10, "full");
    assert_eq!(settlement(&sim).buildings[0].progress, Fx::ZERO, "no work while full");
    // Take three, as a pickup later will; here they're spent.
    let s = sim.world_mut().settlement_mut();
    let wood = s.item_index("wood").unwrap();
    s.buildings[0].output.remove(wood, 3);
    s.accounts[wood as usize].spent += 3;
    steps(&mut sim, 3 * period - 1);
    assert_eq!(output(&sim), 9);
    steps(&mut sim, 1);
    assert_eq!(output(&sim), 10);
    let balance = &settlement(&sim).balances()["wood"];
    assert_eq!((balance.supplied, balance.stored, balance.spent), (13, 10, 3));
}

#[test]
fn an_elder_works_at_the_multiplier_up_to_the_possessed_cap() {
    let mut sim = sacred_tree();
    occupy(&mut sim, Slot::Occupied(Occupant::Elder(ELDER)));
    let period = 5 * TICKS_PER_SECOND as u64 / 4;
    steps(&mut sim, period - 1);
    assert_eq!(output(&sim), 0);
    steps(&mut sim, 1);
    assert_eq!(output(&sim), 1);
    steps(&mut sim, 19 * period);
    assert_eq!(output(&sim), 20);
    steps(&mut sim, 4 * period);
    assert_eq!(output(&sim), 20, "the possessed cap");
    // A sprite taking over finds it above its own cap.
    occupy(&mut sim, Slot::Occupied(Occupant::Sprite(ELDER)));
    steps(&mut sim, 10 * TICKS_PER_SECOND as u64);
    assert_eq!(output(&sim), 20);
}

#[test]
fn work_carries_over_when_the_occupant_changes() {
    let mut sim = sacred_tree();
    occupy(&mut sim, Slot::Occupied(Occupant::Sprite(ELDER)));
    // Half a sprite's period, then the elder does the other half 4x faster.
    steps(&mut sim, 150);
    occupy(&mut sim, Slot::Occupied(Occupant::Elder(ELDER)));
    steps(&mut sim, 37);
    assert_eq!(output(&sim), 0);
    steps(&mut sim, 1);
    assert_eq!(output(&sim), 1);
}

#[test]
fn a_prebuilt_building_must_stand_clear() {
    let base = Scenario::parse(SACRED_TREE).unwrap();
    let error = |edit: &dyn Fn(&mut Scenario)| {
        let mut scenario = base.clone();
        edit(&mut scenario);
        load(&scenario).err().expect("refused")
    };
    let at = |x: f64, y: f64| [fx(x), fx(y)];
    let tree = error(&|s| s.buildings[0].position = at(0.5, -0.5));
    assert!(tree.contains("overlaps an obstacle or another building"), "{tree}");
    let other = error(&|s| s.buildings.push(Building { kind: "stone_gatherer".into(), position: at(-4.0, -3.0) }));
    assert!(other.contains("building 1 (stone_gatherer)") && other.contains("another building"), "{other}");
    let pile = error(&|s| s.buildings[0].position = at(-2.4, 2.2));
    assert!(pile.contains("covers depot tree_wood's dock"), "{pile}");
    let edge = error(&|s| s.buildings[0].position = at(8.5, 0.0));
    assert!(edge.contains("reaches outside the arena"), "{edge}");
    let spawn = error(&|s| s.buildings[0].position = at(0.0, 3.6));
    assert!(spawn.contains("spawns inside an obstacle"), "{spawn}");
    // Two that just clear each other both stand.
    let mut two = base.clone();
    two.buildings.push(Building { kind: "stone_gatherer".into(), position: at(4.6, -2.2) });
    assert_eq!(load(&two).unwrap().settlement().unwrap().buildings.len(), 2);
}

#[test]
fn scenes_without_buildings_have_none() {
    let forest = Scenario::parse(super::tests::FOREST).unwrap();
    let world = World::new_settlement(1, &Content::builtin(), &forest, &[]).unwrap();
    let s = world.settlement().unwrap();
    assert!(s.buildings.is_empty() && s.building_kinds.is_empty());
}
