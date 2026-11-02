use content::Content;
use sim::scenario::Scenario;
use sim::Simulation;

use crate::{dump_ron, read, read_header, write, LoadError, FORMAT_VERSION};

const TOTAL: u64 = 240;

/// Saving at tick `at`, loading and continuing gives the same hashes as never stopping.
fn save_and_continue(at: u64) {
    let content = Content::builtin();
    let scenario = Scenario::small();
    let uninterrupted = scenario.run(TOTAL, 4);

    let mut first = scenario.start(4);
    scenario.run_from(&mut first, at);
    let bytes = write(first.world(), &content);

    let mut resumed = Simulation::from_world(read(&bytes, &content).unwrap(), 4);
    resumed.world().check_invariants();
    let rest = scenario.run_from(&mut resumed, TOTAL - at);
    for (expected, actual) in uninterrupted[at as usize..].iter().zip(&rest) {
        if let Some(difference) = expected.first_difference(actual) {
            panic!("save at {at}: first difference at {difference}");
        }
    }
}

#[test]
fn save_at_tick_zero_and_continue() {
    save_and_continue(0);
}

#[test]
fn save_mid_game_and_continue() {
    save_and_continue(137);
}

#[test]
fn saving_a_loaded_world_gives_the_same_bytes() {
    let content = Content::builtin();
    let scenario = Scenario::small();
    let mut sim = scenario.start(2);
    scenario.run_from(&mut sim, 60);
    let bytes = write(sim.world(), &content);
    let again = write(&read(&bytes, &content).unwrap(), &content);
    assert_eq!(bytes, again);

    let header = read_header(&bytes).unwrap();
    assert_eq!(header.tick, 60);
    assert_eq!(header.rules_hash, content.rules_hash);
}

#[test]
fn other_format_versions_are_refused() {
    let content = Content::builtin();
    let mut bytes = write(Scenario::small().start(1).world(), &content);
    bytes[4..8].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
    assert_eq!(
        read(&bytes, &content).unwrap_err(),
        LoadError::FormatVersion { found: FORMAT_VERSION + 1, expected: FORMAT_VERSION }
    );
    assert_eq!(read(b"nonsense", &content).unwrap_err(), LoadError::NotASave);
}

/// Adding a definition shifts indices; loading remaps them by name.
#[test]
fn content_indices_are_remapped_by_name() {
    let builtin = Content::builtin();
    let scenario = Scenario::small();
    let mut sim = scenario.start(2);
    scenario.run_from(&mut sim, 30);
    let bytes = write(sim.world(), &builtin);

    let extra = "(sim: (radius: 0.25, max_hp: 5, speed: 3, spawn_weight: 1), view: (color: (1, 1, 1)))";
    let mut sources = content::builtin_sources();
    sources.push(("mover/aardvark.ron".into(), extra.into()));
    let changed = Content::from_sources(sources).unwrap();
    assert_eq!(changed.names.movers[0], "mover/aardvark");

    let loaded = read(&bytes, &changed).unwrap();
    loaded.check_invariants();
    let names_before: Vec<&str> = sim
        .world()
        .chunks()
        .iter()
        .flat_map(|c| c.movers.kind.iter().map(|&k| builtin.names.movers[k as usize].as_str()))
        .collect();
    let names_after: Vec<&str> = loaded
        .chunks()
        .iter()
        .flat_map(|c| c.movers.kind.iter().map(|&k| changed.names.movers[k as usize].as_str()))
        .collect();
    assert_eq!(names_before, names_after);

    // And the remapped world keeps running.
    let mut resumed = Simulation::from_world(loaded, 2);
    resumed.step(&[]);
    resumed.world().check_invariants();
}

#[test]
fn missing_content_is_named() {
    let builtin = Content::builtin();
    let bytes = write(Scenario::small().start(1).world(), &builtin);
    let only_small = Content::from_sources([(
        "mover/small.ron".to_string(),
        "(sim: (radius: 0.5, max_hp: 20, speed: 7.5, spawn_weight: 6), view: (color: (1, 0, 0)))".to_string(),
    )])
    .unwrap();
    assert_eq!(read(&bytes, &only_small).unwrap_err(), LoadError::MissingContent { name: "mover/large".into() });
}

#[test]
fn dumps_print_as_ron() {
    let content = Content::builtin();
    let scenario = Scenario::small();
    let mut sim = scenario.start(1);
    scenario.run_from(&mut sim, 5);
    let dump = dump_ron(&write(sim.world(), &content)).unwrap();
    assert!(dump.contains("// header"));
    assert!(dump.contains("// chunk/63"));
    assert!(dump.contains("mover/small"));
    // Positions print as plain numbers, not raw bits.
    let pos = dump.find("pos: FxVec3(").expect("a position");
    let x = dump[pos..].find("x: ").map(|i| pos + i + 3).unwrap();
    assert!(dump[x..x + 12].contains('.'), "{}", &dump[pos..pos + 80]);
}

/// Prints the save size for the 100k-mover world. Run with `--test_arg=--ignored`.
#[test]
#[ignore]
fn large_save_size() {
    let content = Content::builtin();
    let scenario = Scenario::large();
    let mut sim = scenario.start(8);
    scenario.run_from(&mut sim, 10);
    let bytes = write(sim.world(), &content);
    println!("{} movers: {} KiB", sim.world().mover_count(), bytes.len() / 1024);
}

/// A settlement saved mid-haul continues exactly: agents, reservations,
/// docks and items in flight all come back.
#[test]
fn settlement_save_and_continue() {
    let content = Content::builtin();
    let text = include_str!("../../app/assets/levels/forest_supply.ron");
    let scenario = content::scenario::Scenario::parse(text).unwrap();
    let start = || sim::World::new_settlement(5, &content, &scenario, &[]).unwrap();
    let mut uninterrupted = Simulation::from_world(start(), 2);
    let expected: Vec<_> = (0..600).map(|_| uninterrupted.step(&[])).collect();

    let mut first = Simulation::from_world(start(), 2);
    for _ in 0..317 {
        first.step(&[]);
    }
    assert!(!first.world().settlement().unwrap().transfers.is_empty() || first.world().settlement().unwrap().agents.iter().any(|a| a.hauler().unwrap().assignment.is_some()));
    let bytes = write(first.world(), &content);
    let mut resumed = Simulation::from_world(read(&bytes, &content).unwrap(), 2);
    for expected in &expected[317..] {
        let actual = resumed.step(&[]);
        if let Some(difference) = expected.first_difference(&actual) {
            panic!("first difference at {difference}");
        }
    }
}

/// Character references are remapped by name, and a save that needs a
/// character the content lacks is refused.
#[test]
fn settlement_characters_remap_by_name() {
    let content = Content::builtin();
    let text = include_str!("../../app/assets/levels/forest_supply.ron");
    let scenario = content::scenario::Scenario::parse(text).unwrap();
    let mut running = Simulation::from_world(sim::World::new_settlement(5, &content, &scenario, &[]).unwrap(), 2);
    for _ in 0..2_000 {
        if !running.world().settlement().unwrap().threats.is_empty() {
            break;
        }
        running.step(&[]);
    }
    let world = running.world();
    assert!(!world.settlement().unwrap().threats.is_empty(), "a threat is saved");
    let bytes = write(world, &content);
    // Waves' characters, then threats'.
    let character_names = |world: &sim::World, content: &Content| -> Vec<String> {
        let s = world.settlement().unwrap();
        let name = |ix: u16| content.names.characters[ix as usize].clone();
        s.waves.iter().map(|w| name(w.character)).chain(s.threats.iter().map(|t| name(t.character))).collect()
    };
    let saved = character_names(world, &content);

    // A character sorting first shifts every index by one.
    let mut sources = content::builtin_sources();
    let first = "(base: \"ghoul\", sim: (), view: ())";
    sources.push(("character/aaa.ron".to_string(), first.to_string()));
    let shifted = Content::from_sources(sources).unwrap();
    let loaded = read(&bytes, &shifted).unwrap();
    assert_eq!(character_names(&loaded, &shifted), saved);
    assert_eq!(loaded.settlement().unwrap().characters, shifted.rules.characters, "rules restored on load");

    // Without the forest ghoul, and the Sacred Tree's, which builds on it.
    let gone = ["character/ghoul_forest.ron", "character/ghoul_sacred_tree.ron"];
    let without: Vec<_> = content::builtin_sources().into_iter().filter(|(path, _)| !gone.contains(&path.as_str())).collect();
    let missing = Content::from_sources(without).unwrap();
    assert!(matches!(read(&bytes, &missing), Err(LoadError::MissingContent { name }) if name == "character/ghoul_forest"));
}

/// A Sacred Tree saved while wood flies to a site, a holder since format
/// 26 (`InventoryRef::Site`, in the elder's carry job and the units in the
/// air), continues exactly.
#[test]
fn a_town_saved_mid_delivery_continues() {
    use fx::{Fx, FxVec2};
    use sim::settlement::avatar::Intent;
    use sim::settlement::{InteractTarget, InventoryRef};
    use sim::{Command, CommandKind, PlayerId};
    let content = Content::builtin();
    let text = include_str!("../../app/assets/levels/sacred_tree_supply.ron");
    let scenario = content::scenario::Scenario::parse(text).unwrap();
    // The tree's trunk, as the scene's architecture blocks it.
    let trunk = [sim::terrain::Disc { center: FxVec2::ZERO, radius: Fx::ONE }];
    let start = || Simulation::from_world(sim::World::new_settlement(5, &content, &scenario, &trunk).unwrap(), 2);
    let probe = start();
    let (center, dock) = {
        let s = probe.world().settlement().unwrap();
        (probe.world().terrain().center, s.depots[0].position)
    };
    let site = FxVec2::new(center.x + Fx::from_int(5), center.y);
    let command = |kind| Command { player: PlayerId(0), kind };
    let walk = |target| command(CommandKind::MoveAvatar { agent: 0, intent: Intent::GoTo { target, run: true } });
    // A Stone Gatherer east of the tree, five wood in the tree's wood pile;
    // the elder takes it to the site and delivers.
    let turn = |sim: &Simulation, tick: u64| -> Vec<Command> {
        match tick {
            0 => vec![command(CommandKind::PlaceBlueprint { kind: 1, position: site }), command(CommandKind::AddStock { depot: 0, item: 0, units: 5 }), walk(dock)],
            150 => vec![command(CommandKind::Interact { agent: 0, target: InteractTarget::Depot(0) })],
            300 => {
                let b = &sim.world().settlement().unwrap().buildings[1];
                vec![walk(b.output_position + (b.output_position - b.position).normalize_or_zero() * Fx::from_ratio(2, 5))]
            }
            700 => vec![command(CommandKind::Interact { agent: 0, target: InteractTarget::Site(1) })],
            _ => vec![],
        }
    };
    let mut uninterrupted = start();
    let expected: Vec<_> = (0..900).map(|tick| {
        let commands = turn(&uninterrupted, tick);
        uninterrupted.step(&commands)
    }).collect();
    assert_eq!(uninterrupted.world().settlement().unwrap().buildings[1].site().unwrap().1.count(0), 2, "delivered");

    let mut first = start();
    let mut tick = 0;
    loop {
        let commands = turn(&first, tick);
        first.step(&commands);
        tick += 1;
        let s = first.world().settlement().unwrap();
        if s.transfers.values().any(|t| t.to == InventoryRef::Site(1)) && s.agents[0].carry.job.is_some() {
            break;
        }
        assert!(tick < 900, "no delivery in flight");
    }
    let bytes = write(first.world(), &content);
    let mut resumed = Simulation::from_world(read(&bytes, &content).unwrap(), 2);
    for expected in &expected[tick as usize..] {
        let commands = turn(&resumed, tick);
        let actual = resumed.step(&commands);
        if let Some(difference) = expected.first_difference(&actual) {
            panic!("first difference at {difference}");
        }
        tick += 1;
    }
}

/// A Sacred Tree saved while a dropped bundle flies onto its new ground
/// pile, which a town's pile lab holds since format 27 (`PileLab::town`),
/// continues exactly.
#[test]
fn a_town_saved_mid_drop_continues() {
    use fx::{Fx, FxVec2};
    use sim::settlement::avatar::Intent;
    use sim::settlement::{InteractTarget, InventoryRef};
    use sim::{Command, CommandKind, PlayerId};
    let content = Content::builtin();
    let text = include_str!("../../app/assets/levels/sacred_tree_supply.ron");
    let scenario = content::scenario::Scenario::parse(text).unwrap();
    // The tree's trunk, as the scene's architecture blocks it.
    let trunk = [sim::terrain::Disc { center: FxVec2::ZERO, radius: Fx::ONE }];
    let start = || Simulation::from_world(sim::World::new_settlement(5, &content, &scenario, &trunk).unwrap(), 2);
    let probe = start();
    let (center, dock) = {
        let s = probe.world().settlement().unwrap();
        (probe.world().terrain().center, s.depots[0].position)
    };
    let open = FxVec2::new(center.x + Fx::from_int(3), center.y - Fx::from_int(4));
    let command = |kind| Command { player: PlayerId(0), kind };
    let walk = |target| command(CommandKind::MoveAvatar { agent: 0, intent: Intent::GoTo { target, run: true } });
    // Five wood from the tree's wood pile, dropped in the open.
    let turn = |tick: u64| -> Vec<Command> {
        match tick {
            0 => vec![command(CommandKind::AddStock { depot: 0, item: 0, units: 5 }), walk(dock)],
            150 => vec![command(CommandKind::Interact { agent: 0, target: InteractTarget::Depot(0) })],
            300 => vec![walk(open)],
            600 => vec![command(CommandKind::Drop { agent: 0 })],
            _ => vec![],
        }
    };
    let mut uninterrupted = start();
    let expected: Vec<_> = (0..800).map(|tick| uninterrupted.step(&turn(tick))).collect();
    let piles = &uninterrupted.world().settlement().unwrap().piles.as_ref().unwrap().piles;
    assert_eq!(piles.values().map(|p| p.units()).collect::<Vec<_>>(), vec![5], "dropped");

    let mut first = start();
    let mut tick = 0;
    loop {
        first.step(&turn(tick));
        tick += 1;
        let s = first.world().settlement().unwrap();
        if s.transfers.values().any(|t| matches!(t.to, InventoryRef::Pile(_))) && s.agents[0].carry.job.is_some() {
            break;
        }
        assert!(tick < 800, "no drop in flight");
    }
    let bytes = write(first.world(), &content);
    let mut resumed = Simulation::from_world(read(&bytes, &content).unwrap(), 2);
    for expected in &expected[tick as usize..] {
        let actual = resumed.step(&turn(tick));
        if let Some(difference) = expected.first_difference(&actual) {
            panic!("first difference at {difference}");
        }
        tick += 1;
    }
}

/// A Sacred Tree saved while the elder possesses the Wood Gatherer, a seat
/// since format 28 (`AvatarState::seat`), with the sprite it put out
/// waiting beside it, continues exactly: the elder leaves and the sprite
/// goes back in as it would have.
#[test]
fn a_town_saved_while_possessed_continues() {
    use fx::{Fx, FxVec2};
    use sim::settlement::avatar::Intent;
    use sim::settlement::{Hold, Seat, SpriteJob};
    use sim::{Command, CommandKind, PlayerId};
    let content = Content::builtin();
    let text = include_str!("../../app/assets/levels/sacred_tree_supply.ron");
    let scenario = content::scenario::Scenario::parse(text).unwrap();
    // The tree's trunk, as the scene's architecture blocks it.
    let trunk = [sim::terrain::Disc { center: FxVec2::ZERO, radius: Fx::ONE }];
    let start = || Simulation::from_world(sim::World::new_settlement(5, &content, &scenario, &trunk).unwrap(), 2);
    let probe = start();
    let leave = {
        let s = probe.world().settlement().unwrap();
        s.leave_spot(0, s.agents[0].radius(), probe.world().terrain()).unwrap()
    };
    let command = |kind| Command { player: PlayerId(0), kind };
    // Ash staffs the Wood Gatherer; the elder walks up, possesses it,
    // and leaves later.
    let turn = |tick: u64| -> Vec<Command> {
        match tick {
            0 => vec![
                command(CommandKind::SetSpriteJob { agent: 1, job: SpriteJob::Staff(0) }),
                command(CommandKind::MoveAvatar { agent: 0, intent: Intent::GoTo { target: leave, run: false } }),
            ],
            900 => vec![command(CommandKind::Board { agent: 0 })],
            1500 => vec![command(CommandKind::Board { agent: 0 })],
            _ => vec![],
        }
    };
    let mut uninterrupted = start();
    let expected: Vec<_> = (0..2400).map(|tick| uninterrupted.step(&turn(tick))).collect();
    let s = uninterrupted.world().settlement().unwrap();
    assert_eq!((s.seat(0), s.agents[1].sprite().unwrap().holds), (None, Some(Hold::Inside(0))), "left, and Ash is back in");

    let mut first = start();
    for tick in 0..1200 {
        first.step(&turn(tick));
    }
    let s = first.world().settlement().unwrap();
    assert_eq!((s.seat(0), s.agents[1].sprite().unwrap().holds), (Some(Seat::Building(0)), None), "possessed, Ash put out");
    let bytes = write(first.world(), &content);
    let mut resumed = Simulation::from_world(read(&bytes, &content).unwrap(), 2);
    for (tick, expected) in expected.iter().enumerate().skip(1200) {
        let actual = resumed.step(&turn(tick as u64));
        if let Some(difference) = expected.first_difference(&actual) {
            panic!("first difference at {difference}");
        }
    }
}

/// A Sacred Tree saved while the elder's Burst cools down, its ready tick
/// kept since format 29 (`AvatarState::burst_ready`), continues exactly:
/// boarding again, a Burst before the cooldown ends is refused and one
/// after it works, as they would have.
#[test]
fn a_town_saved_while_a_burst_cools_down_continues() {
    use fx::{Fx, FxVec2};
    use sim::settlement::avatar::Intent;
    use sim::settlement::Seat;
    use sim::{Command, CommandKind, PlayerId};
    let content = Content::builtin();
    let text = include_str!("../../app/assets/levels/sacred_tree_supply.ron");
    let scenario = content::scenario::Scenario::parse(text).unwrap();
    // The tree's trunk, as the scene's architecture blocks it.
    let trunk = [sim::terrain::Disc { center: FxVec2::ZERO, radius: Fx::ONE }];
    let start = || Simulation::from_world(sim::World::new_settlement(5, &content, &scenario, &trunk).unwrap(), 2);
    let probe = start();
    let leave = {
        let s = probe.world().settlement().unwrap();
        s.leave_spot(0, s.agents[0].radius(), probe.world().terrain()).unwrap()
    };
    let cooldown = probe.world().settlement().unwrap().building_kind(0).burst_cooldown;
    let command = |kind| Command { player: PlayerId(0), kind };
    let (board, burst) = (CommandKind::Board { agent: 0 }, CommandKind::Burst { agent: 0 });
    // The elder walks up, possesses the Wood Gatherer, bursts and leaves;
    // back in, it bursts too early and then once the cooldown is over.
    let first_burst = 960;
    let turn = |tick: u64| -> Vec<Command> {
        match tick {
            0 => vec![command(CommandKind::MoveAvatar { agent: 0, intent: Intent::GoTo { target: leave, run: false } })],
            900 | 1200 | 2400 => vec![command(board.clone())],
            960 | 2500 => vec![command(burst.clone())],
            t if t == first_burst + cooldown + 10 => vec![command(burst.clone())],
            _ => vec![],
        }
    };
    let total = first_burst + cooldown + 60;
    let mut uninterrupted = start();
    let expected: Vec<_> = (0..total).map(|tick| uninterrupted.step(&turn(tick))).collect();
    let s = uninterrupted.world().settlement().unwrap();
    assert_eq!(s.agents[0].avatar().unwrap().burst_ready, first_burst + 2 * cooldown + 10, "the second Burst went off");

    let mut first = start();
    for tick in 0..1500 {
        first.step(&turn(tick));
    }
    let s = first.world().settlement().unwrap();
    assert_eq!((s.seat(0), s.agents[0].avatar().unwrap().burst_ready), (None, first_burst + cooldown), "left, cooling down");
    let bytes = write(first.world(), &content);
    let mut resumed = Simulation::from_world(read(&bytes, &content).unwrap(), 2);
    for (tick, expected) in expected.iter().enumerate().skip(1500) {
        let actual = resumed.step(&turn(tick as u64));
        if let Some(difference) = expected.first_difference(&actual) {
            panic!("first difference at {difference}");
        }
    }
    assert_eq!(resumed.world().settlement().unwrap().seat(0), Some(Seat::Building(0)));
}

/// A Sacred Tree saved while a manned ballista shoots, a shot in the air
/// and its turret reloading (format 30: `BuildingState::turret` and the
/// `building_fire` queue), continues exactly: the elder loads it with
/// stone, a sprite mans it, and it shoots the ghouls walking past.
#[test]
fn a_town_saved_while_a_ballista_shoots_continues() {
    use fx::{Fx, FxVec2};
    use sim::settlement::avatar::Intent;
    use sim::settlement::{InteractTarget, SpriteJob};
    use sim::{Command, CommandKind, PlayerId};
    let content = Content::builtin();
    let text = include_str!("../../app/assets/levels/sacred_tree_supply.ron");
    let mut scenario = content::scenario::Scenario::parse(text).unwrap();
    let at = |x: i64, y: i64| [Fx::from_ratio(x, 10), Fx::from_ratio(y, 10)];
    scenario.buildings.push(content::scenario::Building { kind: "ballista".into(), position: at(50, 6) });
    scenario.depots[1].stock.push(content::scenario::Stack { item: "stone".into(), units: 5 });
    // Only this test's ghouls, none of the map's own waves.
    scenario.waves.clear();
    scenario.waves.push(content::scenario::Wave {
        id: "east".into(),
        position: at(85, 6),
        target: at(20, 0),
        character: "ghoul_forest".into(),
        first: Fx::from_int(25),
        every: Fx::from_int(4),
        count: 2,
    });
    // The tree's trunk, as the scene's architecture blocks it.
    let trunk = [sim::terrain::Disc { center: FxVec2::ZERO, radius: Fx::ONE }];
    let start = || Simulation::from_world(sim::World::new_settlement(5, &content, &scenario, &trunk).unwrap(), 2);
    let probe = start();
    let (dock, stand, away) = {
        let s = probe.world().settlement().unwrap();
        let b = &s.buildings[1];
        let center = probe.world().terrain().center;
        (s.depots[1].position, b.output_position + (b.output_position - b.position).normalize_or_zero() * Fx::from_ratio(2, 5), center + FxVec2::new(Fx::ZERO, Fx::from_int(6)))
    };
    let hz = content::TICKS_PER_SECOND as u64;
    let command = |kind| Command { player: PlayerId(0), kind };
    let go = |target| command(CommandKind::MoveAvatar { agent: 0, intent: Intent::GoTo { target, run: true } });
    let interact = |target| command(CommandKind::Interact { agent: 0, target });
    // The elder takes the stone to the ballista, and steps away; Ash mans it.
    let turn = |tick: u64| -> Vec<Command> {
        match tick {
            0 => vec![go(dock), command(CommandKind::SetSpriteJob { agent: 1, job: SpriteJob::Staff(2) })],
            t if t == 4 * hz => vec![interact(InteractTarget::Depot(1))],
            t if t == 7 * hz => vec![go(stand)],
            t if t == 13 * hz => vec![interact(InteractTarget::Output(1))],
            t if t == 17 * hz => vec![go(away)],
            _ => vec![],
        }
    };
    let total = 45 * hz;
    let mut uninterrupted = start();
    let expected: Vec<_> = (0..total).map(|tick| uninterrupted.step(&turn(tick))).collect();
    let s = uninterrupted.world().settlement().unwrap();
    assert!(s.combat.shots >= 3, "{} shots", s.combat.shots);
    assert_eq!(s.accounts[1].spent, s.combat.shots, "a stone a shot");

    // Save on a tick a shot is in the air.
    let mut first = start();
    let mut tick = 0;
    loop {
        first.step(&turn(tick));
        tick += 1;
        let s = first.world().settlement().unwrap();
        if s.combat.shots == 2 && !s.projectiles.is_empty() {
            break;
        }
        assert!(tick < total, "never shot twice");
    }
    let bytes = write(first.world(), &content);
    let mut resumed = Simulation::from_world(read(&bytes, &content).unwrap(), 2);
    for (t, expected) in expected.iter().enumerate().skip(tick as usize) {
        let actual = resumed.step(&turn(t as u64));
        if let Some(difference) = expected.first_difference(&actual) {
            panic!("first difference at {difference}");
        }
    }
}
