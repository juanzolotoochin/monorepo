//! Kestrel: an asset-free voxel action scene. See ../README.md.
mod assets;
mod effects;
mod gameplay;
mod impact;
mod navigation;
mod preview;
mod scene;
mod sound;

use bevy::{app::AppExit, prelude::*};
use gameplay::*;

fn main() {
    let mut scenes = assets::SceneLibrary::new();
    let args: Vec<_> = std::env::args().collect();
    if let Some(pair) = args.windows(2).find(|pair| pair[0] == "--scene") {
        scenes.initial = match pair[1].as_str() {
            "mining-deck" => 0,
            "boulder-lab" => 1,
            name => {
                eprintln!("Unknown scene {name}; use mining-deck or boulder-lab");
                std::process::exit(2);
            }
        };
    }
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.018, 0.025, 0.045)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.57, 0.70, 1.0),
            brightness: 180.0,
            ..default()
        })
        .insert_resource(scenes)
        .init_resource::<assets::ModelCache>()
        .init_resource::<Controls>()
        .init_resource::<effects::Feedback>()
        .insert_resource(sound::Sound::new())
        .add_plugins(
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
        )
        .add_systems(Startup, (scene::setup, scene::hud))
        .add_systems(
            Update,
            (
                assets::switch_scene,
                mouse_commands,
                preview::playback,
                move_player,
                combat,
                animate_character,
                effects::update_effects,
                effects::camera_feedback,
                scene::update_hud,
                preview::capture,
            )
                .chain(),
        )
        .add_systems(Update, (draw_navigation, keyboard_actions))
        .add_systems(
            PostUpdate,
            assets::update_captions.after(bevy::transform::TransformSystems::Propagate),
        )
        .run();
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
