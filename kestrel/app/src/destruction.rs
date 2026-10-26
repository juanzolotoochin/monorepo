//! Cosmetic fracture and debris for authoritative destructible cover.
//! The simulation decides what broke; these entities never block gameplay.
use bevy::prelude::*;
use service::WorldScoped;
use std::collections::HashSet;
use voxel_assets::{BreakStyle, DestructiblePiece, MeshData};

#[derive(Resource)]
pub struct CoverFx {
    shard: Handle<Mesh>,
    dust: Handle<Mesh>,
    dust_material: Handle<StandardMaterial>,
}

impl FromWorld for CoverFx {
    fn from_world(world: &mut World) -> Self {
        let corners = [
            Vec3::new(0.0, 0.85, 0.0),
            Vec3::new(-0.75, -0.55, -0.6),
            Vec3::new(0.75, -0.55, -0.6),
            Vec3::new(0.0, -0.55, 0.85),
        ];
        let faces = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let mut data = MeshData::default();
        for face in faces {
            let a = corners[face[0]];
            let b = corners[face[1]];
            let c = corners[face[2]];
            let normal = (b - a).cross(c - a).normalize();
            for vertex in [a, b, c] {
                data.positions.push(vertex.to_array());
                data.normals.push(normal.to_array());
                data.indices.push(data.indices.len() as u32);
            }
        }
        let shard = world.resource_mut::<Assets<Mesh>>().add(crate::assets::mesh(&data));
        let mut ring = MeshData::default();
        for side in 0..16 {
            let a = side as f32 * std::f32::consts::TAU / 16.0;
            let b = (side + 1) as f32 * std::f32::consts::TAU / 16.0;
            let start = ring.positions.len() as u32;
            for (angle, radius) in [(a, 0.68), (a, 1.0), (b, 0.68), (b, 1.0)] {
                ring.positions.push([angle.cos() * radius, 0.0, angle.sin() * radius]);
                ring.normals.push([0.0, 1.0, 0.0]);
            }
            ring.indices.extend_from_slice(&[start, start + 2, start + 1, start + 1, start + 2, start + 3]);
        }
        let dust = world.resource_mut::<Assets<Mesh>>().add(crate::assets::mesh(&ring));
        let dust_material = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
            base_color: Color::srgba(0.77, 0.80, 0.78, 0.42),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        });
        Self { shard, dust, dust_material }
    }
}

#[derive(Component)]
pub struct CoverFragment {
    velocity: Vec3,
    spin: Vec3,
    floor: f32,
    age: f32,
    resting: f32,
    bounces: u8,
    hold: f32,
    initial_scale: Vec3,
    shatter: bool,
    materials: Vec<Handle<StandardMaterial>>,
}

#[derive(Component)]
pub(crate) struct DustRing {
    age: f32,
    material: Handle<StandardMaterial>,
}

fn random(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed as f32 / u32::MAX as f32
}

fn floor(world: &World, at: Vec3) -> f32 {
    world.resource::<crate::lockstep::SessionService>().ground_height(at).unwrap_or(0.0)
}

fn material(world: &World, root: Entity) -> Option<Handle<StandardMaterial>> {
    let child = world.get::<Children>(root)?.iter().find(|&child| world.get::<crate::style::CoverBody>(child).is_some())?;
    Some(world.get::<MeshMaterial3d<StandardMaterial>>(child)?.0.clone())
}

/// Launch a few chips on every hit, and the authored whole-piece response on
/// the final hit. The detached mesh itself tumbles for Topple or breaks down
/// among more shards for Shatter.
pub fn hit(world: &mut World, root: Entity, piece: &DestructiblePiece, index: usize, health: u16, tick: u64) {
    let Some(material) = material(world, root) else { return };
    let original = world.resource::<Assets<StandardMaterial>>().get(&material).expect("cover material").clone();
    world.init_resource::<CoverFx>();
    let shard = world.resource::<CoverFx>().shard.clone();
    let center = Vec3::from_array(piece.center);
    let outward = (center - Vec3::from_array(piece.asset_center)) * Vec3::new(1.0, 0.0, 1.0);
    let outward = outward.normalize_or_zero();
    let mut seed = (index as u32).wrapping_mul(0x9e37_79b9)
        ^ (tick as u32).wrapping_mul(0x85eb_ca6b) ^ u32::from(health).wrapping_mul(0xc2b2_ae35) ^ 0x6d2b_79f5;
    let count = if health > 0 { 4 } else if piece.break_style == BreakStyle::Shatter { 12 } else { 8 };
    let ground = floor(world, center);
    for _ in 0..count {
        let scatter = Vec3::new(random(&mut seed) * 2.0 - 1.0, 0.0, random(&mut seed) * 2.0 - 1.0);
        let direction = (outward * 1.5 + scatter).normalize_or_zero();
        let size = 0.13 + random(&mut seed) * if health > 0 { 0.10 } else { 0.24 };
        let offset = Vec3::new((random(&mut seed) - 0.5) * piece.size[0] * 0.7,
            (random(&mut seed) - 0.5) * piece.size[1] * 0.6,
            (random(&mut seed) - 0.5) * piece.size[2] * 0.7);
        let debris_material = world.resource_mut::<Assets<StandardMaterial>>().add(original.clone());
        world.spawn((
            WorldScoped::default(),
            Mesh3d(shard.clone()),
            MeshMaterial3d(debris_material.clone()),
            Transform::from_translation(center + offset).with_scale(Vec3::splat(size)),
            CoverFragment {
                velocity: direction * (2.0 + random(&mut seed) * 3.0) + Vec3::Y * (2.0 + random(&mut seed) * 3.0),
                spin: Vec3::new(4.0 + random(&mut seed) * 9.0, 3.0 + random(&mut seed) * 8.0, 4.0 + random(&mut seed) * 9.0),
                floor: ground + size * 0.3,
                age: 0.0, resting: 0.0, bounces: 0, hold: 2.4 + random(&mut seed),
                initial_scale: Vec3::splat(size), shatter: false,
                materials: vec![debris_material],
            },
        ));
    }
    if health == 0 {
        let dust = world.resource::<CoverFx>().dust.clone();
        let base = world.resource::<CoverFx>().dust_material.clone();
        let appearance = world.resource::<Assets<StandardMaterial>>().get(&base).expect("dust material").clone();
        let dust_material = world.resource_mut::<Assets<StandardMaterial>>().add(appearance);
        world.spawn((WorldScoped::default(), Mesh3d(dust), MeshMaterial3d(dust_material.clone()),
            bevy::light::NotShadowCaster,
            Transform::from_translation(Vec3::new(center.x, ground + 0.035, center.z)),
            DustRing { age: 0.0, material: dust_material }));
        let (fragment_root, materials) = if piece.repairable {
            // The authored body tumbles away; the original root and its floor
            // base remain in place so the same body can be restored later.
            let parts: Vec<_> = world.get::<Children>(root).into_iter().flat_map(|children| children.iter())
                .filter(|&child| world.get::<crate::style::CoverBody>(child).is_some())
                .filter_map(|child| Some((world.get::<Mesh3d>(child)?.0.clone(),
                    world.get::<MeshMaterial3d<StandardMaterial>>(child)?.0.clone(),
                    *world.get::<Transform>(child)?)))
                .collect();
            let detached = world.spawn((WorldScoped::default(), Transform::from_translation(center))).id();
            let mut materials = Vec::new();
            for (mesh, source, transform) in parts {
                let appearance = world.resource::<Assets<StandardMaterial>>().get(&source).expect("cover material").clone();
                let unique = world.resource_mut::<Assets<StandardMaterial>>().add(appearance);
                let child = world.spawn((Mesh3d(mesh), MeshMaterial3d(unique.clone()), transform)).id();
                world.entity_mut(detached).add_child(child);
                materials.push(unique);
            }
            (detached, materials)
        } else {
            let materials = world.get::<Children>(root).into_iter().flat_map(|children| children.iter())
                .filter_map(|child| world.get::<MeshMaterial3d<StandardMaterial>>(child).map(|handle| handle.0.clone()))
                .collect();
            (root, materials)
        };
        let impulse = outward * 2.0 + Vec3::new(random(&mut seed) - 0.5, 0.0, random(&mut seed) - 0.5);
        world.entity_mut(fragment_root).insert(CoverFragment {
            velocity: impulse + Vec3::Y * if piece.break_style == BreakStyle::Shatter { 3.8 } else { 2.8 },
            spin: Vec3::new(2.5 + random(&mut seed) * 3.0, 2.0 + random(&mut seed) * 4.0, 2.5 + random(&mut seed) * 3.0),
            floor: ground + piece.size[1] * 0.35,
            age: 0.0, resting: 0.0, bounces: 0,
            hold: if piece.break_style == BreakStyle::Shatter { 2.3 } else { 3.5 },
            initial_scale: Vec3::ONE,
            shatter: piece.break_style == BreakStyle::Shatter,
            materials,
        });
    }
}

/// Fragments bounce, settle, remain visible on the floor, then shrink away.
pub fn animate(time: Res<Time>, mut commands: Commands, mut pieces: Query<(Entity, &mut Transform, &mut CoverFragment)>,
    mut rings: Query<(Entity, &mut Transform, &mut DustRing), Without<CoverFragment>>,
    mut materials: ResMut<Assets<StandardMaterial>>) {
    let dt = time.delta_secs().min(0.05);
    for (entity, mut transform, mut fragment) in &mut pieces {
        fragment.age += dt;
        if fragment.resting == 0.0 {
            fragment.velocity.y -= 13.0 * dt;
            transform.translation += fragment.velocity * dt;
            transform.rotation *= Quat::from_euler(EulerRot::XYZ, fragment.spin.x * dt, fragment.spin.y * dt, fragment.spin.z * dt);
            if transform.translation.y <= fragment.floor {
                transform.translation.y = fragment.floor;
                if fragment.bounces < 2 && fragment.velocity.y.abs() > 1.0 {
                    fragment.velocity.y = fragment.velocity.y.abs() * 0.28;
                    fragment.velocity.x *= 0.58;
                    fragment.velocity.z *= 0.58;
                    fragment.spin *= 0.5;
                    fragment.bounces += 1;
                } else {
                    fragment.velocity = Vec3::ZERO;
                    fragment.resting = 0.0001;
                }
            }
        } else {
            fragment.resting += dt;
            let fade = ((fragment.resting - fragment.hold) / 0.8).clamp(0.0, 1.0);
            if fade >= 1.0 {
                for handle in &fragment.materials { materials.remove(handle); }
                commands.entity(entity).despawn();
                continue;
            }
            for handle in &fragment.materials {
                if let Some(mut material) = materials.get_mut(handle) {
                    let mut color = material.base_color.to_srgba();
                    color.alpha = 1.0 - fade;
                    material.base_color = Color::Srgba(color);
                    material.alpha_mode = AlphaMode::Blend;
                }
            }
            transform.translation.y -= dt * fade * 0.12;
        }
        let fade = ((fragment.resting - fragment.hold) / 0.8).clamp(0.0, 1.0);
        let fracture = if fragment.shatter { 1.0 - (fragment.age / 0.4).clamp(0.0, 0.65) } else { 1.0 };
        transform.scale = fragment.initial_scale * (1.0 - fade * 0.25) * fracture;
    }
    for (entity, mut transform, mut ring) in &mut rings {
        ring.age += dt;
        let fraction = (ring.age / 0.58).clamp(0.0, 1.0);
        if fraction >= 1.0 {
            materials.remove(&ring.material);
            commands.entity(entity).despawn();
            continue;
        }
        transform.scale = Vec3::splat(0.25 + fraction * 1.35);
        if let Some(mut material) = materials.get_mut(&ring.material) {
            material.base_color = Color::srgba(0.77, 0.80, 0.78, 0.42 * (1.0 - fraction));
        }
    }
}

/// The cover and fragments own their material instances; release them when a
/// scene is replaced, including fragments still resting on its floor.
pub fn release_materials(world: &mut World) {
    let mut handles = HashSet::new();
    let mut cover = world.query_filtered::<&MeshMaterial3d<StandardMaterial>, With<crate::style::DamageMaterial>>();
    handles.extend(cover.iter(world).map(|material| material.0.clone()));
    let mut bases = world.query_filtered::<&MeshMaterial3d<StandardMaterial>, With<crate::style::RepairBaseMaterial>>();
    handles.extend(bases.iter(world).map(|material| material.0.clone()));
    let mut debris = world.query::<&CoverFragment>();
    for fragment in debris.iter(world) { handles.extend(fragment.materials.iter().cloned()); }
    let mut rings = world.query::<&DustRing>();
    handles.extend(rings.iter(world).map(|ring| ring.material.clone()));
    let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
    for handle in handles { materials.remove(&handle); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn fragments_rest_on_the_floor_before_fading() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, animate);
        let entity = app.world_mut().spawn((Transform::from_xyz(0.0, 1.0, 0.0), CoverFragment {
            velocity: Vec3::new(0.0, 2.0, 0.0), spin: Vec3::ONE,
            floor: 0.2, age: 0.0, resting: 0.0, bounces: 0,
            hold: 2.0, initial_scale: Vec3::ONE, shatter: false, materials: vec![],
        })).id();
        for _ in 0..90 {
            app.world_mut().resource_mut::<Time>().advance_by(Duration::from_millis(16));
            app.update();
        }
        assert!(app.world().get_entity(entity).is_ok());
        let transform = app.world().get::<Transform>(entity).unwrap();
        assert!((transform.translation.y - 0.2).abs() < 0.01);
        for _ in 0..180 {
            app.world_mut().resource_mut::<Time>().advance_by(Duration::from_millis(16));
            app.update();
        }
        assert!(app.world().get_entity(entity).is_err());
    }
}
