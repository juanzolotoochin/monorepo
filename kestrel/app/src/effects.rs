use crate::{
    character::{LocalBlade, Player},
    combat::Obstacle,
    gameplay::Controls,
    impact::{self, ImpactTarget},
    peers::Peer,
    style::WeaponMuzzle,
};
use bevy::{camera::ScalingMode, prelude::*};
use voxel_assets::ImpactResponse;

/// Particles, flashes and camera shake (World layer).
#[derive(Resource)]
pub struct Effects;

impl service::Service for Effects {
    type Layer = service::WorldLayer;
    type Deps = ();

    fn init(_world: &mut World) -> Self {
        Effects
    }

    fn systems(app: &mut App) {
        use service::{AppServices, Phase, WorldState};
        app.init_resource::<PendingLasers>()
            .add_service_systems::<Self, _>(
                Phase::Present,
                (update_effects, camera_feedback.run_if(crate::control::rendered)).run_if(in_state(WorldState::Active)),
            )
            .add_systems(
                PostUpdate,
                emit_lasers
                    .after(bevy::transform::TransformSystems::Propagate)
                    .run_if(in_state(WorldState::Active)),
            );
    }
}

/// Hit stop, screen shake and flash: shared by the player's combat and the
/// effects, and reset when a world starts.
#[derive(Resource, Default)]
pub struct Feedback {
    pub hit_stop: f32,
    pub trauma: f32,
    pub flash: f32,
}
#[derive(Component)]
pub struct CameraRig {
    pub base: Vec3,
    pub target: Vec3,
    pub zoom: f32,
}
#[derive(Resource)]
pub struct FxAssets {
    pub cube: Handle<Mesh>,
    pub spark: [Handle<StandardMaterial>; 4],
    pub debris: [Handle<StandardMaterial>; 4],
    pub trail: Handle<StandardMaterial>,
    pub laser: Handle<StandardMaterial>,
    pub blood: [Handle<StandardMaterial>; 2],
}
#[derive(Resource, Default)]
pub struct PendingLasers(pub Vec<LaserShot>);

pub struct LaserShot {
    pub agent: u16,
    pub target: Vec3,
    pub hit_actor: bool,
    pub seed: u32,
}

#[derive(Component)]
pub(crate) struct LaserBolt {
    from: Vec3,
    direction: Vec3,
    distance: f32,
    age: f32,
    impact: Option<(Vec3, Vec3)>,
    hit_actor: bool,
    seed: u32,
}

const LASER_SPEED: f32 = 80.0;
const LASER_LENGTH: f32 = 0.65;
const LASER_WIDTH: f32 = 0.12;
#[derive(Component)]
pub struct Particle {
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
    pub size: Vec3,
    pub gravity: f32,
    pub bounce: bool,
}
#[derive(Component)]
pub struct SeveredHead {
    floor_height: f32,
    original_rotation: Quat,
    spin: f32,
}
#[derive(Component)]
pub struct Flash {
    age: f32,
    intensity: f32,
}

fn random(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed as f32 / u32::MAX as f32
}

/// Stable per-shot variation, independent of render frame timing.
pub fn laser_impact_seed(tick: u64, agent: u16) -> u32 {
    (tick as u32).wrapping_mul(0x9e37_79b9)
        ^ ((tick >> 32) as u32)
        ^ (agent as u32).wrapping_mul(0x85eb_ca6b)
}

pub fn impact(
    commands: &mut Commands,
    assets: &FxAssets,
    position: Vec3,
    normal: Vec3,
    number: u32,
    kind: ImpactResponse,
) {
    let response = impact::response(kind);
    let bank = impact::index(kind);
    let mut seed = number.wrapping_mul(747796405).wrapping_add(2891336453);
    for i in 0..response.sparks + response.debris {
        let random_dir = Vec3::new(
            random(&mut seed) * 2.0 - 1.0,
            random(&mut seed) * 1.3,
            random(&mut seed) * 2.0 - 1.0,
        );
        let direction = (normal * 1.4 + random_dir).normalize_or(Vec3::Y);
        let speed = response.speed * (0.5 + random(&mut seed));
        let size = if i < response.sparks {
            Vec3::new(0.045, 0.045, 0.20 + random(&mut seed) * 0.20)
        } else {
            Vec3::splat(0.06 + random(&mut seed) * 0.08)
        };
        commands.spawn((
            Mesh3d(assets.cube.clone()),
            MeshMaterial3d(if i < response.sparks {
                assets.spark[bank].clone()
            } else {
                assets.debris[bank].clone()
            }),
            Transform::from_translation(position + normal * 0.06).with_scale(size),
            Particle {
                velocity: direction * speed,
                age: 0.0,
                life: 0.28 + random(&mut seed) * 0.52,
                size,
                gravity: 14.0,
                bounce: true,
            },
        ));
    }
    // Soft surfaces shed flakes without a hot contact flash or point light.
    if response.light > 0.0 {
        commands.spawn((
            Mesh3d(assets.cube.clone()),
            MeshMaterial3d(assets.spark[bank].clone()),
            Transform::from_translation(position).with_scale(Vec3::splat(0.16)),
            Particle {
                velocity: Vec3::ZERO,
                age: 0.0,
                life: 0.065,
                size: Vec3::splat(0.16),
                gravity: 0.0,
                bounce: false,
            },
        ));
        commands.spawn((
            PointLight {
                color: Color::srgb(response.color[0], response.color[1], response.color[2]),
                intensity: response.light,
                range: 4.5,
                radius: 0.15,
                ..default()
            },
            Transform::from_translation(position + normal * 0.2),
            Flash {
                age: 0.0,
                intensity: response.light,
            },
        ));
    }
}

pub fn blade_echo(commands: &mut Commands, assets: &FxAssets, blade: &GlobalTransform) {
    let (scale, rotation, _) = blade.to_scale_rotation_translation();
    let position = blade.transform_point(Vec3::new(-0.18, 0.015, 1.05));
    let size = Vec3::new(0.035, 0.022, 0.95) * scale;
    commands.spawn((
        Mesh3d(assets.cube.clone()),
        MeshMaterial3d(assets.trail.clone()),
        Transform::from_translation(position)
            .with_rotation(rotation)
            .with_scale(size),
        Particle {
            velocity: Vec3::ZERO,
            age: 0.0,
            life: 0.055,
            size,
            gravity: 0.0,
            bounce: false,
        },
    ));
}

/// Emit from the posed weapon after transform propagation, so the streak
/// follows the hand, elbow and torso through every kind of cut.
pub fn blade_trail(
    mut commands: Commands,
    controls: Res<Controls>,
    blades: Query<&GlobalTransform, With<LocalBlade>>,
    assets: Res<FxAssets>,
) {
    let Some(attack) = controls.attack else {
        return;
    };
    let (windup, active_end, _) = attack.timing();
    if attack.weapon != content::scenario::Weapon::Sword
        || attack.impact_angle.is_some()
        || !(windup..active_end).contains(&attack.age)
    {
        return;
    }
    for blade in &blades {
        blade_echo(&mut commands, &assets, blade);
    }
}

/// Voxel droplets travel away from the cut, then bounce and fade on the ground.
pub fn blood(world: &mut World, position: Vec3, direction: Vec3, seed: u32) {
    let assets = world.resource::<FxAssets>();
    let cube = assets.cube.clone();
    let materials = assets.blood.clone();
    let mut seed = seed.wrapping_mul(747796405).wrapping_add(2891336453);
    for i in 0..22 {
        let spread = Vec3::new(
            random(&mut seed) * 2.0 - 1.0,
            random(&mut seed) * 1.2,
            random(&mut seed) * 2.0 - 1.0,
        );
        let velocity = (direction * 2.2 + spread).normalize_or(Vec3::Y)
            * (2.0 + random(&mut seed) * 2.8)
            + Vec3::Y * 1.5;
        let size = Vec3::splat(0.07 + random(&mut seed) * 0.075);
        world.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(materials[i % 2].clone()),
            Transform::from_translation(position).with_scale(size),
            Particle {
                velocity,
                age: 0.0,
                life: 0.48 + random(&mut seed) * 0.45,
                size,
                gravity: 13.0,
                bounce: true,
            },
        ));
    }
    for i in 0..7 {
        let x = (random(&mut seed) * 2.0 - 1.0) * 0.55;
        let z = (random(&mut seed) * 2.0 - 1.0) * 0.55;
        let size = Vec3::new(
            0.13 + random(&mut seed) * 0.14,
            0.012,
            0.12 + random(&mut seed) * 0.17,
        );
        world.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(materials[i % 2].clone()),
            Transform::from_translation(Vec3::new(position.x + x, 0.012, position.z + z))
                .with_scale(size),
            Particle {
                velocity: Vec3::ZERO,
                age: 0.0,
                life: 3.0,
                size,
                gravity: 0.0,
                bounce: false,
            },
        ));
    }
}

pub fn severed_head(
    world: &mut World,
    template: &crate::rig::RigTemplate,
    transform: Transform,
    seed: u32,
) {
    let side = if seed & 1 == 0 { 1.0 } else { -1.0 };
    if let Some(root) = template.spawn_part(world, voxel_assets::ActorPart::Head, transform) {
        world.entity_mut(root).insert((
            Particle {
                velocity: Vec3::new(side * 2.4, 3.5, 1.2),
                age: 0.,
                life: 2.0,
                size: transform.scale,
                gravity: 12.,
                bounce: true,
            },
            SeveredHead {
                floor_height: transform.scale.y * 0.21,
                original_rotation: transform.rotation,
                spin: side,
            },
        ));
    }
}

pub fn arc(world: &mut World, from: Vec3, to: Vec3, seed: u32) {
    let assets = world.resource::<FxAssets>();
    let cube = assets.cube.clone();
    let glow = assets.spark[2].clone();
    let delta = to - from;
    let segments = (delta.length() * 9.0).ceil().max(3.0) as usize;
    let mut seed = seed.wrapping_mul(747796405).wrapping_add(2891336453);
    for i in 0..segments {
        let f = (i as f32 + 0.5) / segments as f32;
        let jitter = Vec3::new(
            random(&mut seed) - 0.5,
            random(&mut seed) - 0.5,
            random(&mut seed) - 0.5,
        ) * 0.12;
        let size = Vec3::splat(0.045 + random(&mut seed) * 0.035);
        world.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(glow.clone()),
            Transform::from_translation(from + delta * f + jitter).with_scale(size),
            Particle {
                velocity: Vec3::ZERO,
                age: 0.,
                life: 0.13,
                size,
                gravity: 0.,
                bounce: false,
            },
        ));
    }
}

fn laser_segment(distance: f32, age: f32) -> Option<(f32, f32)> {
    let travel = 0.18 + age * LASER_SPEED;
    let front = travel.min(distance);
    let tail = (travel - LASER_LENGTH).max(0.0);
    (front > tail).then_some((tail, front))
}

fn laser_reached(distance: f32, age: f32) -> bool {
    0.18 + age * LASER_SPEED >= distance
}

fn obstacle_impact(target: Vec3, obstacles: &[crate::navigation::Disc]) -> Option<(Vec3, Vec3)> {
    obstacles.iter().find_map(|obstacle| {
        let offset = Vec3::new(target.x - obstacle.center.x, 0.0, target.z - obstacle.center.z);
        ((offset.length() - obstacle.radius).abs() <= 0.15)
            .then(|| (target, offset.normalize_or(Vec3::Y)))
    })
}

/// Spawn visual shots after pose propagation, using the actual weapon mesh's
/// authored muzzle marker for both the local avatar and replicated peers.
pub fn emit_lasers(world: &mut World) {
    let shots = std::mem::take(&mut world.resource_mut::<PendingLasers>().0);
    if shots.is_empty() {
        return;
    }
    let me = world.resource::<crate::lockstep::WorldMirror>().player();
    let mut markers = world.query::<(Entity, &WeaponMuzzle, &GlobalTransform)>();
    let muzzles: Vec<_> = markers.iter(world).filter_map(|(entity, marker, transform)| {
        if marker.0 != content::scenario::Weapon::Laser {
            return None;
        }
        let mut ancestor = Some(entity);
        let mut owner = None;
        while let Some(current) = ancestor {
            owner = if world.get::<Player>(current).is_some() {
                me
            } else {
                world.get::<Peer>(current).map(Peer::agent)
            };
            if owner.is_some() { break; }
            ancestor = world.get::<ChildOf>(current).map(ChildOf::parent);
        }
        let agent = owner?;
        Some((agent, transform.translation()))
    }).collect();
    let mut obstacle_query = world.query::<&Obstacle>();
    let obstacles: Vec<_> = obstacle_query.iter(world).map(|obstacle| obstacle.0).collect();
    let assets = world.resource::<FxAssets>();
    let (cube, red) = (assets.cube.clone(), assets.laser.clone());
    for shot in shots {
        let Some(&(_, from)) = muzzles.iter().find(|(agent, _)| *agent == shot.agent) else { continue };
        let delta = shot.target - from;
        let distance = delta.length();
        if distance < 0.05 { continue; }
        let direction = delta / distance;
        let (_, front) = laser_segment(distance, 0.0).unwrap();
        let transform = Transform::from_translation(from + direction * (front * 0.5))
            .with_rotation(Quat::from_rotation_arc(Vec3::Z, direction))
            .with_scale(Vec3::new(LASER_WIDTH, LASER_WIDTH, front));
        let obstacle_hit = obstacle_impact(shot.target, &obstacles);
        world.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(red.clone()),
            transform,
            GlobalTransform::from(transform),
            bevy::light::NotShadowCaster,
            LaserBolt { from, direction, distance, age: 0.0, impact: obstacle_hit, hit_actor: shot.hit_actor, seed: shot.seed },
        ));
    }
}

pub fn deflection(world: &mut World, position: Vec3, seed: u32) {
    let assets = world.resource::<FxAssets>();
    let cube = assets.cube.clone();
    let glow = assets.spark[1].clone();
    let mut seed = seed.wrapping_mul(747796405).wrapping_add(2891336453);
    for _ in 0..14 {
        let velocity = Vec3::new(
            random(&mut seed) * 2. - 1.,
            random(&mut seed) * 1.5,
            random(&mut seed) * 2. - 1.,
        ) * 2.5;
        let size = Vec3::new(0.035, 0.035, 0.16);
        world.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(glow.clone()),
            Transform::from_translation(position).with_scale(size),
            Particle {
                velocity,
                age: 0.,
                life: 0.33,
                size,
                gravity: 7.,
                bounce: true,
            },
        ));
    }
}

pub fn update_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut feedback: ResMut<Feedback>,
    mut particles: Query<
        (Entity, &mut Transform, &mut Particle, Option<&SeveredHead>),
        (Without<ImpactTarget>, Without<Player>, Without<LaserBolt>),
    >,
    mut bolts: Query<
        (Entity, &mut Transform, &mut LaserBolt),
        (Without<Particle>, Without<ImpactTarget>),
    >,
    assets: Res<FxAssets>,
    sound: Res<crate::sound::Sound>,
    mut flashes: Query<(Entity, &mut PointLight, &mut Flash)>,
    mut rocks: Query<
        (&mut Transform, &mut ImpactTarget),
        (Without<Particle>, Without<Player>, Without<LaserBolt>),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs().min(0.1);
    feedback.hit_stop = (feedback.hit_stop - dt).max(0.0);
    feedback.trauma = (feedback.trauma - dt * 1.5).max(0.0);
    feedback.flash = (feedback.flash - dt * 5.0).max(0.0);
    for (entity, mut transform, mut particle, head) in &mut particles {
        particle.age += dt;
        if particle.age >= particle.life {
            commands.entity(entity).despawn();
            continue;
        }
        particle.velocity.y -= particle.gravity * dt;
        transform.translation += particle.velocity * dt;
        let floor_height = head.map_or(0.045, |head| head.floor_height);
        if particle.bounce && transform.translation.y < floor_height {
            transform.translation.y = floor_height;
            particle.velocity.y = particle.velocity.y.abs() * 0.32;
            particle.velocity.x *= 0.65;
            particle.velocity.z *= 0.65;
        }
        if let Some(head) = head {
            transform.rotation = head.original_rotation
                * Quat::from_rotation_z(head.spin * particle.age * 7.0)
                * Quat::from_rotation_x(particle.age * 2.5);
        } else if particle.velocity.length_squared() > 0.1 {
            transform.rotation = Quat::from_rotation_arc(Vec3::Z, particle.velocity.normalize());
        }
        let fade = (1.0 - particle.age / particle.life).powf(0.65);
        transform.scale = particle.size * fade;
    }
    for (entity, mut transform, mut bolt) in &mut bolts {
        bolt.age += dt;
        if laser_reached(bolt.distance, bolt.age) {
            if bolt.hit_actor {
                sound.play(crate::sound::Cue::LaserImpact);
                bolt.hit_actor = false;
            }
            if let Some((position, normal)) = bolt.impact.take() {
                sound.play(crate::sound::Cue::LaserImpact);
                impact(
                    &mut commands,
                    &assets,
                    position,
                    normal,
                    bolt.seed ^ position.x.to_bits() ^ position.z.to_bits(),
                    ImpactResponse::Metal,
                );
            }
        }
        let Some((tail, front)) = laser_segment(bolt.distance, bolt.age) else {
            commands.entity(entity).despawn();
            continue;
        };
        transform.translation = bolt.from + bolt.direction * ((tail + front) * 0.5);
        transform.scale.z = front - tail;
    }
    for (entity, mut light, mut flash) in &mut flashes {
        flash.age += dt;
        light.intensity = flash.intensity * (1.0 - flash.age / 0.1).max(0.0).powi(2);
        if flash.age > 0.1 {
            commands.entity(entity).despawn();
        }
    }
    for (mut transform, mut rock) in &mut rocks {
        let base = rock.base;
        transform.translation = base + rock.kick;
        rock.kick *= (-28.0 * dt).exp();
        for slot in &mut rock.slots {
            let response = impact::response(slot.kind);
            slot.flash = (slot.flash - dt / response.flash_seconds).max(0.0);
            if let Some(mut mat) = materials.get_mut(&slot.handle) {
                // Add to, rather than overwrite, authored emission (e.g. crystal).
                mat.emissive = LinearRgba {
                    red: slot.rest_emissive.red + response.color[0] * slot.flash * 3.0,
                    green: slot.rest_emissive.green + response.color[1] * slot.flash * 3.0,
                    blue: slot.rest_emissive.blue + response.color[2] * slot.flash * 3.0,
                    ..slot.rest_emissive
                };
            }
        }
    }
}

pub fn camera_feedback(
    time: Res<Time>,
    feedback: Res<Feedback>,
    input: Res<debug_ui::GameInput>,
    camera: Single<(&mut Transform, &mut Projection, &mut CameraRig), With<Camera>>,
) {
    let (mut transform, mut projection, mut rig) = camera.into_inner();
    rig.zoom = (rig.zoom - input.scroll().y * 0.8).clamp(3.0, 26.0);
    if let Projection::Orthographic(p) = &mut *projection {
        if let ScalingMode::FixedVertical { viewport_height } = &mut p.scaling_mode {
            *viewport_height +=
                (rig.zoom - *viewport_height) * (1.0 - (-12.0 * time.delta_secs()).exp());
        }
    }
    let t = time.elapsed_secs();
    let amount = feedback.trauma.powi(2);
    // Bounded and decaying; never accumulate displacement or alter the aiming plane.
    let offset =
        Vec3::new((t * 97.0).sin(), (t * 121.0).cos() * 0.45, (t * 83.0).sin()) * amount * 0.32;
    *transform = Transform::from_translation(rig.base + offset)
        .looking_at(rig.target + offset * 0.3, Vec3::Y);
}

#[cfg(test)]
mod laser_tests {
    use super::*;

    #[test]
    fn bolt_is_short_and_crosses_twelve_meters_quickly() {
        let distance = 12.0;
        let first = laser_segment(distance, 0.0).unwrap();
        assert_eq!(first.0, 0.0);
        assert!(first.1 <= 0.2);
        let middle = laser_segment(distance, 0.1).unwrap();
        assert!(middle.1 - middle.0 <= LASER_LENGTH + 1e-5);
        assert!(middle.0 > 4.0);
        assert!(laser_segment(distance, 0.25).is_none());
    }

    #[test]
    fn obstacle_impact_only_at_collision_boundary() {
        let obstacles = [crate::navigation::Disc { center: Vec3::new(3.5, 0.0, -2.5), radius: 0.9 }];
        assert!(obstacle_impact(Vec3::new(3.5, 0.8, -1.575), &obstacles).is_some());
        assert!(obstacle_impact(Vec3::new(3.5, 0.8, 2.0), &obstacles).is_none());
    }

    #[test]
    fn impact_still_triggers_when_a_frame_skips_past_the_endpoint() {
        assert!(laser_segment(4.0, 2.0 / 30.0).is_none());
        assert!(laser_reached(4.0, 2.0 / 30.0));
    }

    #[test]
    fn repeated_hits_at_one_cover_position_get_distinct_deterministic_bursts() {
        assert_eq!(laser_impact_seed(42, 1), laser_impact_seed(42, 1));
        assert_ne!(laser_impact_seed(42, 1), laser_impact_seed(43, 1));
        assert_ne!(laser_impact_seed(42, 1), laser_impact_seed(42, 2));
    }
}
