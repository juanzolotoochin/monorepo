//! The Bevy view of the simulation. Data flows one way: the simulation
//! produces an [`Extract`] each tick, and the view draws it, interpolating
//! between the last two ticks. Nothing here feeds back into the simulation
//! except commands, which the app sends through the turn path.
//!
//! Coordinates: the simulation's x and y are the ground plane and z is up.
//! Bevy is y-up, so a simulation point (x, y, z) is drawn at (x, z, y).

mod extract;

use std::collections::HashMap;

use bevy::camera::ScalingMode;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;

pub use extract::{Drawn, Extract, Interpolator, MoverExtract};

/// The interpolation state, written by the app after stepping the simulation.
#[derive(Resource, Default)]
pub struct SimView {
    pub interpolator: Interpolator,
    /// How far the render time is between the previous and current tick, in [0, 1].
    pub alpha: f32,
}

/// What the view needs from the content's `view` sections and the world size.
#[derive(Clone)]
pub struct ViewConfig {
    /// Per mover kind, by dense index.
    pub mover_colors: Vec<(f32, f32, f32)>,
    /// World size in tiles.
    pub world_size: Vec2,
    pub chunk_tiles: f32,
}

pub struct ViewPlugin(pub ViewConfig);

/// Runs after the app has stepped the simulation and updated [`SimView`].
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct DrawSystems;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Config(self.0.clone()))
            .init_resource::<SimView>()
            .init_resource::<Visuals>()
            .add_message::<ShowMarker>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (sync_movers, draw_chunk_grid, camera_controls, spawn_markers, fade_markers).in_set(DrawSystems),
            );
    }
}

#[derive(Resource)]
struct Config(ViewConfig);

#[derive(Resource, Default)]
struct Visuals {
    entities: HashMap<u64, Entity>,
    mesh: Handle<Mesh>,
    /// Per kind: (awake, asleep).
    materials: Vec<(Handle<StandardMaterial>, Handle<StandardMaterial>)>,
    marker: Handle<Mesh>,
    marker_material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct MoverVisual;

#[derive(Component)]
pub struct MainCamera;

/// A cosmetic marker that fades out; used for instant click feedback.
#[derive(Component)]
struct Marker {
    age: f32,
}

/// Simulation ground point to Bevy position.
pub fn to_bevy(pos: [f32; 3]) -> Vec3 {
    Vec3::new(pos[0], pos[2], pos[1])
}

fn setup(
    mut commands: Commands,
    config: Res<Config>,
    mut visuals: ResMut<Visuals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let config = &config.0;
    visuals.mesh = meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap());
    visuals.materials = config
        .mover_colors
        .iter()
        .map(|&(r, g, b)| {
            let awake = materials.add(StandardMaterial { base_color: Color::srgb(r, g, b), ..default() });
            let asleep = materials.add(StandardMaterial { base_color: Color::srgb(r * 0.35, g * 0.35, b * 0.35), ..default() });
            (awake, asleep)
        })
        .collect();
    visuals.marker = meshes.add(Torus::new(0.8, 1.0));
    visuals.marker_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.8),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });

    let size = config.world_size;
    let center = Vec3::new(size.x / 2.0, 0.0, size.y / 2.0);
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(size.x, size.y))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.2, 0.17),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_translation(center),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(-0.5, 1.0, 0.3).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        MainCamera,
        Camera3d::default(),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical { viewport_height: size.y.min(120.0) },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(center + Vec3::new(100.0, 140.0, 100.0)).looking_at(center, Vec3::Y),
    ));
}

/// Spawns, despawns and moves one Bevy entity per mover in the extract.
fn sync_movers(
    mut commands: Commands,
    view: Res<SimView>,
    mut visuals: ResMut<Visuals>,
    mut transforms: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>), With<MoverVisual>>,
) {
    if !view.is_changed() {
        return;
    }
    let mut seen = HashMap::with_capacity(visuals.entities.len());
    for drawn in view.interpolator.sample(view.alpha) {
        let (awake, asleep) = &visuals.materials[drawn.kind as usize];
        let material = if drawn.asleep { asleep.clone() } else { awake.clone() };
        let mut pos = to_bevy(drawn.pos);
        pos.y += drawn.radius;
        let transform = Transform::from_translation(pos).with_scale(Vec3::splat(drawn.radius));
        let entity = match visuals.entities.get(&drawn.id) {
            Some(&entity) => {
                if let Ok((mut t, mut m)) = transforms.get_mut(entity) {
                    *t = transform;
                    if m.0 != material {
                        m.0 = material;
                    }
                }
                entity
            }
            None => commands
                .spawn((MoverVisual, Mesh3d(visuals.mesh.clone()), MeshMaterial3d(material), transform))
                .id(),
        };
        seen.insert(drawn.id, entity);
    }
    for (id, entity) in &visuals.entities {
        if !seen.contains_key(id) {
            commands.entity(*entity).despawn();
        }
    }
    visuals.entities = seen;
}

fn draw_chunk_grid(config: Res<Config>, mut gizmos: Gizmos) {
    let config = &config.0;
    let color = Color::srgba(0.6, 0.7, 0.6, 0.15);
    let mut x = 0.0;
    while x <= config.world_size.x {
        gizmos.line(Vec3::new(x, 0.02, 0.0), Vec3::new(x, 0.02, config.world_size.y), color);
        x += config.chunk_tiles;
    }
    let mut y = 0.0;
    while y <= config.world_size.y {
        gizmos.line(Vec3::new(0.0, 0.02, y), Vec3::new(config.world_size.x, 0.02, y), color);
        y += config.chunk_tiles;
    }
}

/// WASD or arrows pan, the mouse wheel zooms.
fn camera_controls(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    camera: Single<(&mut Transform, &mut Projection), With<MainCamera>>,
) {
    let (mut transform, mut projection) = camera.into_inner();
    let Projection::Orthographic(ortho) = &mut *projection else { return };
    let ScalingMode::FixedVertical { viewport_height } = &mut ortho.scaling_mode else { return };
    if scroll.delta.y != 0.0 {
        *viewport_height = (*viewport_height * (1.0 - 0.1 * scroll.delta.y.signum())).clamp(10.0, 600.0);
    }
    let forward = Vec3::new(-1.0, 0.0, -1.0).normalize();
    let right = Vec3::new(1.0, 0.0, -1.0).normalize();
    let mut pan = Vec3::ZERO;
    for (keys_for, dir) in [
        ([KeyCode::KeyW, KeyCode::ArrowUp], forward),
        ([KeyCode::KeyS, KeyCode::ArrowDown], -forward),
        ([KeyCode::KeyD, KeyCode::ArrowRight], right),
        ([KeyCode::KeyA, KeyCode::ArrowLeft], -right),
    ] {
        if keys.any_pressed(keys_for) {
            pan += dir;
        }
    }
    transform.translation += pan * *viewport_height * 0.8 * time.delta_secs();
}

/// The point on the ground under the cursor, in simulation coordinates (x, y).
pub fn cursor_ground(window: &Window, camera: &Camera, transform: &GlobalTransform) -> Option<Vec2> {
    let ray = camera.viewport_to_world(transform, window.cursor_position()?).ok()?;
    let point = ray.get_point(ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?);
    Some(Vec2::new(point.x, point.z))
}

/// Asks the view for a fading ring at a simulation ground point: instant,
/// cosmetic feedback while the command travels through the input delay.
#[derive(Message, Clone, Copy, Debug)]
pub struct ShowMarker {
    pub at: Vec2,
    pub radius: f32,
}

fn spawn_markers(mut commands: Commands, mut requests: MessageReader<ShowMarker>, visuals: Res<Visuals>) {
    for request in requests.read() {
        commands.spawn((
            Marker { age: 0.0 },
            Mesh3d(visuals.marker.clone()),
            MeshMaterial3d(visuals.marker_material.clone()),
            Transform::from_translation(Vec3::new(request.at.x, 0.05, request.at.y))
                .with_scale(Vec3::new(request.radius, 0.05, request.radius)),
        ));
    }
}

fn fade_markers(mut commands: Commands, time: Res<Time>, mut markers: Query<(Entity, &mut Marker, &mut Transform)>) {
    for (entity, mut marker, mut transform) in &mut markers {
        marker.age += time.delta_secs();
        if marker.age > 0.4 {
            commands.entity(entity).despawn();
        } else {
            transform.scale.x *= 1.0 + time.delta_secs() * 1.5;
            transform.scale.z = transform.scale.x;
        }
    }
}
