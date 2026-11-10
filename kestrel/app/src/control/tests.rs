use super::protocol::{Command, PileCheck};
use super::*;
fn run(steps: Vec<Command>) -> Value {
    run_at(steps, 30)
}
fn run_at(steps: Vec<Command>, fps: u32) -> Value {
    run_for(steps, fps, 1200)
}
fn run_for(steps: Vec<Command>, fps: u32, max_frames: u32) -> Value {
    let script = Script {
        version: 1,
        scene: "pile-lab".into(),
        fps,
        width: 1440,
        height: 960,
        max_frames,
        steps,
    };
    let options = Options {
        simulation: true,
        script: Some(script),
        ..default()
    };
    let scenes = SceneLibrary::new();
    let hub = Hub::new(options.clone()).unwrap();
    let mut app = crate::test_app(options, scenes, hub, crate::flow::Launch::single_player(2));
    for _ in 0..max_frames + 100 {
        app.update();
        let world = app.world_mut();
        let p = player_position(world);
        let view = world.get_resource::<crate::scene_view::SceneView>();
        if let Some(view) = view.filter(|view| view.index == 3) {
            assert!(
                crate::jetpack::height(&view.pack, p).is_none_or(|y| p.y >= y - 0.025),
                "player entered terrain: {p:?}"
            );
        }
        let hub = app.world().resource::<Hub>();
        if !hub.latest.is_null() && !hub.latest["totals"].is_null() {
            assert_eq!(
                hub.latest["airborne"].as_array().unwrap().len() as u64,
                hub.latest["airborne_units"].as_u64().unwrap()
            );
            assert_eq!(
                hub.latest["visible_mineral_units"].as_u64(),
                hub.latest["totals"]["created"].as_u64()
            );
        }
        if hub.finished.is_some() || hub.failed.is_some() {
            return hub.report(0);
        }
    }
    panic!("script failed to finish");
}
#[test]
fn command_script_transports_exactly_four_units_and_exposes_world_state() {
    let report = run(vec![
        Command::Add {
            dispenser: 0,
            units: 8,
        },
        Command::Walk {
            position: [-1.0, 0.0],
        },
        Command::Grab {
            pile: 0,
            units: 4,
            material: Some(voxel_assets::Mineral::AmberOre),
            seconds_per_unit: 0.65,
            approach: Some([-2.3, -2.8]),
        },
        Command::Walk {
            position: [2.0, 1.0],
        },
        Command::DropAll {
            position: [4.0, 4.5],
            seconds_per_unit: 0.65,
            approach: Some([2.9, 5.0]),
        },
        Command::Assert {
            cargo_units: Some(0),
            total_units: Some(104),
            piles: vec![PileCheck { id: 0, units: 4 }, PileCheck { id: 5, units: 4 }],
        },
    ]);
    assert_eq!(report["status"], "passed");
    assert_eq!(
        report["receipts"][2]["state"]["player"]["cargo"]["units"],
        4
    );
    assert_eq!(report["state"]["visible_mineral_units"], 104);
    assert_eq!(report["state"]["totals"]["conserved"], true);
    assert_eq!(report["receipts"].as_array().unwrap().len(), 6);
}
#[test]
fn invalid_exact_pickups_fail_without_mutating_quantities_or_running_later_steps() {
    let report = run(vec![
        Command::Add {
            dispenser: 0,
            units: 3,
        },
        Command::Grab {
            pile: 0,
            units: 4,
            material: None,
            seconds_per_unit: 0.0,
            approach: None,
        },
        Command::Add {
            dispenser: 0,
            units: 10,
        },
    ]);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["state"]["player"]["cargo"]["units"], 0);
    assert_eq!(report["state"]["piles"][0]["units"], 3);
    assert_eq!(report["receipts"].as_array().unwrap().len(), 2);
}
#[test]
fn inspection_works_in_other_scenes_and_commands_validate_bounds() {
    let report = run(vec![
        Command::Scene {
            scene: "boulder-lab".into(),
        },
        Command::Inspect,
        Command::Walk {
            position: [99.0, 0.0],
        },
    ]);
    assert_eq!(report["status"], "failed");
    assert_eq!(
        report["receipts"][1]["state"]["targets"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    assert!(report["state"]["player"]["cargo"].is_null());
}

#[test]
fn jetpack_script_climbs_crosses_chasm_hovers_safely_and_lands() {
    for fps in [20, 30, 60] {
        let report = run_at(
            vec![
                Command::Scene {
                    scene: "jetpack-lab".into(),
                },
                Command::Jetpack { enabled: true },
                Command::Walk {
                    position: [0.0, 2.0],
                },
                Command::Wait { frames: fps },
                Command::Walk {
                    position: [2.0, 2.0],
                },
                Command::Jetpack { enabled: false },
                Command::Wait { frames: fps },
                Command::Inspect,
                Command::Jetpack { enabled: true },
                Command::Walk {
                    position: [5.0, 1.0],
                },
                Command::Wait { frames: fps },
                Command::Jetpack { enabled: false },
                Command::Wait { frames: fps * 2 },
            ],
            fps,
        );
        assert_eq!(report["status"], "passed", "{report}");
        let receipts = report["receipts"].as_array().unwrap();
        let top = receipts[3]["state"]["player"]["position"][1]
            .as_f64()
            .unwrap();
        assert!((top - 3.8).abs() < 0.05, "{top}");
        let gap = &receipts[6]["state"]["player"];
        assert_eq!(gap["jetpack"]["requested"], false);
        assert_eq!(gap["jetpack"]["airborne"], true);
        assert!(gap["position"][1].as_f64().unwrap() > 3.7);
        let final_player = &report["state"]["player"];
        assert_eq!(final_player["jetpack"]["airborne"], false);
        assert!((final_player["position"][1].as_f64().unwrap() - 3.5).abs() < 0.01);
    }
}

#[test]
fn space_lifts_and_releases_and_scene_switch_restores_deck() {
    use bevy::prelude::*;
    let options = Options {
        simulation: true,
        ..default()
    };
    let scenes = SceneLibrary::new();
    let hub = Hub::new(options.clone()).unwrap();
    let mut app = crate::test_app(options, scenes, hub, crate::flow::Launch::single_player(3));
    crate::start_world(&mut app);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    for _ in 0..60 {
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }
    let p = player_position(app.world_mut());
    let hover = app.world().resource::<crate::scene_view::SceneView>().hover();
    assert!(hover > 1.0 && (p.y - hover).abs() < 0.01);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Space);
    for _ in 0..60 {
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }
    assert!(player_position(app.world_mut()).y.abs() < 0.01);
    assert!(!app.world().resource::<crate::jetpack::Flight>().airborne);
    for index in [0, 3, 1, 3, 2] {
        crate::switch_world(&mut app, index);
        let world = app.world_mut();
        assert!(!world.resource::<crate::jetpack::Flight>().requested);
        let count = world
            .query_filtered::<Entity, With<crate::jetpack::TerrainVisual>>()
            .iter(world)
            .count();
        assert_eq!(count > 0, index == 3);
        for visibility in world
            .query_filtered::<&Visibility, With<crate::jetpack::Deck>>()
            .iter(world)
        {
            assert_eq!(*visibility == Visibility::Hidden, index == 3);
        }
    }
}

#[test]
fn field_trial_can_be_solved_through_shared_control_without_copying_stock() {
    for fps in [20, 60] {
        let report = run_at(
            vec![
                Command::Scene {
                    scene: "surprise-lab".into(),
                },
                Command::MemoryRecord,
                Command::Walk {
                    position: [-4.0, -2.0],
                },
                Command::Wait { frames: fps / 2 },
                Command::MemoryDeploy,
                Command::Walk {
                    position: [-1.0, 3.0],
                },
                Command::Wait { frames: fps * 4 },
                Command::AssertTrial {
                    unlocked: true,
                    collected: false,
                },
                Command::Walk {
                    position: [5.0, 0.0],
                },
                Command::AssertTrial {
                    unlocked: true,
                    collected: true,
                },
            ],
            fps,
        );
        assert_eq!(report["status"], "passed", "{report}");
        assert_eq!(report["state"]["memory"]["collected"], true);
        let deployed = &report["receipts"][4]["state"];
        assert!((deployed["player"]["position"][0].as_f64().unwrap() + 5.0).abs() < 0.01);
        assert_eq!(deployed["memory"]["recording"], false);
        assert!(!deployed["memory"]["ghost_position"].is_null());
    }
}

#[test]
fn trial_cannot_be_completed_alone_and_scene_reset_cleans_it_up() {
    let report = run(vec![
        Command::Scene {
            scene: "surprise-lab".into(),
        },
        Command::Walk {
            position: [-4.0, -2.0],
        },
        Command::Wait { frames: 60 },
        Command::Walk {
            position: [-1.0, 3.0],
        },
        Command::Wait { frames: 60 },
        Command::AssertTrial {
            unlocked: false,
            collected: false,
        },
        Command::MemoryRecord,
        Command::Wait { frames: 15 },
        Command::MemoryDeploy,
        Command::Scene {
            scene: "pile-lab".into(),
        },
    ]);
    assert_eq!(report["status"], "passed", "{report}");
    assert_eq!(report["state"]["memory"]["sample_count"], 0);
    assert_eq!(report["state"]["memory"]["ghost_position"], Value::Null);
    assert_eq!(report["state"]["totals"]["created"], 96);
}

#[test]
fn projection_does_not_rewind_or_duplicate_materials_and_rejects_empty_tapes() {
    let report = run(vec![Command::MemoryDeploy]);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["state"]["totals"]["created"], 96);
    let report = run(vec![
        Command::Add {
            dispenser: 0,
            units: 8,
        },
        Command::Grab {
            pile: 0,
            units: 4,
            material: None,
            seconds_per_unit: 0.0,
            approach: None,
        },
        Command::MemoryRecord,
        Command::Walk {
            position: [0.0, 1.0],
        },
        Command::MemoryDeploy,
        Command::Wait { frames: 60 },
    ]);
    assert_eq!(report["status"], "passed", "{report}");
    assert_eq!(report["state"]["player"]["cargo"]["units"], 4);
    assert_eq!(report["state"]["totals"]["created"], 104);
    assert_eq!(report["state"]["totals"]["conserved"], true);
    assert_eq!(report["state"]["visible_mineral_units"], 104);
}

#[test]
fn recording_is_bounded_and_automatically_deploys_then_can_be_dismissed() {
    let report = run(vec![
        Command::MemoryRecord,
        Command::Wait { frames: 270 },
        Command::Inspect,
        Command::MemoryClear,
        Command::Inspect,
    ]);
    assert_eq!(report["status"], "passed", "{report}");
    let memory = &report["receipts"][2]["state"]["memory"];
    assert_eq!(memory["recording"], false);
    assert!(!memory["ghost_position"].is_null());
    assert!(memory["recorded_seconds"].as_f64().unwrap() <= 8.0);
    assert!(memory["sample_count"].as_u64().unwrap() <= 8 * content::TICKS_PER_SECOND as u64 + 3);
    assert_eq!(report["state"]["memory"]["ghost_position"], Value::Null);
    assert_eq!(report["state"]["memory"]["sample_count"], 0);
}

#[test]
fn atlantis_is_navigable_reactive_and_inspectable_without_runtime_asset_generation() {
    let report = run(vec![
        Command::Scene {
            scene: "atlantis".into(),
        },
        Command::Walk {
            position: [2.0, 2.7],
        },
        Command::Strike { target: [3.5, 2.7] },
        Command::Walk {
            position: [3.0, -2.5],
        },
        Command::Walk {
            position: [0.0, -4.0],
        },
    ]);
    assert_eq!(report["status"], "passed", "{report}");
    let state = &report["state"];
    assert_eq!(state["style"]["id"], "atlantean-aetherworks");
    assert_eq!(state["style"]["baked"], true);
    assert!(
        state["style"]["environment_statistics"]["voxels"]
            .as_u64()
            .unwrap()
            > 100_000
    );
    assert!(state["player"]["movement"]["impacts"].as_u64().unwrap() > 0);
    assert_eq!(state["targets"].as_array().unwrap().len(), 2);
}

#[test]
fn player_can_divert_real_supply_chain_stock_while_workers_continue() {
    let report = run(vec![
        Command::Scene {
            scene: "forest-outpost".into(),
        },
        Command::SettlementLink {
            id: "mill_iron".into(),
            enabled: false,
        },
        Command::SettlementTransfer {
            depot: "ore".into(),
            item: "iron".into(),
            units: 4,
            deposit: false,
        },
        Command::Inspect,
        Command::SettlementTransfer {
            depot: "mill_input".into(),
            item: "iron".into(),
            units: 4,
            deposit: true,
        },
        Command::Inspect,
        Command::Wait { frames: 60 },
    ]);
    assert_eq!(report["status"], "passed", "{report}");
    let receipts = report["receipts"].as_array().unwrap();
    assert_eq!(
        receipts[3]["state"]["settlement"]["simulation"]["agents"]["player"]["cargo"]["items"]
            ["iron"],
        4
    );
    let settlement = &report["state"]["settlement"];
    assert_eq!(
        settlement["simulation"]["agents"]["player"]["cargo"]["items"],
        json!({})
    );
    assert!(
        settlement["simulation"]["metrics"]["delivered"]
            .as_u64()
            .unwrap()
            >= 4
    );
    assert!(settlement["balances"]
        .as_object()
        .unwrap()
        .values()
        .all(|b| b["conserved"] == true));
}

/// A sprite's job names a sprite: a hauler isn't one.
#[test]
fn a_sprite_job_needs_a_sprite() {
    for sprite in ["nobody", "bramble"] {
        let report = run(vec![
            Command::Scene {
                scene: "forest-outpost".into(),
            },
            Command::SettlementSpriteJob {
                sprite: sprite.into(),
                job: Some("wood_gatherer".into()),
            },
        ]);
        assert_eq!(report["status"], "failed", "{report}");
        assert!(report["error"].as_str().unwrap().contains("unknown sprite"), "{report}");
    }
}

/// In the Sacred Tree, a sprite given the Wood Gatherer's job walks to it,
/// steps in and works: wood comes out on its output pile, and the view
/// reads the sprite as working and the building as worked by it (A7).
#[test]
fn a_sprite_staffs_the_wood_gatherer_and_wood_comes_out() {
    let report = run(vec![
        Command::Scene {
            scene: "sacred-tree".into(),
        },
        Command::Inspect,
        Command::SettlementSpriteJob {
            sprite: "sprite_ash".into(),
            job: Some("wood_gatherer".into()),
        },
        // About 5 s to walk there and 5 s to make the first wood.
        Command::Wait { frames: 420 },
        Command::Inspect,
    ]);
    assert_eq!(report["status"], "passed", "{report}");
    let receipts = report["receipts"].as_array().unwrap();
    let before = &receipts[1]["state"]["settlement"]["simulation"];
    assert_eq!(before["buildings"][0]["status"], "idle", "{before}");
    assert_eq!(before["sprites"]["sprite_ash"]["activity"], "idle", "{before}");
    let simulation = &report["state"]["settlement"]["simulation"];
    let building = &simulation["buildings"][0];
    assert_eq!(building["kind"], "wood_gatherer");
    assert!(building["output"]["items"]["wood"].as_u64().unwrap() >= 1, "{building}");
    assert_eq!(building["status"], "working", "{building}");
    assert_eq!(building["occupant"], json!({"sprite": "sprite_ash"}));
    let ash = &simulation["sprites"]["sprite_ash"];
    assert_eq!(ash["job"], "wood_gatherer");
    assert_eq!(ash["activity"], "working");
    assert_eq!(simulation["sprites"]["sprite_fern"]["activity"], "idle");
    assert!(report["state"]["settlement"]["balances"]
        .as_object()
        .unwrap()
        .values()
        .all(|b| b["conserved"] == true));
}

/// The town's first loop with no UI: a sprite works the Wood Gatherer, the
/// elder picks its wood up as a bundle and carries it to the tree's wood
/// pile. Stone can't go there, and a refusal reads as words.
#[test]
fn the_elder_carries_the_gatherer_wood_to_the_tree() {
    let report = run(vec![
        Command::Scene {
            scene: "sacred-tree".into(),
        },
        Command::SettlementSpriteJob {
            sprite: "sprite_ash".into(),
            job: Some("wood_gatherer".into()),
        },
        // Four times as fast: about 5 s to walk there and 20 s for 4 wood.
        Command::SettlementRate { rate: 4.0 },
        Command::Wait { frames: 240 },
        Command::SettlementPickUp { building: 0 },
        Command::Inspect,
        Command::SettlementDeposit {
            depot: "tree_wood".into(),
        },
        Command::Inspect,
    ]);
    assert_eq!(report["status"], "passed", "{report}");
    let receipts = report["receipts"].as_array().unwrap();
    let picked = receipts[4]["state"]["settlement"]["simulation"]["agents"]["elder"]["cargo"]["items"]["wood"].as_u64().unwrap();
    assert!((4..=5).contains(&picked), "{picked}");
    let simulation = &report["state"]["settlement"]["simulation"];
    assert_eq!(simulation["agents"]["elder"]["cargo"]["items"], json!({}));
    assert_eq!(simulation["depots"]["tree_wood"]["items"]["wood"], picked);
    assert!(report["state"]["settlement"]["balances"]
        .as_object()
        .unwrap()
        .values()
        .all(|b| b["conserved"] == true));
    // Nothing there: the step fails with the sim's reason.
    for (command, error) in [
        (Command::SettlementWithdraw { depot: "tree_stone".into() }, "Nothing there to pick up"),
        (Command::SettlementDeposit { depot: "tree_wood".into() }, "Carrying nothing to deposit"),
        (Command::SettlementPickUp { building: 3 }, "unknown building"),
    ] {
        let report = run(vec![Command::Scene { scene: "sacred-tree".into() }, command]);
        assert_eq!(report["status"], "failed", "{report}");
        assert!(report["error"].as_str().unwrap().contains(error), "{report}");
    }
}

/// The interact key's path, as a script: the target the highlight shows,
/// then one interact each to pick up at the Wood Gatherer's output (the
/// elder steps in), deposit at the tree's wood pile, and open a sprite's
/// job menu, whose pick sets its job (D5).
#[test]
fn the_elder_interacts_its_way_through_the_town() {
    let wait = |frames| Command::Wait { frames };
    let walk = |x, z| Command::Walk { position: [x, z] };
    let target = |prompt: Option<&str>| Command::SettlementInteractTarget { prompt: prompt.map(String::from) };
    let report = run(vec![
        Command::Scene {
            scene: "sacred-tree".into(),
        },
        target(Some("")),
        Command::SettlementSpriteJob {
            sprite: "sprite_ash".into(),
            job: Some("wood_gatherer".into()),
        },
        Command::SettlementRate { rate: 4.0 },
        wait(240),
        // Beside the output pile, a step short of it.
        walk(-3.3, 0.5),
        target(None),
        Command::SettlementInteract,
        // At the wood pile.
        walk(-1.7, 2.4),
        target(None),
        Command::SettlementInteract,
        // In front of Fern.
        walk(-0.6, 4.3),
        target(Some("SET JOB")),
        Command::SettlementInteract,
        Command::Inspect,
        Command::SettlementJobMenu {
            job: Some("ballista".into()),
        },
        wait(2),
    ]);
    assert_eq!(report["status"], "passed", "{report}");
    let receipts = report["receipts"].as_array().unwrap();
    let state = |i: usize| &receipts[i]["state"]["settlement"];
    let pick_up = state(6)["interact"]["prompt"].as_str().unwrap().to_string();
    assert_eq!(state(6)["interact"]["target"], json!({"Output": 0}), "{}", state(6)["interact"]);
    let picked = state(7)["simulation"]["agents"]["elder"]["cargo"]["items"]["wood"].as_u64().unwrap();
    assert!((4..=5).contains(&picked), "{picked}");
    assert_eq!(pick_up, format!("PICK UP {picked} WOOD"));
    assert_eq!(state(9)["interact"]["prompt"], format!("DEPOSIT {picked} WOOD"));
    assert_eq!(state(10)["simulation"]["depots"]["tree_wood"]["items"]["wood"], picked);
    assert_eq!(state(10)["simulation"]["agents"]["elder"]["cargo"]["items"], json!({}));
    let menu = &state(14)["interact"]["menu"];
    assert_eq!(menu["options"], json!(["IDLE", "GATHER WOOD", "MAN BALLISTA", "MINE"]), "{}", state(14)["interact"]);
    let simulation = &report["state"]["settlement"]["simulation"];
    assert_eq!(simulation["sprites"]["sprite_fern"]["job"], "ballista");
    assert_eq!(report["state"]["settlement"]["interact"]["menu"], Value::Null);
    assert!(report["state"]["settlement"]["balances"]
        .as_object()
        .unwrap()
        .values()
        .all(|b| b["conserved"] == true));
    // Nothing in reach at the start: an interact fails with the reason.
    let report = run(vec![Command::Scene { scene: "sacred-tree".into() }, Command::SettlementInteract]);
    assert_eq!(report["status"], "failed", "{report}");
    assert!(report["error"].as_str().unwrap().contains("Nothing in reach"), "{report}");
}

/// Dropping, as a script (M2.4): wood from the tree dropped on the ground
/// with the drop verb (Q's path), picked back up with one interact as an
/// output is, and deposited at the tree with another.
#[test]
fn the_elder_drops_wood_and_picks_it_back_up() {
    let walk = |x, z| Command::Walk { position: [x, z] };
    let target = |prompt: &str| Command::SettlementInteractTarget { prompt: Some(prompt.into()) };
    let report = run(vec![
        Command::Scene {
            scene: "sacred-tree".into(),
        },
        Command::SettlementStock {
            depot: "tree_wood".into(),
            item: "wood".into(),
            units: 5,
        },
        walk(-1.7, 2.4),
        Command::SettlementInteract,
        // Out in the open, east of the tree.
        walk(3.0, -3.0),
        Command::SettlementDrop,
        Command::Inspect,
        target("PICK UP 5 WOOD"),
        Command::SettlementInteract,
        Command::Wait { frames: 2 },
        Command::Inspect,
        walk(-1.7, 2.4),
        target("DEPOSIT 5 WOOD"),
        Command::SettlementInteract,
    ]);
    assert_eq!(report["status"], "passed", "{report}");
    let receipts = report["receipts"].as_array().unwrap();
    let simulation = |i: usize| &receipts[i]["state"]["settlement"]["simulation"];
    let dropped = &simulation(6)["ground_piles"];
    assert_eq!(dropped.as_array().map(Vec::len), Some(1), "{dropped}");
    assert_eq!(dropped[0]["stock"]["items"], json!({"wood": 5}), "{dropped}");
    assert_eq!(simulation(6)["agents"]["elder"]["cargo"]["items"], json!({}));
    assert_eq!(simulation(10)["agents"]["elder"]["cargo"]["items"]["wood"], 5);
    assert_eq!(simulation(10)["ground_piles"], Value::Null, "the emptied pile went");
    let last = &report["state"]["settlement"];
    assert_eq!(last["simulation"]["depots"]["tree_wood"]["items"]["wood"], 5);
    assert_eq!(last["simulation"]["agents"]["elder"]["cargo"]["items"], json!({}));
    assert!(last["balances"].as_object().unwrap().values().all(|b| b["conserved"] == true));
    // Empty-handed, a drop fails with the sim's reason.
    let report = run(vec![Command::Scene { scene: "sacred-tree".into() }, Command::SettlementDrop]);
    assert_eq!(report["status"], "failed", "{report}");
    assert!(report["error"].as_str().unwrap().contains("Nothing carried"), "{report}");
}

/// Building, as a script (D1, D4): the build menu, a Wood Gatherer's
/// ghost refused on the tree and valid in the open, put down; wood carried
/// from the tree and delivered, the leftover put back and stone fetched,
/// both parts installed, each by the interact key's own path; then a
/// sprite assigned works it and wood comes out. Ash works the prebuilt one
/// first, so the new one is the free one.
#[test]
fn the_elder_builds_a_wood_gatherer_that_a_sprite_works() {
    let walk = |x, z| Command::Walk { position: [x, z] };
    let target = |prompt: &str| Command::SettlementInteractTarget { prompt: Some(prompt.into()) };
    let stock = |depot: &str, item: &str, units| Command::SettlementStock { depot: depot.into(), item: item.into(), units };
    let report = run(vec![
        Command::Scene {
            scene: "sacred-tree".into(),
        },
        Command::SettlementSpriteJob {
            sprite: "sprite_ash".into(),
            job: Some("wood_gatherer".into()),
        },
        stock("tree_wood", "wood", 5),
        stock("tree_stone", "stone", 2),
        Command::SettlementBuild { kind: None },
        Command::Inspect,
        Command::SettlementBuild {
            kind: Some("wood_gatherer".into()),
        },
        // Facing the tree, the ghost stands on it.
        walk(0.0, 2.6),
        Command::Inspect,
        // Facing -z east of the tree, it stands in the open.
        walk(3.0, 1.0),
        walk(3.0, 0.0),
        Command::Inspect,
        Command::SettlementPlaceBlueprint { kind: None, position: None },
        Command::Inspect,
        // Five wood from the tree's wood pile.
        walk(-1.7, 2.4),
        Command::SettlementInteract,
        // In front of the site, where its output pile will be.
        walk(2.0, -0.6),
        target("DELIVER 2 WOOD"),
        Command::SettlementInteract,
        target("INSTALL FOUNDATION"),
        Command::SettlementInteract,
        target("DELIVER 2 WOOD"),
        Command::SettlementInteract,
        // The wood left over back in the tree, and its two stone.
        Command::SettlementDeposit { depot: "tree_wood".into() },
        Command::SettlementWithdraw { depot: "tree_stone".into() },
        walk(2.0, -0.6),
        target("DELIVER 2 STONE"),
        Command::SettlementInteract,
        target("INSTALL MECHANISM"),
        Command::SettlementInteract,
        Command::Inspect,
        Command::SettlementSpriteJob {
            sprite: "sprite_fern".into(),
            job: Some("wood_gatherer".into()),
        },
        Command::SettlementRate { rate: 4.0 },
        Command::Wait { frames: 600 },
    ]);
    assert_eq!(report["status"], "passed", "{report}");
    let receipts = report["receipts"].as_array().unwrap();
    let state = |i: usize| &receipts[i]["state"]["settlement"];
    assert_eq!(state(5)["interact"]["build_menu"], json!(["wood_gatherer", "ballista", "lantern"]), "{}", state(5)["interact"]);
    let ghost = |i: usize| &state(i)["interact"]["ghost"];
    assert_eq!(state(8)["interact"]["placing"], "wood_gatherer");
    assert_eq!((&ghost(8)["valid"], &ghost(8)["reason"]), (&json!(false), &json!("BLOCKED")), "{}", ghost(8));
    assert_eq!((&ghost(11)["valid"], &ghost(11)["reason"]), (&json!(true), &Value::Null), "{}", ghost(11));
    // Put down where the ghost stood: a site waiting for its foundation.
    let site = &state(13)["simulation"]["buildings"][1];
    assert_eq!((&site["kind"], &site["status"]), (&json!("wood_gatherer"), &json!("site")), "{site}");
    assert_eq!(site["site"], json!({"part": 0, "parts": 2, "materials": [["wood", 0, 2]], "ready": false}));
    let (at, stood) = (&site["position"], &ghost(11)["position"]);
    assert!((0..2).all(|i| (at[i].as_f64().unwrap() - stood[i].as_f64().unwrap()).abs() < 0.05), "{at} vs {stood}");
    assert_eq!(state(13)["interact"]["placing"], Value::Null);
    // Delivered, the foundation reads ready; then built.
    assert_eq!(state(19)["simulation"]["buildings"][1]["site"]["ready"], true, "{}", state(19)["simulation"]["buildings"][1]);
    assert_eq!(state(21)["simulation"]["buildings"][1]["site"]["part"], 1);
    let built = &state(30)["simulation"]["buildings"][1];
    assert_eq!((&built["status"], &built["site"]), (&json!("idle"), &Value::Null), "{built}");
    assert_eq!(state(30)["simulation"]["agents"]["elder"]["cargo"]["items"], json!({}));
    assert_eq!(state(30)["simulation"]["depots"]["tree_wood"]["items"]["wood"], 1);
    // Fern staffs it, and wood comes out.
    let last = &report["state"]["settlement"];
    let new = &last["simulation"]["buildings"][1];
    assert_eq!(new["occupant"], json!({"sprite": "sprite_fern"}), "{new}");
    assert!(new["output"]["items"]["wood"].as_u64().is_some_and(|n| n >= 1), "{new}");
    assert_eq!((&last["balances"]["wood"]["consumed"], &last["balances"]["stone"]["consumed"]), (&json!(4), &json!(2)));
    assert!(last["balances"].as_object().unwrap().values().all(|b| b["conserved"] == true));
}

#[test]
fn player_stands_on_baked_raised_roads_and_steps_back_to_the_ground() {
    let report = run(vec![
        Command::Scene {
            scene: "forest-outpost".into(),
        },
        Command::Walk {
            position: [1., -5.175],
        },
        Command::Inspect,
        Command::Walk { position: [1., 0.] },
        Command::Inspect,
    ]);
    assert_eq!(report["status"], "passed", "{report}");
    let receipts = report["receipts"].as_array().unwrap();
    let raised = receipts[2]["state"]["player"]["position"][1]
        .as_f64()
        .unwrap();
    let ground = report["state"]["player"]["position"][1].as_f64().unwrap();
    assert!((raised - 0.15).abs() < 0.005, "road support {raised}");
    assert!(ground.abs() < 0.005, "ground support {ground}");
}

/// Debug UI capture against the real command path. Simulation runs have no
/// egui backend, so a test sets the capture the backend's pass would
/// measure; routing reads it the same way.
mod debug_ui_capture {
    use super::*;
    use crate::flow::Launch;
    use debug_ui::input::CaptureSnapshot;
    use debug_ui::Capture;

    fn app_in(scene: &str) -> App {
        let options = Options { simulation: true, ..default() };
        let hub = Hub::new(options.clone()).unwrap();
        let scenes = SceneLibrary::new();
        let index = scenes.index(scene).unwrap();
        let mut app = crate::test_app(options, scenes, hub, Launch::single_player(index));
        crate::start_world(&mut app);
        app
    }

    fn capture(app: &mut App, capture: Capture) {
        app.world_mut().resource_mut::<CaptureSnapshot>().0 = capture;
    }

    /// Runs frames as Bevy's input plugin would: edges last one frame.
    fn frames(app: &mut App, n: usize) {
        for _ in 0..n {
            app.update();
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        }
    }

    fn keys(app: &mut App) -> Mut<'_, ButtonInput<KeyCode>> {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>()
    }

    fn requested(app: &App) -> bool {
        app.world().resource::<crate::jetpack::Flight>().requested
    }

    fn cancellations(app: &App) -> u64 {
        app.world().resource::<debug_ui::GameInput>().cancellations()
    }

    fn moved(app: &mut App, n: usize) -> f32 {
        let start = player_position(app.world_mut());
        frames(app, n);
        crate::navigation::flat(player_position(app.world_mut()) - start).length()
    }

    const KEYBOARD: Capture = Capture { keyboard: true, pointer: false, pointer_interrupt: false };

    #[test]
    fn keyboard_capture_switches_a_held_jetpack_off_once_and_it_stays_off() {
        let mut app = app_in("jetpack-lab");
        keys(&mut app).press(KeyCode::Space);
        frames(&mut app, 30);
        assert!(requested(&app), "Space held lifts off");
        capture(&mut app, KEYBOARD);
        frames(&mut app, 30);
        assert!(!requested(&app), "capture switches the jetpack off through its usual command");
        assert_eq!(cancellations(&app), 1);
        // The capture ends while Space is still held: no relaunch.
        capture(&mut app, Capture::default());
        frames(&mut app, 30);
        assert!(!requested(&app));
        keys(&mut app).release(KeyCode::Space);
        frames(&mut app, 1);
        keys(&mut app).press(KeyCode::Space);
        frames(&mut app, 30);
        assert!(requested(&app), "a fresh press flies again");
        assert_eq!(cancellations(&app), 1);
    }

    #[test]
    fn keyboard_capture_stops_a_held_walk_until_released_and_pressed_again() {
        let mut app = app_in("field");
        keys(&mut app).press(KeyCode::KeyW);
        assert!(moved(&mut app, 30) > 0.3, "W walks");
        capture(&mut app, KEYBOARD);
        frames(&mut app, 20);
        assert!(moved(&mut app, 20) < 0.01, "capture stops the walk");
        capture(&mut app, Capture::default());
        assert!(moved(&mut app, 30) < 0.01, "W still held after the capture doesn't resume it");
        keys(&mut app).release(KeyCode::KeyW);
        frames(&mut app, 1);
        keys(&mut app).press(KeyCode::KeyW);
        assert!(moved(&mut app, 30) > 0.3, "a fresh press walks");
        assert_eq!(cancellations(&app), 1);
    }

    #[test]
    fn capture_leaves_an_issued_destination_running() {
        let mut app = app_in("field");
        let goal = player_position(app.world_mut()) + Vec3::new(2.0, 0.0, 0.0);
        let goal = Vec3::new(goal.x, 0.0, goal.z);
        app.world_mut().resource_scope(|world, mut service: Mut<SessionService>| {
            world.resource_mut::<PlayerControl>().go_to(&mut service, goal, false);
        });
        capture(&mut app, Capture { keyboard: true, pointer: true, pointer_interrupt: true });
        frames(&mut app, 240);
        let at = player_position(app.world_mut());
        assert!(crate::navigation::flat(at - goal).length() < 0.15, "the walk kept going: {at:?} vs {goal:?}");
    }

    #[test]
    fn scripts_keep_running_while_the_ui_has_both_devices() {
        let script = Script {
            version: 1,
            scene: "jetpack-lab".into(),
            fps: 30,
            width: 1440,
            height: 960,
            max_frames: 1200,
            steps: vec![
                Command::Jetpack { enabled: true },
                Command::Walk { position: [0.0, 2.0] },
                Command::Wait { frames: 30 },
                Command::AssertFlight { airborne: true, altitude: 3.8, tolerance: 0.5 },
            ],
        };
        let options = Options { simulation: true, script: Some(script), ..default() };
        let hub = Hub::new(options.clone()).unwrap();
        let mut app = crate::test_app(options, SceneLibrary::new(), hub, Launch::single_player(3));
        capture(&mut app, Capture { keyboard: true, pointer: true, pointer_interrupt: true });
        for _ in 0..1300 {
            app.update();
            let hub = app.world().resource::<Hub>();
            if hub.finished.is_some() || hub.failed.is_some() {
                let report = hub.report(0);
                assert_eq!(report["status"], "passed", "{report}");
                return;
            }
        }
        panic!("script failed to finish");
    }
}

/// Possessing, as a script (D3, A3): a sprite works the Wood Gatherer; the
/// elder walks up and boards it, which puts the sprite out beside it; the
/// gatherer works four times as fast while possessed; the elder leaves and
/// the sprite goes back in with no new job. Boarding with a bundle in hand
/// fails with the sim's reason.
#[test]
fn the_elder_possesses_the_wood_gatherer_and_its_sprite_goes_back_in() {
    let wood = |state: &Value| state["settlement"]["simulation"]["buildings"][0]["output"]["items"]["wood"].as_u64().unwrap_or(0);
    let report = run(vec![
        Command::Scene {
            scene: "sacred-tree".into(),
        },
        Command::SettlementSpriteJob {
            sprite: "sprite_ash".into(),
            job: Some("wood_gatherer".into()),
        },
        Command::Wait { frames: 420 },
        Command::SettlementBoard { building: 0 },
        Command::Wait { frames: 2 },
        Command::Inspect,
        Command::Wait { frames: 300 },
        Command::Inspect,
        Command::SettlementLeave,
        Command::Wait { frames: 150 },
        Command::Inspect,
    ]);
    assert_eq!(report["status"], "passed", "{report}");
    let receipts = report["receipts"].as_array().unwrap();
    let possessed = &receipts[5]["state"];
    let settlement = &possessed["settlement"];
    assert_eq!(settlement["seat"], json!({"building": 0}), "{settlement}");
    let building = &settlement["simulation"]["buildings"][0];
    assert_eq!(building["occupant"], json!({"elder": "elder"}), "{building}");
    let ash = &settlement["simulation"]["sprites"]["sprite_ash"];
    assert_eq!((ash["job"].as_str(), ash["activity"].as_str()), (Some("wood_gatherer"), Some("waiting")), "{ash}");
    // Ten seconds possessed: a unit every 1.25 s, against one per 5 s.
    let made = wood(&receipts[7]["state"]) - wood(possessed);
    assert!((7..=9).contains(&made), "{made} units in 10 s");
    let last = &receipts[10]["state"]["settlement"];
    assert_eq!(last["seat"], Value::Null);
    assert_eq!(last["simulation"]["buildings"][0]["occupant"], json!({"sprite": "sprite_ash"}), "{last}");
    assert_eq!(last["simulation"]["sprites"]["sprite_ash"]["activity"], "working");
    assert!(last["balances"].as_object().unwrap().values().all(|b| b["conserved"] == true));

    let report = run(vec![
        Command::Scene { scene: "sacred-tree".into() },
        Command::SettlementStock { depot: "tree_wood".into(), item: "wood".into(), units: 5 },
        Command::Walk { position: [-1.7, 2.4] },
        Command::SettlementInteract,
        Command::SettlementBoard { building: 0 },
    ]);
    assert_eq!(report["status"], "failed", "{report}");
    assert!(report["error"].as_str().unwrap().contains("Drop the bundle first (Q)"), "{report}");
}

/// The elder builds a ballista, loads it with the stone left over and
/// possesses it: `settlement_fire` shoots a bolt at a point past its range,
/// which comes down at the range, one stone spent, the turret turned there;
/// a second shot right after fails while it reloads. Firing outside a
/// ballista fails too.
#[test]
fn the_elder_fires_a_possessed_ballista_at_the_reticle() {
    let walk = |x, z| Command::Walk { position: [x, z] };
    let target = |prompt: &str| Command::SettlementInteractTarget { prompt: Some(prompt.into()) };
    let stock = |depot: &str, item: &str, units| Command::SettlementStock { depot: depot.into(), item: item.into(), units };
    let ammo = |state: &Value| state["settlement"]["simulation"]["buildings"][1]["output"]["items"]["stone"].as_u64().unwrap_or(0);
    let report = run_for(
        vec![
            Command::Scene { scene: "sacred-tree".into() },
            Command::SettlementFire { x: 1.0, y: 0.0 },
        ],
        30,
        600,
    );
    assert_eq!(report["status"], "failed", "{report}");
    assert!(report["error"].as_str().unwrap().contains("Possess a ballista to fire"), "{report}");
    let report = run_for(
        vec![
            Command::Scene { scene: "sacred-tree".into() },
            stock("tree_wood", "wood", 4),
            stock("tree_stone", "stone", 5),
            Command::SettlementBuild { kind: Some("ballista".into()) },
            // Facing -z east of the tree, as the Wood Gatherer goes up in
            // the build test.
            walk(3.0, 1.0),
            walk(3.0, 0.0),
            Command::SettlementPlaceBlueprint { kind: None, position: None },
            walk(-1.7, 2.4),
            Command::SettlementInteract,
            walk(2.0, -0.6),
            target("DELIVER 2 WOOD"),
            Command::SettlementInteract,
            target("INSTALL FOUNDATION"),
            Command::SettlementInteract,
            target("DELIVER 2 WOOD"),
            Command::SettlementInteract,
            walk(1.7, 2.4),
            Command::SettlementInteract,
            walk(2.0, -0.6),
            target("DELIVER 2 STONE"),
            Command::SettlementInteract,
            target("INSTALL MECHANISM"),
            Command::SettlementInteract,
            target("LOAD 3 STONE"),
            Command::SettlementInteract,
            Command::SettlementBoard { building: 1 },
            Command::Inspect,
            Command::SettlementFire { x: 0.0, y: 10.0 },
            Command::Inspect,
            Command::SettlementFire { x: 1.0, y: 0.0 },
        ],
        30,
        2400,
    );
    assert_eq!(report["status"], "failed", "{report}");
    assert!(report["error"].as_str().unwrap().contains("Reloading"), "{report}");
    let receipts = report["receipts"].as_array().unwrap();
    // The failed shot has its receipt too.
    let (before, after) = (&receipts[receipts.len() - 4]["state"], &receipts[receipts.len() - 2]["state"]);
    assert_eq!((ammo(before), ammo(after)), (3, 2), "one stone a shot");
    let ballista = &after["settlement"]["simulation"]["buildings"][1];
    assert_eq!(ballista["kind"], "ballista", "{ballista}");
    let (at, aim) = (&ballista["position"], &ballista["turret"]["aim"]);
    let d = |i: usize| aim[i].as_f64().unwrap() - at[i].as_f64().unwrap();
    assert!(d(0).abs() < 0.05 && (d(1) - 3.8).abs() < 0.05, "turned to the reticle at the range: {aim} from {at}");
    assert!(after["settlement"]["balances"].as_object().unwrap().values().all(|b| b["conserved"] == true));
}

/// The elder builds a ballista and possesses it, with no stone in it:
/// `settlement_fireball` throws a fireball at the reticle anyway (it costs
/// none), which comes down and sets the ground alight; a second one right
/// after fails while the elder's fireball cooldown runs.
#[test]
fn the_elder_throws_a_fireball_from_a_possessed_ballista() {
    let walk = |x, z| Command::Walk { position: [x, z] };
    let target = |prompt: &str| Command::SettlementInteractTarget { prompt: Some(prompt.into()) };
    let stock = |depot: &str, item: &str, units| Command::SettlementStock { depot: depot.into(), item: item.into(), units };
    let report = run_for(
        vec![
            Command::Scene { scene: "sacred-tree".into() },
            stock("tree_wood", "wood", 4),
            stock("tree_stone", "stone", 2),
            Command::SettlementBuild { kind: Some("ballista".into()) },
            walk(3.0, 1.0),
            walk(3.0, 0.0),
            Command::SettlementPlaceBlueprint { kind: None, position: None },
            walk(-1.7, 2.4),
            Command::SettlementInteract,
            walk(2.0, -0.6),
            target("DELIVER 2 WOOD"),
            Command::SettlementInteract,
            target("INSTALL FOUNDATION"),
            Command::SettlementInteract,
            target("DELIVER 2 WOOD"),
            Command::SettlementInteract,
            walk(1.7, 2.4),
            Command::SettlementInteract,
            walk(2.0, -0.6),
            target("DELIVER 2 STONE"),
            Command::SettlementInteract,
            target("INSTALL MECHANISM"),
            Command::SettlementInteract,
            Command::SettlementBoard { building: 1 },
            Command::Inspect,
            Command::SettlementFireball { x: 2.0, y: 1.0 },
            Command::Wait { frames: 30 },
            Command::Inspect,
            Command::SettlementFireball { x: 1.0, y: 0.0 },
        ],
        30,
        2400,
    );
    assert_eq!(report["status"], "failed", "{report}");
    assert!(report["error"].as_str().unwrap().contains("Fireball ready in"), "{report}");
    let receipts = report["receipts"].as_array().unwrap();
    let (before, after) = (&receipts[receipts.len() - 5]["state"], &receipts[receipts.len() - 2]["state"]);
    assert_eq!(before["settlement"]["burst"]["ready"], true, "{}", before["settlement"]["burst"]);
    assert!(before["settlement"]["burst"]["fireball_to"].is_array());
    let burst = &after["settlement"]["burst"];
    assert_eq!(burst["ready"], false, "{burst}");
    assert!(burst["fireball_cooldown"].as_f64().unwrap() > 15.0, "{burst}");
    let simulation = &after["settlement"]["simulation"];
    let fires = simulation["fires"].as_array().expect("a fire on the ground");
    assert_eq!(fires.len(), 1, "{simulation}");
    let (at, fire) = (&simulation["buildings"][1]["position"], &fires[0]["position"]);
    let d = |i: usize| fire[i].as_f64().unwrap() - at[i].as_f64().unwrap();
    assert!((d(0) - 2.0).abs() < 0.05 && (d(1) - 1.0).abs() < 0.05, "at the reticle: {fire} from {at}");
    assert!(after["settlement"]["balances"].as_object().unwrap().values().all(|b| b["conserved"] == true));
}

/// Possessing the Wood Gatherer, `settlement_burst` puts five units in its
/// output at once; a second right after fails with the cooldown left, and
/// once the cooldown is over (the elder waiting outside, so the output has
/// room), a Burst works again. `settlement.burst` reports the cooldown.
#[test]
fn the_elder_bursts_the_wood_gatherer_and_waits_out_the_cooldown() {
    let wood = |state: &Value| state["settlement"]["simulation"]["buildings"][0]["output"]["items"]["wood"].as_u64().unwrap_or(0);
    let report = run(vec![
        Command::Scene { scene: "sacred-tree".into() },
        Command::SettlementBoard { building: 0 },
        Command::Inspect,
        Command::SettlementBurst,
        Command::Inspect,
        Command::SettlementBurst,
    ]);
    assert_eq!(report["status"], "failed", "{report}");
    assert!(report["error"].as_str().unwrap().contains("Burst ready in"), "{report}");
    let receipts = report["receipts"].as_array().unwrap();
    let (before, after) = (&receipts[2]["state"], &receipts[4]["state"]);
    assert_eq!(before["settlement"]["burst"]["ready"], true, "{}", before["settlement"]["burst"]);
    // Five at once, and perhaps a unit of the elder's own work meanwhile.
    let added = wood(after) - wood(before);
    assert!((5..=6).contains(&added), "{added} units");
    let burst = &after["settlement"]["burst"];
    assert_eq!(burst["ready"], false, "{burst}");
    assert!(burst["cooldown"].as_f64().unwrap() > 29.0, "{burst}");
    assert!(burst["reason"].as_str().unwrap().starts_with("Burst ready in"), "{burst}");
    assert!(after["settlement"]["balances"].as_object().unwrap().values().all(|b| b["conserved"] == true));

    // At 20 frames a second, so the script's frame budget covers the wait.
    let report = run_at(
        vec![
            Command::Scene { scene: "sacred-tree".into() },
            Command::SettlementBoard { building: 0 },
            Command::SettlementBurst,
            Command::SettlementLeave,
            Command::Wait { frames: 610 },
            Command::Inspect,
            Command::SettlementBoard { building: 0 },
            Command::Inspect,
            Command::SettlementBurst,
            Command::Inspect,
        ],
        20,
    );
    assert_eq!(report["status"], "passed", "{report}");
    let receipts = report["receipts"].as_array().unwrap();
    let outside = &receipts[5]["state"]["settlement"]["burst"];
    assert_eq!((outside["cooldown"].as_f64(), outside["reason"].as_str()), (Some(0.0), Some("Possess a building to Burst")), "{outside}");
    let added = wood(&receipts[9]["state"]) - wood(&receipts[7]["state"]);
    assert!((5..=6).contains(&added), "{added} units");
}

/// The happy path, section 5 of the requirements (A6), step by step from a
/// fresh start with no cheat commands (no stock added, no speed-up): only
/// the player's own verbs, at the game's own pace (about two minutes of
/// play, at 20 frames a second). Each pickup waits for a full bundle of 5,
/// so every leftover is known and goes back to the tree. No building makes
/// stone (M6.4 of the mining design): steps 4 to 6, which built and worked
/// a Stone Gatherer, are the elder digging stone out of an exposed vein
/// below the hill and carrying it up to the tree, and Reed sent to mine
/// (M6.5), fetching what the elder left on the ground.
#[test]
fn the_happy_path_plays_from_a_fresh_start() {
    const FPS: u32 = 20;
    let wait = |seconds: u32| Command::Wait { frames: seconds * FPS };
    let walk = |x, z| Command::Walk { position: [x, z] };
    let target = |prompt: &str| Command::SettlementInteractTarget { prompt: Some(prompt.into()) };
    let job = |sprite: &str, kind: &str| Command::SettlementSpriteJob { sprite: sprite.into(), job: Some(kind.into()) };
    let pick_up = |building| Command::SettlementPickUp { building };
    let deposit = |depot: &str| Command::SettlementDeposit { depot: depot.into() };
    let withdraw = |depot: &str| Command::SettlementWithdraw { depot: depot.into() };
    // Each `inspect` is a receipt to read back.
    fn inspect(steps: &mut Vec<Command>) -> usize {
        steps.push(Command::Inspect);
        steps.len() - 1
    }
    let mut steps = vec![Command::Scene { scene: "sacred-tree".into() }];
    // 1. The clearing: four idle sprites and a built Wood Gatherer.
    let start = inspect(&mut steps);
    // 2. Fern to Gather Wood, with E's job menu in front of Fern.
    steps.extend([walk(-0.6, 4.3), target("SET JOB"), Command::SettlementInteract, Command::SettlementJobMenu { job: Some("wood_gatherer".into()) }, wait(14)]);
    let first_wood = inspect(&mut steps);
    // 3. A bundle of 5 wood to the tree.
    steps.extend([wait(30), pick_up(0), deposit("tree_wood")]);
    let stocked = inspect(&mut steps);
    // 4. Down the east ramp to the stone vein beside it, 11 m off its
    // middle (one of the four M6.3 guarantees for the scene's seed, at
    // arena tile (18, -11); `hill_tests` prints them). The elder
    // stands two tiles east of the vein's middle, facing west, and digs the
    // tile in front, a stone layer; then steps into that pit and digs the
    // vein's middle. Each layer drops 3 to 6 stone onto a pile beside it.
    // The town's ground is a volume (M8.3): the first dig is a step down
    // (Ctrl+F), the second, from inside that pit, a level one (F), each
    // taking the tile's top cell.
    steps.extend([walk(21.0, -11.0), walk(20.0, -11.0)]);
    let at_vein = inspect(&mut steps);
    let dig = |direction| Command::DigFront { direction };
    steps.extend([dig(sim::volume::DigDirection::Down), wait(3), walk(19.0, -11.0), dig(sim::volume::DigDirection::Level), wait(3)]);
    let dug = inspect(&mut steps);
    // 5. A bundle of 5 stone from the pile; the rest stays on the ground.
    steps.extend([target("PICK UP 5 STONE"), Command::SettlementInteract]);
    let picked = inspect(&mut steps);
    // 6. Reed to Mine: it fetches the rest of the dug stone from the
    // ground (and digs more) while the elder goes up the ramp to the
    // tree's stone pile.
    steps.extend([job("sprite_reed", "mine"), deposit("tree_stone")]);
    let stone_stocked = inspect(&mut steps);
    // 7. A second Wood Gatherer north-east: wood from the first one's
    // output, stone from the tree, leftovers back in the tree.
    steps.extend([
        pick_up(0),
        Command::SettlementPlaceBlueprint { kind: Some("wood_gatherer".into()), position: Some([4.5, 4.0]) },
        walk(3.1, 3.3),
        target("DELIVER 2 WOOD"),
        Command::SettlementInteract,
        target("INSTALL FOUNDATION"),
        Command::SettlementInteract,
        target("DELIVER 2 WOOD"),
        Command::SettlementInteract,
        deposit("tree_wood"),
        withdraw("tree_stone"),
        walk(3.1, 3.3),
        target("DELIVER 2 STONE"),
        Command::SettlementInteract,
        target("INSTALL MECHANISM"),
        Command::SettlementInteract,
        deposit("tree_stone"),
    ]);
    let wood_built = inspect(&mut steps);
    // 8. Ash to Gather Wood: it takes the new one. The elder empties the
    // first one's output meanwhile, so neither is full.
    steps.extend([job("sprite_ash", "wood_gatherer"), pick_up(0), deposit("tree_wood"), wait(15)]);
    let both = inspect(&mut steps);
    steps.push(wait(6));
    let both_later = inspect(&mut steps);
    // 9. E at the first Wood Gatherer's slot: Fern is put out, the elder
    // works it at 4x, and Burst works.
    steps.extend([walk(-5.1, 2.6), target("POSSESS, FERN STEPS OUT"), Command::SettlementInteract]);
    let possessed = inspect(&mut steps);
    steps.push(wait(5));
    let possessed_later = inspect(&mut steps);
    steps.push(Command::SettlementBurst);
    let burst = inspect(&mut steps);
    // 10. Leaving: Fern goes back in on its own.
    steps.extend([target("LEAVE"), Command::SettlementInteract, wait(6)]);
    let left = inspect(&mut steps);

    let report = run_for(steps, FPS, 6000);
    assert_eq!(report["status"], "passed", "{report}");
    // All of it is played before the map's first ghouls top the hill: they
    // set off from the ring 28 m out at the first dusk, 150 s (M7.2), and
    // take about 21 s to reach the ramps' tops (M7.7), so no ghoul gets in
    // the way.
    let ticks = report["state"]["sim_tick"].as_u64().unwrap();
    eprintln!("the happy path took {} s", ticks / 60);
    assert!(ticks < 170 * content::TICKS_PER_SECOND as u64, "the happy path took {} s", ticks / 60);
    let receipts = report["receipts"].as_array().unwrap();
    let seconds = |i: usize| receipts[i]["state"]["sim_tick"].as_u64().unwrap() / content::TICKS_PER_SECOND as u64;
    eprintln!("dug at {} s, stone at the tree at {} s, the mechanism in at {} s", seconds(dug), seconds(stone_stocked), seconds(wood_built));
    let sim = |i: usize| &receipts[i]["state"]["settlement"]["simulation"];
    let building = |i: usize, b: usize| &sim(i)["buildings"][b];
    let output = |i: usize, b: usize, item: &str| building(i, b)["output"]["items"][item].as_u64().unwrap_or(0);
    let tree = |i: usize, depot: &str, item: &str| sim(i)["depots"][depot]["items"][item].as_u64().unwrap_or(0);
    let sprite = |i: usize, id: &str| &sim(i)["sprites"][id];
    let cargo = |i: usize| &sim(i)["agents"]["elder"]["cargo"]["items"];
    // 1.
    assert_eq!(sim(start)["buildings"].as_array().map(Vec::len), Some(1));
    assert_eq!((&building(start, 0)["kind"], &building(start, 0)["status"]), (&json!("wood_gatherer"), &json!("idle")));
    for id in ["sprite_ash", "sprite_fern", "sprite_moss", "sprite_reed"] {
        assert_eq!(sprite(start, id)["activity"], "idle", "{id}");
    }
    // 2.
    assert_eq!(building(first_wood, 0)["occupant"], json!({"sprite": "sprite_fern"}), "{}", building(first_wood, 0));
    assert!(output(first_wood, 0, "wood") >= 1);
    // 3.
    assert_eq!(tree(stocked, "tree_wood", "wood"), 5);
    // 4. No Stone Gatherer stands; the vein lies in front.
    let digging = |i: usize| &receipts[i]["state"]["digging"];
    assert!(receipts[at_vein]["state"]["settlement"]["simulation"]["buildings"].as_array().unwrap().iter().all(|b| b["kind"] != "stone_gatherer"));
    assert_eq!(digging(at_vein)["nearest_ore"]["tile"], json!([19, -11]), "{}", digging(at_vein));
    assert_eq!(digging(dug)["pits"], json!([{"tile": [18, -11], "depth": 1}, {"tile": [19, -11], "depth": 1}]), "{}", digging(dug));
    let piles = sim(dug)["ground_piles"].as_array().unwrap();
    let on_ground: u64 = piles.iter().map(|p| p["stock"]["items"]["stone"].as_u64().unwrap_or(0)).sum();
    assert!((6..=12).contains(&on_ground), "two stone layers, 3 to 6 each: {piles:?}");
    assert_eq!(cargo(dug), &json!({}));
    // 5.
    assert_eq!(cargo(picked)["stone"], 5, "{}", cargo(picked));
    // 6.
    assert_eq!(tree(stone_stocked, "tree_stone", "stone"), 5);
    assert_eq!(cargo(stone_stocked), &json!({}));
    // 7.
    let second = building(wood_built, 1);
    assert_eq!((&second["kind"], &second["status"]), (&json!("wood_gatherer"), &json!("idle")), "{second}");
    // 3 stone left in the tree, and whatever Reed brought meanwhile.
    assert_eq!(tree(wood_built, "tree_wood", "wood"), 6);
    assert!(tree(wood_built, "tree_stone", "stone") >= 3);
    assert_eq!(cargo(wood_built), &json!({}));
    // 8. Both staffed and working, and both made wood in the 6 s between.
    for i in [both, both_later] {
        assert_eq!((&building(i, 0)["occupant"], &building(i, 1)["occupant"]), (&json!({"sprite": "sprite_fern"}), &json!({"sprite": "sprite_ash"})));
        assert_eq!((&building(i, 0)["status"], &building(i, 1)["status"]), (&json!("working"), &json!("working")));
    }
    assert!(output(both_later, 0, "wood") > output(both, 0, "wood") && output(both_later, 1, "wood") > output(both, 1, "wood"));
    // 9. Fern put out and waiting (the other Wood Gatherer is Ash's), the
    // building at 4x: a unit every 1.25 s, against one per 5 s.
    let seated = &receipts[possessed]["state"]["settlement"];
    assert_eq!(seated["seat"], json!({"building": 0}), "{seated}");
    assert_eq!(building(possessed, 0)["occupant"], json!({"elder": "elder"}));
    assert_eq!((&sprite(possessed, "sprite_fern")["job"], &sprite(possessed, "sprite_fern")["activity"]), (&json!("wood_gatherer"), &json!("waiting")));
    let made = output(possessed_later, 0, "wood") - output(possessed, 0, "wood");
    assert!((3..=5).contains(&made), "{made} units in 5 s");
    let added = output(burst, 0, "wood") - output(possessed_later, 0, "wood");
    assert!((5..=6).contains(&added), "a Burst added {added}");
    // 10. Fern back in, with no job sent since step 2's.
    let last = &receipts[left]["state"]["settlement"];
    assert_eq!(last["seat"], Value::Null);
    assert_eq!(building(left, 0)["occupant"], json!({"sprite": "sprite_fern"}), "{}", building(left, 0));
    assert_eq!((&sprite(left, "sprite_fern")["job"], &sprite(left, "sprite_fern")["activity"]), (&json!("wood_gatherer"), &json!("working")));
    assert_eq!(building(left, 1)["occupant"], json!({"sprite": "sprite_ash"}));
    // 6, later: Reed, mining, has brought the stone the elder left on the
    // ground to the tree.
    assert_eq!(sprite(left, "sprite_reed")["job"], "mine");
    let left_over = on_ground - 5;
    // (A unit may still be on its way, or in a sprite's hands for the last
    // trip, as the receipt is taken.)
    let stone = &receipts[left]["state"]["settlement"]["balances"]["stone"];
    let on_the_way = stone["in_transit"].as_u64().unwrap_or(0) + stone["carried"].as_u64().unwrap_or(0);
    assert!(tree(left, "tree_stone", "stone") + on_the_way >= 3 + left_over, "Reed brought the {left_over} left on the ground: {} and {on_the_way} on the way", tree(left, "tree_stone", "stone"));
    for i in [start, first_wood, stocked, at_vein, dug, picked, stone_stocked, wood_built, both, both_later, possessed, possessed_later, burst, left] {
        assert!(receipts[i]["state"]["settlement"]["balances"].as_object().unwrap().values().all(|b| b["conserved"] == true), "receipt {i}");
    }
}

/// The build key is refused while the elder possesses a building, with a
/// hint that names the key that leaves.
#[test]
fn building_waits_until_the_elder_leaves_its_building() {
    let report = run(vec![
        Command::Scene { scene: "sacred-tree".into() },
        Command::SettlementBoard { building: 0 },
        Command::SettlementBuild { kind: None },
    ]);
    assert_eq!(report["status"], "failed", "{report}");
    assert!(report["error"].as_str().unwrap().contains("Leave the building first (E)"), "{report}");
    let report = run(vec![
        Command::Scene { scene: "sacred-tree".into() },
        Command::SettlementBoard { building: 0 },
        Command::SettlementBuild { kind: Some("ballista".into()) },
    ]);
    assert_eq!(report["status"], "failed", "{report}");
    assert!(report["error"].as_str().unwrap().contains("Leave the building first (E)"), "{report}");
}
