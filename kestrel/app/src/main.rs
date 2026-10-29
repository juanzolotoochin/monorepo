//! Kestrel: an asset-free voxel action scene. See ../README.md.
mod actor;
mod assets;
mod atmosphere;
mod character;
mod character_catalog;
mod combat;
mod control;
mod destruction;
mod effects;
mod event_prediction;
mod flow;
mod gameplay;
mod hurt;
mod impact;
mod input;
mod jetpack;
mod legacy;
mod lockstep;
mod menu;
mod digging;
mod nametags;
mod navigation;
mod obstacles;
mod palimpsest;
mod piano;
mod pause;
mod peers;
mod pile_model;
mod prediction;
mod piles;
mod preview;
mod rig;
mod scene;
mod scene_view;
mod settlement;
mod sound;
mod style;
mod telemetry;
mod vehicles;
mod vfx;
mod vfx_stress;
mod viewer;

use bevy::prelude::*;
use service::{AppServices, ServicesPlugin};
use gameplay::Controls;

/// The command line; in the browser, the page's query string
/// (`control::Options::args_from_query`).
#[cfg(not(target_arch = "wasm32"))]
fn command_line() -> Vec<String> {
    std::env::args().collect()
}
#[cfg(target_arch = "wasm32")]
fn command_line() -> Vec<String> {
    use wasm_bindgen::prelude::wasm_bindgen;
    #[wasm_bindgen]
    extern "C" {
        type Location;
        #[wasm_bindgen(thread_local_v2, js_name = location)]
        static LOCATION: Location;
        #[wasm_bindgen(method, getter)]
        fn search(this: &Location) -> String;
    }
    control::Options::args_from_query(&LOCATION.with(Location::search))
}

/// Reports a bad command line and exits; in the browser, logs it to the
/// console, where stderr doesn't reach, and stops.
fn exit_with(error: String) -> ! {
    #[cfg(target_arch = "wasm32")]
    {
        #[wasm_bindgen::prelude::wasm_bindgen]
        extern "C" {
            #[wasm_bindgen(js_namespace = console, js_name = error)]
            fn console_error(message: &str);
        }
        console_error(&error);
        panic!("{error}");
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        eprintln!("{error}");
        std::process::exit(2)
    }
}

fn main() {
    let args = command_line();
    let options = control::Options::from_args(&args).unwrap_or_else(|error| {
        exit_with(format!("{error}"))
    });
    let scenes = assets::SceneLibrary::new();
    let mut scene = options
        .initial_scene(&scenes)
        .unwrap_or_else(|error| {
            exit_with(format!("{error}"))
        })
        .unwrap_or(0);
    let value = |flag: &str| args.windows(2).find(|pair| pair[0] == flag).map(|pair| pair[1].clone());
    let network = network(&args).unwrap_or_else(|error| {
        exit_with(format!("{error}"))
    });
    if network.is_some() {
        // Networked games start on the field unless asked otherwise. The host
        // may switch scenes and everyone follows; a client shows whatever
        // scene the host's world is.
        scene = scenes.index("field").expect("the field scene exists");
    }
    if let Some(name) = value("--scene") {
        scene = scenes.index(&name).unwrap_or_else(|error| {
            exit_with(format!("{error}"))
        });
    }
    let hub = control::Hub::new(options.clone()).unwrap_or_else(|e| {
        exit_with(format!("{e}"))
    });
    let replay = value("--replay-input").map(|path| {
        let read = |path: &str| std::fs::read(path).unwrap_or_else(|e| {
            exit_with(format!("{path}: {e}"))
        });
        let log = save::InputLog::from_bytes(&read(&path)).unwrap_or_else(|e| {
            exit_with(format!("{path}: {e}"))
        });
        (read(&format!("{path}.gsav")), log)
    });
    // `--input-margin-ms <ms>` pins a joining client's input margin, for
    // network tests; by default it adapts to the measured network.
    let input_margin_ms = value("--input-margin-ms").map(|ms| {
        ms.trim().parse::<u64>().unwrap_or_else(|e| {
            exit_with(format!("--input-margin-ms: invalid number {ms}: {e}"))
        })
    });
    // `--profile <name>` joins with that profile's saved player ID instead
    // of the default one, to run several players on one machine.
    let profile = value("--profile");
    let defaults = flow::Defaults {
        // `--name`, or who is logged in.
        name: value("--name")
            .or_else(|| std::env::var("USERNAME").or_else(|_| std::env::var("USER")).ok())
            .unwrap_or_default(),
        scene,
        record: value("--record-input"),
        port: value("--host").and_then(|port| port.parse().ok()).unwrap_or(flow::DEFAULT_PORT),
        address: value("--join").unwrap_or_else(|| format!("127.0.0.1:{}", flow::DEFAULT_PORT)),
        input_margin_ms,
        profile: profile.clone(),
    };
    // A player's run with no session flags opens the menu (or, with
    // `--character-studio`, Character Studio); anything else starts its
    // session right away.
    let asked = network.is_some()
        || replay.is_some()
        || ["--scene", "--record-input", "--capture-dir"].iter().any(|flag| args.iter().any(|arg| arg == flag));
    let studio = args.iter().any(|arg| arg == "--character-studio");
    let launch = (!studio && (asked || !options.menus())).then(|| flow::Launch {
        network,
        scene,
        name: defaults.name.clone(),
        record: defaults.record.clone(),
        replay,
        input_margin_ms,
        profile,
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
    let fullscreen = options.fullscreen;
    let offscreen = options.frames_dir.is_some();
    let fps = options.fps();
    let deterministic = simulation || options.script.is_some() || options.studio_frames.is_some();
    let menus = options.menus();
    let stress = options.vfx_stress.clone();
    let mut app = App::new();
    // Development only: synthetic controllers replace native ones and play
    // a timed scenario (`--gamepad-script`, see `input::synthetic`).
    if let Some(script) = options.gamepad_script.clone() {
        app.insert_resource(input::synthetic::SyntheticGamepads::default()).insert_resource(script);
    }
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
        .init_resource::<piano::PianoInput>()
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
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<atmosphere::GrassMaterial>>()
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
        #[allow(unused_mut)] // The wasm canvas configuration below mutates it.
        let mut window = Window {
            title: "KESTREL // 07 — WASD: move · Shift: run · Left click: strike · M: sound".into(),
            resolution: (1440, 960).into(),
            mode: if fullscreen {
                bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Primary)
            } else {
                bevy::window::WindowMode::Windowed
            },
            ..default()
        };
        #[cfg(target_arch = "wasm32")]
        {
            window.canvas = Some("#kestrel".into());
            window.fit_canvas_to_parent = true;
        }
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(window),
            ..default()
        }));
        // Native controllers on Linux and Windows (see input/native.rs).
        // A synthetic scenario (`--gamepad-script`) takes their place.
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        app.add_plugins(bevy_gilrs::GilrsPlugin);
    }
    if !simulation {
        app.add_plugins(bevy::pbr::MaterialPlugin::<atmosphere::GrassMaterial>::default());
        atmosphere::register_shader(&mut app);
        // GPU particles (kestrel/VFX_SPIKE_HANDOFF.md): they need the renderer
        // and compute shaders, so the web build uses WebGPU.
        app.add_plugins(bevy_sprinkles::SprinklesPlugin);
    }
    app.add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default());
    // Routed input in every run; the egui debug overlay only in windowed
    // runs a player drives (as `control::interactive`), whatever the menus.
    app.add_plugins(debug_ui::DebugUiPlugin { interactive: !simulation && !offscreen });
    if !simulation {
        app.add_plugins((
            bevy::render::diagnostic::RenderDiagnosticsPlugin,
            bevy::render::diagnostic::MeshAllocatorDiagnosticPlugin,
        ));
        // Debug only: the VFX spike's stress harness (`vfx_stress.rs`).
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(spec) = stress {
            app.add_plugins(vfx_stress::StressPlugin(spec));
        }
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
    // The frame (see kestrel/docs/service-layers.md): telemetry and
    // the control port wrap it; the debug UI's pass and input routing run
    // before the Input phase (`debug_ui::DebugUiSystems`); everything else
    // belongs to a service, and services tick by layer, then in the order
    // registered here.
    // Character Studio poses the player's body outside a world, so its
    // playback state exists in every run; only players' runs can open it.
    // The debug menu manager comes first: it outlives its providers.
    app.add_plugins(ServicesPlugin)
        .add_service::<debug_ui::ImguiManager>()
        .add_service::<input::InputService>()
        .add_service::<flow::Flow>()
        .add_service::<viewer::Viewer>()
        .add_service::<telemetry::TelemetryPanels>()
        .add_service::<vfx::Vfx>();
    if menus {
        app.add_service::<menu::MainMenu>().add_service::<pause::Pause>();
    }
    app.add_service::<lockstep::SessionService>()
        .add_service::<lockstep::WorldMirror>()
        .add_service::<scene_view::SceneView>()
        .add_systems(PostUpdate, piano::update.run_if(in_state(service::WorldState::Active)))
        .add_service::<settlement::Settlement>()
        .add_service::<piles::PileLab>()
        .add_service::<character::Avatar>()
        .add_service::<peers::Peers>()
        .add_service::<digging::Digging>()
        .add_service::<vehicles::Vehicles>()
        .add_service::<jetpack::Flight>()
        .add_service::<palimpsest::Memory>()
        .add_service::<effects::Effects>()
        .add_service::<atmosphere::AtmosphereVisuals>()
        .add_service::<scene::Hud>()
        .add_service::<input::gameplay::Dispatcher>()
        .add_service::<lockstep::PlayerControl>()
        .add_systems(First, (telemetry::begin, control::begin).chain())
        .add_systems(Update, telemetry::shortcut.in_set(debug_ui::DebugUiSystems::Shortcuts))
        .add_systems(
            PostUpdate,
            telemetry::draw_preview_corrections.run_if(resource_exists::<bevy::gizmos::config::GizmoConfigStore>),
        )
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
    test_front_app(options, scenes, hub, Some(launch))
}

/// Builds the app for a test, launching a session or (with no launch)
/// staying in the front end.
#[cfg(test)]
fn test_front_app(options: control::Options, scenes: assets::SceneLibrary, hub: control::Hub, launch: Option<flow::Launch>) -> App {
    let scene = launch.as_ref().map_or(0, |launch| launch.scene);
    let defaults = flow::Defaults { name: String::new(), scene, record: None, port: flow::DEFAULT_PORT, address: String::new(), input_margin_ms: None, profile: None };
    let mut app = build_app(options, scenes, hub, flow::Start { launch, defaults });
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
