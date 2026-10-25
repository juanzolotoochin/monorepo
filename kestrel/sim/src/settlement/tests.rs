//! Ported from the original game's tests (the retired `kestrel/core`).

use content::scenario::{Agent, CargoLayout, Depot, Equipment, HaulLink, Item, Producer, Scenario, Stack};
use content::{Content, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};

use super::{Phase, Settlement};
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
        skin: "worker".into(),
        position: p(-2.0, 1.0),
        speed: fx(2.0),
        radius: Some(fx(0.18)),
        capacity: 3,
        links: vec!["wood".into()],
        transfer_seconds: fx(0.1),
        cargo_anchor: [0.0, 0.5, -0.2],
        cargo_layout: CargoLayout::default(),
        equipment: None,
    }
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

fn world(scenario: &Scenario, obstacles: &[Disc]) -> Simulation {
    let world = World::new_settlement(1, &Content::builtin(), scenario, obstacles).unwrap();
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
    assert_eq!(a.cargo_capacity, 8);
    assert!(a.trailer.is_some());
    assert!(s.metrics.delivered > 10, "delivered {}", s.metrics.delivered);
    assert_eq!(s.balances()["cart"].equipped, 1);
    assert_eq!(s.balances()["wood"].consumed, 2);
    let hitch = crate::nav::distance(a.position, a.trailer.as_ref().unwrap().position);
    assert!((hitch - fx(1.1)).abs() < fx(0.15), "trailer at {hitch}");
}

#[test]
fn disabling_links_drains_committed_jobs() {
    let mut sim = world(&scenario(), &[]);
    run(&mut sim, 1);
    sim.step(&[Command { player: PlayerId(0), kind: CommandKind::SetLink { link: 0, enabled: false } }]);
    run(&mut sim, 20);
    assert!(settlement(&sim).agents.iter().all(|a| a.assignment.is_none()));
    let delivered = settlement(&sim).metrics.delivered;
    assert!(delivered > 0);
    run(&mut sim, 5);
    assert_eq!(settlement(&sim).metrics.delivered, delivered);
}

#[test]
fn blocked_docks_are_rejected_and_unreachable_jobs_never_reserve_stock() {
    let mut config = scenario();
    config.depots[1].position = p(8.0, 8.0);
    let blocked = World::new_settlement(1, &Content::builtin(), &config, &[Disc { center: FxVec2::new(fx(8.0), fx(8.0)), radius: fx(0.1) }]);
    assert!(blocked.is_err());

    // A wall between the forest and the mill (the yard moves off it).
    let mut config = scenario();
    config.depots[3].position = p(-1.0, 2.0);
    let wall: Vec<Disc> = (-16..=16).map(|n| Disc { center: FxVec2::new(Fx::ZERO, fx(n as f64 * 0.5)), radius: fx(0.3) }).collect();
    let mut sim = world(&config, &wall);
    run(&mut sim, 3);
    let s = settlement(&sim);
    assert_eq!(s.metrics.picked, 0);
    assert!(s.agents.iter().all(|a| a.assignment.is_none()));
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
        assert!(crate::nav::distance(a.position, b.position) >= fx(0.36), "haulers overlapped at tick {}", sim.world().tick());
        s.validate().unwrap();
    }
    let s = settlement(&sim);
    assert!(s.stock("forest", "iron") >= 9, "forest iron {}", s.stock("forest", "iron"));
    assert!(s.stock("mill", "wood") >= 9, "mill wood {}", s.stock("mill", "wood"));
    assert!(s.agents.iter().map(|a| a.avoidance_steers).sum::<u64>() > 0);
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
    assert!(bramble.equipped && bramble.cargo_capacity == 9);
    assert!(s.agents.iter().filter(|a| !a.controlled).all(|a| a.phase != Phase::Attaching));
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
            for a in &s.agents {
                println!(
                    "  {:<12} {:?} pos=({:.2},{:.2}) route={} job={:?} waits={} traffic={} steers={} dock={:?}",
                    a.id, a.phase, a.position.x.to_f64(), a.position.y.to_f64(), a.route.len(), a.assignment.as_ref().map(|j| (j.link, j.units, j.loaded, j.unloaded)),
                    a.dock_wait_ticks, a.traffic_wait_ticks, a.avoidance_steers, a.departure_dock
                );
            }
            println!("  owners {:?}", s.dock_owner);
            let terrain = sim.world().terrain();
            for a in s.agents.iter().filter(|a| !a.controlled) {
                for &l in &a.links {
                    let link = &s.links[l as usize];
                    let size = super::haulers::job_size(a, link, &s.depots);
                    let to_from = a.plan(terrain, s.depots[link.from as usize].position, &[]).len();
                    let haul = crate::nav::path(terrain, s.depots[link.from as usize].position, s.depots[link.to as usize].position, a.radius()).len();
                    println!("  plan {:<12} {:<14} size={size} to_from={to_from} haul={haul} retries_at={}", a.id, link.id, a.retries);
                }
            }
            for p in &s.producers {
                println!("  producer {:<10} enabled={} remaining={} cycles={}", p.id, p.enabled, p.remaining, p.cycles);
            }
            for d in &s.depots {
                println!("  depot {:<12} stock={:?} out={:?} in={:?} free={}", d.id, d.stock, d.out, d.incoming, d.free());
            }
            for l in &s.links {
                let to = &s.depots[l.to as usize];
                let demand = l.target.saturating_sub(to.stock[l.item as usize] + to.incoming[l.item as usize]);
                println!("  link {:<14} avail={} free={} demand={}", l.id, s.depots[l.from as usize].available(l.item), to.free(), demand);
            }
        }
    }
}
