//! Procedural voxel art: shared cuboid geometry, authored silhouettes and palette.
use crate::{
    assets::{IndustrialProp, ModelCache, SceneLibrary},
    character::{Part, PartKind, Player},
    combat::{Obstacle, ACTIVE_END, ATTACK_END, REBOUND_END, WINDUP},
    effects::{CameraRig, FxAssets},
    gameplay::Controls,
    obstacles::{BULKHEADS, CRATES, REACTOR},
    sound::Sound,
};
use bevy::{
    anti_alias::taa::TemporalAntiAliasing, camera::Hdr, camera::ScalingMode,
    core_pipeline::tonemapping::Tonemapping, pbr::ScreenSpaceAmbientOcclusion,
    post_process::bloom::Bloom, prelude::*,
};

fn material(
    materials: &mut Assets<StandardMaterial>,
    rgb: [f32; 3],
    metal: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::srgb(rgb[0], rgb[1], rgb[2]),
        perceptual_roughness: 0.72,
        metallic: metal,
        ..default()
    })
}
fn glow(
    materials: &mut Assets<StandardMaterial>,
    rgb: [f32; 3],
    strength: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::srgb(rgb[0], rgb[1], rgb[2]),
        emissive: LinearRgba::rgb(rgb[0] * strength, rgb[1] * strength, rgb[2] * strength),
        ..default()
    })
}
fn block(
    parent: &mut ChildSpawnerCommands,
    cube: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    pos: Vec3,
    size: Vec3,
) {
    parent.spawn((
        Mesh3d(cube.clone()),
        MeshMaterial3d(mat.clone()),
        Transform::from_translation(pos).with_scale(size),
    ));
}
fn object(
    commands: &mut Commands,
    cube: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    pos: Vec3,
    size: Vec3,
) {
    commands.spawn((
        crate::jetpack::Deck,
        Mesh3d(cube.clone()),
        MeshMaterial3d(mat.clone()),
        Transform::from_translation(pos).with_scale(size),
    ));
}
fn hash(x: i32, y: i32, z: i32) -> u32 {
    let mut n = (x as u32).wrapping_mul(374761393)
        ^ (y as u32).wrapping_mul(668265263)
        ^ (z as u32).wrapping_mul(2246822519);
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    n ^ (n >> 16)
}

#[cfg(test)]
mod scene_construction_tests {
    use super::*;

    #[test]
    fn static_game_world_is_deferred_until_play() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<SceneWorldBuilt>()
            .add_systems(Startup, setup);
        app.update();
        assert!(!app.world().resource::<SceneWorldBuilt>().0);
        let world = app.world_mut();
        let mut decks = world.query_filtered::<Entity, With<crate::jetpack::Deck>>();
        assert_eq!(decks.iter(world).count(), 0);
        app.add_systems(Update, ensure_environment);
        app.update();
        assert!(app.world().resource::<SceneWorldBuilt>().0);
        let world = app.world_mut();
        let mut decks = world.query_filtered::<Entity, With<crate::jetpack::Deck>>();
        assert!(decks.iter(world).count() > 0);
    }
}

#[derive(Resource)]
pub struct SceneWorldAssets {
    cube: Handle<Mesh>,
    dark: Handle<StandardMaterial>,
    frame: Handle<StandardMaterial>,
    panel: Handle<StandardMaterial>,
    ivory: Handle<StandardMaterial>,
    orange: Handle<StandardMaterial>,
    gold: Handle<StandardMaterial>,
    cyan: Handle<StandardMaterial>,
    amber: Handle<StandardMaterial>,
    dust: Handle<StandardMaterial>,
    floors: [Handle<StandardMaterial>; 3],
    rock_dark: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
pub struct SceneWorldBuilt(bool);

pub fn ensure_environment(
    mut commands: Commands,
    assets: Res<SceneWorldAssets>,
    mut built: ResMut<SceneWorldBuilt>,
) {
    if built.0 { return; }
    built.0 = true;
    let SceneWorldAssets {
        cube, dark, frame, panel, ivory, orange, gold, cyan,
        amber, dust, floors, rock_dark,
    } = &*assets;
    // A heavy suspended deck with stepped strata and a luminous perimeter.
    object(
        &mut commands,
        &cube,
        &dark,
        Vec3::new(0.0, -0.72, 0.0),
        Vec3::new(17.5, 1.4, 17.5),
    );
    object(
        &mut commands,
        &cube,
        &frame,
        Vec3::new(0.0, -1.32, 0.0),
        Vec3::new(16.8, 0.26, 16.8),
    );
    for x in -8_i32..=8 {
        for z in -8_i32..=8 {
            let h = hash(x, 0, z);
            let sandy = ((x + 4).pow(2) + (z + 1).pow(2) < 9) || (x > 4 && z < 0);
            let mat = if sandy && h % 5 != 0 {
                &dust
            } else {
                &floors[(h % 3) as usize]
            };
            object(
                &mut commands,
                &cube,
                mat,
                Vec3::new(x as f32, -0.06, z as f32),
                Vec3::new(0.974, 0.12, 0.974),
            );
            if (z == 2 || z == -5) && x % 3 == 0 {
                object(
                    &mut commands,
                    &cube,
                    &cyan,
                    Vec3::new(x as f32, 0.008, z as f32 + 0.42),
                    Vec3::new(0.54, 0.014, 0.025),
                );
            }
            if h % 19 == 0 {
                for i in 0..3 {
                    object(
                        &mut commands,
                        &cube,
                        &dark,
                        Vec3::new(x as f32 - 0.22 + i as f32 * 0.20, 0.008, z as f32),
                        Vec3::new(0.065, 0.018, 0.56),
                    );
                }
            }
        }
    }
    for i in -8..=8 {
        for (x, z, sx, sz) in [
            (i as f32, -8.6, 0.94, 0.18),
            (i as f32, 8.6, 0.94, 0.18),
            (-8.6, i as f32, 0.18, 0.94),
            (8.6, i as f32, 0.18, 0.94),
        ] {
            object(
                &mut commands,
                &cube,
                &frame,
                Vec3::new(x, 0.08, z),
                Vec3::new(sx, 0.22, sz),
            );
            if i % 2 == 0 {
                object(
                    &mut commands,
                    &cube,
                    &cyan,
                    Vec3::new(x, -0.20, z),
                    Vec3::new(sx * 0.6, 0.035, sz * 0.6),
                );
            }
        }
    }
    // Hazard paint at the front edge, with inset side brackets.
    for i in 0..9 {
        object(
            &mut commands,
            &cube,
            if i % 2 == 0 { &gold } else { &dark },
            Vec3::new(-1.8 + i as f32 * 0.45, 0.008, 7.9),
            Vec3::new(0.29, 0.017, 0.50),
        );
    }
    for x in [-7.0, 7.0] {
        for z in [-7.0, -2.0, 3.0, 7.0] {
            object(
                &mut commands,
                &cube,
                &dark,
                Vec3::new(x, -1.5, z),
                Vec3::new(0.8, 0.5, 0.8),
            );
            object(
                &mut commands,
                &cube,
                &orange,
                Vec3::new(x, -1.25, z),
                Vec3::new(0.82, 0.12, 0.82),
            );
        }
    }

    // Boulders come from compiled scene packs (see assets.rs).

    // Back-left coolant relay: chunky housing, fins, emissive core and conduits.
    commands
        .spawn((
            Transform::from_translation(REACTOR.center),
            Visibility::default(),
            Obstacle(REACTOR),
            IndustrialProp(REACTOR),
        ))
        .with_children(|p| {
            block(
                p,
                &cube,
                &dark,
                Vec3::new(0.0, 0.22, 0.0),
                Vec3::new(3.0, 0.44, 2.6),
            );
            block(
                p,
                &cube,
                &panel,
                Vec3::new(0.0, 0.57, 0.0),
                Vec3::new(2.6, 0.28, 2.2),
            );
            block(
                p,
                &cube,
                &cyan,
                Vec3::new(0.0, 1.40, 0.0),
                Vec3::new(1.05, 1.55, 1.05),
            );
            for y in [0.83, 1.17, 1.51, 1.85, 2.19] {
                block(
                    p,
                    &cube,
                    &dark,
                    Vec3::new(0.0, y, 0.0),
                    Vec3::new(1.65, 0.16, 1.65),
                );
            }
            for x in [-1.02, 1.02] {
                for z in [-0.92, 0.92] {
                    block(
                        p,
                        &cube,
                        &ivory,
                        Vec3::new(x, 1.53, z),
                        Vec3::new(0.34, 2.10, 0.34),
                    );
                    block(
                        p,
                        &cube,
                        &orange,
                        Vec3::new(x, 0.82, z),
                        Vec3::new(0.36, 0.26, 0.36),
                    );
                }
            }
            block(
                p,
                &cube,
                &panel,
                Vec3::new(0.0, 2.64, 0.0),
                Vec3::new(2.5, 0.32, 2.1),
            );
            block(
                p,
                &cube,
                &dark,
                Vec3::new(0.0, 2.92, 0.0),
                Vec3::new(1.9, 0.26, 1.6),
            );
            block(
                p,
                &cube,
                &cyan,
                Vec3::new(0.0, 3.12, 0.0),
                Vec3::new(0.46, 0.12, 0.46),
            );
        });
    for i in 0..6 {
        object(
            &mut commands,
            &cube,
            &frame,
            Vec3::new(-5.0 + i as f32 * 0.46, 0.05, -7.1),
            Vec3::new(0.38, 0.12, 0.32),
        );
    }

    // Industrial bulkhead: interrupted silhouette, antennas and warning lamps.
    for (bulkhead, height) in BULKHEADS.into_iter().zip([2.8, 1.55, 1.8, 3.4]) {
        commands
            .spawn((
                Transform::from_translation(bulkhead.center),
                Visibility::default(),
                Obstacle(bulkhead),
                IndustrialProp(bulkhead),
            ))
            .with_children(|p| {
                block(
                    p,
                    &cube,
                    &dark,
                    Vec3::new(0.0, height * 0.5, 0.0),
                    Vec3::new(1.35, height, 0.92),
                );
                block(
                    p,
                    &cube,
                    &panel,
                    Vec3::new(0.0, height * 0.5, 0.48),
                    Vec3::new(1.10, height * 0.70, 0.10),
                );
                block(
                    p,
                    &cube,
                    &orange,
                    Vec3::new(0.0, 0.40, 0.55),
                    Vec3::new(1.10, 0.18, 0.06),
                );
                block(
                    p,
                    &cube,
                    &frame,
                    Vec3::new(0.0, height + 0.12, 0.0),
                    Vec3::new(1.65, 0.24, 1.12),
                );
                block(
                    p,
                    &cube,
                    &amber,
                    Vec3::new(0.0, height - 0.12, 0.54),
                    Vec3::new(0.65, 0.08, 0.07),
                );
                for i in 0..4 {
                    block(
                        p,
                        &cube,
                        &dark,
                        Vec3::new(-0.32 + i as f32 * 0.21, height * 0.58, 0.55),
                        Vec3::new(0.065, height * 0.32, 0.06),
                    );
                }
                if height > 2.0 {
                    block(
                        p,
                        &cube,
                        &frame,
                        Vec3::new(0.0, height + 0.9, 0.0),
                        Vec3::new(0.12, 1.5, 0.12),
                    );
                    block(
                        p,
                        &cube,
                        &amber,
                        Vec3::new(0.0, height + 1.7, 0.0),
                        Vec3::splat(0.17),
                    );
                }
            });
    }
    // Low supply crates, not arbitrary decoration the character can walk through.
    for crate_disc in CRATES {
        commands
            .spawn((
                Transform::from_translation(crate_disc.center),
                Visibility::default(),
                Obstacle(crate_disc),
                IndustrialProp(crate_disc),
            ))
            .with_children(|p| {
                block(
                    p,
                    &cube,
                    &dark,
                    Vec3::new(0.0, 0.48, 0.0),
                    Vec3::new(1.05, 0.96, 0.92),
                );
                block(
                    p,
                    &cube,
                    &orange,
                    Vec3::new(0.0, 0.48, 0.0),
                    Vec3::new(1.10, 0.56, 0.95),
                );
                for x in [-0.36, 0.36] {
                    block(
                        p,
                        &cube,
                        &frame,
                        Vec3::new(x, 0.50, 0.0),
                        Vec3::new(0.12, 1.08, 1.02),
                    );
                }
                block(
                    p,
                    &cube,
                    &cyan,
                    Vec3::new(0.0, 0.60, 0.49),
                    Vec3::new(0.22, 0.055, 0.03),
                );
            });
    }
    // A sparse constellation of distant cuboid asteroids, below the deck.
    for i in 0..20 {
        let h = hash(i, 87, 5);
        let a = i as f32 * 2.399;
        let r = 16.0 + (h % 13) as f32;
        let p = Vec3::new(a.cos() * r, -4.0 - (h % 8) as f32, a.sin() * r);
        object(
            &mut commands,
            &cube,
            &rock_dark,
            p,
            Vec3::splat(0.4 + (h % 5) as f32 * 0.2),
        );
    }
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let cube = meshes.add(Cuboid::default());
    let dark = material(&mut materials, [0.065, 0.095, 0.14], 0.35);
    let frame = material(&mut materials, [0.16, 0.22, 0.28], 0.5);
    let panel = material(&mut materials, [0.31, 0.39, 0.43], 0.35);
    let ivory = material(&mut materials, [0.72, 0.77, 0.73], 0.18);
    let orange = material(&mut materials, [0.91, 0.33, 0.095], 0.20);
    let gold = material(&mut materials, [0.93, 0.63, 0.20], 0.3);
    let cyan = glow(&mut materials, [0.10, 0.78, 1.0], 5.5);
    let amber = glow(&mut materials, [1.0, 0.28, 0.045], 4.0);
    let white = glow(&mut materials, [0.65, 0.87, 1.0], 5.0);
    let dust = material(&mut materials, [0.25, 0.22, 0.29], 0.0);
    let floors = [
        material(&mut materials, [0.17, 0.22, 0.28], 0.25),
        material(&mut materials, [0.19, 0.24, 0.30], 0.25),
        material(&mut materials, [0.15, 0.20, 0.26], 0.25),
    ];
    commands.insert_resource(crate::style::MaterialBindings(
        [
            (&dark, "shadow"),
            (&frame, "metal"),
            (&panel, "metal"),
            (&ivory, "stone"),
            (&orange, "metal"),
            (&gold, "trim"),
            (&cyan, "magic"),
        ]
        .into_iter()
        .map(|(handle, role)| {
            (
                handle.clone(),
                role.to_string(),
                materials.get(handle).unwrap().clone(),
            )
        })
        .collect(),
    ));
    let camera_base = Vec3::new(20.0, 20.0, 20.0);
    let camera_target = Vec3::new(0.0, 0.0, -0.4);
    commands.spawn((
        Camera3d::default(),
        Hdr,
        Msaa::Off,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 21.5,
            },
            ..OrthographicProjection::default_3d()
        }),
        Tonemapping::TonyMcMapface,
        Bloom {
            intensity: 0.18,
            ..Bloom::NATURAL
        },
        ScreenSpaceAmbientOcclusion::default(),
        TemporalAntiAliasing::default(),
        Transform::from_translation(camera_base).looking_at(camera_target, Vec3::Y),
        CameraRig {
            base: camera_base,
            target: camera_target,
            zoom: 21.5,
        },
    ));
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.79, 0.63),
            illuminance: 10500.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-8.0, 12.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    for (index, (p, color, intensity)) in [
        (
            Vec3::new(-4.8, 3.0, -4.8),
            Color::srgb(0.05, 0.65, 1.0),
            150000.0,
        ),
        (
            Vec3::new(6.0, 3.5, -5.0),
            Color::srgb(1.0, 0.30, 0.08),
            110000.0,
        ),
        (
            Vec3::new(2.0, 5.0, 5.0),
            Color::srgb(0.32, 0.58, 1.0),
            65000.0,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        commands.spawn((
            crate::style::AccentLight {
                role: index,
                intensity,
            },
            PointLight {
                color,
                intensity,
                radius: 0.8,
                range: 12.0,
                ..default()
            },
            Transform::from_translation(p),
        ));
    }

    let rock_dark = material(&mut materials, [0.14, 0.17, 0.23], 0.10);
    commands.insert_resource(SceneWorldAssets {
        cube: cube.clone(), dark: dark.clone(), frame: frame.clone(),
        panel: panel.clone(), ivory: ivory.clone(), orange: orange.clone(),
        gold: gold.clone(), cyan: cyan.clone(), amber: amber.clone(),
        dust: dust.clone(), floors: floors.clone(), rock_dark: rock_dark.clone(),
    });

    // Compact articulated exosuit. Each joint has a stable pivot shared by local and peer posing.
    commands
        .spawn((
            Player,
            Transform::from_xyz(0.0, 0.0, 2.0)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
            Visibility::default(),
        ))
        .with_children(|p| {
            let body = Transform::from_xyz(0.0, 0.94, 0.0);
            p.spawn((
                Part {
                    kind: PartKind::Body,
                    bind: body,
                    rest: body,
                },
                body,
                Visibility::default(),
            ))
            .with_children(|hip| {
                // The backpack is part of the chest below. Keep its exhaust
                // on the animated body, so the effect follows torso motion.
                for x in [-0.14, 0.14] {
                    hip.spawn((
                        crate::jetpack::Exhaust,
                        Mesh3d(cube.clone()),
                        MeshMaterial3d(cyan.clone()),
                        Transform::from_xyz(x, -0.49, -0.48)
                            .with_scale(Vec3::new(0.10, 0.5, 0.10)),
                        Visibility::Hidden,
                    ));
                }
                block(
                    hip,
                    &cube,
                    &dark,
                    Vec3::new(0.0, -0.16, 0.0),
                    Vec3::new(0.55, 0.22, 0.36),
                );
                let chest = Transform::IDENTITY;
                hip.spawn((
                    Part {
                        kind: PartKind::Chest,
                        bind: chest,
                        rest: chest,
                    },
                    chest,
                    Visibility::default(),
                ))
                .with_children(|b| {
                    block(
                        b,
                        &cube,
                        &dark,
                        Vec3::new(0.0, 0.10, 0.0),
                        Vec3::new(0.66, 0.66, 0.38),
                    );
                    block(
                        b,
                        &cube,
                        &ivory,
                        Vec3::new(0.0, 0.18, 0.19),
                        Vec3::new(0.56, 0.43, 0.14),
                    );
                    block(
                        b,
                        &cube,
                        &orange,
                        Vec3::new(0.0, 0.35, 0.28),
                        Vec3::new(0.25, 0.14, 0.055),
                    );
                    block(
                        b,
                        &cube,
                        &cyan,
                        Vec3::new(-0.14, 0.20, 0.28),
                        Vec3::new(0.07, 0.15, 0.035),
                    );
                    block(
                        b,
                        &cube,
                        &frame,
                        Vec3::new(0.0, -0.18, 0.0),
                        Vec3::new(0.74, 0.14, 0.46),
                    );
                    block(
                        b,
                        &cube,
                        &gold,
                        Vec3::new(0.0, -0.18, 0.25),
                        Vec3::new(0.18, 0.12, 0.06),
                    );
                    block(
                        b,
                        &cube,
                        &frame,
                        Vec3::new(0.0, 0.17, -0.32),
                        Vec3::new(0.52, 0.58, 0.28),
                    );
                    for x in [-0.14, 0.14] {
                        block(
                            b,
                            &cube,
                            &frame,
                            Vec3::new(x, 0.22, -0.48),
                            Vec3::new(0.07, 0.27, 0.025),
                        );
                    }
                    let head = Transform::from_xyz(0.0, 0.64, 0.0);
                    b.spawn((
                        Part {
                            kind: PartKind::Head,
                            bind: head,
                            rest: head,
                        },
                        head,
                        Visibility::default(),
                    ))
                    .with_children(|h| {
                        block(
                            h,
                            &cube,
                            &dark,
                            Vec3::new(0.0, -0.12, 0.0),
                            Vec3::new(0.28, 0.17, 0.28),
                        );
                        block(
                            h,
                            &cube,
                            &ivory,
                            Vec3::new(0.0, 0.10, 0.0),
                            Vec3::new(0.53, 0.43, 0.48),
                        );
                        block(
                            h,
                            &cube,
                            &frame,
                            Vec3::new(0.0, 0.31, 0.0),
                            Vec3::new(0.58, 0.11, 0.52),
                        );
                        block(
                            h,
                            &cube,
                            &dark,
                            Vec3::new(0.0, 0.10, 0.25),
                            Vec3::new(0.49, 0.22, 0.045),
                        );
                        block(
                            h,
                            &cube,
                            &cyan,
                            Vec3::new(0.0, 0.13, 0.28),
                            Vec3::new(0.42, 0.06, 0.025),
                        );
                        block(
                            h,
                            &cube,
                            &cyan,
                            Vec3::new(0.0, 0.06, 0.28),
                            Vec3::new(0.07, 0.10, 0.03),
                        );
                    });
                    for side in [-1.0, 1.0] {
                        let arm = Transform::from_xyz(side * 0.43, 0.33, 0.0);
                        b.spawn((
                            Part {
                                kind: PartKind::Arm(side),
                                bind: arm,
                                rest: arm,
                            },
                            arm,
                            Visibility::default(),
                        ))
                        .with_children(|a| {
                            // A separate pauldron, upper arm, elbow, gauntlet and hand
                            // let the cloth and armour move without stretching a slab.
                            block(
                                a,
                                &cube,
                                if side > 0.0 { &orange } else { &ivory },
                                Vec3::new(side * 0.015, -0.02, 0.0),
                                Vec3::new(0.30, 0.19, 0.38),
                            );
                            block(
                                a,
                                &cube,
                                &dark,
                                Vec3::new(0.0, -0.17, 0.0),
                                Vec3::new(0.15, 0.24, 0.18),
                            );
                            block(
                                a,
                                &cube,
                                &gold,
                                Vec3::new(side * 0.08, -0.05, 0.17),
                                Vec3::new(0.045, 0.10, 0.035),
                            );
                            let elbow = Transform::from_xyz(0.0, -0.29, 0.015);
                            a.spawn((
                                Part {
                                    kind: PartKind::Forearm(side),
                                    bind: elbow,
                                    rest: elbow,
                                },
                                elbow,
                                Visibility::default(),
                            ))
                            .with_children(|f| {
                                block(f, &cube, &frame, Vec3::ZERO, Vec3::new(0.19, 0.15, 0.20));
                                block(
                                    f,
                                    &cube,
                                    &dark,
                                    Vec3::new(0.0, -0.13, 0.01),
                                    Vec3::new(0.15, 0.26, 0.17),
                                );
                                block(
                                    f,
                                    &cube,
                                    &ivory,
                                    Vec3::new(0.0, -0.17, 0.10),
                                    Vec3::new(0.20, 0.20, 0.07),
                                );
                                block(
                                    f,
                                    &cube,
                                    &cyan,
                                    Vec3::new(0.0, -0.21, 0.145),
                                    Vec3::new(0.16, 0.035, 0.025),
                                );
                                let wrist = Transform::from_xyz(0.0, -0.28, 0.015);
                                f.spawn((
                                    Part {
                                        kind: PartKind::Hand(side),
                                        bind: wrist,
                                        rest: wrist,
                                    },
                                    wrist,
                                    Visibility::default(),
                                ))
                                .with_children(|h| {
                                    block(
                                        h,
                                        &cube,
                                        &dark,
                                        Vec3::new(0.0, -0.06, 0.035),
                                        Vec3::new(0.17, 0.14, 0.19),
                                    );
                                    block(
                                        h,
                                        &cube,
                                        &ivory,
                                        Vec3::new(0.0, -0.065, 0.14),
                                        Vec3::new(0.14, 0.09, 0.07),
                                    );
                                    if side > 0.0 {
                                        let weapon = Transform::from_xyz(-0.04, 0.08, 0.05);
                                        h.spawn((
                                            crate::character::LocalBlade,
                                            Part {
                                                kind: PartKind::WeaponGrip,
                                                bind: weapon,
                                                rest: weapon,
                                            },
                                            weapon,
                                            Visibility::default(),
                                        ))
                                        .with_children(
                                            |s| {
                                                block(
                                                    s,
                                                    &cube,
                                                    &dark,
                                                    Vec3::new(0.0, 0.0, 0.12),
                                                    Vec3::new(0.15, 0.18, 0.48),
                                                );
                                                for z in [0.0, 0.12, 0.24] {
                                                    block(
                                                        s,
                                                        &cube,
                                                        &gold,
                                                        Vec3::new(0.0, 0.0, z),
                                                        Vec3::new(0.17, 0.20, 0.04),
                                                    );
                                                }
                                                block(
                                                    s,
                                                    &cube,
                                                    &orange,
                                                    Vec3::new(0.0, 0.0, 0.38),
                                                    Vec3::new(0.55, 0.17, 0.16),
                                                );
                                                block(
                                                    s,
                                                    &cube,
                                                    &frame,
                                                    Vec3::new(0.0, 0.0, 1.08),
                                                    Vec3::new(0.32, 0.16, 1.35),
                                                );
                                                block(
                                                    s,
                                                    &cube,
                                                    &ivory,
                                                    Vec3::new(0.03, 0.10, 1.03),
                                                    Vec3::new(0.22, 0.07, 1.16),
                                                );
                                                block(
                                                    s,
                                                    &cube,
                                                    &dark,
                                                    Vec3::new(0.02, 0.14, 0.92),
                                                    Vec3::new(0.09, 0.03, 0.62),
                                                );
                                                block(
                                                    s,
                                                    &cube,
                                                    &cyan,
                                                    Vec3::new(-0.18, 0.015, 1.10),
                                                    Vec3::new(0.055, 0.09, 1.32),
                                                );
                                                block(
                                                    s,
                                                    &cube,
                                                    &white,
                                                    Vec3::new(-0.11, 0.0, 1.84),
                                                    Vec3::new(0.15, 0.12, 0.20),
                                                );
                                                for z in [0.65, 0.88, 1.11] {
                                                    block(
                                                        s,
                                                        &cube,
                                                        &orange,
                                                        Vec3::new(0.07, 0.15, z),
                                                        Vec3::new(0.12, 0.03, 0.06),
                                                    );
                                                }
                                            },
                                        );
                                    }
                                });
                            });
                        });
                    }
                });
            for side in [-1.0, 1.0] {
                let leg = Transform::from_xyz(side * 0.20, -0.36, 0.0);
                hip.spawn((
                    Part {
                        kind: PartKind::Leg(side),
                        bind: leg,
                        rest: leg,
                    },
                    leg,
                    Visibility::default(),
                ))
                .with_children(|l| {
                    block(
                        l,
                        &cube,
                        &dark,
                        Vec3::new(0.0, -0.10, 0.0),
                        Vec3::new(0.26, 0.39, 0.28),
                    );
                    let shin = Transform::from_xyz(0.0, -0.27, 0.0);
                    l.spawn((
                        Part {
                            kind: PartKind::Shin(side),
                            bind: shin,
                            rest: shin,
                        },
                        shin,
                        Visibility::default(),
                    ))
                    .with_children(|s| {
                        // The knee skin has its own slot at the hinge. It follows
                        // the shin joint without merging into either limb mesh.
                        s.spawn((
                            Part {
                                kind: PartKind::Knee(side),
                                bind: Transform::IDENTITY,
                                rest: Transform::IDENTITY,
                            },
                            Transform::IDENTITY,
                            Visibility::default(),
                        ));
                        block(
                            s,
                            &cube,
                            &ivory,
                            Vec3::new(0.0, -0.07, 0.025),
                            Vec3::new(0.27, 0.30, 0.31),
                        );
                        block(
                            s,
                            &cube,
                            &orange,
                            Vec3::new(0.0, 0.03, 0.20),
                            Vec3::new(0.19, 0.12, 0.04),
                        );
                        let foot = Transform::from_xyz(0.0, -0.21, 0.0);
                        s.spawn((
                            Part {
                                kind: PartKind::Foot(side),
                                bind: foot,
                                rest: foot,
                            },
                            foot,
                            Visibility::default(),
                        ))
                        .with_children(|f| {
                            block(
                                f,
                                &cube,
                                &dark,
                                Vec3::new(0.0, -0.01, 0.12),
                                Vec3::new(0.32, 0.16, 0.47),
                            );
                        });
                    });
                });
            }
            });
        });
    commands.insert_resource(FxAssets {
        cube,
        spark: crate::impact::KINDS
            .map(|kind| glow(&mut materials, crate::impact::response(kind).color, 12.0)),
        debris: [
            rock_dark,
            material(&mut materials, [0.5, 0.57, 0.65], 0.9),
            glow(&mut materials, [0.08, 0.5, 1.0], 1.5),
            material(&mut materials, [0.12, 0.5, 0.23], 0.0),
        ],
        trail: materials.add(StandardMaterial {
            base_color: Color::srgba(0.15, 0.75, 1.0, 0.16),
            emissive: LinearRgba::rgb(0.1, 1.6, 2.5),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
        blood: [
            material(&mut materials, [0.42, 0.025, 0.035], 0.55),
            material(&mut materials, [0.73, 0.075, 0.055], 0.48),
        ],
    });
}

#[derive(Component)]
pub struct SceneLabel;
#[derive(Component)]
pub struct CombatPanel;
#[derive(Component)]
pub struct ContactPanel;
#[derive(Component)]
pub struct ControlHelp;
#[derive(Component)]
pub struct SceneShortcutHelp;
#[derive(Component)]
pub struct HudRoot;
#[derive(Component)]
pub struct Status;
#[derive(Component)]
pub struct Gauge;
#[derive(Component)]
pub struct ContactCount;

pub fn hud(mut commands: Commands) {
    let white = Color::srgb(0.83, 0.90, 0.93);
    let muted = Color::srgb(0.42, 0.54, 0.62);
    let cyan = Color::srgb(0.27, 0.88, 0.95);
    commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(36),
                top: px(30),
                flex_direction: FlexDirection::Column,
                row_gap: px(7),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("K E S T R E L   / /   0 7"),
                TextFont {
                    font_size: FontSize::Px(27.0),
                    ..default()
                },
                TextColor(white),
            ));
            p.spawn((
                SceneLabel,
                Text::new("LOADING SCENE"),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(muted),
            ));
            p.spawn((
                Node {
                    width: px(42),
                    height: px(2),
                    margin: UiRect::top(px(9)),
                    ..default()
                },
                BackgroundColor(cyan),
            ));
        });
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(36),
                top: px(32),
                align_items: AlignItems::End,
                flex_direction: FlexDirection::Column,
                row_gap: px(7),
                ..default()
            },
            ContactPanel,
            HudRoot,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("[ LIVE ]   SURFACE LINK ESTABLISHED"),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(cyan),
            ));
            p.spawn((
                ContactCount,
                Text::new("CONTACTS   00"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(muted),
            ));
        });
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(32),
                left: px(36),
                width: px(254),
                padding: UiRect::all(px(18)),
                flex_direction: FlexDirection::Column,
                row_gap: px(9),
                border: UiRect::left(px(2)),
                ..default()
            },
            CombatPanel,
            HudRoot,
            BackgroundColor(Color::srgba(0.025, 0.045, 0.07, 0.88)),
            BorderColor::all(cyan),
        ))
        .with_children(|p| {
            p.spawn((
                crate::style::WeaponLabel,
                Text::new("MK. IV  /  INDUSTRIAL CLEAVER"),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(muted),
            ));
            p.spawn((
                Status,
                Text::new("READY"),
                TextFont {
                    font_size: FontSize::Px(20.0),
                    ..default()
                },
                TextColor(white),
            ));
            p.spawn((
                Node {
                    width: percent(100),
                    height: px(3),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.13, 0.20, 0.25)),
            ))
            .with_children(|b| {
                b.spawn((
                    Gauge,
                    Node {
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(cyan),
                ));
            });
        });
    commands
        .spawn((HudRoot, Node {
            position_type: PositionType::Absolute,
            bottom: px(38),
            right: px(36),
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            align_items: AlignItems::End,
            ..default()
        },))
        .with_children(|p| {
            p.spawn((
                ControlHelp,
                Text::new("RMB   MOVE     /     SHIFT + LMB   STRIKE"),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(white),
            ));
            p.spawn((
                SceneShortcutHelp,
                Text::new(
                    "1 - 9 / 0  SCENES    F4  GLADE    SPACE  JETPACK    WASD  STEER    SHIFT  RUN    WHEEL  ZOOM    M  AUDIO",
                ),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(muted),
            ));
        });
}

pub fn update_hud(
    controls: Res<Controls>,
    sound: Res<Sound>,
    library: Res<SceneLibrary>,
    cache: Res<ModelCache>,
    flight: Res<crate::jetpack::Flight>,
    game: Res<crate::lockstep::Game>,
    player: Single<&Transform, With<Player>>,
    mut scene_label: Single<&mut Text, (With<SceneLabel>, Without<Status>, Without<ContactCount>)>,
    mut status: Single<(&mut Text, &mut TextColor), With<Status>>,
    mut gauge: Single<&mut Node, With<Gauge>>,
    mut contacts: Single<&mut Text, (With<ContactCount>, Without<Status>)>,
) {
    let (label, progress, color) = if let Some(a) = controls.attack {
        if a.impact_angle.is_some() {
            (
                "IMPACT / REBOUND",
                1.0 - a.age / REBOUND_END,
                Color::srgb(1.0, 0.51, 0.17),
            )
        } else if a.age < WINDUP {
            ("WIND-UP", a.age / WINDUP, Color::srgb(0.93, 0.70, 0.30))
        } else if a.age < ACTIVE_END {
            ("CUT", 1.0, Color::srgb(0.55, 0.97, 1.0))
        } else {
            (
                "RECOVERING",
                (a.age - ACTIVE_END) / (ATTACK_END - ACTIVE_END),
                Color::srgb(0.42, 0.65, 0.72),
            )
        }
    } else if flight.airborne {
        (
            if library.has_feature(voxel_assets::SceneFeature::Jump) {
                "JUMP / AIRBORNE"
            } else if flight.requested {
                "JETPACK / HOVER"
            } else {
                "JETPACK / SAFE LANDING"
            },
            1.0,
            Color::srgb(0.34, 0.88, 0.93),
        )
    } else if matches!(
        flight.jump,
        sim::settlement::avatar::JumpPhase::Preparing { .. }
    ) {
        ("JUMP / TAKEOFF", 1.0, Color::srgb(0.65, 0.94, 0.8))
    } else if matches!(
        flight.jump,
        sim::settlement::avatar::JumpPhase::Landing { .. }
    ) {
        ("JUMP / LANDING", 1.0, Color::srgb(0.65, 0.94, 0.8))
    } else {
        ("READY", 1.0, Color::srgb(0.34, 0.88, 0.93))
    };
    if status.0 .0 != label {
        status.0 .0 = label.into();
    }
    status.1 .0 = color;
    gauge.width = percent(progress.clamp(0.0, 1.0) * 100.0);
    if scene_label.0 != library.title() {
        scene_label.0 = library.title().into();
    }
    let audio = if sound.muted { "  /  MUTED" } else { "" };
    let last = controls
        .last_material
        .map_or_else(|| "—".into(), |kind| format!("{kind:?}").to_uppercase());
    let text = format!(
        "CONTACTS {:02} / HIT {last} / DYNAMIC BUILDS {}{audio}",
        controls.impacts, cache.runtime_generations
    );
    let text = if library.has_feature(voxel_assets::SceneFeature::FlightHud) {
        format!(
            "ALT {:.1}m / SUPPORT {:.1}m / HOVER {:.1}m / HOLD SPACE TO FLY",
            player.translation.y,
            flight.support,
            crate::jetpack::HOVER
        )
    } else if library.has_feature(voxel_assets::SceneFeature::Digging) {
        let net = if game.network.is_some() {
            format!(" / NET / {}", game.status.to_uppercase())
        } else {
            String::new()
        };
        let tile = sim::terrain::Terrain::pit_tile(game.to_sim(player.translation));
        let pit = game.world().map_or(0, |w| w.terrain().pit(tile));
        format!(
            "PIT {pit}/{} / F TO DIG / HOLD SPACE TO FLY OUT{net}",
            sim::terrain::MAX_LAYERS
        )
    } else if game.network.is_some() {
        format!("NET / {}", game.status.to_uppercase())
    } else {
        text
    };
    if contacts.0 != text {
        contacts.0 = text;
    }
}

#[cfg(test)]
mod character_tests {
    use super::*;

    #[test]
    fn weapon_is_descended_from_the_right_hand_and_elbow() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Startup, setup);
        app.update();
        let world = app.world_mut();
        let sword = world
            .query::<(Entity, &Part)>()
            .iter(world)
            .find_map(|(entity, part)| matches!(part.kind, PartKind::WeaponGrip).then_some(entity))
            .unwrap();
        let mut entity = sword;
        for check in [
            PartKind::Hand(1.0),
            PartKind::Forearm(1.0),
            PartKind::Arm(1.0),
            PartKind::Chest,
            PartKind::Body,
        ] {
            entity = world.get::<ChildOf>(entity).unwrap().parent();
            let actual = world.get::<Part>(entity).unwrap().kind;
            assert!(
                matches!(
                    (actual, check),
                    (PartKind::Hand(a), PartKind::Hand(b))
                        | (PartKind::Forearm(a), PartKind::Forearm(b))
                        | (PartKind::Arm(a), PartKind::Arm(b)) if a == b
                ) || matches!(
                    (actual, check),
                    (PartKind::Body, PartKind::Body) | (PartKind::Chest, PartKind::Chest)
                )
            );
        }
    }

    #[test]
    fn knee_skins_follow_the_shin_hinges() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Startup, setup);
        app.update();
        let world = app.world_mut();
        let knees: Vec<_> = world
            .query::<(Entity, &Part)>()
            .iter(world)
            .filter_map(|(entity, part)| match part.kind {
                PartKind::Knee(side) => Some((entity, side)),
                _ => None,
            })
            .collect();
        assert_eq!(knees.len(), 2);
        for (knee, side) in knees {
            let shin = world.get::<ChildOf>(knee).unwrap().parent();
            let thigh = world.get::<ChildOf>(shin).unwrap().parent();
            let pelvis = world.get::<ChildOf>(thigh).unwrap().parent();
            assert!(
                matches!(world.get::<Part>(shin).unwrap().kind, PartKind::Shin(s) if s == side)
            );
            assert!(matches!(world.get::<Part>(pelvis).unwrap().kind, PartKind::Body));
            assert!(
                matches!(world.get::<Part>(thigh).unwrap().kind, PartKind::Leg(s) if s == side)
            );
        }
    }
}
