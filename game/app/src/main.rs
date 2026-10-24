//! Kestrel: an asset-free voxel action scene. See ../README.md.
mod assets;
mod control;
mod effects;
mod flow;
mod gameplay;
mod impact;
mod jetpack;
mod legacy;
mod lockstep;
mod menu;
mod digging;
mod nametags;
mod navigation;
mod obstacles;
mod palimpsest;
mod peers;
mod pile_model;
mod piles;
mod preview;
mod rig;
mod scene;
mod scene_view;
mod settlement;
mod sound;
mod style;
mod telemetry;

use bevy::prelude::*;
use service::{AppServices, ServicesPlugin};
use gameplay::*;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let options = control::Options::from_args(&args).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2)
    });
    let scenes = assets::SceneLibrary::new();
    let mut scene = options
        .initial_scene(&scenes)
        .unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(2)
        })
        .unwrap_or(0);
    let value = |flag: &str| args.windows(2).find(|pair| pair[0] == flag).map(|pair| pair[1].clone());
    let network = network(&args).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2)
    });
    if network.is_some() {
        // Networked games start on the field unless asked otherwise. The host
        // may switch scenes and everyone follows; a client shows whatever
        // scene the host's world is.
        scene = scenes.index("field").expect("the field scene exists");
    }
    if let Some(name) = value("--scene") {
        scene = scenes.index(&name).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(2)
        });
    }
    let hub = control::Hub::new(options.clone()).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });
    let replay = value("--replay-input").map(|path| {
        let read = |path: &str| std::fs::read(path).unwrap_or_else(|e| {
            eprintln!("{path}: {e}");
            std::process::exit(2)
        });
        let log = save::InputLog::from_bytes(&read(&path)).unwrap_or_else(|e| {
            eprintln!("{path}: {e}");
            std::process::exit(2)
        });
        (read(&format!("{path}.gsav")), log)
    });
    let defaults = flow::Defaults {
        // `--name`, or who is logged in.
        name: value("--name")
            .or_else(|| std::env::var("USERNAME").or_else(|_| std::env::var("USER")).ok())
            .unwrap_or_default(),
        scene,
        record: value("--record-input"),
        port: value("--host").and_then(|port| port.parse().ok()).unwrap_or(flow::DEFAULT_PORT),
        address: value("--join").unwrap_or_else(|| format!("127.0.0.1:{}", flow::DEFAULT_PORT)),
    };
    // A player's run with no session flags opens the menu; anything else
    // starts its session right away.
    let asked = network.is_some() || value("--scene").is_some() || replay.is_some();
    let launch = (asked || !options.menus()).then(|| flow::Launch {
        network,
        scene,
        name: defaults.name.clone(),
        record: defaults.record.clone(),
        replay,
    });
    let mut app = build_app(options, scenes, hub, flow::Start { launch, defaults });
    app.run();
}

/// `--host <port>` hosts a networked game; `--join <host:port>` joins one
/// (the host may be a name, such as a Tailscale machine name).
fn network(args: &[String]) -> Result<Option<lockstep::Network>, String> {
    let value = |flag: &str| args.windows(2).find(|pair| pair[0] == flag).map(|pair| pair[1].as_str());
    match (value("--host"), value("--join")) {
        (Some(_), Some(_)) => Err("Pass --host or --join, not both".into()),
        (Some(port), None) => flow::host(port).map(Some).map_err(|e| format!("--host: {e}")),
        (None, Some(host)) => flow::join(host).map(Some).map_err(|e| format!("--join: {e}")),
        (None, None) => Ok(None),
    }
}

fn build_app(options: control::Options, scenes: assets::SceneLibrary, hub: control::Hub, start: flow::Start) -> App {
    use bevy::{app::ScheduleRunnerPlugin, time::TimeUpdateStrategy};
    use std::time::Duration;
    let simulation = options.simulation;
    let offscreen = options.frames_dir.is_some();
    let fps = options.fps();
    let deterministic = simulation || options.script.is_some();
    let menus = options.menus();
    let mut app = App::new();
    app.insert_resource(options)
        .insert_resource(start)
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
    // The frame (see game/docs/proposals/service_layers.md): telemetry and
    // the control port wrap it; everything else belongs to a service, and
    // services tick by layer, then in the order registered here.
    app.add_plugins(ServicesPlugin).add_service::<flow::Flow>();
    if menus {
        app.add_service::<menu::MainMenu>();
    }
    app.add_service::<lockstep::SessionService>()
        .add_service::<lockstep::WorldMirror>()
        .add_service::<scene_view::SceneView>()
        .add_service::<settlement::Settlement>()
        .add_service::<piles::PileLab>()
        .add_service::<gameplay::Avatar>()
        .add_service::<peers::Peers>()
        .add_service::<digging::Digging>()
        .add_service::<jetpack::Flight>()
        .add_service::<palimpsest::Memory>()
        .add_service::<effects::Effects>()
        .add_service::<scene::Hud>()
        .add_service::<lockstep::PlayerControl>()
        .add_systems(First, (telemetry::begin, control::begin).chain())
        .add_systems(
            Last,
            ((telemetry::end, control::end).chain(), control::capture_frame.run_if(control::rendered))
                .chain()
                .before(service::StopOnExit),
        );
    app
}

/// Builds the app as `run` would, for tests that drive frames by hand:
/// finishing the plugins attaches each service's systems to its layer.
#[cfg(test)]
fn test_app(options: control::Options, scenes: assets::SceneLibrary, hub: control::Hub, launch: flow::Launch) -> App {
    let defaults = flow::Defaults { name: String::new(), scene: launch.scene, record: None, port: flow::DEFAULT_PORT, address: String::new() };
    let mut app = build_app(options, scenes, hub, flow::Start { launch: Some(launch), defaults });
    app.finish();
    app.cleanup();
    app
}

/// Runs frames until the launch's world is up and its views are set up.
#[cfg(test)]
fn start_world(app: &mut App) {
    for _ in 0..20 {
        app.update();
        let active = app.world().get_resource::<State<service::WorldState>>().is_some_and(|s| *s.get() == service::WorldState::Active);
        if active {
            return;
        }
    }
    panic!("the world never started");
}

/// Asks the session for another scene and runs frames until its world is up.
#[cfg(test)]
fn switch_world(app: &mut App, index: usize) {
    app.world_mut().resource_mut::<lockstep::SessionService>().request_scene(index);
    for _ in 0..20 {
        app.update();
        let active = app.world().get_resource::<State<service::WorldState>>().is_some_and(|s| *s.get() == service::WorldState::Active);
        let shown = app.world().get_resource::<scene_view::SceneView>().map(|view| view.index);
        if active && shown == Some(index) {
            return;
        }
    }
    panic!("the world never switched to scene {index}");
}
