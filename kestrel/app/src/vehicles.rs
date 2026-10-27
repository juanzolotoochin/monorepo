//! Vehicles: every vehicle the settlement parks is drawn as a rover, a
//! chassis on four wheels. The simulation moves it (`sim::settlement::vehicle`);
//! this only draws it.
//!
//! The local player's own vehicle is drawn under them, from the player's
//! drawn transform: that already holds the prediction, its smoothing, and
//! the newest confirmed pose in single player, so driver and rover never
//! drift apart. Other vehicles are interpolated between the last two ticks.
//! Each wheel sits on the ground under its drawn position, eased so a
//! terrace step lifts it over a few frames, and the chassis tilts to them.

use bevy::prelude::*;
use fx::Fx;
use service::{AppServices, Phase, Service, WorldLayer, WorldScoped, WorldState};
use sim::settlement::vehicle::wheel_points;
use sim::settlement::VehicleState;

use crate::character::Player;
use crate::lockstep::{SessionService, WorldMirror};

const WHEEL_RADIUS: f32 = 0.34;
const WHEEL_WIDTH: f32 = 0.26;
/// The chassis's middle above the wheels' contact.
const CHASSIS_LIFT: f32 = 0.46;
/// How quickly a wheel eases onto new ground, per second.
const WHEEL_EASE: f32 = 22.0;
/// Higher than the wheels' ground by this, the rover is in the air.
const AIRBORNE: f32 = 0.05;

/// The rovers drawn in this world.
#[derive(Resource, Default)]
pub struct Vehicles {
    parts: Option<Parts>,
}

struct Parts {
    chassis: Handle<Mesh>,
    hood: Handle<Mesh>,
    bar: Handle<Mesh>,
    wheel: Handle<Mesh>,
    paint: Handle<StandardMaterial>,
    trim: Handle<StandardMaterial>,
    tire: Handle<StandardMaterial>,
}

/// A drawn rover: its root turns with the heading, the chassis tilts.
#[derive(Component)]
pub struct Rover {
    index: usize,
    chassis: Entity,
    wheels: [Entity; 4],
    /// Each wheel's drawn contact height, eased toward the ground.
    contact: Option<[f32; 4]>,
    /// How far each wheel has turned, in radians.
    spin: f32,
    last: Option<Vec3>,
}

impl Service for Vehicles {
    type Layer = WorldLayer;
    type Deps = (WorldMirror, crate::character::Avatar);

    fn init(_world: &mut World) -> Self {
        Vehicles::default()
    }

    fn systems(app: &mut App) {
        app.init_resource::<crate::prediction::DrawnAvatar>()
            .add_service_systems::<Self, _>(Phase::Present, draw.run_if(in_state(WorldState::Active)));
    }
}

fn parts(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Parts {
    let material = |materials: &mut Assets<StandardMaterial>, r, g, b, roughness| {
        materials.add(StandardMaterial { base_color: Color::srgb(r, g, b), perceptual_roughness: roughness, ..default() })
    };
    Parts {
        chassis: meshes.add(Cuboid::new(1.0, 0.3, 2.0)),
        hood: meshes.add(Cuboid::new(0.95, 0.28, 0.6)),
        bar: meshes.add(Cuboid::new(1.0, 0.7, 0.08)),
        wheel: meshes.add(Cylinder::new(WHEEL_RADIUS, WHEEL_WIDTH)),
        paint: material(materials, 0.93, 0.68, 0.10, 0.55),
        trim: material(materials, 0.18, 0.18, 0.2, 0.7),
        tire: material(materials, 0.08, 0.08, 0.08, 0.95),
    }
}

fn spawn(commands: &mut Commands, parts: &Parts, index: usize, wheelbase: f32, track: f32) -> Entity {
    let chassis = commands
        .spawn((Mesh3d(parts.chassis.clone()), MeshMaterial3d(parts.paint.clone()), Transform::default()))
        .with_children(|c| {
            c.spawn((Mesh3d(parts.hood.clone()), MeshMaterial3d(parts.paint.clone()), Transform::from_xyz(0.0, 0.25, 0.65)));
            c.spawn((Mesh3d(parts.bar.clone()), MeshMaterial3d(parts.trim.clone()), Transform::from_xyz(0.0, 0.5, -0.55)));
        })
        .id();
    let local = [(-track, wheelbase), (track, wheelbase), (-track, -wheelbase), (track, -wheelbase)].map(|(x, z)| Vec3::new(x / 2.0, 0.0, z / 2.0));
    let wheels = local.map(|at| commands.spawn((Mesh3d(parts.wheel.clone()), MeshMaterial3d(parts.tire.clone()), Transform::from_translation(at))).id());
    let mut root = commands.spawn((
        Rover { index, chassis, wheels, contact: None, spin: 0.0, last: None },
        WorldScoped::default(),
        Transform::default(),
        Visibility::default(),
    ));
    root.add_child(chassis);
    for wheel in wheels {
        root.add_child(wheel);
    }
    root.id()
}

/// Where a vehicle is drawn this frame: arena position on the ground, the
/// chassis base height, and its heading.
struct Pose {
    at: Vec3,
    base: f32,
    heading: f32,
}

fn interpolated(mirror: &WorldMirror, index: usize, now: &VehicleState) -> Pose {
    let before = mirror.previous.as_ref().and_then(|p| p.vehicles.get(index)).unwrap_or(now);
    let t = mirror.alpha;
    let at = mirror.to_view(before.position).lerp(mirror.to_view(now.position), t);
    let base = before.height.to_f32() + (now.height - before.height).to_f32() * t;
    let (a, b) = (before.heading.to_f32(), now.heading.to_f32());
    let turn = (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    Pose { at, base, heading: a + turn * t }
}

fn draw(
    mut commands: Commands,
    mut vehicles: ResMut<Vehicles>,
    mirror: Res<WorldMirror>,
    service: Res<SessionService>,
    drawn: Res<crate::prediction::DrawnAvatar>,
    player: Query<&Transform, (With<Player>, Without<Rover>)>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rovers: Query<(Entity, &mut Rover)>,
    mut transforms: Query<&mut Transform, Without<Player>>,
) {
    let (Some(current), Some(terrain)) = (&mirror.current, service.world().map(sim::World::terrain)) else { return };
    if current.vehicles.is_empty() {
        return;
    }
    let parts = vehicles.parts.get_or_insert_with(|| parts(&mut meshes, &mut materials));
    let tuning = |v: &VehicleState| current.characters[v.character as usize].vehicle.expect("a vehicle character");
    let drawn_count = rovers.iter().count();
    for (index, v) in current.vehicles.iter().enumerate().skip(drawn_count) {
        let t = tuning(v);
        spawn(&mut commands, parts, index, t.wheelbase.to_f32(), t.track.to_f32());
    }
    // The vehicle the local player is drawn sitting in, if any.
    let mine = drawn.0.as_ref().and_then(|d| d.avatar().driving).map(usize::from);
    let dt = time.delta_secs();
    for (entity, mut rover) in &mut rovers {
        let Some(v) = current.vehicles.get(rover.index) else { continue };
        let t = tuning(v);
        let pose = match (mine == Some(rover.index), player.single()) {
            (true, Ok(p)) => {
                let (yaw, _, _) = p.rotation.to_euler(EulerRot::YXZ);
                Pose { at: Vec3::new(p.translation.x, 0.0, p.translation.z), base: p.translation.y - t.seat.to_f32(), heading: yaw }
            }
            _ => interpolated(&mirror, rover.index, v),
        };
        // The ground under each wheel where it is drawn.
        let points = wheel_points(mirror.to_sim(pose.at), Fx::quantize(pose.heading), &t);
        let mut ground = points.map(|p| terrain.ground(p).map_or(pose.base, |g| g.to_f32()));
        let mean = ground.iter().sum::<f32>() / 4.0;
        if pose.base > mean + AIRBORNE {
            // In the air the wheels hang at the chassis.
            ground = [pose.base; 4];
        }
        let jumped = rover.last.is_none_or(|last| last.distance(pose.at) > 1.5);
        let ease = 1.0 - (-WHEEL_EASE * dt).exp();
        let contact = match rover.contact {
            Some(c) if !jumped => std::array::from_fn(|i| c[i] + (ground[i] - c[i]) * ease),
            _ => ground,
        };
        rover.contact = Some(contact);
        if let Some(last) = rover.last.filter(|_| !jumped) {
            let forward = Vec3::new(pose.heading.sin(), 0.0, pose.heading.cos());
            rover.spin += (pose.at - last).dot(forward) / WHEEL_RADIUS;
        }
        rover.last = Some(pose.at);

        let (front, rear) = ((contact[0] + contact[1]) / 2.0, (contact[2] + contact[3]) / 2.0);
        let (left, right) = ((contact[0] + contact[2]) / 2.0, (contact[1] + contact[3]) / 2.0);
        let pitch = ((front - rear) / t.wheelbase.to_f32()).atan();
        let roll = ((left - right) / t.track.to_f32()).atan();
        let lift = contact.iter().sum::<f32>() / 4.0 + CHASSIS_LIFT;
        if let Ok(mut root) = transforms.get_mut(entity) {
            *root = Transform::from_translation(pose.at).with_rotation(Quat::from_rotation_y(pose.heading));
        }
        if let Ok(mut chassis) = transforms.get_mut(rover.chassis) {
            chassis.translation.y = lift.max(pose.base + CHASSIS_LIFT);
            chassis.rotation = Quat::from_rotation_x(-pitch) * Quat::from_rotation_z(-roll);
        }
        for (i, &wheel) in rover.wheels.iter().enumerate() {
            if let Ok(mut w) = transforms.get_mut(wheel) {
                w.translation.y = contact[i] + WHEEL_RADIUS;
                w.rotation = Quat::from_rotation_x(rover.spin) * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
            }
        }
    }
}
