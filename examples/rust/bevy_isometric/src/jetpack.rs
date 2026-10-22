//! Terrain-aware hover flight. The same controller runs for keyboard and scripted demos.
use crate::{
    assets::SceneLibrary,
    gameplay::{Controls, Player},
    navigation::{self, Disc, RADIUS},
};
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
    let scene = &library.scenes.get(library.current)?.scene;
    match &scene.terrain {
        None => Some(0.0),
        Some(t) => t
            .platforms
            .iter()
            .filter(|b| p.x >= b.min[0] && p.x <= b.max[0] && p.z >= b.min[1] && p.z <= b.max[1])
            .map(|b| b.height)
            .max_by(f32::total_cmp),
    }
}
fn footprint(library: &SceneLibrary, p: Vec3) -> Option<f32> {
    let mut top: f32 = f32::NEG_INFINITY;
    for offset in [
        Vec3::ZERO,
        Vec3::X * RADIUS,
        -Vec3::X * RADIUS,
        Vec3::Z * RADIUS,
        -Vec3::Z * RADIUS,
    ] {
        top = top.max(height(library, p + offset)?);
    }
    Some(top)
}
fn walkable(library: &SceneLibrary, a: Vec3, b: Vec3) -> bool {
    let steps = (navigation::flat(b - a).length() / 0.1).ceil().max(1.0) as usize;
    let Some(mut last) = footprint(library, a) else {
        return false;
    };
    for i in 1..=steps {
        let Some(next) = footprint(library, a.lerp(b, i as f32 / steps as f32)) else {
            return false;
        };
        if (next - last).abs() > 0.25 {
            return false;
        }
        last = next;
    }
    true
}
pub fn route(
    library: &SceneLibrary,
    flight: &Flight,
    start: Vec3,
    goal: Vec3,
    obstacles: &[Disc],
) -> Vec<Vec3> {
    navigation::path_with(start, goal, obstacles, |a, b| {
        flight.requested || flight.airborne || walkable(library, a, b)
    })
}
pub fn can_move(library: &SceneLibrary, a: Vec3, b: Vec3, flying: bool) -> bool {
    if !flying {
        return walkable(library, a, b);
    }
    // Check the swept body rather than the center so no frame can enter a wall.
    let steps = (navigation::flat(b - a).length() / 0.05).ceil().max(1.0) as usize;
    (0..=steps).all(|i| {
        let p = a.lerp(b, i as f32 / steps as f32);
        [
            Vec3::ZERO,
            Vec3::X * RADIUS,
            -Vec3::X * RADIUS,
            Vec3::Z * RADIUS,
            -Vec3::Z * RADIUS,
        ]
        .iter()
        .all(|&offset| height(library, p + offset).is_none_or(|y| y <= a.y + 0.02))
    })
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

pub fn update(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    library: Res<SceneLibrary>,
    mut flight: ResMut<Flight>,
    sound: Res<crate::sound::Sound>,
    controls: Res<Controls>,
    mut player: Single<&mut Transform, (With<Player>, Without<Exhaust>)>,
    mut exhaust: Query<(&mut Transform, &mut Visibility), With<Exhaust>>,
) {
    if keys.pressed(KeyCode::Space) {
        flight.requested = true;
    }
    if keys.just_released(KeyCode::Space) {
        flight.requested = false;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        flight.requested = false;
        flight.airborne = false;
        player.translation = Vec3::from_array(library.scenes[library.current].scene.spawn);
    }
    let ground = height(&library, player.translation);
    let landing = footprint(&library, player.translation)
        .filter(|y| ground.is_some_and(|g| (*y - g).abs() < 0.01));
    if let Some(y) = ground {
        flight.support = y;
    }
    let active = flight.requested || (flight.airborne && landing.is_none());
    let direction = controls
        .route
        .first()
        .map(|p| navigation::flat(*p - player.translation).normalize_or_zero())
        .unwrap_or(controls.velocity.normalize_or_zero());
    let mut target = ground.unwrap_or(flight.support);
    if active {
        // Look ahead far enough to climb at walking/running speed; pause at a face if needed.
        for i in 0..=20 {
            let p = player.translation + direction * (i as f32 * 0.1);
            for offset in [
                Vec3::ZERO,
                Vec3::X * RADIUS,
                -Vec3::X * RADIUS,
                Vec3::Z * RADIUS,
                -Vec3::Z * RADIUS,
            ] {
                if let Some(y) = height(&library, p + offset) {
                    target = target.max(y);
                }
            }
        }
        target += HOVER;
    }
    let dt = time.delta_secs().min(0.1);
    let delta = (target - player.translation.y).clamp(-3.0 * dt, 5.5 * dt);
    player.translation.y += delta;
    flight.airborne = active || ground.is_none_or(|y| player.translation.y > y + 0.01);
    flight.sound_timer -= dt;
    if flight.airborne && flight.sound_timer <= 0.0 {
        sound.play(crate::sound::Cue::Jetpack);
        flight.sound_timer = 0.16;
    }
    if !flight.airborne {
        flight.sound_timer = 0.0;
    }
    for (mut transform, mut visibility) in &mut exhaust {
        *visibility = if flight.airborne {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        transform.scale.y =
            0.4 + 0.1 * (time.elapsed_secs() * 35.0).sin() + if delta > 0.0 { 0.25 } else { 0.0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn lab() -> SceneLibrary {
        let mut l = SceneLibrary::new();
        l.current = 3;
        l
    }
    #[test]
    fn ground_routes_cannot_cross_walls_or_void() {
        let l = lab();
        let f = Flight::default();
        let a = Vec3::new(-5.0, 0.0, 3.0);
        assert!(!route(&l, &f, a, Vec3::new(-4.0, 0.0, 0.0), &[]).is_empty());
        assert!(route(&l, &f, a, Vec3::new(0.0, 0.0, 2.0), &[]).is_empty());
        assert!(route(&l, &f, a, Vec3::new(2.0, 0.0, 2.0), &[]).is_empty());
        let f = Flight {
            requested: true,
            ..default()
        };
        assert!(!route(&l, &f, a, Vec3::new(5.0, 0.0, 2.0), &[]).is_empty());
    }
    #[test]
    fn swept_body_waits_until_above_wall() {
        let l = lab();
        assert!(!can_move(
            &l,
            Vec3::new(-2.5, 1.8, 2.0),
            Vec3::new(-2.1, 1.8, 2.0),
            true
        ));
        assert!(can_move(
            &l,
            Vec3::new(-2.5, 2.1, 2.0),
            Vec3::new(-2.1, 2.1, 2.0),
            true
        ));
    }
}

/// Pick the closest visible platform top, rather than projecting elevated clicks onto y=0.
pub fn cursor_surface(
    camera: &Camera,
    transform: &GlobalTransform,
    window: &Window,
    library: &SceneLibrary,
) -> Option<Vec3> {
    let ray = camera
        .viewport_to_world(transform, window.cursor_position()?)
        .ok()?;
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
