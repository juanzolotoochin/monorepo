use crate::{
    gameplay::Player,
    impact::{self, ImpactTarget},
};
use bevy::{camera::ScalingMode, input::mouse::AccumulatedMouseScroll, prelude::*};
use voxel_assets::ImpactResponse;

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
}
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

pub fn blade_echo(
    commands: &mut Commands,
    assets: &FxAssets,
    origin: Vec3,
    facing: Quat,
    angle: f32,
) {
    let rotation = facing * Quat::from_rotation_y(angle);
    let position =
        origin + facing * Vec3::new(0.38, 0.90, 0.0) + rotation * Vec3::new(0.0, 0.0, 1.1);
    let size = Vec3::new(0.07, 0.035, 1.55);
    commands.spawn((
        Mesh3d(assets.cube.clone()),
        MeshMaterial3d(assets.trail.clone()),
        Transform::from_translation(position)
            .with_rotation(rotation)
            .with_scale(size),
        Particle {
            velocity: Vec3::ZERO,
            age: 0.0,
            life: 0.09,
            size,
            gravity: 0.0,
            bounce: false,
        },
    ));
}

pub fn update_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut feedback: ResMut<Feedback>,
    mut particles: Query<
        (Entity, &mut Transform, &mut Particle),
        (Without<ImpactTarget>, Without<Player>),
    >,
    mut flashes: Query<(Entity, &mut PointLight, &mut Flash)>,
    mut rocks: Query<(&mut Transform, &mut ImpactTarget), (Without<Particle>, Without<Player>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs().min(0.1);
    feedback.hit_stop = (feedback.hit_stop - dt).max(0.0);
    feedback.trauma = (feedback.trauma - dt * 1.5).max(0.0);
    feedback.flash = (feedback.flash - dt * 5.0).max(0.0);
    for (entity, mut transform, mut particle) in &mut particles {
        particle.age += dt;
        if particle.age >= particle.life {
            commands.entity(entity).despawn();
            continue;
        }
        particle.velocity.y -= particle.gravity * dt;
        transform.translation += particle.velocity * dt;
        if particle.bounce && transform.translation.y < 0.045 {
            transform.translation.y = 0.045;
            particle.velocity.y = particle.velocity.y.abs() * 0.32;
            particle.velocity.x *= 0.65;
            particle.velocity.z *= 0.65;
        }
        if particle.velocity.length_squared() > 0.1 {
            transform.rotation = Quat::from_rotation_arc(Vec3::Z, particle.velocity.normalize());
        }
        let fade = (1.0 - particle.age / particle.life).powf(0.65);
        transform.scale = particle.size * fade;
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
            if let Some(mat) = materials.get_mut(&slot.handle) {
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
    scroll: Res<AccumulatedMouseScroll>,
    camera: Single<(&mut Transform, &mut Projection, &mut CameraRig), With<Camera>>,
) {
    let (mut transform, mut projection, mut rig) = camera.into_inner();
    rig.zoom = (rig.zoom - scroll.delta.y * 0.8).clamp(12.5, 26.0);
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
