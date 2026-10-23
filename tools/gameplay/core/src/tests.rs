use crate::*;
fn stack(item: &str, units: u32) -> Stack {
    Stack {
        item: item.into(),
        units,
    }
}
fn depot(id: &str, position: [f32; 2], capacity: u32, stock: Vec<Stack>) -> Depot {
    Depot {
        id: id.into(),
        label: id.into(),
        position,
        capacity,
        stock,
        stock_anchor: [0., 0.08, -0.6],
        stock_layout: stock_layout(),
    }
}
fn agent(id: &str) -> Agent {
    Agent {
        controlled: false,
        id: id.into(),
        label: id.into(),
        skin: "worker".into(),
        position: [-2.0, 1.0],
        speed: 2.0,
        radius: 0.18,
        capacity: 3,
        links: vec!["wood".into()],
        transfer_seconds: 0.1,
        cargo_layout: CargoLayout::default(),
        cargo_anchor: [0.0, 0.5, -0.2],
        equipment: None,
    }
}
fn scenario() -> Scenario {
    Scenario {
        extent: 8.,
        id: "test".into(),
        items: ["wood", "iron", "arrow", "cart"]
            .iter()
            .map(|s| Item {
                id: (*s).into(),
                label: (*s).into(),
                skin: (*s).into(),
                scale: 0.2,
            })
            .collect(),
        depots: vec![
            depot("forest", [-3.0, 0.0], 64, vec![stack("wood", 32)]),
            depot("mill", [3.0, 0.0], 32, vec![]),
            depot(
                "factory",
                [-1.0, 2.0],
                16,
                vec![stack("wood", 2), stack("iron", 1)],
            ),
            depot("yard", [0.0, 2.0], 2, vec![]),
            depot("ammo", [3.0, 2.0], 32, vec![]),
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
    }
}
fn run(sim: &mut Simulation, seconds: u32, fps: u32) {
    for _ in 0..seconds * fps {
        sim.advance(1.0 / fps as f64).unwrap();
        sim.validate_state().unwrap();
    }
}

#[test]
fn melee_staggers_a_wounded_ghoul_then_it_resumes_advancing() {
    let mut config = scenario();
    let mut player = agent("player");
    player.controlled = true;
    player.links.clear();
    player.position = [0.0, 0.0];
    config.agents.push(player);
    config.waves.push(Wave {
        id: "ghouls".into(),
        position: [0.0, 6.0],
        target: [0.0, -6.0],
        skin: "ghoul".into(),
        first: 3600.0,
        every: 10.0,
        speed: 1.0,
        hp: 2,
        damage: 1,
        count: 1,
    });
    let mut sim = Simulation::new(config, vec![]).unwrap();
    sim.set_controlled_pose("player", [0.0, 0.0], 0.0).unwrap();
    sim.threats.insert(
        999,
        Threat {
            id: 999,
            wave: 0,
            position: [0.0, 1.1],
            heading: 0.0,
            travelled: 0.0,
            hp: 2,
            route: vec![[0.0, -6.0]],
            stagger_until: 0,
            knockback: [0.0, 0.0],
        },
    );
    assert_eq!(sim.strike("player").unwrap(), 1);
    assert_eq!(sim.threats[&999].hp, 1);
    assert_eq!(sim.metrics.melee_hits, 1);
    run(&mut sim, 1, 60);
    let advanced = sim.threats[&999].position[1];
    assert!(sim.metrics.knockback_meters > 0.3);
    assert!(
        advanced < 1.5,
        "wounded ghoul should resume approaching: {advanced}"
    );
    sim.set_controlled_pose("player", [0.0, advanced - 1.0], 0.0)
        .unwrap();
    assert_eq!(sim.strike("player").unwrap(), 1);
    assert!(!sim.threats.contains_key(&999));
    assert_eq!(sim.metrics.melee_kills, 1);
}
#[test]
fn reservations_and_in_flight_units_prevent_overbooking_and_preserve_every_item() {
    let mut config = scenario();
    config.depots[1].capacity = 4;
    config.links[0].target = 32;
    let mut sim = Simulation::new(config, vec![]).unwrap();
    let mut saw_flight = false;
    for _ in 0..40 * 60 {
        sim.advance(1.0 / 60.0).unwrap();
        sim.validate_state().unwrap();
        saw_flight |= !sim.transfers.is_empty();
    }
    assert!(saw_flight);
    assert_eq!(sim.depots["mill"].units("wood"), 4);
    assert_eq!(sim.metrics.delivered, 4);
    assert_eq!(sim.balances()["wood"].supplied, 34);
    assert!(sim.add_stock("mill", "wood", 1).is_err());
}
#[test]
fn fixed_step_results_do_not_depend_on_render_frame_rate_and_pause_is_lossless() {
    let mut a = Simulation::new(scenario(), vec![]).unwrap();
    let mut b = a.clone();
    run(&mut a, 30, 20);
    run(&mut b, 30, 60);
    assert_eq!(a.tick, b.tick);
    assert_eq!(a.metrics.delivered, b.metrics.delivered);
    assert_eq!(a.depots["mill"].items, b.depots["mill"].items);
    for (id, agent) in &a.agents {
        assert_eq!(agent.position, b.agents[id].position);
    }
    let tick = a.tick;
    a.paused = true;
    run(&mut a, 2, 30);
    assert_eq!(a.tick, tick);
    a.paused = false;
    a.set_rate(2.0).unwrap();
    a.advance(0.5).unwrap();
    assert_eq!(a.tick, tick + 60);
    assert!(a.advance(f64::NAN).is_err());
    assert!(a.set_rate(8.0).is_err());
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
        radius: 0.7,
        speed_factor: 0.9,
        offset: [0.0, 0.0, -1.1],
        cargo_layout: CargoLayout::default(),
        cargo_anchor: [0.0, 0.6, 0.0],
    });
    config.producers.push(Producer {
        description: String::new(),
        radius: 0.,
        id: "factory".into(),
        label: "Factory".into(),
        position: [-1.0, 2.0],
        skin: "factory".into(),
        input: "factory".into(),
        output: "yard".into(),
        ingredients: vec![stack("wood", 2), stack("iron", 1)],
        products: vec![stack("cart", 1)],
        seconds: 0.3,
        max_cycles: Some(1),
    });
    let mut sim = Simulation::new(
        config,
        vec![navigation::Disc {
            center: [0.0, 0.0],
            radius: 0.7,
        }],
    )
    .unwrap();
    run(&mut sim, 45, 30);
    assert_eq!(sim.metrics.equipped, 1);
    assert_eq!(sim.producers["factory"].cycles, 1);
    assert_eq!(sim.agents["a"].cargo.capacity, 8);
    assert!(sim.agents["a"].trailer.is_some());
    assert!(sim.metrics.delivered > 10);
    assert_eq!(sim.balances()["cart"].equipped, 1);
    assert_eq!(sim.balances()["wood"].consumed, 2);
    let a = &sim.agents["a"];
    assert!(
        (navigation::distance(a.position, a.trailer.as_ref().unwrap().position) - 1.1).abs() < 0.15
    );
}
#[test]
fn processing_reserves_output_space_and_defense_cannot_fire_free_ammunition() {
    let mut config = scenario();
    config.agents.clear();
    config.producers.push(Producer {
        description: String::new(),
        radius: 0.,
        id: "mill".into(),
        label: "Mill".into(),
        position: [0.0, 0.0],
        skin: "mill".into(),
        input: "forest".into(),
        output: "ammo".into(),
        ingredients: vec![stack("wood", 1)],
        products: vec![stack("arrow", 4)],
        seconds: 0.2,
        max_cycles: None,
    });
    config.depots[4].capacity = 4;
    config.defenders.push(Defender {
        description: String::new(),
        label: String::new(),
        radius: 0.,
        id: "turret".into(),
        position: [3.0, 2.0],
        skin: "turret".into(),
        depot: "ammo".into(),
        ammo: "arrow".into(),
        range: 6.0,
        period: 0.3,
        damage: 1,
        projectile_speed: 12.0,
    });
    config.waves.push(Wave {
        id: "ghouls".into(),
        position: [7.0, 2.0],
        target: [0.0, 2.0],
        skin: "ghoul".into(),
        first: 0.0,
        every: 1.5,
        speed: 0.5,
        hp: 2,
        damage: 1,
        count: 8,
    });
    let mut sim = Simulation::new(config, vec![]).unwrap();
    sim.set_machine("mill", false).unwrap();
    run(&mut sim, 2, 30);
    assert_eq!(sim.metrics.shots, 0);
    sim.set_machine("mill", true).unwrap();
    run(&mut sim, 20, 30);
    assert!(sim.metrics.kills >= 6);
    assert!(sim.health > 0);
    assert_eq!(sim.accounts["arrow"].spent, sim.metrics.shots);
    assert!(sim.metrics.cycles > 1);
}
#[test]
fn disabling_links_drains_committed_jobs_and_unreachable_jobs_never_reserve_stock() {
    let mut sim = Simulation::new(scenario(), vec![]).unwrap();
    run(&mut sim, 1, 30);
    sim.set_link("wood", false).unwrap();
    run(&mut sim, 20, 30);
    assert!(sim.agents.values().all(|a| a.assignment.is_none()));
    let delivered = sim.metrics.delivered;
    run(&mut sim, 5, 30);
    assert_eq!(sim.metrics.delivered, delivered);
    let mut config = scenario();
    config.depots[1].position = [8.0, 8.0];
    let mut sim = Simulation::new(
        config,
        vec![navigation::Disc {
            center: [8.0, 8.0],
            radius: 0.1,
        }],
    );
    assert!(sim.is_err());
    let mut config = scenario();
    config.depots[3].position = [-1.0, 2.0];
    let mut obstacles = vec![];
    for n in -16..=16 {
        obstacles.push(navigation::Disc {
            center: [0.0, n as f32 * 0.5],
            radius: 0.3,
        });
    }
    sim = Simulation::new(config, obstacles);
    let mut sim = sim.unwrap();
    run(&mut sim, 3, 30);
    assert_eq!(sim.metrics.picked, 0);
    assert!(sim.agents.values().all(|a| a.assignment.is_none()));
}
#[test]
fn scenario_validation_rejects_unknown_references_and_duplicate_inventory_entries() {
    let mut config = scenario();
    config.links[0].item = "unobtainium".into();
    assert!(config.validate().is_err());
    config = scenario();
    config.depots[0].stock.push(stack("wood", 1));
    assert!(config.validate().is_err());
    config = scenario();
    config.agents[0].speed = f32::NAN;
    assert!(config.validate().is_err());
}

#[test]
fn controlled_transfers_share_reservations_and_cancellation_preserves_airborne_units() {
    let mut config = scenario();
    let mut player = agent("player");
    player.controlled = true;
    player.links.clear();
    player.capacity = 8;
    config.agents.push(player);
    let mut sim = Simulation::new(config, vec![]).unwrap();
    // Determine fixture IDs from the authored haul link.
    let depot = sim.config().links[0].from.clone();
    let item = sim.config().links[0].item.clone();
    let p = sim
        .config()
        .depots
        .iter()
        .find(|d| d.id == depot)
        .unwrap()
        .position;
    sim.set_controlled_pose("player", p, 0.).unwrap();
    let available = sim.transfer_limit("player", &depot, &item, false).min(3);
    assert!(available > 0);
    sim.transfer("player", &depot, &item, available, false)
        .unwrap();
    assert!(sim.transfer("player", &depot, &item, 1, false).is_err());
    sim.advance(1. / 60.).unwrap();
    assert!(sim
        .transfers
        .values()
        .any(|t| matches!(&t.to,Endpoint::Cargo(id) if id=="player")));
    sim.cancel_transfer("player").unwrap();
    for _ in 0..30 {
        sim.advance(1. / 60.).unwrap();
        sim.validate_state().unwrap();
    }
    assert_eq!(sim.agents["player"].cargo.units(&item), 1);
    sim.transfer("player", &depot, &item, 1, true).unwrap();
    for _ in 0..30 {
        sim.advance(1. / 60.).unwrap();
        sim.validate_state().unwrap();
    }
    assert_eq!(sim.agents["player"].cargo.total(), 0);
}

#[test]
fn larger_scenes_route_around_obstacles_without_expanding_legacy_bounds() {
    use navigation::{path_with_bounds, Disc};
    let obstacle = Disc {
        center: [9., 0.],
        radius: 0.9,
    };
    assert!(navigation::path([7., 0.], [11., 0.], &[obstacle], 0.2).is_empty());
    let route = path_with_bounds([7., 0.], [11., 0.], &[obstacle], 0.2, 12., |_, _| true);
    assert!(route.len() > 1);
    assert_eq!(route.last(), Some(&[11., 0.]));
    assert!(route.iter().flatten().all(|v| v.abs() <= 12.));
    assert!(path_with_bounds([0., 0.], [25., 0.], &[], 0.2, 12., |_, _| true).is_empty());
}

#[test]
fn opposing_haulers_pass_without_body_overlap_and_clear_the_docks() {
    let mut config = scenario();
    config.agents.truncate(2);
    config.agents[0].position = [-2., 0.];
    config.agents[1].position = [2., 0.];
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
    let mut sim = Simulation::new(config, vec![]).unwrap();
    for _ in 0..3600 {
        sim.advance(1. / 60.).unwrap();
        let a = &sim.agents["a"];
        let b = &sim.agents["b"];
        assert!(
            navigation::distance(a.position, b.position) >= 0.36,
            "haulers overlapped at tick {}",
            sim.tick
        );
        sim.validate_state().unwrap();
    }
    assert!(sim.depots["forest"].units("iron") >= 9);
    assert!(sim.depots["mill"].units("wood") >= 9);
    assert!(sim.metrics.avoidance_steers > 0);
}

#[test]
fn cargo_layout_validation_rejects_invalid_divisors_and_nonfinite_offsets() {
    let mut config = scenario();
    config.agents[0].cargo_layout.columns = 0;
    assert!(config.validate().is_err());
    config.agents[0].cargo_layout = CargoLayout::default();
    config.depots[0].stock_anchor[0] = f32::NAN;
    assert!(config.validate().is_err());
    let layout = CargoLayout::default();
    let offsets: Vec<_> = (0..8).map(|i| layout.offset(i)).collect();
    for (i, p) in offsets.iter().enumerate() {
        assert!(p[0].abs() <= 0.1 && p[2].abs() <= 0.1);
        assert!(!offsets[..i].contains(p));
    }
}

#[test]
fn cart_reversal_uses_a_forward_arc_and_keeps_the_animal_out_of_the_trailer() {
    let hitch = 1.85;
    let mut body = [0., 0.];
    let mut trailer = [0., -hitch];
    let route =
        navigation::articulated_path(body, [0., -6.], 0., trailer, hitch, &[], 0.75, 0.7, 12.);
    assert!(route.len() > 10, "reversal must include a turn");
    assert!(route[0][1] > 0., "start forward, never through the cart");
    for next in route {
        let distance = navigation::distance(body, next);
        let steps = (distance / 0.02).ceil().max(1.) as u32;
        let start = body;
        for n in 1..=steps {
            let f = n as f32 / steps as f32;
            body = [
                start[0] + (next[0] - start[0]) * f,
                start[1] + (next[1] - start[1]) * f,
            ];
            let d = navigation::distance(body, trailer);
            if d > hitch {
                trailer = [
                    body[0] + (trailer[0] - body[0]) * hitch / d,
                    body[1] + (trailer[1] - body[1]) * hitch / d,
                ];
            }
            assert!(
                navigation::distance(body, trailer) >= 1.45,
                "animal clipped its cart"
            );
        }
    }
    assert_eq!(body, [0., -6.]);
}
