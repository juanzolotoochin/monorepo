//! Terrain-aware hover flight. The same controller runs for keyboard and scripted demos.
use crate::assets::SceneLibrary;
use bevy::prelude::*;

pub const HOVER: f32 = 1.8;
#[derive(Resource, Default)]
pub struct Flight {
    pub requested: bool,
    pub airborne: bool,
    pub support: f32,
    current: Option<usize>,
    sound_timer: f32,
}
#[derive(Component)]
pub struct Deck;
#[derive(Component)]
pub struct TerrainVisual;
#[derive(Component)]
pub struct Exhaust;

pub fn height(library: &SceneLibrary, p: Vec3) -> Option<f32> {
    let pack = library.scenes.get(library.current)?;
    let scene = &pack.scene;
    let surface = pack
        .environment
        .iter()
        .flat_map(|e| &e.walkable)
        .filter(|b| p.x >= b.min[0] && p.x <= b.max[0] && p.z >= b.min[1] && p.z <= b.max[1])
        .map(|b| b.height)
        .max_by(f32::total_cmp);
    match &scene.terrain {
        None => Some(surface.unwrap_or(0.0)),
        Some(t) => t
            .platforms
            .iter()
            .filter(|b| p.x >= b.min[0] && p.x <= b.max[0] && p.z >= b.min[1] && p.z <= b.max[1])
            .map(|b| b.height)
            .max_by(f32::total_cmp),
    }
}
pub fn scene_changed(
    mut commands: Commands,
    library: Res<SceneLibrary>,
    mut flight: ResMut<Flight>,
    mut deck: Query<&mut Visibility, With<Deck>>,
    old: Query<Entity, With<TerrainVisual>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<Option<(Handle<Mesh>, [Handle<StandardMaterial>; 3])>>,
) {
    if flight.current == Some(library.current) {
        return;
    }
    *flight = Flight {
        current: Some(library.current),
        ..default()
    };
    for e in &old {
        commands.entity(e).despawn();
    }
    let terrain = &library.scenes[library.current].scene.terrain;
    for mut visibility in &mut deck {
        *visibility = if terrain.is_some() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    let Some(terrain) = terrain else {
        return;
    };
    let (cube, [side, top, glow]) = cache.get_or_insert_with(|| {
        (
            meshes.add(Cuboid::default()),
            [
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.09, 0.14, 0.21),
                    metallic: 0.6,
                    ..default()
                }),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.24, 0.32, 0.37),
                    metallic: 0.35,
                    ..default()
                }),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.05, 0.8, 1.0),
                    emissive: LinearRgba::rgb(0.1, 3.0, 5.0),
                    ..default()
                }),
            ],
        )
    });
    let mut block = |pos: Vec3, size: Vec3, mat: &Handle<StandardMaterial>| {
        commands.spawn((
            TerrainVisual,
            Mesh3d(cube.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(pos).with_scale(size),
        ));
    };
    for p in &terrain.platforms {
        let x = (p.min[0] + p.max[0]) * 0.5;
        let z = (p.min[1] + p.max[1]) * 0.5;
        let w = p.max[0] - p.min[0];
        let d = p.max[1] - p.min[1];
        block(
            Vec3::new(x, (p.height - 3.08) * 0.5, z),
            Vec3::new(w, p.height + 2.92, d),
            &side,
        );
        for ix in 0..w.ceil() as u32 {
            for iz in 0..d.ceil() as u32 {
                let tw = (w - ix as f32).min(1.0);
                let td = (d - iz as f32).min(1.0);
                block(
                    Vec3::new(
                        p.min[0] + ix as f32 + tw * 0.5,
                        p.height - 0.04,
                        p.min[1] + iz as f32 + td * 0.5,
                    ),
                    Vec3::new(tw * 0.96, 0.08, td * 0.96),
                    &top,
                );
            }
        }
        for edge in [p.min[1] + 0.07, p.max[1] - 0.07] {
            block(
                Vec3::new(x, p.height + 0.01, edge),
                Vec3::new((w - 0.14).max(0.01), 0.025, 0.035),
                &glow,
            );
        }
        for edge in [p.min[0] + 0.07, p.max[0] - 0.07] {
            block(
                Vec3::new(edge, p.height + 0.01, z),
                Vec3::new(0.035, 0.025, (d - 0.14).max(0.01)),
                &glow,
            );
        }
    }
}

/// Mirrors the avatar's flight for the HUD, sound and exhaust. The flight
/// itself is simulated (`kestrel/sim::settlement::avatar`).
pub fn update(
    time: Res<Time>,
    game: Res<crate::lockstep::Game>,
    mut flight: ResMut<Flight>,
    sound: Res<crate::sound::Sound>,
    mut exhaust: Query<(&mut Transform, &mut Visibility), With<Exhaust>>,
) {
    let (Some(current), Some(index)) = (&game.current, game.player()) else { return };
    let avatar = &current.agents[index as usize].avatar;
    let climbing = game
        .previous
        .as_ref()
        .and_then(|p| p.agents.get(index as usize))
        .is_some_and(|before| avatar.height > before.avatar.height);
    flight.requested = avatar.flight.requested;
    flight.airborne = avatar.flight.airborne;
    flight.support = avatar.flight.support.to_f32();
    let dt = time.delta_secs().min(0.1);
    flight.sound_timer -= dt;
    if flight.airborne && flight.sound_timer <= 0.0 {
        sound.play(crate::sound::Cue::Jetpack);
        flight.sound_timer = 0.16;
    }
    if !flight.airborne {
        flight.sound_timer = 0.0;
    }
    for (mut transform, mut visibility) in &mut exhaust {
        *visibility = if flight.airborne { Visibility::Inherited } else { Visibility::Hidden };
        transform.scale.y = 0.4 + 0.1 * (time.elapsed_secs() * 35.0).sin() + if climbing { 0.25 } else { 0.0 };
    }
}

/// Pick the closest visible platform top (or the dug ground in a digging
/// scene), rather than projecting elevated clicks onto y=0.
pub fn cursor_surface(
    camera: &Camera,
    transform: &GlobalTransform,
    window: &Window,
    library: &SceneLibrary,
    game: &crate::lockstep::Game,
) -> Option<Vec3> {
    let ray = camera
        .viewport_to_world(transform, window.cursor_position()?)
        .ok()?;
    if library.has_feature(voxel_assets::SceneFeature::Digging) {
        return crate::digging::ground_hit(ray, game);
    }
    let Some(terrain) = &library.scenes[library.current].scene.terrain else {
        return crate::gameplay::cursor_ground(camera, transform, window);
    };
    terrain
        .platforms
        .iter()
        .filter_map(|platform| {
            let distance =
                ray.intersect_plane(Vec3::Y * platform.height, InfinitePlane3d::new(Vec3::Y))?;
            let p = ray.get_point(distance);
            (p.x >= platform.min[0]
                && p.x <= platform.max[0]
                && p.z >= platform.min[1]
                && p.z <= platform.max[1])
                .then_some((distance, p))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p)| p)
        .or_else(|| crate::gameplay::cursor_ground(camera, transform, window))
}
