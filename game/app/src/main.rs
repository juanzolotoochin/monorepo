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

use bevy::{app::AppExit, prelude::*};
use service::{AppServices, Phase, Service, ServicesPlugin, WorldLayer, WorldState};
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
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--scene") {
        scene = scenes.index(&pair[1]).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(2)
        });
    }
    let hub = control::Hub::new(options.clone()).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });
    let replay = args.windows(2).find(|pair| pair[0] == "--replay-input").map(|pair| {
        let read = |path: &str| std::fs::read(path).unwrap_or_else(|e| {
            eprintln!("{path}: {e}");
            std::process::exit(2)
        });
        let log = save::InputLog::from_bytes(&read(&pair[1])).unwrap_or_else(|e| {
            eprintln!("{}: {e}", pair[1]);
            std::process::exit(2)
        });
        (read(&format!("{}.gsav", pair[1])), log)
    });
    let launch = flow::Launch {
        network,
        scene,
        // `--name`, or who is logged in.
        name: args
            .windows(2)
            .find(|pair| pair[0] == "--name")
            .map(|pair| pair[1].clone())
            .or_else(|| std::env::var("USERNAME").or_else(|_| std::env::var("USER")).ok())
            .unwrap_or_default(),
        record: args.windows(2).find(|pair| pair[0] == "--record-input").map(|pair| pair[1].clone()),
        replay,
    };
    let mut app = build_app(options, scenes, hub, launch);
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

/// The views that don't have services of their own yet: they live as long
/// as a world, and run only while it is active.
#[derive(Resource)]
struct Views;

impl Service for Views {
    type Layer = WorldLayer;
    type Deps = (lockstep::WorldMirror, scene_view::SceneView);

    fn init(_world: &mut World) -> Self {
        Views
    }
}

fn build_app(options: control::Options, scenes: assets::SceneLibrary, hub: control::Hub, launch: flow::Launch) -> App {
    use bevy::{app::ScheduleRunnerPlugin, time::TimeUpdateStrategy};
    use std::time::Duration;
    let simulation = options.simulation;
    let offscreen = options.frames_dir.is_some();
    let fps = options.fps();
    let deterministic = simulation || options.script.is_some();
    let mut app = App::new();
    app.insert_resource(options)
        .insert_resource(launch)
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
    // The frame, in the service layers' phases (see
    // game/docs/proposals/service_layers.md). Telemetry and the control port
    // wrap the whole frame; everything else belongs to a service.
    let active = in_state(WorldState::Active);
    app.add_plugins(ServicesPlugin)
        .add_service::<flow::Flow>()
        .add_service::<lockstep::SessionService>()
        .add_service::<lockstep::WorldMirror>()
        .add_service::<scene_view::SceneView>()
        .add_service::<piles::PileLab>()
        .add_service::<Views>()
        .add_service::<digging::Digging>()
        .add_service::<jetpack::Flight>()
        .add_service::<palimpsest::Memory>()
        .add_service::<lockstep::PlayerControl>()
        .add_systems(First, (telemetry::begin, control::begin).chain())
        .add_systems(
            Last,
            ((telemetry::end, control::end).chain(), control::capture_frame.run_if(control::rendered))
                .chain()
                .before(service::StopOnExit),
        )
        .add_service_systems::<Views, _>(
            Phase::Input,
            settlement::setup.run_if(active.clone()),
        )
        .add_service_systems::<lockstep::PlayerControl, _>(
            Phase::Input,
            (
                (piles::input, settlement::input, lockstep::mouse_input, lockstep::attack_input)
                    .chain()
                    .run_if(control::interactive),
                lockstep::keyboard_input,
                preview::playback.run_if(control::interactive),
            )
                .chain()
                .run_if(active.clone()),
        )
        .add_service_systems::<Views, _>(
            Phase::Mirror,
            (
                lockstep::mirror_settlement,
                lockstep::follow_avatar,
                peers::update.run_if(control::rendered),
            )
                .chain()
                .run_if(active.clone()),
        )
        .add_service_systems::<Views, _>(
            Phase::Present,
            (
                settlement::tick,
                (combat, animate_character, style::select_weapon).chain(),
                effects::update_effects,
                effects::camera_feedback.run_if(control::rendered),
                scene::update_hud.run_if(control::rendered),
                preview::capture.run_if(control::interactive),
            )
                .chain()
                .run_if(active.clone()),
        );
    if !simulation {
        app.add_systems(
            Update,
            (draw_navigation.run_if(active.clone()), keyboard_actions).run_if(control::interactive),
        )
        .add_systems(
            PostUpdate,
            (
                assets::update_captions,
                piles::captions,
                settlement::captions,
                nametags::update,
                digging::feedback,
            )
                .after(bevy::transform::TransformSystems::Propagate)
                .run_if(active),
        );
    }
    app
}

/// Builds the app as `run` would, for tests that drive frames by hand:
/// finishing the plugins attaches each service's systems to its layer.
#[cfg(test)]
fn test_app(options: control::Options, scenes: assets::SceneLibrary, hub: control::Hub, launch: flow::Launch) -> App {
    let mut app = build_app(options, scenes, hub, launch);
    app.finish();
    app.cleanup();
    app
}

/// Runs frames until the launch's world is up and its views are set up.
#[cfg(test)]
fn start_world(app: &mut App) {
    for _ in 0..20 {
        app.update();
        let active = app.world().get_resource::<State<WorldState>>().is_some_and(|s| *s.get() == WorldState::Active);
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
        let active = app.world().get_resource::<State<WorldState>>().is_some_and(|s| *s.get() == WorldState::Active);
        let shown = app.world().get_resource::<scene_view::SceneView>().map(|view| view.index);
        if active && shown == Some(index) {
            return;
        }
    }
    panic!("the world never switched to scene {index}");
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
