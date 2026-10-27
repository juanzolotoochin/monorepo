//! Ported from the original game's tests (the retired `kestrel/core`).

use content::scenario::{Agent, CargoLayout, Depot, Equipment, HaulLink, Item, Producer, Scenario, Stack};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::action::{self, Action};
use super::avatar::{character_of, Intent};
use super::haulers::{self, EquipmentDef, Trailer};
use super::{AgentIx, AgentState, Phase, Settlement};
use crate::messages::ActionMessages;
use crate::terrain::Disc;
use crate::{Command, CommandKind, PlayerId, Simulation, World};

fn fx(v: f64) -> Fx {
    Fx::from_bits((v * 4294967296.0).round() as i64)
}

fn p(x: f64, y: f64) -> [Fx; 2] {
    [fx(x), fx(y)]
}

fn stack(item: &str, units: u32) -> Stack {
    Stack { item: item.into(), units }
}

fn depot(id: &str, position: [Fx; 2], capacity: u32, stock: Vec<Stack>) -> Depot {
    Depot {
        id: id.into(),
        label: id.into(),
        position,
        capacity,
        stock,
        stock_anchor: [0.0, 0.08, -0.6],
        stock_layout: CargoLayout::depot(),
    }
}

fn agent(id: &str) -> Agent {
    Agent {
        controlled: false,
        id: id.into(),
        label: id.into(),
        character: "test_hauler".into(),
        position: p(-2.0, 1.0),
        capacity: 3,
        links: vec!["wood".into()],
        cargo_anchor: [0.0, 0.5, -0.2],
        cargo_layout: CargoLayout::default(),
        equipment: None,
    }
}

/// A player's avatar, at the center.
fn player() -> Agent {
    Agent { controlled: true, character: "test_player".into(), position: p(0.0, -2.0), capacity: 8, links: vec![], ..agent("player") }
}

fn scenario() -> Scenario {
    Scenario {
        extent: Fx::from_int(8),
        id: "test".into(),
        weapons: vec![],
        weapon_tuning: vec![],
        items: ["wood", "iron", "arrow", "cart"]
            .iter()
            .map(|s| Item { id: (*s).into(), label: (*s).into(), skin: (*s).into(), scale: 0.2 })
            .collect(),
        depots: vec![
            depot("forest", p(-3.0, 0.0), 64, vec![stack("wood", 32)]),
            depot("mill", p(3.0, 0.0), 32, vec![]),
            depot("factory", p(-1.0, 2.0), 16, vec![stack("wood", 2), stack("iron", 1)]),
            depot("yard", p(0.0, 2.0), 2, vec![]),
            depot("ammo", p(3.0, 2.0), 32, vec![]),
        ],
        sources: vec![],
        links: vec![HaulLink {
            id: "wood".into(),
            from: "forest".into(),
            to: "mill".into(),
            item: "wood".into(),
            target: 24,
            priority: 1,
        }],
        agents: vec![agent("a"), agent("b"), agent("c")],
        producers: vec![],
        defenders: vec![],
        waves: vec![],
        base_health: 10,
        player_health: 0,
    }
}

/// The builtin content plus the original fixtures' haulers.
fn test_content() -> Content {
    let mut sources = content::builtin_sources();
    sources.push(("character/test_hauler.ron".into(), super::combat_tests::TEST_HAULER.into()));
    sources.push(("character/test_player.ron".into(), super::combat_tests::TEST_PLAYER.into()));
    Content::from_sources(sources).unwrap()
}

fn world(scenario: &Scenario, obstacles: &[Disc]) -> Simulation {
    let world = World::new_settlement(1, &test_content(), scenario, obstacles).unwrap();
    Simulation::from_world(world, 4)
}

fn settlement(sim: &Simulation) -> &Settlement {
    sim.world().settlement().unwrap()
}

/// Runs whole seconds, checking every invariant after every tick.
fn run(sim: &mut Simulation, seconds: u64) {
    for _ in 0..seconds * TICKS_PER_SECOND as u64 {
        sim.step(&[]);
        settlement(sim).validate().unwrap();
    }
}

#[test]
fn reservations_and_in_flight_units_prevent_overbooking_and_preserve_every_item() {
    let mut config = scenario();
    config.depots[1].capacity = 4;
    config.links[0].target = 32;
    let mut sim = world(&config, &[]);
    let mut saw_flight = false;
    for _ in 0..40 * TICKS_PER_SECOND as u64 {
        sim.step(&[]);
        settlement(&sim).validate().unwrap();
        saw_flight |= !settlement(&sim).transfers.is_empty();
    }
    let s = settlement(&sim);
    assert!(saw_flight);
    assert_eq!(s.stock("mill", "wood"), 4);
    assert_eq!(s.metrics.delivered, 4);
    assert_eq!(s.balances()["wood"].supplied, 34);
}

#[test]
fn recipes_and_equipment_produce_a_cart_then_upgrade_real_transport_capacity() {
    let mut config = scenario();
    config.agents.truncate(1);
    config.agents[0].equipment = Some(Equipment {
        depot: "yard".into(),
        item: "cart".into(),
        skin: "cart".into(),
        capacity: 8,
        radius: fx(0.7),
        speed_factor: fx(0.9),
        offset: [Fx::ZERO, Fx::ZERO, fx(-1.1)],
        cargo_layout: CargoLayout::default(),
        cargo_anchor: [0.0, 0.6, 0.0],
    });
    config.producers.push(Producer {
        description: String::new(),
        radius: Fx::ZERO,
        id: "factory".into(),
        label: "Factory".into(),
        position: p(-1.0, 2.0),
        skin: "factory".into(),
        input: "factory".into(),
        output: "yard".into(),
        ingredients: vec![stack("wood", 2), stack("iron", 1)],
        products: vec![stack("cart", 1)],
        seconds: fx(0.3),
        max_cycles: Some(1),
    });
    let mut sim = world(&config, &[Disc { center: FxVec2::ZERO, radius: fx(0.7) }]);
    run(&mut sim, 45);
    let s = settlement(&sim);
    assert_eq!(s.metrics.equipped, 1);
    assert_eq!(s.producers[0].cycles, 1);
    let a = s.agent("a");
    assert_eq!(a.carry.inventory.capacity, 8);
    assert!(a.hauler().unwrap().trailer.is_some());
    assert!(s.metrics.delivered > 10, "delivered {}", s.metrics.delivered);
    assert_eq!(s.balances()["cart"].equipped, 1);
    assert_eq!(s.balances()["wood"].consumed, 2);
    let hitch = crate::nav::distance(a.body.position, a.hauler().unwrap().trailer.as_ref().unwrap().position);
    assert!((hitch - fx(1.1)).abs() < fx(0.15), "trailer at {hitch}");
}

#[test]
fn disabling_links_drains_committed_jobs() {
    let mut sim = world(&scenario(), &[]);
    run(&mut sim, 1);
    sim.step(&[Command { player: PlayerId(0), kind: CommandKind::SetLink { link: 0, enabled: false } }]);
    run(&mut sim, 20);
    assert!(settlement(&sim).agents.iter().all(|a| a.hauler().unwrap().assignment.is_none()));
    let delivered = settlement(&sim).metrics.delivered;
    assert!(delivered > 0);
    run(&mut sim, 5);
    assert_eq!(settlement(&sim).metrics.delivered, delivered);
}

#[test]
fn blocked_docks_are_rejected_and_unreachable_jobs_never_reserve_stock() {
    let mut config = scenario();
    config.depots[1].position = p(8.0, 8.0);
    let blocked = World::new_settlement(1, &test_content(), &config, &[Disc { center: FxVec2::new(fx(8.0), fx(8.0)), radius: fx(0.1) }]);
    assert!(blocked.is_err());

    // A wall between the forest and the mill (the yard moves off it).
    let mut config = scenario();
    config.depots[3].position = p(-1.0, 2.0);
    let wall: Vec<Disc> = (-16..=16).map(|n| Disc { center: FxVec2::new(Fx::ZERO, fx(n as f64 * 0.5)), radius: fx(0.3) }).collect();
    let mut sim = world(&config, &wall);
    run(&mut sim, 3);
    let s = settlement(&sim);
    assert_eq!(s.metrics.picked, 0);
    assert!(s.agents.iter().all(|a| a.hauler().unwrap().assignment.is_none()));
}

#[test]
fn opposing_haulers_pass_without_body_overlap_and_clear_the_docks() {
    let mut config = scenario();
    config.agents.truncate(2);
    config.agents[0].position = p(-2.0, 0.0);
    config.agents[1].position = p(2.0, 0.0);
    config.agents[1].links = vec!["return_iron".into()];
    config.depots[1].stock = vec![stack("iron", 12)];
    config.links.push(HaulLink {
        id: "return_iron".into(),
        from: "mill".into(),
        to: "forest".into(),
        item: "iron".into(),
        target: 12,
        priority: 1,
    });
    let mut sim = world(&config, &[]);
    for _ in 0..60 * TICKS_PER_SECOND as u64 {
        sim.step(&[]);
        let s = settlement(&sim);
        let (a, b) = (s.agent("a"), s.agent("b"));
        assert!(crate::nav::distance(a.body.position, b.body.position) >= fx(0.36), "haulers overlapped at tick {}", sim.world().tick());
        s.validate().unwrap();
    }
    let s = settlement(&sim);
    assert!(s.stock("forest", "iron") >= 9, "forest iron {}", s.stock("forest", "iron"));
    assert!(s.stock("mill", "wood") >= 9, "mill wood {}", s.stock("mill", "wood"));
    assert!(s.agents.iter().map(|a| a.hauler().unwrap().avoidance_steers).sum::<u64>() > 0);
}

/// Runs a hauler's brain (`haulers::think`) on `a` as stage 2 would on the
/// next tick, reading the settlement as it is now.
fn think(sim: &Simulation, a: &mut AgentState, me: AgentIx) -> Option<Action> {
    let (s, terrain, tick) = (settlement(sim), sim.world().terrain(), sim.world().tick());
    let footprints: Vec<_> = s.agents.iter().map(haulers::footprint).collect();
    let ctx = haulers::Ctx {
        tick,
        characters: &s.characters,
        terrain,
        piles: s.piles.as_ref(),
        footprints: &footprints,
        dock_owner: &s.dock_owner,
        depots: &s.depots,
        links: &s.links,
        transfers: &s.transfers,
        view: s.view(me, tick, terrain),
    };
    haulers::think(a, me, &ctx)
}

fn go_to(target: FxVec2) -> Option<Action> {
    Some(Action::Move(Intent::GoTo { target, run: false }))
}

/// The brain picks an action and changes its phase, but leaves the route to
/// `act`.
#[test]
fn hauler_brain_picks_actions_without_moving() {
    let mut sim = world(&scenario(), &[]);
    let me = settlement(&sim).agent_index("a").unwrap();

    // Idle at home: nothing to do. Away from home: go back.
    let mut a = settlement(&sim).agents[me as usize].clone();
    a.hauler_mut().unwrap().phase = Phase::Idle;
    assert_eq!(think(&sim, &mut a, me), None);
    assert_eq!(a.hauler().unwrap().phase, Phase::Idle);
    a.body.position = a.body.home + FxVec2::new(Fx::ONE, Fx::ZERO);
    let route = a.body.route.clone();
    assert_eq!(think(&sim, &mut a, me), go_to(a.body.home));
    assert_eq!(a.hauler().unwrap().phase, Phase::Returning);
    assert_eq!(a.body.route, route);

    // A hauler whose load has landed sets off for the drop dock.
    let mill = settlement(&sim).depots[settlement(&sim).depot_index("mill").unwrap() as usize].position;
    for _ in 0..30 * TICKS_PER_SECOND as u64 {
        sim.step(&[]);
        let s = settlement(&sim);
        let Some(me) = s.agents.iter().position(|a| {
            a.hauler().is_some_and(|h| h.phase == Phase::Loading) && a.carry.job.is_none() && a.carry.inventory.total_in() == 0
        }) else {
            continue;
        };
        let mut a = s.agents[me].clone();
        let route = a.body.route.clone();
        assert_eq!(think(&sim, &mut a, me as AgentIx), go_to(mill));
        assert_eq!(a.hauler().unwrap().phase, Phase::ToDrop);
        assert_eq!(a.body.route, route, "think leaves the route to act");
        return;
    }
    panic!("no hauler finished loading");
}

/// `GoTo` plans as the hauler always has (around its trailer when towing),
/// `Follow` takes the route as given, and an avatar ignores `Follow`.
#[test]
fn haulers_act_on_go_to_and_follow() {
    let mut config = scenario();
    config.agents.push(player());
    let sim = world(&config, &[Disc { center: FxVec2::ZERO, radius: fx(0.7) }]);
    let (s, terrain) = (settlement(&sim), sim.world().terrain());
    let me = s.agent_index("a").unwrap();
    let view = s.view(me, sim.world().tick(), terrain);
    let mill = s.depots[s.depot_index("mill").unwrap() as usize].position;
    let go = Action::Move(Intent::GoTo { target: mill, run: false });

    let a = s.agents[me as usize].clone();
    let mut hauler = a.hauler().unwrap().clone();
    let mut body = a.body.clone();
    haulers::act(&mut body, &hauler, &go, &view);
    assert!(!body.route.is_empty());
    assert_eq!(body.route, hauler.plan(&a.body, terrain, mill, &[]));

    // Towing a cart.
    hauler.equipment = Some(EquipmentDef { depot: 3, item: 3, capacity: 8, speed_factor: fx(0.9), radius: fx(0.7), hitch: fx(1.1) });
    hauler.equipped = true;
    let (position, heading) = hauler.trailer_spot(&a.body, terrain);
    hauler.trailer = Some(Trailer { position, heading, travelled: Fx::ZERO });
    let mut body = a.body.clone();
    body.travel_heading = heading;
    let towed = hauler.plan(&body, terrain, mill, &[]);
    haulers::act(&mut body, &hauler, &go, &view);
    assert!(!body.route.is_empty());
    assert_eq!(body.route, towed);

    let route = vec![a.body.position + FxVec2::new(fx(1.0), fx(1.0)), mill];
    haulers::act(&mut body, &hauler, &Action::Follow(route.clone()), &view);
    assert_eq!(body.route, route);

    let player = s.agent_index("player").unwrap();
    let mut a = s.agents[player as usize].clone();
    assert!(a.avatar().is_some());
    let before = a.clone();
    let mut out = ActionMessages::default();
    let def = character_of(&s.characters, &before);
    action::act(&mut a, &Action::Follow(route), &s.view(player, sim.world().tick(), terrain), def, &mut out);
    assert_eq!(a, before);
    assert!(out.is_empty());
}

/// A command naming a hauler changes nothing: commands drive only avatars.
#[test]
fn commands_never_drive_haulers() {
    let hashes = |command: bool| {
        let mut sim = world(&scenario(), &[]);
        run(&mut sim, 1);
        if command {
            let terrain = sim.world().terrain().clone();
            let tick = sim.world().tick();
            let s = sim.world_mut().settlement_mut();
            let hauler = s.agent_index("a").unwrap();
            let goal = s.depots[s.depot_index("yard").unwrap() as usize].position;
            let (agent, action) = Action::of(&CommandKind::MoveAvatar { agent: hauler, intent: Intent::GoTo { target: goal, run: false } }).unwrap();
            // Past the guard, the action would send the hauler elsewhere.
            let mut unguarded = s.clone();
            let mut out = ActionMessages::default();
            unguarded.act_agent(tick, agent, &action, &terrain, &mut out);
            assert_ne!(unguarded.agents[hauler as usize].body.route, s.agents[hauler as usize].body.route);
            s.act(tick, agent, &action, &terrain, &mut out);
            assert!(out.is_empty());
        }
        (0..5 * TICKS_PER_SECOND as u64).map(|_| sim.step(&[]).total).collect::<Vec<_>>()
    };
    assert_eq!(hashes(true), hashes(false));
}

pub(super) const FOREST: &str = include_str!("../../../app/assets/levels/forest_supply.ron");

/// The forest outpost's supply chain runs for five minutes with every
/// invariant holding: haulers deliver, the cartworks builds the cart, the ox
/// takes it, and the mill keeps making arrows.
#[test]
fn forest_supply_runs_and_its_balances_hold() {
    let scenario = Scenario::parse(FOREST).unwrap();
    let mut sim = world(&scenario, &[]);
    run(&mut sim, 300);
    let s = settlement(&sim);
    assert!(s.metrics.delivered > 100, "delivered {}", s.metrics.delivered);
    assert_eq!(s.producers[s.producer_index("cartworks").unwrap() as usize].cycles, 1);
    assert!(s.producers[s.producer_index("sawmill").unwrap() as usize].cycles > 3);
    let bramble = s.agent("bramble");
    assert!(bramble.hauler().unwrap().equipped && bramble.carry.inventory.capacity == 9);
    assert!(s.agents.iter().filter_map(|a| a.hauler()).all(|h| h.phase != Phase::Attaching));
}

#[test]
fn settlements_are_deterministic_across_thread_counts() {
    let scenario = Scenario::parse(FOREST).unwrap();
    let run = |threads| {
        let world = World::new_settlement(3, &Content::builtin(), &scenario, &[]).unwrap();
        let mut sim = Simulation::from_world(world, threads);
        (0..30 * TICKS_PER_SECOND as u64).map(|_| sim.step(&[]).total).collect::<Vec<_>>()
    };
    assert_eq!(run(1), run(8));
}

/// Prints the forest scenario's progress per minute. Run with `--ignored --nocapture`.
#[test]
#[ignore]
fn forest_probe() {
    let scenario = Scenario::parse(FOREST).unwrap();
    let mut sim = world(&scenario, &[]);
    for second in 1..=300u64 {
        run(&mut sim, 1);
        if second % 30 == 0 {
            let s = settlement(&sim);
            println!(
                "t={second}s delivered={} picked={} cycles={} equipped={} assignments={} retries={}",
                s.metrics.delivered, s.metrics.picked, s.metrics.cycles, s.metrics.equipped, s.metrics.assignments, s.metrics.assignment_retries
            );
            for (a, h) in s.agents.iter().filter_map(|a| Some((a, a.hauler()?))) {
                println!(
                    "  {:<12} {:?} pos=({:.2},{:.2}) route={} job={:?} waits={} traffic={} steers={} dock={:?}",
                    a.id, h.phase, a.body.position.x.to_f64(), a.body.position.y.to_f64(), a.body.route.len(), h.assignment.as_ref().map(|j| (j.link, a.carry.job.as_ref().map(|c| (c.direction, c.remaining)))),
                    h.dock_wait_ticks, h.traffic_wait_ticks, h.avoidance_steers, h.departure_dock
                );
            }
            println!("  owners {:?}", s.dock_owner);
            let terrain = sim.world().terrain();
            for (a, h) in s.agents.iter().filter_map(|a| Some((a, a.hauler()?))) {
                for &l in &h.links {
                    let link = &s.links[l as usize];
                    let size = super::haulers::job_size(a, link, &s.depots);
                    let to_from = h.plan(&a.body, terrain, s.depots[link.from as usize].position, &[]).len();
                    let haul = crate::nav::path(terrain, s.depots[link.from as usize].position, s.depots[link.to as usize].position, a.radius()).len();
                    println!("  plan {:<12} {:<14} size={size} to_from={to_from} haul={haul} retries_at={}", a.id, link.id, h.retries);
                }
            }
            for p in &s.producers {
                println!("  producer {:<10} enabled={} remaining={} cycles={}", p.id, p.enabled, p.remaining, p.cycles);
            }
            for d in &s.depots {
                println!("  depot {:<12} stock={:?} out={:?} in={:?} free={}", d.id, d.inventory.counts, d.inventory.reserved_out, d.inventory.reserved_in, d.inventory.free());
            }
            for l in &s.links {
                let to = &s.depots[l.to as usize];
                let demand = l.target.saturating_sub(to.inventory.count(l.item) + to.inventory.reserved_in[l.item as usize]);
                println!("  link {:<14} avail={} free={} demand={}", l.id, s.depots[l.from as usize].inventory.available(l.item), to.inventory.free(), demand);
            }
        }
    }
}
