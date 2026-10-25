//! Kestrel: an asset-free voxel action scene. See ../README.md.
mod assets;
mod character;
mod character_catalog;
mod combat;
mod control;
mod effects;
mod gameplay;
mod atmosphere;
mod impact;
mod jetpack;
mod legacy;
mod lockstep;
mod menu;
mod digging;
mod nametags;
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
mod viewer;

use bevy::{app::AppExit, prelude::*};
use gameplay::*;
use character::animate_character;
use combat::combat;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mut options = control::Options::from_args(&args).unwrap_or_else(|error| {
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
    options.menu = !options.simulation
        && options.script.is_none()
        && options.port.is_none()
        && options.frames_dir.is_none()
        && network.is_none()
        && !args.iter().any(|arg| matches!(arg.as_str(), "--scene" | "--record-input" | "--replay-input" | "--capture-dir"));
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
    // `--name`, or who is logged in.
    game.name = args
        .windows(2)
        .find(|pair| pair[0] == "--name")
        .map(|pair| pair[1].clone())
        .or_else(|| std::env::var("USERNAME").or_else(|_| std::env::var("USER")).ok())
        .unwrap_or_default();
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
    let studio = std::env::args().any(|arg| arg == "--character-studio");
    let mut launch_menu = menu::LaunchMenu::new(options.menu, &scenes);
    let mut viewer = viewer::Viewer::default();
    if studio {
        viewer.begin();
        launch_menu.open = false;
        let args: Vec<_> = std::env::args().collect();
        let requested_character = args.windows(2)
            .find(|pair| pair[0] == "--studio-character")
            .map(|pair| pair[1].as_str());
        viewer.character = requested_character
            .map(|id| scenes.characters.index_for_id(id).unwrap_or_else(|error| panic!("{error}")))
            .or_else(|| scenes.characters.iter().position(|entry| entry.scene_index == scenes.initial))
            .or_else(|| (!scenes.characters.is_empty()).then_some(0))
            .expect("Character Studio needs at least one baked actor");
        if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--studio-clip") {
            viewer.clip = viewer::Clip::from_id(&pair[1]).unwrap_or(viewer::Clip::Run);
        }
        if !viewer.clip.supports(&scenes.characters[viewer.character]) {
            viewer.clip = if viewer::Clip::Run.supports(&scenes.characters[viewer.character]) {
                viewer::Clip::Run
            } else {
                viewer::Clip::Idle
            };
        }
        if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--studio-angle") {
            viewer.angle = match pair[1].as_str() {
                "front" => viewer::ViewAngle::Front,
                "side" => viewer::ViewAngle::Side,
                _ => viewer::ViewAngle::ThreeQuarter,
            };
        }
    }
    let mut app = App::new();
    app.init_resource::<lockstep::Game>()
        .insert_resource(options)
        .insert_resource(hub)
        .init_resource::<control::Video>()
        .insert_resource(launch_menu)
        .insert_resource(viewer)
        .insert_resource(ClearColor(Color::srgb(0.018, 0.025, 0.045)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.57, 0.70, 1.0),
            brightness: 180.0,
            ..default()
        })
        .insert_resource(scenes)
        .init_resource::<assets::ModelCache>()
        .init_resource::<scene::SceneWorldBuilt>()
        .init_resource::<Controls>()
        .init_resource::<style::ActiveStyle>()
        .init_resource::<atmosphere::AtmosphereVisuals>()
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
    if !simulation {
        app.add_plugins(bevy::pbr::MaterialPlugin::<atmosphere::GrassMaterial>::default());
        atmosphere::register_shader(&mut app);
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
    app.add_systems(Startup, menu::setup.after(scene::hud));
    if offscreen {
        app.add_systems(Startup, control::setup_render.after(scene::setup));
    }
    app.add_systems(
        Update,
        (
            (
                telemetry::begin,
                scene::ensure_environment.run_if(viewer::not_active),
                assets::switch_scene.run_if(viewer::not_active),
                viewer::select_character_pack,
                jetpack::scene_changed.run_if(viewer::not_active),
                palimpsest::setup.run_if(viewer::not_active),
                style::apply,
                atmosphere::sync,
            )
                .chain(),
            (piles::setup_scene, settlement::setup, lockstep::build_world)
                .chain().run_if(viewer::not_active),
            control::begin,
            (piles::input, settlement::input, lockstep::mouse_input, lockstep::attack_input)
                .chain()
                .run_if(control::interactive).run_if(viewer::not_active),
            lockstep::keyboard_input.run_if(viewer::not_active),
            preview::playback.run_if(control::interactive).run_if(viewer::not_active),
            lockstep::step.run_if(viewer::not_active),
            (
                lockstep::mirror_settlement,
                piles::mirror,
                lockstep::follow_avatar,
                peers::update.run_if(control::rendered),
                digging::update,
                digging::follow_camera,
                jetpack::update,
                palimpsest::tick,
            )
                .chain().run_if(viewer::not_active),
            settlement::tick.run_if(viewer::not_active),
            (
                piles::spawn_tosses,
                piles::update_transit,
                piles::render,
                piles::update_display.run_if(control::rendered),
                piles::showcase.run_if(control::interactive),
            )
                .chain().run_if(viewer::not_active),
            (viewer::drive, combat.run_if(viewer::not_active), animate_character, style::select_weapon, viewer::pose_selected_rig).chain(),
            (
                preview::frame_character.run_if(control::interactive),
                effects::update_effects,
            ).chain().run_if(viewer::not_active),
            atmosphere::animate,
            effects::camera_feedback.run_if(control::rendered).run_if(viewer::not_active),
            scene::update_hud.run_if(control::rendered).run_if(viewer::not_active),
            preview::capture.run_if(control::interactive).run_if(viewer::not_active),
            lockstep::end_frame.run_if(viewer::not_active),
            lockstep::end_session.run_if(viewer::not_active),
            (telemetry::end, control::end).chain(),
            control::capture_frame.run_if(control::rendered),
        )
            .chain()
            .run_if(menu::playing),
    );
    app.add_systems(Update, menu::setup.before(menu::update));
    app.add_systems(Update, menu::update.before(telemetry::begin).run_if(control::interactive));
    app.add_systems(Update, viewer::setup.before(viewer::input));
    app.add_systems(Update, viewer::input.before(telemetry::begin).run_if(control::interactive));
    app.add_systems(Update, viewer::sync_hud.after(viewer::input));
    app.add_systems(Update, viewer::camera.after(effects::camera_feedback).run_if(menu::playing));
    app.add_systems(Update, viewer::capture.after(viewer::camera).run_if(control::interactive));
    app.add_systems(Update, menu::capture.after(menu::update).run_if(control::interactive));
    app.add_systems(Last, on_exit);
    if !simulation {
        app.add_systems(
            Update,
            keyboard_actions.run_if(control::interactive).run_if(viewer::not_active),
        )
        .add_systems(Update, draw_navigation.run_if(control::interactive).run_if(menu::playing).run_if(viewer::not_active))
        .add_systems(
            PostUpdate,
            (
                viewer::metrics,
                effects::blade_trail,
                assets::update_captions,
                piles::captions,
                settlement::captions,
                nametags::update,
                digging::feedback,
            )
                .after(bevy::transform::TransformSystems::Propagate)
                .run_if(menu::playing),
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
