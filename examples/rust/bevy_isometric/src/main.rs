//! Kestrel: an asset-free voxel action scene. See ../README.md.
mod assets;
mod control;
mod effects;
mod gameplay;
mod impact;
mod jetpack;
mod navigation;
mod palimpsest;
mod pile_model;
mod piles;
mod preview;
mod rig;
mod scene;
mod settlement;
mod sound;
mod style;

use bevy::{app::AppExit, prelude::*};
use gameplay::*;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let options = control::Options::from_args(&args).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2)
    });
    let mut scenes = assets::SceneLibrary::new();
    if let Some(index) = options.initial_scene() {
        scenes.initial = index;
    }
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--scene") {
        scenes.initial = match pair[1].as_str() {
            "mining-deck" => 0,
            "boulder-lab" => 1,
            "pile-lab" => 2,
            "jetpack-lab" => 3,
            "surprise-lab" => 4,
            "atlantis" => 5,
            "forest-outpost" => 6,
            name => {
                eprintln!("Unknown scene {name}");
                std::process::exit(2)
            }
        };
    }
    let hub = control::Hub::new(options.clone()).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });
    build_app(options, scenes, hub).run();
}

fn build_app(options: control::Options, scenes: assets::SceneLibrary, hub: control::Hub) -> App {
    use bevy::{app::ScheduleRunnerPlugin, time::TimeUpdateStrategy};
    use std::time::Duration;
    let simulation = options.simulation;
    let offscreen = options.frames_dir.is_some();
    let fps = options.fps();
    let deterministic = simulation || options.script.is_some();
    let mut app = App::new();
    app.insert_resource(options)
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
                assets::switch_scene,
                jetpack::scene_changed,
                palimpsest::setup,
                style::apply,
                settlement::setup,
            )
                .chain(),
            piles::setup_scene,
            (control::begin, settlement::tick).chain(),
            piles::input.run_if(control::interactive),
            mouse_commands.run_if(control::interactive),
            preview::playback.run_if(control::interactive),
            (jetpack::update, move_player, palimpsest::tick).chain(),
            piles::arrive,
            piles::update_transit,
            piles::render,
            piles::update_display.run_if(control::rendered),
            piles::showcase.run_if(control::interactive),
            combat,
            animate_character,
            effects::update_effects,
            effects::camera_feedback.run_if(control::rendered),
            scene::update_hud.run_if(control::rendered),
            preview::capture.run_if(control::interactive),
            control::end,
            control::capture_frame.run_if(control::rendered),
        )
            .chain(),
    );
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
