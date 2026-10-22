//! An asset-free isometric garden. See ../README.md for controls.
use bevy::{app::AppExit, camera::ScalingMode, prelude::*};

const EDGE: f32 = 6.0;
const PLAYER_RADIUS: f32 = 0.3;
const SPEED: f32 = 3.0;
// Camera looks from +X/+Z, so screen-right is +X/-Z.
const RIGHT: Vec3 = Vec3::new(
    std::f32::consts::FRAC_1_SQRT_2,
    0.0,
    -std::f32::consts::FRAC_1_SQRT_2,
);
const UP: Vec3 = Vec3::new(
    -std::f32::consts::FRAC_1_SQRT_2,
    0.0,
    -std::f32::consts::FRAC_1_SQRT_2,
);

#[derive(Component)]
struct Player;

#[derive(Component)]
struct Obstacle(f32);

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.10, 0.15, 0.20)))
        .add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title:
                        "Isometric garden | WASD / arrows: move | Shift: run | R: reset | Esc: quit"
                            .into(),
                    resolution: (1100, 800).into(),
                    ..default()
                }),
                ..default()
            }),
        )
        .add_systems(Startup, setup)
        .add_systems(Update, (move_player, exit_on_escape))
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 19.0,
            },
            ..OrthographicProjection::default_3d()
        }),
        // Equal X/Y/Z distances give a true isometric projection.
        Transform::from_xyz(14.0, 14.0, 14.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 9000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(5.0, 10.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let tile = meshes.add(Cuboid::new(0.98, 0.25, 0.98));
    let grass = [
        materials.add(Color::srgb(0.30, 0.48, 0.32)),
        materials.add(Color::srgb(0.34, 0.52, 0.35)),
    ];
    let path = materials.add(Color::srgb(0.70, 0.64, 0.48));
    for x in -6_i32..=6 {
        for z in -6_i32..=6 {
            commands.spawn((
                Mesh3d(tile.clone()),
                MeshMaterial3d(if x == 0 || z == 0 {
                    path.clone()
                } else {
                    grass[((x + z).rem_euclid(2)) as usize].clone()
                }),
                Transform::from_xyz(x as f32, -0.125, z as f32),
            ));
        }
    }

    let trunk = meshes.add(Cylinder::new(0.22, 1.2));
    let crown = meshes.add(Cone::new(0.95, 2.0));
    let bark = materials.add(Color::srgb(0.32, 0.20, 0.13));
    let leaves = materials.add(Color::srgb(0.12, 0.33, 0.24));
    for (x, z) in [
        (-4.0, -3.0),
        (-2.0, -4.0),
        (3.0, -4.0),
        (4.0, 2.0),
        (-4.0, 3.0),
    ] {
        commands.spawn((
            Obstacle(0.65),
            Mesh3d(trunk.clone()),
            MeshMaterial3d(bark.clone()),
            Transform::from_xyz(x, 0.6, z),
        ));
        commands.spawn((
            Mesh3d(crown.clone()),
            MeshMaterial3d(leaves.clone()),
            Transform::from_xyz(x, 1.9, z),
        ));
    }
    let rock = meshes.add(Sphere::new(0.65).mesh().uv(12, 8));
    let stone = materials.add(Color::srgb(0.46, 0.51, 0.55));
    for (x, z) in [(2.5, 2.0), (-2.0, 2.5), (4.5, -1.5)] {
        commands.spawn((
            Obstacle(0.65),
            Mesh3d(rock.clone()),
            MeshMaterial3d(stone.clone()),
            Transform::from_xyz(x, 0.35, z).with_scale(Vec3::new(1.0, 0.7, 0.85)),
        ));
    }

    let coat = materials.add(Color::srgb(0.95, 0.52, 0.17));
    let face = materials.add(Color::srgb(1.0, 0.83, 0.59));
    let nose = meshes.add(Cuboid::new(0.16, 0.16, 0.25));
    commands
        .spawn((
            Player,
            Mesh3d(meshes.add(Capsule3d::new(PLAYER_RADIUS, 0.65))),
            MeshMaterial3d(coat),
            Transform::from_xyz(0.0, 0.65, 0.0),
        ))
        .with_children(|parent| {
            // The nose indicates facing direction (local +Z).
            parent.spawn((
                Mesh3d(nose),
                MeshMaterial3d(face),
                Transform::from_xyz(0.0, 0.32, 0.30),
            ));
        });
}

fn direction(input: Vec2) -> Vec3 {
    let input = input.normalize_or_zero();
    RIGHT * input.x + UP * input.y
}

fn can_stand(position: Vec3, obstacles: impl IntoIterator<Item = (Vec3, f32)>) -> bool {
    position.x.abs() <= EDGE
        && position.z.abs() <= EDGE
        && obstacles.into_iter().all(|(center, radius)| {
            Vec2::new(position.x - center.x, position.z - center.z).length_squared()
                >= (PLAYER_RADIUS + radius).powi(2)
        })
}

fn move_player(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut player: Single<&mut Transform, With<Player>>,
    obstacles: Query<(&Transform, &Obstacle), Without<Player>>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        player.translation = Vec3::new(0.0, 0.65, 0.0);
        player.rotation = Quat::IDENTITY;
        return;
    }
    let pressed = |a, b| {
        if keys.pressed(a) || keys.pressed(b) {
            1.0
        } else {
            0.0
        }
    };
    let movement = direction(Vec2::new(
        pressed(KeyCode::KeyD, KeyCode::ArrowRight) - pressed(KeyCode::KeyA, KeyCode::ArrowLeft),
        pressed(KeyCode::KeyW, KeyCode::ArrowUp) - pressed(KeyCode::KeyS, KeyCode::ArrowDown),
    ));
    let sprint = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        1.8
    } else {
        1.0
    };
    // Bound the step after pauses so movement cannot tunnel through obstacles.
    let step = movement * SPEED * sprint * time.delta_secs().min(1.0 / 30.0);
    // Resolve axes separately to slide along obstacles and island edges.
    for delta in [Vec3::X * step.x, Vec3::Z * step.z] {
        let next = player.translation + delta;
        if can_stand(next, obstacles.iter().map(|(t, o)| (t.translation, o.0))) {
            player.translation = next;
        }
    }
    if movement != Vec3::ZERO {
        player.rotation = Quat::from_rotation_y(movement.x.atan2(movement.z));
    }
}

fn exit_on_escape(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_relative_movement_has_constant_speed() {
        assert_eq!(direction(Vec2::ZERO), Vec3::ZERO);
        assert!((direction(Vec2::ONE).length() - 1.0).abs() < 1e-6);
        assert!(direction(Vec2::Y).x < 0.0 && direction(Vec2::Y).z < 0.0);
        assert!(direction(Vec2::X).x > 0.0 && direction(Vec2::X).z < 0.0);
    }

    #[test]
    fn collision_respects_obstacles_and_island_boundary() {
        assert!(can_stand(Vec3::ZERO, []));
        assert!(!can_stand(Vec3::new(EDGE + 0.1, 0.0, 0.0), []));
        assert!(!can_stand(Vec3::new(0.8, 0.0, 0.0), [(Vec3::ZERO, 0.65)]));
        assert!(can_stand(Vec3::new(1.0, 0.0, 0.0), [(Vec3::ZERO, 0.65)]));
    }
}
