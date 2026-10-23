//! Kestrel: an asset-free voxel action scene. See ../README.md.
mod assets;
mod control;
mod effects;
mod gameplay;
mod impact;
mod jetpack;
mod legacy;
mod lockstep;
mod navigation;
mod palimpsest;
mod peers;
mod pile_model;
mod piles;
mod preview;
mod rig;
mod scene;
mod settlement;
mod sound;
mod style;
mod telemetry;

use bevy::{app::AppExit, prelude::*};
use gameplay::*;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let options = control::Options::from_args(&args).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2)
    });
    let mut scenes = assets::SceneLibrary::new();
    if let Some(index) = options.initial_scene(&scenes).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2)
    }) {
        scenes.initial = index;
    }
    let network = network(&args).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2)
    });
    if network.is_some() {
        // Networked games start on the field unless asked otherwise, and
        // every peer must pick the same scene.
        scenes.initial = scenes.index("field").expect("the field scene exists");
        scenes.locked = true;
    }
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--scene") {
        scenes.initial = scenes.index(&pair[1]).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(2)
        });
    }
    let hub = control::Hub::new(options.clone()).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });
    let mut game = lockstep::Game::default();
    game.network = network;
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--record-input") {
        game.record = Some(pair[1].clone());
    }
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--replay-input") {
        let read = |path: &str| std::fs::read(path).unwrap_or_else(|e| {
            eprintln!("{path}: {e}");
            std::process::exit(2)
        });
        let log = save::InputLog::from_bytes(&read(&pair[1])).unwrap_or_else(|e| {
            eprintln!("{}: {e}", pair[1]);
            std::process::exit(2)
        });
        game.replay = Some((read(&format!("{}.gsav", pair[1])), log));
    }
    let mut app = build_app(options, scenes, hub);
    app.insert_resource(game);
    app.run();
}

/// `--host <port>` hosts a networked game; `--join <host:port>` joins one
/// (the host may be a name, such as a Tailscale machine name).
fn network(args: &[String]) -> Result<Option<lockstep::Network>, String> {
    use std::net::ToSocketAddrs;
    let value = |flag: &str| args.windows(2).find(|pair| pair[0] == flag).map(|pair| pair[1].as_str());
    match (value("--host"), value("--join")) {
        (Some(_), Some(_)) => Err("Pass --host or --join, not both".into()),
        (Some(port), None) => {
            let port: u16 = port.parse().map_err(|e| format!("Invalid --host port {port}: {e}"))?;
            Ok(Some(lockstep::Network::Host(([0, 0, 0, 0], port).into())))
        }
        (None, Some(host)) => {
            let addr = host
                .to_socket_addrs()
                .map_err(|e| format!("Cannot resolve --join {host}: {e}"))?
                .find(|a| a.is_ipv4())
                .ok_or_else(|| format!("No IPv4 address for --join {host}"))?;
            Ok(Some(lockstep::Network::Join(addr)))
        }
        (None, None) => Ok(None),
    }
}

/// Writes the recording and leaves the session when the app exits.
fn on_exit(mut exits: MessageReader<AppExit>, mut game: ResMut<lockstep::Game>) {
    if exits.read().next().is_some() {
        game.write_recording();
        if let Some(session) = game.session.as_mut() {
            session.shutdown();
        }
    }
}

fn build_app(options: control::Options, scenes: assets::SceneLibrary, hub: control::Hub) -> App {
    use bevy::{app::ScheduleRunnerPlugin, time::TimeUpdateStrategy};
    use std::time::Duration;
    let simulation = options.simulation;
    let offscreen = options.frames_dir.is_some();
    let fps = options.fps();
    let deterministic = simulation || options.script.is_some();
    let mut app = App::new();
    app.init_resource::<lockstep::Game>()
        .insert_resource(options)
        .insert_resource(hub)
        .init_resource::<control::Video>()
        .insert_resource(ClearColor(Color::srgb(0.018, 0.025, 0.045)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.57, 0.70, 1.0),
            brightness: 180.0,
            ..default()
        })
        .insert_resource(scenes)
        .init_resource::<assets::ModelCache>()
        .init_resource::<Controls>()
        .init_resource::<style::ActiveStyle>()
        .init_resource::<telemetry::Telemetry>()
        .init_resource::<settlement::Settlement>()
        .init_resource::<palimpsest::Memory>()
        .init_resource::<jetpack::Flight>()
        .init_resource::<piles::PileLab>()
        .init_resource::<effects::Feedback>()
        .insert_resource(if simulation || offscreen {
            sound::Sound::recording()
        } else {
            sound::Sound::new()
        });
    if simulation {
        app.add_plugins(
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(1))),
        )
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>();
    } else if offscreen {
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: bevy::window::ExitCondition::DontExit,
                    ..default()
                })
                .set(bevy::render::RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                .disable::<bevy::winit::WinitPlugin>(),
        )
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / f64::from(fps),
        )));
    } else {
        app.add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title:
                        "KESTREL // 07 — Right click: move · Shift + left click: strike · M: sound"
                            .into(),
                    resolution: (1440, 960).into(),
                    ..default()
                }),
                ..default()
            }),
        );
    }
    app.add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default());
    if !simulation {
        app.add_plugins((
            bevy::render::diagnostic::RenderDiagnosticsPlugin,
            bevy::render::diagnostic::MeshAllocatorDiagnosticPlugin,
        ));
    }
    if deterministic {
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / f64::from(fps),
        )));
    }
    app.add_systems(Startup, (scene::setup, scene::hud).chain());
    if offscreen {
        app.add_systems(Startup, control::setup_render.after(scene::setup));
    }
    app.add_systems(
        Update,
        (
            (
                telemetry::begin,
                assets::switch_scene,
                jetpack::scene_changed,
                palimpsest::setup,
                style::apply,
            )
                .chain(),
            (piles::setup_scene, settlement::setup, lockstep::build_world).chain(),
            control::begin,
            (piles::input, settlement::input, lockstep::mouse_input, lockstep::attack_input)
                .chain()
                .run_if(control::interactive),
            lockstep::keyboard_input,
            preview::playback.run_if(control::interactive),
            lockstep::step,
            (
                lockstep::mirror_settlement,
                piles::mirror,
                lockstep::follow_avatar,
                peers::update.run_if(control::rendered),
                jetpack::update,
                palimpsest::tick,
            )
                .chain(),
            settlement::tick,
            (
                piles::spawn_tosses,
                piles::update_transit,
                piles::render,
                piles::update_display.run_if(control::rendered),
                piles::showcase.run_if(control::interactive),
            )
                .chain(),
            (combat, animate_character, style::select_weapon).chain(),
            effects::update_effects,
            effects::camera_feedback.run_if(control::rendered),
            scene::update_hud.run_if(control::rendered),
            preview::capture.run_if(control::interactive),
            lockstep::end_frame,
            (telemetry::end, control::end).chain(),
            control::capture_frame.run_if(control::rendered),
        )
            .chain(),
    );
    app.add_systems(Last, on_exit);
    if !simulation {
        app.add_systems(
            Update,
            (draw_navigation, keyboard_actions).run_if(control::interactive),
        )
        .add_systems(
            PostUpdate,
            (
                assets::update_captions,
                piles::captions,
                settlement::captions,
            )
                .after(bevy::transform::TransformSystems::Propagate),
        );
    }
    app
}

fn keyboard_actions(
    keys: Res<ButtonInput<KeyCode>>,
    mut exit: MessageWriter<AppExit>,
    mut sound: ResMut<sound::Sound>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
    if keys.just_pressed(KeyCode::KeyM) {
        sound.muted = !sound.muted;
    }
}
