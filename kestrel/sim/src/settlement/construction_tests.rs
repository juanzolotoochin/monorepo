//! Construction (D1, D4): a blueprint goes down where the ground allows,
//! deliveries fill its next part with only what that part still needs, and
//! an install uses them up. Installing the last part makes the building,
//! which a sprite then staffs. Every test checks every invariant on every
//! tick, conservation among them. The Sacred Tree has no building that
//! makes stone (M6.4 of the mining design); the tests of the steps use a
//! test-only Stone Gatherer as a second kind, and the build order digs.

use content::scenario::Scenario;
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::avatar::{BundleRefusal, Intent};
use super::{AgentIx, BuildingIx, BuildingKindIx, DepotIx, Event, InteractTarget, Interaction, ItemIx, PlacementRefusal, Settlement, Slot, SpriteJob, Stage};
use crate::terrain::{Disc, Terrain};
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
const WOOD_GATHERER: BuildingKindIx = 0;
const BALLISTA: BuildingKindIx = 1;
/// The test-only kind (`tests::with_stone_gatherer`), after the ballista.
const STONE_GATHERER: BuildingKindIx = 2;
/// The prebuilt Wood Gatherer, and the first site put down.
const PREBUILT: BuildingIx = 0;
const FIRST_SITE: BuildingIx = 1;

/// The scene, with a test-only Stone Gatherer to build and `rocks` on the
/// ground besides the tree's trunk, in world tiles.
fn sacred_tree_with(rocks: &[Disc]) -> Simulation {
    let scenario = super::tests::with_stone_gatherer(SACRED_TREE);
    // `new_settlement` takes arena coordinates.
    let center = World::new_settlement(1, &Content::builtin(), &scenario, &[]).unwrap().terrain().center;
    let mut obstacles = vec![Disc { center: FxVec2::ZERO, radius: Fx::ONE }];
    obstacles.extend(rocks.iter().map(|r| Disc { center: r.center - center, radius: r.radius }));
    let world = World::new_settlement(1, &Content::builtin(), &scenario, &obstacles).unwrap();
    Simulation::from_world(world, 1)
}

fn sacred_tree() -> Simulation {
    sacred_tree_with(&[])
}

/// The scene's own supply on its map (M6.3), where stone is dug, with no
/// waves.
fn sacred_tree_map() -> Simulation {
    let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
    scenario.waves.clear();
    super::hill_tests::sacred_tree_map(scenario)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

/// A scenario point (meters from the clearing's center) in world tiles.
fn at(sim: &Simulation, x: f64, y: f64) -> FxVec2 {
    let center = sim.world().terrain().center;
    let fx = |v: f64| Fx::from_bits((v * 4294967296.0).round() as i64);
    FxVec2::new(center.x + fx(x), center.y + fx(y))
}

fn command(kind: CommandKind) -> Command {
    Command { player: PlayerId(0), kind }
}

fn place(kind: BuildingKindIx, position: FxVec2) -> Command {
    command(CommandKind::PlaceBlueprint { kind, position })
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

fn check(sim: &Simulation, kind: BuildingKindIx, position: FxVec2) -> Result<FxVec2, PlacementRefusal> {
    settlement(sim).placement_check(kind, position, sim.world().terrain())
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

/// Walks the elder to where it stands at building `b`'s output pile (or
/// where its site takes deliveries).
fn walk_to_building(sim: &mut Simulation, b: BuildingIx) {
    let building = &settlement(sim).buildings[b as usize];
    let spot = building.output_position + (building.output_position - building.position).normalize_or_zero() * Fx::from_ratio(2, 5);
    walk_to(sim, spot);
}

fn walk_to_dock(sim: &mut Simulation, depot: DepotIx) {
    let dock = settlement(sim).depots[depot as usize].position;
    walk_to(sim, dock);
}

/// Interacts with the elder's target, which must be `expected`, and waits
/// for any units to land.
fn use_target(sim: &mut Simulation, expected: InteractTarget) -> Interaction {
    let (t, interaction) = target(sim).expect("a target in reach");
    assert_eq!(t, expected, "{interaction:?}");
    step(sim, &[interact(t)]);
    assert_eq!(refusal(sim), None);
    until(sim, 10 * HZ, settled);
    interaction
}

fn site(sim: &Simulation, b: BuildingIx) -> (u8, Vec<u32>) {
    match &settlement(sim).buildings[b as usize].stage {
        Stage::Site { part, materials } => (*part, materials.counts.clone()),
        Stage::Built => panic!("built"),
    }
}

fn placed(sim: &Simulation) -> Option<BuildingIx> {
    settlement(sim).events.iter().find_map(|e| match *e {
        Event::BlueprintPlaced { building } => Some(building),
        _ => None,
    })
}

/// Puts the elder's bundle in the tree, if it holds anything.
fn empty_bundle(sim: &mut Simulation) {
    for (item, depot) in [(WOOD, TREE_WOOD), (STONE, TREE_STONE)] {
        if cargo(sim, item) > 0 {
            walk_to_dock(sim, depot);
            use_target(sim, InteractTarget::Depot(depot));
        }
    }
}

/// Digs the exposed stone vein nearest the tree on its east side (M6.3
/// puts one in each quarter): two layers, the first from beside the vein
/// and the second from inside that pit, both facing back west. Then
/// carries every dug stone to the tree's stone pile. Returns how much
/// was dug.
fn dig_stone(sim: &mut Simulation) -> u32 {
    let terrain = sim.world().terrain();
    let mine = terrain.mine().unwrap();
    let tree = Terrain::pit_tile(terrain.center);
    let veins = mine.spec.hill.expect("the scene's hill").exposed_veins(mine.spec.seed);
    let (x, y) = veins.into_iter().filter(|v| v.0 > tree.0).min_by_key(|v| (v.0 - tree.0).pow(2) + (v.1 - tree.1).pow(2)).expect("an east vein");
    for from in [(x + 2, y), (x + 1, y)] {
        walk_to(sim, Terrain::pit_center((from.0 + 1, from.1)));
        walk_to(sim, Terrain::pit_center(from));
        step(sim, &[command(CommandKind::DigFront { agent: ELDER, direction: crate::volume::DigDirection::Down })]);
        assert!(settlement(sim).agents[ELDER as usize].avatar().unwrap().dig.is_some(), "a dig from {from:?}");
        until(sim, 5 * HZ, |sim| settlement(sim).agents[ELDER as usize].avatar().unwrap().dig.is_none());
    }
    let dug = settlement(sim).accounts[STONE as usize].generated as u32;
    let stone_pile = |sim: &Simulation| {
        let piles = settlement(sim).piles.as_ref()?;
        piles.piles.iter().find(|(_, p)| p.item() == Some(STONE)).map(|(&id, p)| (id, p.position))
    };
    for _ in 0..4 {
        let Some((id, at)) = stone_pile(sim) else { break };
        let from = settlement(sim).agents[ELDER as usize].body.position;
        walk_to(sim, at + (from - at).normalize_or_zero() * Fx::from_ratio(3, 5));
        step(sim, &[interact(InteractTarget::Pile(id))]);
        assert_eq!(refusal(sim), None);
        until(sim, 10 * HZ, settled);
        empty_bundle(sim);
    }
    assert_eq!(stone_pile(sim), None, "every dug stone carried");
    dug
}

/// Picks up from building `b`'s output once it holds `units`.
fn pick_up(sim: &mut Simulation, b: BuildingIx, item: ItemIx, units: u32) {
    until(sim, (units as u64 * 8 + 20) * HZ, |sim| settlement(sim).buildings[b as usize].output.count(item) >= units);
    walk_to_building(sim, b);
    use_target(sim, InteractTarget::Output(b));
}

/// Every reason a blueprint is refused, each where only it applies, and a
/// spot that passes; the check reads the same ground the sim does.
#[test]
fn a_placement_is_refused_for_each_reason() {
    let sim = sacred_tree();
    let east = at(&sim, 5.0, 0.0);
    let output = check(&sim, STONE_GATHERER, east).expect("open ground to the east");
    assert_eq!(check(&sim, 9, east), Err(PlacementRefusal::UnknownKind));
    assert_eq!(check(&sim, STONE_GATHERER, at(&sim, 127.5, 0.0)), Err(PlacementRefusal::OutsideArena));
    assert_eq!(check(&sim, STONE_GATHERER, at(&sim, 0.5, -0.5)), Err(PlacementRefusal::Blocked), "the tree");
    assert_eq!(check(&sim, STONE_GATHERER, at(&sim, -4.6, 1.2)), Err(PlacementRefusal::Blocked), "the Wood Gatherer");
    assert_eq!(check(&sim, STONE_GATHERER, at(&sim, 0.0, 3.6)), Err(PlacementRefusal::Blocked), "the elder");
    assert_eq!(check(&sim, STONE_GATHERER, at(&sim, -2.4, 2.2)), Err(PlacementRefusal::CoversDock(TREE_WOOD)));
    assert_eq!(check(&sim, STONE_GATHERER, at(&sim, -3.3, 0.2)), Err(PlacementRefusal::CoversSpot(PREBUILT)), "the Wood Gatherer's output");
    // A rock where its output pile would go, then where its occupant would
    // stand (a quarter turn from the pile).
    let rock = |center| Disc { center, radius: Fx::from_ratio(1, 5) };
    let blocked = sacred_tree_with(&[rock(output)]);
    assert_eq!(check(&blocked, STONE_GATHERER, east), Err(PlacementRefusal::OutputBlocked));
    let out = (output - east).normalize_or_zero();
    let slot = east + FxVec2::new(-out.y, out.x) * (Fx::from_ratio(9, 10) + Fx::from_ratio(2, 5));
    let blocked = sacred_tree_with(&[rock(slot)]);
    assert_eq!(check(&blocked, STONE_GATHERER, east), Err(PlacementRefusal::SlotBlocked));
    // A ring of rocks around it, every spot inside clear but none reachable.
    let ring: Vec<Disc> = (0..36)
        .map(|i| {
            let angle = Fx::TAU.mul_int(i).div_int(36);
            Disc { center: east + FxVec2::from_angle(angle) * Fx::from_ratio(13, 5), radius: Fx::from_ratio(3, 10) }
        })
        .collect();
    let walled = sacred_tree_with(&ring);
    assert_eq!(check(&walled, STONE_GATHERER, east), Err(PlacementRefusal::Unreachable));
    // The words the ghost shows.
    assert_eq!(PlacementRefusal::CoversSpot(0).message(), "TOO CLOSE TO A PILE");
    assert_eq!(PlacementRefusal::Unreachable.message(), "CAN'T BE REACHED");
}

/// A placement lays a site waiting for its first part, whose footprint is
/// an obstacle from then on. Two that overlap on one tick resolve in
/// command order: the first stands, the second is refused.
#[test]
fn two_placements_on_one_tick_resolve_in_command_order() {
    let mut sim = sacred_tree();
    let (first, second) = (at(&sim, 5.0, 0.0), at(&sim, 5.6, 0.6));
    assert!(check(&sim, WOOD_GATHERER, second).is_ok(), "either fits alone");
    let revision = sim.world().terrain().revision;
    step(&mut sim, &[place(STONE_GATHERER, first), place(WOOD_GATHERER, second)]);
    let s = settlement(&sim);
    assert_eq!(s.buildings.len(), 2);
    let b = &s.buildings[FIRST_SITE as usize];
    assert_eq!((b.kind, b.position, b.slot), (STONE_GATHERER, first, Slot::Empty));
    assert_eq!(site(&sim, FIRST_SITE), (0, vec![0, 0, 0]));
    assert!(s.events.contains(&Event::BlueprintPlaced { building: FIRST_SITE }));
    assert!(s.events.contains(&Event::BlueprintRefused { kind: WOOD_GATHERER, reason: PlacementRefusal::Blocked }));
    let terrain = sim.world().terrain();
    assert!(terrain.obstacles.contains(&Disc { center: first, radius: Fx::from_ratio(9, 10) }));
    assert!(terrain.revision > revision);
    // Sprites staff built buildings only: a site is passed over.
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: ASH, job: SpriteJob::Staff(STONE_GATHERER) })]);
    for _ in 0..5 * HZ {
        step(&mut sim, &[]);
    }
    assert_eq!(settlement(&sim).buildings[FIRST_SITE as usize].slot, Slot::Empty);
    assert_eq!(settlement(&sim).agents[ASH as usize].sprite().unwrap().holds, None);
}

/// One interact while carrying delivers every unit the next part still
/// needs, the rest staying in the bundle; a bundle of something it doesn't
/// need is refused. Empty-handed, the site reads out what it needs, and its
/// part goes in only once that is all there.
#[test]
fn a_site_takes_only_what_its_next_part_needs() {
    let mut sim = sacred_tree();
    let spot = at(&sim, 5.0, 0.0);
    step(&mut sim, &[place(STONE_GATHERER, spot)]);
    assert_eq!(placed(&sim), Some(FIRST_SITE));
    let site_target = InteractTarget::Site(FIRST_SITE);
    // Empty-handed, nothing delivered: its needs, and an interact does
    // nothing.
    walk_to_building(&mut sim, FIRST_SITE);
    assert_eq!(target(&sim), Some((site_target, Interaction::Needs { building: FIRST_SITE, part: 0, needs: vec![(WOOD, 2)] })));
    step(&mut sim, &[interact(site_target)]);
    assert_eq!((refusal(&sim), site(&sim, FIRST_SITE)), (None, (0, vec![0, 0, 0])));
    // A bundle of stone: the foundation takes wood only.
    step(&mut sim, &[command(CommandKind::AddStock { depot: TREE_STONE, item: STONE, units: 3 })]);
    walk_to_dock(&mut sim, TREE_STONE);
    use_target(&mut sim, InteractTarget::Depot(TREE_STONE));
    walk_to_building(&mut sim, FIRST_SITE);
    assert_eq!(target(&sim), None, "no use for stone");
    step(&mut sim, &[interact(site_target)]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::NotNeeded));
    empty_bundle(&mut sim);
    // One wood: delivered, and the foundation still needs one.
    step(&mut sim, &[command(CommandKind::AddStock { depot: TREE_WOOD, item: WOOD, units: 1 })]);
    walk_to_dock(&mut sim, TREE_WOOD);
    use_target(&mut sim, InteractTarget::Depot(TREE_WOOD));
    walk_to_building(&mut sim, FIRST_SITE);
    let Interaction::Bundle(job) = use_target(&mut sim, site_target) else { panic!("a delivery") };
    assert_eq!(job.remaining, 1);
    assert_eq!((cargo(&sim, WOOD), site(&sim, FIRST_SITE)), (0, (0, vec![1, 0, 0])));
    assert_eq!(target(&sim), Some((site_target, Interaction::Needs { building: FIRST_SITE, part: 0, needs: vec![(WOOD, 1)] })));
    // Five more: it takes the one it lacks and leaves four in the bundle.
    step(&mut sim, &[command(CommandKind::AddStock { depot: TREE_WOOD, item: WOOD, units: 5 })]);
    walk_to_dock(&mut sim, TREE_WOOD);
    use_target(&mut sim, InteractTarget::Depot(TREE_WOOD));
    walk_to_building(&mut sim, FIRST_SITE);
    let Interaction::Bundle(job) = use_target(&mut sim, site_target) else { panic!("a delivery") };
    assert_eq!(job.remaining, 1);
    assert_eq!((cargo(&sim, WOOD), site(&sim, FIRST_SITE)), (4, (0, vec![2, 0, 0])));
    // Complete: the install, even with wood left in the bundle.
    assert_eq!(target(&sim), Some((site_target, Interaction::Install { building: FIRST_SITE, part: 0 })));
    // A pickup is no use on a site, whatever is asked.
    let pickup = command(CommandKind::Bundle { agent: ELDER, other: super::InventoryRef::Site(FIRST_SITE), direction: super::Direction::Pickup });
    step(&mut sim, &[pickup]);
    assert_eq!(refusal(&sim), Some(BundleRefusal::DepositOnly));
}

/// An install uses up the part's materials, which the balances count as
/// consumed, and moves on to the next part; the last makes the building,
/// which a sprite then staffs, and which turns out stone.
#[test]
fn installing_the_last_part_makes_the_building_and_a_sprite_staffs_it() {
    let mut sim = sacred_tree();
    let spot = at(&sim, 5.0, 0.0);
    step(&mut sim, &[place(STONE_GATHERER, spot)]);
    let site_target = InteractTarget::Site(FIRST_SITE);
    step(&mut sim, &[command(CommandKind::AddStock { depot: TREE_WOOD, item: WOOD, units: 5 })]);
    walk_to_dock(&mut sim, TREE_WOOD);
    use_target(&mut sim, InteractTarget::Depot(TREE_WOOD));
    walk_to_building(&mut sim, FIRST_SITE);
    use_target(&mut sim, site_target);
    assert_eq!(use_target(&mut sim, site_target), Interaction::Install { building: FIRST_SITE, part: 0 });
    assert!(settlement(&sim).events.contains(&Event::PartInstalled { building: FIRST_SITE, part: 0, built: false }));
    assert_eq!(site(&sim, FIRST_SITE), (1, vec![0, 0, 0]));
    let wood = &settlement(&sim).balances()["wood"];
    assert_eq!((wood.supplied, wood.consumed, wood.carried), (5, 2, 3));
    assert!(wood.conserved);
    // The mechanism: two more wood, then the install.
    let Interaction::Bundle(job) = use_target(&mut sim, site_target) else { panic!("a delivery") };
    assert_eq!((job.remaining, cargo(&sim, WOOD)), (2, 1));
    assert_eq!(use_target(&mut sim, site_target), Interaction::Install { building: FIRST_SITE, part: 1 });
    let s = settlement(&sim);
    assert!(s.events.contains(&Event::PartInstalled { building: FIRST_SITE, part: 1, built: true }));
    assert!(s.buildings[FIRST_SITE as usize].built());
    assert_eq!(s.balances()["wood"].consumed, 4);
    // Built and empty, it is inert, and an output now: nothing to pick up.
    assert_eq!(target(&sim), None);
    for _ in 0..10 * HZ {
        step(&mut sim, &[]);
    }
    assert_eq!(settlement(&sim).buildings[FIRST_SITE as usize].output.total(), 0);
    // Fern staffs it, and stone comes out.
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: FERN, job: SpriteJob::Staff(STONE_GATHERER) })]);
    until(&mut sim, 30 * HZ, |sim| settlement(sim).buildings[FIRST_SITE as usize].output.count(STONE) >= 1);
    assert_eq!(settlement(&sim).buildings[FIRST_SITE as usize].slot, Slot::Occupied(super::Occupant::Sprite(FERN)));
    assert!(settlement(&sim).balances().values().all(|b| b.conserved));
}

/// From an empty tree on the map, the whole build order (A5, handoff
/// "Tests"): no building makes stone, so the elder digs it from an exposed
/// vein and carries it to the tree; with the prebuilt Wood Gatherer's wood
/// it puts up a second Wood Gatherer and a ballista (4 wood and 2 stone
/// each), with no stock added and no softlock. Both end up staffed. The
/// elder bootstraps alone, so every stone is counted: a miner sprite
/// (M6.5) can do the stone's part too (`miner_tests`), and the happy path
/// sends one after the elder's first dig.
#[test]
fn both_kinds_build_from_an_empty_tree_and_dug_stone() {
    let mut sim = sacred_tree_map();
    assert!(settlement(&sim).depots.iter().all(|d| d.inventory.total() == 0));
    assert!(settlement(&sim).building_kinds.iter().all(|k| k.product != Some(STONE)), "no building makes stone");
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: ASH, job: SpriteJob::Staff(WOOD_GATHERER) })]);
    let dug = dig_stone(&mut sim);
    assert!(dug >= 6, "two stone layers, 3 to 6 each: {dug}");
    assert_eq!(settlement(&sim).depots[TREE_STONE as usize].inventory.count(STONE), dug);
    // The second Wood Gatherer north-east, then a ballista east of the
    // tree: each a foundation, then the mechanism's wood and stone.
    for (kind, (x, y)) in [(WOOD_GATHERER, (4.5, 4.0)), (BALLISTA, (5.0, 0.0))] {
        let spot = at(&sim, x, y);
        step(&mut sim, &[place(kind, spot)]);
        let b = placed(&sim).expect("placed");
        pick_up(&mut sim, PREBUILT, WOOD, 4);
        walk_to_building(&mut sim, b);
        use_target(&mut sim, InteractTarget::Site(b));
        assert_eq!(use_target(&mut sim, InteractTarget::Site(b)), Interaction::Install { building: b, part: 0 });
        assert_eq!(settlement(&sim).buildings[b as usize].needs(settlement(&sim).building_kind(b)), vec![(WOOD, 2), (STONE, 2)]);
        use_target(&mut sim, InteractTarget::Site(b));
        empty_bundle(&mut sim);
        walk_to_dock(&mut sim, TREE_STONE);
        use_target(&mut sim, InteractTarget::Depot(TREE_STONE));
        walk_to_building(&mut sim, b);
        use_target(&mut sim, InteractTarget::Site(b));
        assert_eq!(use_target(&mut sim, InteractTarget::Site(b)), Interaction::Install { building: b, part: 1 });
        assert!(settlement(&sim).buildings[b as usize].built());
        empty_bundle(&mut sim);
    }
    // Moss takes the free Wood Gatherer, the new one; Fern mans the
    // ballista.
    let (second, ballista) = (FIRST_SITE, FIRST_SITE + 1);
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: MOSS, job: SpriteJob::Staff(WOOD_GATHERER) })]);
    step(&mut sim, &[command(CommandKind::SetSpriteJob { agent: FERN, job: SpriteJob::Staff(BALLISTA) })]);
    until(&mut sim, 30 * HZ, |sim| settlement(sim).buildings[second as usize].output.count(WOOD) >= 1);
    until(&mut sim, 30 * HZ, |sim| settlement(sim).buildings[ballista as usize].slot == Slot::Occupied(super::Occupant::Sprite(FERN)));
    let s = settlement(&sim);
    assert_eq!(s.buildings[second as usize].slot, Slot::Occupied(super::Occupant::Sprite(MOSS)));
    assert_eq!((s.balances()["wood"].consumed, s.balances()["stone"].consumed), (8, 4));
    assert_eq!(s.depots[TREE_STONE as usize].inventory.count(STONE), dug - 4);
    assert!(s.balances().values().all(|b| b.conserved));
}
