use super::protocol::{Command, PileCheck};
use super::*;
fn run(steps: Vec<Command>) -> Value {
    run_at(steps, 30)
}
fn run_at(steps: Vec<Command>, fps: u32) -> Value {
    let script = Script {
        version: 1,
        scene: "pile-lab".into(),
        fps,
        width: 1440,
        height: 960,
        max_frames: 1200,
        steps,
    };
    let options = Options {
        simulation: true,
        script: Some(script),
        ..default()
    };
    let mut scenes = SceneLibrary::new();
    scenes.initial = 2;
    let hub = Hub::new(options.clone()).unwrap();
    let mut app = crate::build_app(options, scenes, hub);
    for _ in 0..1300 {
        app.update();
        let world = app.world_mut();
        let p = player_position(world);
        let library = world.resource::<SceneLibrary>();
        if library.current == 3 {
            assert!(
                crate::jetpack::height(library, p).is_none_or(|y| p.y >= y - 0.025),
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
    let mut scenes = SceneLibrary::new();
    scenes.initial = 3;
    let hub = Hub::new(options.clone()).unwrap();
    let mut app = crate::build_app(options, scenes, hub);
    app.update();
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
    assert!((p.y - crate::jetpack::HOVER).abs() < 0.01);
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
        app.world_mut().resource_mut::<SceneLibrary>().requested = Some(index);
        app.update();
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
    assert!(memory["sample_count"].as_u64().unwrap() <= 243);
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
