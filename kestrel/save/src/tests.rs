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
    // A Wood Gatherer east of the tree, five wood in the tree's wood pile;
    // the elder takes it to the site and delivers.
    let turn = |sim: &Simulation, tick: u64| -> Vec<Command> {
        match tick {
            0 => vec![command(CommandKind::PlaceBlueprint { kind: 0, position: site }), command(CommandKind::AddStock { depot: 0, item: 0, units: 5 }), walk(dock)],
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
        nights: Vec::new(),
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
            0 => vec![go(dock), command(CommandKind::SetSpriteJob { agent: 1, job: SpriteJob::Staff(1) })],
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

/// A sloped scene with its own depth limit (format 34: `Terrain::sloped`,
/// `Terrain::max_layers` and `MineSpec::layers`) loads with them, and its
/// mine keeps as many layers.
#[test]
fn a_sloped_scene_keeps_its_slopes_and_depth() {
    use fx::Fx;
    let content = Content::builtin();
    let setup = sim::settlement::SceneSetup {
        player: "exosuit".into(),
        extent: Fx::from_int(16),
        diggable: true,
        sloped: true,
        dig_layers: Some(6),
        mine: Some(7),
        ..Default::default()
    };
    let world = sim::World::new_scene(5, &content, &setup).unwrap();
    let loaded = read(&write(&world, &content), &content).unwrap();
    let terrain = loaded.terrain();
    assert!(terrain.sloped);
    assert_eq!(terrain.max_layers, 6);
    assert_eq!(terrain.mine.unwrap().layers, 6);
    assert_eq!(terrain, world.terrain());
}

/// A town's mine (format 35: `MineSpec::veins`, `Terrain::undiggable`)
/// loads with its stone veins and its undiggable tiles.
#[test]
fn a_town_mine_keeps_its_stone_and_undiggable_ground() {
    use fx::Fx;
    let content = Content::builtin();
    let text = include_str!("../../app/assets/levels/sacred_tree_supply.ron");
    let setup = sim::settlement::SceneSetup {
        scenario: Some(content::scenario::Scenario::parse(text).unwrap()),
        player: "elder".into(),
        diggable: true,
        sloped: true,
        dig_layers: Some(6),
        mine: Some(7),
        spawn: [Fx::ZERO, Fx::ZERO, Fx::from_int(3)],
        ..Default::default()
    };
    let mut world = sim::World::new_scene(5, &content, &setup).unwrap();
    let mut terrain = world.terrain().clone();
    terrain.undiggable.push(((-2, -2), (2, 2)));
    world.set_terrain(terrain);
    let loaded = read(&write(&world, &content), &content).unwrap();
    let terrain = loaded.terrain();
    assert_eq!(terrain.mine.unwrap().veins, sim::mine::Veins::Stone);
    assert!(terrain.is_undiggable((1, -2)) && !terrain.is_undiggable((3, 0)));
    assert_eq!(terrain, world.terrain());
}

/// A town's mine with its hill (format 36: `MineSpec::hill`) loads with the
/// hill, its ground and its undiggable plateau and ramps.
#[test]
fn a_town_mine_keeps_its_hill() {
    use fx::{Fx, FxVec2};
    let content = Content::builtin();
    let text = include_str!("../../app/assets/levels/sacred_tree_supply.ron");
    let hill = sim::mine::Hill { center: (0, 0), plateau: 15, steps: 8, ramp_width: 7, ramp_length: 8, foot: 10 };
    let setup = sim::settlement::SceneSetup {
        scenario: Some(content::scenario::Scenario::parse(text).unwrap()),
        player: "elder".into(),
        diggable: true,
        sloped: true,
        dig_layers: Some(6),
        mine: Some(7),
        hill: Some(hill),
        spawn: [Fx::ZERO, Fx::ZERO, Fx::from_int(3)],
        ..Default::default()
    };
    let world = sim::World::new_scene(5, &content, &setup).unwrap();
    let loaded = read(&write(&world, &content), &content).unwrap();
    let terrain = loaded.terrain();
    let center = terrain.center;
    assert_eq!(terrain.mine.unwrap().hill.unwrap().plateau, 15);
    assert_eq!(terrain.height(center), Fx::ZERO, "the plateau");
    assert_eq!(terrain.height(center + FxVec2::new(Fx::ZERO, Fx::from_int(20))), -Fx::from_int(2), "its foot");
    assert!(terrain.is_undiggable(sim::terrain::Terrain::pit_tile(center)));
    assert_eq!(terrain, world.terrain());
}

/// A scene with a volume (format 40: `Terrain::volume`) loads with its
/// spans and ore, and its floors over each other.
#[test]
fn a_volume_scene_keeps_its_cells_and_ore() {
    use fx::Fx;
    let content = Content::builtin();
    let mut volume = sim::volume::Volume::default();
    // Ground to 1 m, a bridge deck from 3 m to 4 m, ore in the deck.
    volume.fill_box((-3, -3), (3, 3), 0, 2);
    volume.fill_box((-1, -3), (1, 3), 6, 8);
    volume.set_ore((0, 0), 7, Some(sim::mine::BLUE));
    let setup = sim::settlement::SceneSetup {
        player: "exosuit".into(),
        extent: Fx::from_int(16),
        sloped: true,
        volume: Some(volume),
        ..Default::default()
    };
    let mut world = sim::World::new_scene(5, &content, &setup).unwrap();
    let mut terrain = world.terrain().clone();
    let center = sim::terrain::Terrain::pit_tile(terrain.center);
    let tunnel = (center.0 + 1, center.1);
    assert!(terrain.clear_cell(tunnel, 1));
    world.set_terrain(terrain);
    let bytes = write(&world, &content);
    let loaded = read(&bytes, &content).unwrap();
    let terrain = loaded.terrain();
    let volume = terrain.volume.as_ref().unwrap();
    assert_eq!(volume.ore((center.0, center.1), 7), Some(sim::mine::BLUE));
    assert!(!volume.is_solid(tunnel, 1) && volume.is_solid(tunnel, 0));
    assert_eq!(volume.floors(center).collect::<Vec<_>>(), [2, 8]);
    assert_eq!(terrain, world.terrain());
    assert_eq!(sim::hash::hash_of(terrain), sim::hash::hash_of(world.terrain()));
    assert_eq!(write(&loaded, &content), bytes, "saved again, the same bytes");
}

/// A volume dig in progress keeps its first layer (format 41), and a
/// pit's dig keeps none.
#[test]
fn a_volume_dig_in_progress_keeps_its_layer() {
    use fx::Fx;
    let content = Content::builtin();
    let mut volume = sim::volume::Volume::default();
    volume.fill_box((-8, -8), (8, 8), -8, 0);
    let setup = sim::settlement::SceneSetup { player: "exosuit".into(), extent: Fx::from_int(8), diggable: true, sloped: true, volume: Some(volume), ..Default::default() };
    let mut sim = sim::Simulation::from_world(sim::World::new_scene(5, &content, &setup).unwrap(), 1);
    let agent = sim.world().settlement().unwrap().player().unwrap();
    sim.step(&[sim::Command { player: sim::PlayerId(0), kind: sim::CommandKind::DigFront { agent, direction: sim::volume::DigDirection::Down } }]);
    let dig = sim.world().settlement().unwrap().agents[agent as usize].avatar().unwrap().dig.clone().expect("a dig started");
    assert_eq!(dig.layer, Some(-1));
    let bytes = write(sim.world(), &content);
    let loaded = read(&bytes, &content).unwrap();
    assert_eq!(loaded.settlement().unwrap().agents[agent as usize].avatar().unwrap().dig, Some(dig));
    assert_eq!(write(&loaded, &content), bytes);
}

/// A volume scene keeps a sprite's feet (format 42): the test walker
/// standing on a block 1 m up, which continues alike after loading.
#[test]
fn a_volume_scene_keeps_a_sprites_height() {
    use fx::Fx;
    let content = Content::builtin();
    let scenario = content::scenario::Scenario::parse(
        r#"(id: "walk", extent: 8.0, items: [(id: "ore", label: "ORE", skin: "ore", scale: 0.2)],
         depots: [], sources: [], links: [],
         agents: [(id: "player", label: "YOU", character: "exosuit", position: (0.0, 0.0), capacity: 8, links: [],
                   cargo_anchor: (0.0, 0.65, -0.19), equipment: None, controlled: true)],
         sprites: [(id: "walker", label: "WALKER", character: "sprite", position: (2.0, 0.0), follow: true)],
         producers: [], defenders: [], waves: [], base_health: 1, player_health: 0)"#,
    )
    .unwrap();
    let mut volume = sim::volume::Volume::default();
    volume.fill_box((-8, -8), (8, 8), -8, 0);
    volume.fill_box((1, -3), (5, 3), 0, 2);
    let setup = sim::settlement::SceneSetup {
        scenario: Some(scenario),
        player: "exosuit".into(),
        extent: Fx::from_int(8),
        diggable: true,
        sloped: true,
        volume: Some(volume),
        piles: Some(sim::settlement::PileSetup { capacity: 12, ..Default::default() }),
        ..Default::default()
    };
    let mut sim = sim::Simulation::from_world(sim::World::new_scene(5, &content, &setup).unwrap(), 1);
    let walker = |w: &sim::World| w.settlement().unwrap().agents.iter().find(|a| a.id == "walker").unwrap().sprite().unwrap().height;
    assert_eq!(walker(sim.world()), Fx::ONE, "on the block");
    let bytes = write(sim.world(), &content);
    let loaded = read(&bytes, &content).unwrap();
    assert_eq!(walker(&loaded), Fx::ONE);
    assert_eq!(write(&loaded, &content), bytes);
    let mut again = Simulation::from_world(loaded, 1);
    for _ in 0..60 {
        assert_eq!(sim.step(&[]).total, again.step(&[]).total);
    }
}

/// A volume scene keeps a hauler's and a threat's feet (format 43, M8.2):
/// a worker and a Sacred Tree ghoul standing on a block 1 m up, which
/// continue alike after loading.
#[test]
fn a_volume_scene_keeps_haulers_and_threats_heights() {
    use fx::Fx;
    let content = Content::builtin();
    let scenario = content::scenario::Scenario::parse(
        r#"(id: "floors", extent: 8.0, items: [(id: "wood", label: "WOOD", skin: "item_lumber", scale: 0.45)],
         depots: [(id: "a", label: "A", position: (-6.0, -2.0), capacity: 4, stock: []),
                  (id: "b", label: "B", position: (-6.0, 2.0), capacity: 4, stock: [])],
         sources: [], links: [(id: "idle", from: "a", to: "b", item: "wood", target: 1, priority: 1)],
         agents: [(id: "player", label: "YOU", character: "exosuit", position: (0.0, 0.0), capacity: 8, links: [],
                   cargo_anchor: (0.0, 0.65, -0.19), equipment: None, controlled: true),
                  (id: "worker", label: "WORKER", character: "worker", position: (2.0, 2.0), capacity: 4, links: ["idle"],
                   cargo_anchor: (0.0, 0.65, -0.19), equipment: None)],
         producers: [], defenders: [],
         waves: [(id: "w", position: (4.0, -2.0), target: (-6.0, -6.0), character: "ghoul_sacred_tree", first: 0.1, every: 1.0, count: 1)],
         base_health: 5, player_health: 0)"#,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let mut volume = sim::volume::Volume::default();
    volume.fill_box((-8, -8), (8, 8), -8, 0);
    volume.fill_box((1, -3), (5, 3), 0, 2);
    let setup = sim::settlement::SceneSetup {
        scenario: Some(scenario),
        player: "exosuit".into(),
        extent: Fx::from_int(8),
        diggable: true,
        sloped: true,
        volume: Some(volume),
        spawn: [Fx::from_int(-6), Fx::ZERO, Fx::from_int(6)],
        ..Default::default()
    };
    let mut sim = Simulation::from_world(sim::World::new_scene(5, &content, &setup).unwrap(), 1);
    for _ in 0..12 {
        sim.step(&[]);
    }
    let heights = |w: &sim::World| {
        let s = w.settlement().unwrap();
        let worker = s.agents.iter().find(|a| a.id == "worker").unwrap().hauler().unwrap().height;
        (worker, s.threats.iter().map(|t| t.height).collect::<Vec<_>>())
    };
    assert_eq!(heights(sim.world()), (Fx::ONE, vec![Fx::ONE]), "both on the block");
    let bytes = write(sim.world(), &content);
    let loaded = read(&bytes, &content).unwrap();
    assert_eq!(heights(&loaded), heights(sim.world()));
    assert_eq!(write(&loaded, &content), bytes);
    let mut again = Simulation::from_world(loaded, 1);
    for _ in 0..120 {
        assert_eq!(sim.step(&[]).total, again.step(&[]).total);
    }
}

/// The scene's `key: Some(...)` section's text, as `tunnel_lab_tests`
/// reads it.
fn section(text: &str, key: &str) -> String {
    let open = format!("{key}: Some(");
    let start = text.find(&open).unwrap_or_else(|| panic!("no {open} in the scene")) + open.len();
    let mut depth = 1;
    let mut chars = text[start..].char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '/' if chars.peek().is_some_and(|&(_, n)| n == '/') => {
                chars.by_ref().find(|&(_, n)| n == '\n');
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return text[start..start + at].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("{key} is not closed");
}

/// A town's volume, made from its mine with the mountain over it (format
/// 46, M8.8 of the Sacred Tree depths), is saved as its edits over the
/// generated ground and remade at load: the Sacred Tree with a few cells
/// dug and an ore taken loads with the same cells, hashes the same and
/// saves to the same bytes, in a few KB instead of the whole volume's
/// 1.5 MB.
#[test]
fn a_towns_volume_is_saved_as_its_edits() {
    use fx::{Fx, FxVec2};
    let content = Content::builtin();
    let scene = include_str!("../../app/assets/levels/sacred_tree.ron");
    let supply = include_str!("../../app/assets/levels/sacred_tree_supply.ron");
    let layout: sim::volume::VolumeLayout = ron::from_str(&section(scene, "volume")).unwrap();
    let setup = sim::settlement::SceneSetup {
        scenario: Some(content::scenario::Scenario::parse(supply).unwrap()),
        player: "elder".into(),
        obstacles: vec![sim::terrain::Disc { center: FxVec2::ZERO, radius: Fx::ONE }],
        diggable: true,
        sloped: true,
        dig_layers: Some(16),
        mine: Some(7),
        mine_volume: Some(layout),
        hill: Some(sim::mine::Hill { center: (0, 0), plateau: 15, steps: 8, ramp_width: 7, ramp_length: 8, foot: 10 }),
        spawn: [Fx::ZERO, Fx::ZERO, Fx::from_ratio(17, 5)],
        ..Default::default()
    };
    let mut world = sim::World::new_scene(5, &content, &setup).unwrap();
    let untouched = write(&world, &content);
    let whole = {
        let volume = postcard::to_allocvec(world.terrain().volume.as_ref().unwrap()).unwrap();
        ruzstd::encoding::compress_to_vec(volume.as_slice(), ruzstd::encoding::CompressionLevel::Fastest).len()
    };
    let mut terrain = world.terrain().clone();
    let center = sim::terrain::Terrain::pit_tile(terrain.center);
    // A stair's worth of cells north of the town, and an ore taken.
    let mut dug = 0;
    for step in 0..8 {
        let tile = (center.0 - 8 + step, center.1 - 22);
        let top = terrain.volume.as_ref().unwrap().top(tile).unwrap();
        for layer in top - 2 - step..top {
            dug += u32::from(terrain.clear_cell(tile, layer));
        }
    }
    let (cell, _) = terrain.volume.as_ref().unwrap().ore_cells().next().unwrap();
    terrain.volume.as_mut().unwrap().set_ore(cell.0, cell.1, None);
    world.set_terrain(terrain);
    let bytes = write(&world, &content);
    eprintln!("the town's save: {} bytes untouched, {} with {dug} cells dug; the whole volume alone {whole} bytes", untouched.len(), bytes.len());
    assert!(dug > 20);
    assert!(bytes.len() < 64 * 1024 && bytes.len() * 10 < whole, "{} bytes", bytes.len());
    let loaded = read(&bytes, &content).unwrap();
    assert_eq!(loaded.terrain(), world.terrain());
    assert_eq!(sim::hash::hash_of(loaded.terrain()), sim::hash::hash_of(world.terrain()));
    assert_eq!(loaded.terrain().volume.as_ref().unwrap().ore(cell.0, cell.1), None);
    assert_eq!(write(&loaded, &content), bytes, "saved again, the same bytes");
}

/// A crawler carrying a pilot, a gunner and a rider (format 48:
/// `Seat::Rider`; 50: `Seat::Gunner`) saves and loads with everyone in
/// their place, saves again to the same bytes, and walks on alike.
#[test]
fn a_crawler_saved_with_riders_aboard_continues() {
    use fx::{Fx, FxVec2};
    use sim::settlement::avatar::Intent;
    use sim::settlement::Seat;
    use sim::{Command, CommandKind, PlayerId, Simulation};
    let content = Content::builtin();
    let mut volume = sim::volume::Volume::default();
    volume.fill_box((-16, -16), (16, 16), -8, 0);
    let setup = sim::settlement::SceneSetup {
        player: "elder".into(),
        extent: Fx::from_int(16),
        sloped: true,
        volume: Some(volume),
        vehicles: vec![sim::settlement::VehicleSetup { character: "crawler".into(), position: [Fx::from_ratio(5, 2), Fx::ZERO], heading: Fx::ZERO }],
        ..Default::default()
    };
    let mut sim = Simulation::from_world(sim::World::new_scene(5, &content, &setup).unwrap(), 1);
    let guests = [PlayerId(1), PlayerId(2)];
    sim.step(&guests.map(|player| Command { player, kind: CommandKind::Join }));
    let s = sim.world().settlement().unwrap();
    let agents = [s.player().unwrap(), s.avatar_of(guests[0]).unwrap(), s.avatar_of(guests[1]).unwrap()];
    let center = sim.world().terrain().center;
    // Each walks up to the crawler's west side and climbs aboard.
    let go: Vec<_> = (0..3)
        .map(|i| {
            let target = center + FxVec2::new(Fx::from_ratio(1, 2), Fx::from_int(i as i32 - 1));
            Command { player: PlayerId(i as u32), kind: CommandKind::MoveAvatar { agent: agents[i], intent: Intent::GoTo { target, run: false } } }
        })
        .collect();
    sim.step(&go);
    for _ in 0..120 {
        sim.step(&[]);
    }
    sim.step(&(0..3).map(|i| Command { player: PlayerId(i as u32), kind: CommandKind::Board { agent: agents[i] } }).collect::<Vec<_>>());
    let s = sim.world().settlement().unwrap();
    let seats: Vec<_> = agents.iter().map(|&a| s.seat(a as usize)).collect();
    assert_eq!(seats[0], Some(Seat::Vehicle(0)), "the pilot");
    assert_eq!(seats[1], Some(Seat::Gunner { vehicle: 0 }), "the gunner (format 50)");
    assert!(seats[2].and_then(|s| s.place()).is_some(), "a rider: {seats:?}");
    sim.step(&[Command { player: PlayerId(0), kind: CommandKind::MoveAvatar { agent: agents[0], intent: Intent::Walk { direction: FxVec2::new(Fx::ZERO, Fx::ONE), run: false } } }]);
    for _ in 0..90 {
        sim.step(&[]);
    }
    let bytes = write(sim.world(), &content);
    let loaded = read(&bytes, &content).unwrap();
    let s = loaded.settlement().unwrap();
    assert_eq!(agents.iter().map(|&a| s.seat(a as usize)).collect::<Vec<_>>(), seats, "everyone in their place");
    assert_eq!(s.vehicles, sim.world().settlement().unwrap().vehicles);
    assert_eq!(write(&loaded, &content), bytes, "saved again, the same bytes");
    let mut resumed = Simulation::from_world(loaded, 1);
    for _ in 0..90 {
        assert_eq!(sim.step(&[]), resumed.step(&[]));
    }
}
