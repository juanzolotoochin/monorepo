//! Optional, scene-scoped vegetation and ambient effects. Grass bends in a GPU vertex shader.
use crate::{assets::SceneLibrary, character::Player};
use voxel_assets::AtmosphereProfile;
use bevy::{
    asset::RenderAssetUsages,
    asset::uuid::Uuid,
    mesh::Indices,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
    shader::ShaderRef,
};
use std::marker::PhantomData;

const GRASS_SHADER: Handle<Shader> = Handle::Uuid(Uuid::from_u128(0x67b09146_4c8e_47f0_a945_4a2d51c1a302), PhantomData);
const BEND_MAP_SIZE: usize = 256;

pub type GrassMaterial = ExtendedMaterial<StandardMaterial, GrassExtension>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GrassExtension {
    #[uniform(100)]
    wind: Vec4,
    #[texture(101)]
    #[sampler(102)]
    bend_map: Handle<Image>,
    #[uniform(103)]
    bend_settings: Vec4,
}

impl MaterialExtension for GrassExtension {
    fn vertex_shader() -> ShaderRef { GRASS_SHADER.clone().into() }
    fn enable_prepass() -> bool { false }
    fn enable_shadows() -> bool { false }
}

pub fn register_shader(app: &mut App) {
    app.world_mut().resource_mut::<Assets<Shader>>()
        .insert(GRASS_SHADER.id(), Shader::from_wesl(include_str!("../shaders/grass.wesl"), "grass.wesl"))
        .expect("static grass shader handle");
}

#[derive(Clone)]
struct Blade {
    root: Vec3,
    height: f32,
    width: f32,
    phase: f32,
    tint: f32,
}

/// A small world-space deformation field. Each texel stores the direction and
/// strength of a bent patch; it decays independently of the character's position.
struct BendField {
    values: Vec<Vec2>,
    previous_player: Option<Vec2>,
}

impl BendField {
    fn new() -> Self {
        Self { values: vec![Vec2::ZERO; BEND_MAP_SIZE * BEND_MAP_SIZE], previous_player: None }
    }

    fn advance(&mut self, player: Vec2, dt: f32, extent: f32, radius: f32, recovery: f32) {
        let fade = (-dt.min(0.25) / recovery).exp();
        for value in &mut self.values { *value *= fade; }
        let previous = self.previous_player.replace(player);
        let Some(previous) = previous else { return; };
        let travel = player - previous;
        let length = travel.length();
        if length < 0.005 || length > 3.0 { return; }
        let direction = travel / length;
        let to_pixel = |v: f32| ((v + extent) / (2.0 * extent) * BEND_MAP_SIZE as f32) as i32;
        let min_x = (to_pixel(previous.x.min(player.x) - radius) - 1).clamp(0, BEND_MAP_SIZE as i32 - 1);
        let max_x = (to_pixel(previous.x.max(player.x) + radius) + 1).clamp(0, BEND_MAP_SIZE as i32 - 1);
        let min_y = (to_pixel(previous.y.min(player.y) - radius) - 1).clamp(0, BEND_MAP_SIZE as i32 - 1);
        let max_y = (to_pixel(previous.y.max(player.y) + radius) + 1).clamp(0, BEND_MAP_SIZE as i32 - 1);
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let point = Vec2::new(
                    (x as f32 + 0.5) / BEND_MAP_SIZE as f32 * 2.0 * extent - extent,
                    (y as f32 + 0.5) / BEND_MAP_SIZE as f32 * 2.0 * extent - extent,
                );
                let along = ((point - previous).dot(travel) / travel.length_squared()).clamp(0.0, 1.0);
                let distance = point.distance(previous + travel * along);
                let strength = (1.0 - distance / radius).clamp(0.0, 1.0).powi(2);
                let value = &mut self.values[y as usize * BEND_MAP_SIZE + x as usize];
                if strength > value.length() { *value = direction * strength; }
            }
        }
    }

    fn write_image(&self, image: &mut Image) {
        let Some(bytes) = image.data.as_mut() else { return; };
        for (value, pixel) in self.values.iter().zip(bytes.chunks_exact_mut(4)) {
            let strength = value.length().min(1.0);
            let direction = if strength > 0.001 { *value / strength } else { Vec2::ZERO };
            pixel[0] = ((direction.x * 0.5 + 0.5) * 255.0) as u8;
            pixel[1] = ((direction.y * 0.5 + 0.5) * 255.0) as u8;
            pixel[2] = (strength * 255.0) as u8;
            pixel[3] = 255;
        }
    }
}

#[derive(Resource, Default)]
pub struct AtmosphereVisuals {
    scene: Option<usize>,
    profile: Option<AtmosphereProfile>,
    blades: Vec<Blade>,
    grass: Option<Handle<Mesh>>,
    material: Option<Handle<GrassMaterial>>,
    bend_map: Option<Handle<Image>>,
    bend_field: Option<BendField>,
    cover: Option<Handle<Mesh>>,
    cover_material: Option<Handle<StandardMaterial>>,
    firefly_mesh: Option<Handle<Mesh>>,
    firefly_material: Option<Handle<StandardMaterial>>,
    firefly_secondary_material: Option<Handle<StandardMaterial>>,
    arc_mesh: Option<Handle<Mesh>>,
    arc_material: Option<Handle<StandardMaterial>>,
    entities: Vec<Entity>,
}

#[derive(Component)]
pub(crate) struct Firefly {
    home: Vec3,
    phase: f32,
    speed: f32,
    trail: u8,
}
#[derive(Component)]
pub(crate) struct ArcSegment(usize);
#[derive(Component)]
pub(crate) struct ArcFlash;

fn hash(mut n: u32) -> f32 {
    n ^= n >> 16;
    n = n.wrapping_mul(0x7feb_352d);
    n ^= n >> 15;
    n = n.wrapping_mul(0x846c_a68b);
    n ^= n >> 16;
    n as f32 / u32::MAX as f32
}

fn in_clearing(profile: &AtmosphereProfile, x: f32, z: f32, margin: f32) -> bool {
    profile.grass_clearings.iter().any(|r| {
        ((x - r[0]) / (r[2] + margin)).powi(2)
            + ((z - r[1]) / (r[3] + margin)).powi(2) < 1.0
    })
}

fn blades(profile: &AtmosphereProfile) -> Vec<Blade> {
    let mut result = Vec::new();
    let cells = (profile.grass_half_extent * 2.0 / profile.grass_spacing).ceil() as u32;
    for z in 0..cells {
        for x in 0..cells {
            let id = z * cells + x;
            if hash(id.wrapping_mul(19)) < 0.05 { continue; }
            let px = -profile.grass_half_extent + x as f32 * profile.grass_spacing
                + (hash(id * 7 + 1) - 0.5) * profile.grass_spacing * 0.6;
            let pz = -profile.grass_half_extent + z as f32 * profile.grass_spacing
                + (hash(id * 7 + 2) - 0.5) * profile.grass_spacing * 0.6;
            if in_clearing(profile, px, pz, 0.0) { continue; }
            result.push(Blade {
                root: Vec3::new(px, profile.grass_root_y, pz),
                height: profile.grass_height[0] + hash(id * 7 + 3) * (profile.grass_height[1] - profile.grass_height[0]),
                width: profile.grass_width[0] + hash(id * 7 + 4) * (profile.grass_width[1] - profile.grass_width[0]),
                phase: hash(id * 7 + 5) * std::f32::consts::TAU,
                tint: hash(id * 7 + 6),
            });
        }
    }
    result
}

fn grass_positions(blades: &[Blade]) -> Vec<[f32; 3]> {
    let mut positions = Vec::with_capacity(blades.len() * 8);
    for blade in blades {
        let tip = blade.root + Vec3::Y * blade.height;
        for direction in [Vec3::X, Vec3::Z] {
            let left = blade.root - direction * blade.width;
            let right = blade.root + direction * blade.width;
            let top_left = tip - direction * blade.width * 0.22;
            let top_right = tip + direction * blade.width * 0.22;
            positions.extend([left.to_array(), right.to_array(), top_left.to_array(), top_right.to_array()]);
        }
    }
    positions
}

fn grass_geometry(blades: &[Blade], profile: &AtmosphereProfile) -> (Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<[f32; 2]>, Vec<u32>) {
    let positions = grass_positions(blades);
    let mut phases = Vec::with_capacity(blades.len() * 8);
    let mut colors = Vec::with_capacity(blades.len() * 8);
    let mut indices = Vec::with_capacity(blades.len() * 12);
    for (i, blade) in blades.iter().enumerate() {
        let tint = 0.58 + blade.tint * 0.72;
        let base_color = [profile.grass_base[0] * tint, profile.grass_base[1] * tint, profile.grass_base[2] * tint, 1.0];
        let tip_color = [profile.grass_tip[0] * tint, profile.grass_tip[1] * tint, profile.grass_tip[2] * tint, 1.0];
        for side in 0..2 {
            let first = (i * 8 + side * 4) as u32;
            phases.extend([[0.0, blade.phase], [0.0, blade.phase], [1.0, blade.phase], [1.0, blade.phase]]);
            colors.extend([base_color, base_color, tip_color, tip_color]);
            indices.extend([first, first + 2, first + 1, first + 1, first + 2, first + 3]);
        }
    }
    (positions, colors, phases, indices)
}

fn ground_cover(profile: &AtmosphereProfile) -> Mesh {
    let cells = (profile.grass_half_extent * 2.0 / 0.34).ceil() as u32;
    let step = profile.grass_half_extent * 2.0 / cells as f32;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    let height = profile.grass_root_y - 0.014;
    for z in 0..cells {
        for x in 0..cells {
            let px = -profile.grass_half_extent + x as f32 * step;
            let pz = -profile.grass_half_extent + z as f32 * step;
            let center = Vec2::new(px + step * 0.5, pz + step * 0.5);
            if in_clearing(profile, center.x, center.y, step * 0.35) { continue; }
            let first = positions.len() as u32;
            for (dx, dz) in [(0.0, 0.0), (step, 0.0), (0.0, step), (step, step)] {
                let vx = px + dx;
                let vz = pz + dz;
                let wave = (vx * 0.47).sin() * (vz * 0.39).cos();
                let variation = 0.93 + wave * 0.07 + hash((vx.abs() * 71.0 + vz.abs() * 37.0) as u32) * 0.04;
                positions.push([vx, height, vz]);
                normals.push([0.0, 1.0, 0.0]);
                colors.push([
                    (profile.grass_base[0] * 0.55 + profile.grass_tip[0] * 0.45) * variation,
                    (profile.grass_base[1] * 0.55 + profile.grass_tip[1] * 0.45) * variation,
                    (profile.grass_base[2] * 0.55 + profile.grass_tip[2] * 0.45) * variation, 1.0,
                ]);
                uvs.push([vx * 0.1, vz * 0.1]);
            }
            indices.extend([first, first + 2, first + 1, first + 1, first + 2, first + 3]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

pub fn sync(world: &mut World) {
    let current = world.resource::<SceneLibrary>().current;
    let studio = world.get_resource::<crate::viewer::Viewer>().is_some_and(|viewer| viewer.active);
    let profile = (!studio).then(|| world.resource::<SceneLibrary>().scenes
        .get(current)
        .and_then(|scene| scene.scene.style.as_ref())
        .and_then(|style| style.atmosphere.clone())).flatten();
    let wanted = profile.as_ref().map(|_| current);
    if world.resource::<AtmosphereVisuals>().scene == wanted { return; }
    let mut state = world.remove_resource::<AtmosphereVisuals>().unwrap_or_default();
    for entity in state.entities.drain(..) { world.despawn(entity); }
    if let Some(mesh) = state.grass.take() { world.resource_mut::<Assets<Mesh>>().remove(&mesh); }
    if let Some(mesh) = state.cover.take() { world.resource_mut::<Assets<Mesh>>().remove(&mesh); }
    if let Some(mesh) = state.firefly_mesh.take() { world.resource_mut::<Assets<Mesh>>().remove(&mesh); }
    if let Some(mesh) = state.arc_mesh.take() { world.resource_mut::<Assets<Mesh>>().remove(&mesh); }
    if let Some(image) = state.bend_map.take() { world.resource_mut::<Assets<Image>>().remove(&image); }
    if let Some(material) = state.material.take() { world.resource_mut::<Assets<GrassMaterial>>().remove(&material); }
    if let Some(material) = state.cover_material.take() { world.resource_mut::<Assets<StandardMaterial>>().remove(&material); }
    if let Some(material) = state.firefly_material.take() { world.resource_mut::<Assets<StandardMaterial>>().remove(&material); }
    if let Some(material) = state.firefly_secondary_material.take() { world.resource_mut::<Assets<StandardMaterial>>().remove(&material); }
    if let Some(material) = state.arc_material.take() { world.resource_mut::<Assets<StandardMaterial>>().remove(&material); }
    state.blades.clear();
    state.bend_field = None;
    state.scene = wanted;
    state.profile = profile;
    if let Some(profile) = state.profile.clone() {
        let cover = world.resource_mut::<Assets<Mesh>>().add(ground_cover(&profile));
        let cover_material = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.97,
            ..default()
        });
        state.entities.push(world.spawn((
            Mesh3d(cover.clone()), MeshMaterial3d(cover_material.clone()),
            Transform::default(), bevy::light::NotShadowCaster,
        )).id());
        state.cover = Some(cover);
        state.cover_material = Some(cover_material);
        state.blades = blades(&profile);
        let (positions, colors, phases, indices) = grass_geometry(&state.blades, &profile);
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; positions.len()])
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, phases)
            .with_inserted_indices(Indices::U32(indices));
        let grass = world.resource_mut::<Assets<Mesh>>().add(mesh);
        let bend_map = world.resource_mut::<Assets<Image>>().add(Image::new_fill(
            Extent3d { width: BEND_MAP_SIZE as u32, height: BEND_MAP_SIZE as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            &[128, 128, 0, 255],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        ));
        let material = world.resource_mut::<Assets<GrassMaterial>>().add(GrassMaterial {
            base: StandardMaterial {
                base_color: Color::srgb(0.70, 0.78, 0.66),
                double_sided: true,
                perceptual_roughness: 0.95,
                ..default()
            },
            extension: GrassExtension {
                wind: Vec4::new(0.0, 0.0, 0.0, profile.grass_half_extent),
                bend_map: bend_map.clone(),
                bend_settings: Vec4::new(profile.grass_bend_radius, 0.0, 0.0, 0.0),
            },
        });
        state.entities.push(world.spawn((Mesh3d(grass.clone()), MeshMaterial3d(material.clone()), Transform::default(), bevy::light::NotShadowCaster)).id());
        state.grass = Some(grass);
        state.material = Some(material);
        state.bend_map = Some(bend_map);
        state.bend_field = Some(BendField::new());
        let sparkle = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(0.11, 0.11, 0.11));
        let firefly_material = |color: [f32; 3]| StandardMaterial {
            base_color: Color::srgb_from_array(color),
            emissive: LinearRgba::rgb(color[0] * 7.0, color[1] * 7.0, color[2] * 7.0),
            unlit: true,
            ..default()
        };
        let glow = world.resource_mut::<Assets<StandardMaterial>>().add(firefly_material(profile.firefly_color));
        let secondary = world.resource_mut::<Assets<StandardMaterial>>().add(firefly_material(profile.firefly_secondary_color));
        for i in 0..profile.fireflies {
            let emitter_index = i as usize % profile.firefly_emitters.len();
            let emitter = profile.firefly_emitters[emitter_index];
            let angle = hash(i * 13 + 1) * std::f32::consts::TAU;
            let radius = hash(i * 13 + 2).sqrt() * emitter[3];
            let home = Vec3::new(
                emitter[0] + radius * angle.cos(),
                emitter[1] + (hash(i * 13 + 3) - 0.5) * 1.1,
                emitter[2] + radius * angle.sin(),
            );
            let color = if emitter_index % 2 == 0 { glow.clone() } else { secondary.clone() };
            for trail in 0..3 {
                let firefly = Firefly {
                    home,
                    phase: hash(i * 13 + 4) * std::f32::consts::TAU,
                    speed: 0.45 + hash(i * 13 + 5) * 0.7,
                    trail,
                };
                let mut entity = world.spawn((
                    Mesh3d(sparkle.clone()), MeshMaterial3d(color.clone()),
                    Transform::from_translation(home), firefly,
                    bevy::light::NotShadowCaster,
                ));
                if trail == 0 && i < profile.firefly_emitters.len() as u32 {
                    entity.insert(PointLight {
                        color: Color::srgb_from_array(if emitter_index % 2 == 0 { profile.firefly_color } else { profile.firefly_secondary_color }),
                        intensity: 2400.0,
                        range: 4.5,
                        radius: 0.18,
                        shadow_maps_enabled: false,
                        ..default()
                    });
                }
                state.entities.push(entity.id());
            }
        }
        state.firefly_mesh = Some(sparkle);
        state.firefly_material = Some(glow);
        state.firefly_secondary_material = Some(secondary);
        if let Some(arc) = &profile.lightning {
            let mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(1.0, 1.0, 1.0));
            let material = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
                base_color: Color::srgb_from_array(arc.color),
                emissive: LinearRgba::rgb(arc.color[0] * 6.0, arc.color[1] * 6.0, arc.color[2] * 6.0),
                unlit: true,
                ..default()
            });
            for index in 0..9 {
                state.entities.push(world.spawn((
                    Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()),
                    Transform::default(), Visibility::Hidden, ArcSegment(index),
                    bevy::light::NotShadowCaster,
                )).id());
            }
            state.entities.push(world.spawn((
                ArcFlash,
                PointLight {
                    color: Color::srgb_from_array(arc.color),
                    intensity: 0.0, range: 7.0, radius: 0.4,
                    shadow_maps_enabled: false, ..default()
                },
                Transform::from_translation(Vec3::from_array(arc.to)),
            )).id());
            state.arc_mesh = Some(mesh);
            state.arc_material = Some(material);
        }
    }
    world.insert_resource(state);
}

pub fn animate(
    time: Res<Time>,
    mut state: ResMut<AtmosphereVisuals>,
    mut grass_materials: ResMut<Assets<GrassMaterial>>,
    mut images: ResMut<Assets<Image>>,
    player: Query<&Transform, (With<Player>, Without<Firefly>)>,
    mut fireflies: Query<(&Firefly, &mut Transform, Option<&mut PointLight>), (Without<Player>, Without<ArcSegment>)>,
    mut segments: Query<(&ArcSegment, &mut Transform, &mut Visibility), (Without<Firefly>, Without<Player>)>,
    mut flashes: Query<&mut PointLight, (With<ArcFlash>, Without<Firefly>)>,
) {
    if state.scene.is_none() { return; }
    let t = time.elapsed_secs();
    let player = player.iter().next().map_or(Vec3::ZERO, |p| p.translation);
    if let Some(mut material) = state.material.as_ref().and_then(|handle| grass_materials.get_mut(handle)) {
        material.extension.wind = Vec4::new(t, player.x, player.z, state.profile.as_ref().unwrap().grass_half_extent);
    }
    if let Some(profile) = state.profile.as_ref() {
        let extent = profile.grass_half_extent;
        let radius = profile.grass_bend_radius;
        let recovery = profile.grass_recovery_seconds;
        let map = state.bend_map.clone();
        if let Some(field) = state.bend_field.as_mut() {
            field.advance(player.xz(), time.delta_secs(), extent, radius, recovery);
            if let Some(mut image) = map.as_ref().and_then(|handle| images.get_mut(handle)) {
                field.write_image(&mut image);
            }
        }
    }
    for (firefly, mut transform, light) in &mut fireflies {
        let a = (t - firefly.trail as f32 * 0.14) * firefly.speed + firefly.phase;
        transform.translation = firefly.home + Vec3::new(a.sin() * 0.42, (a * 1.6).sin() * 0.22, (a * 0.83).cos() * 0.37);
        let pulse = (a * 2.4).sin() * 0.5 + 0.5;
        transform.scale = Vec3::splat((0.7 + pulse * 0.9) * match firefly.trail { 0 => 1.0, 1 => 0.48, _ => 0.22 });
        if let Some(mut light) = light { light.intensity = 950.0 + pulse * 1800.0; }
    }
    if let Some(arc) = state.profile.as_ref().and_then(|p| p.lightning.as_ref()) {
        let phase = t.rem_euclid(arc.period);
        let active = phase < 0.12 || (0.20..0.29).contains(&phase);
        let from = Vec3::from_array(arc.from);
        let to = Vec3::from_array(arc.to);
        let mut points = [Vec3::ZERO; 10];
        for (i, point) in points.iter_mut().enumerate() {
            let f = i as f32 / 9.0;
            let jitter = if i == 0 || i == 9 { 0.0 } else {
                ((i as f32 * 19.7 + (t * 24.0).floor() * 7.3).sin() * 0.5
                    + (i as f32 * 4.1).cos() * 0.5) * 0.22
            };
            *point = from.lerp(to, f) + Vec3::new(jitter, 0.0, jitter * 0.35);
        }
        for (segment, mut transform, mut visibility) in &mut segments {
            *visibility = if active { Visibility::Inherited } else { Visibility::Hidden };
            if !active { continue; }
            let start = points[segment.0];
            let end = points[segment.0 + 1];
            let direction = end - start;
            transform.translation = (start + end) * 0.5;
            transform.rotation = Quat::from_rotation_arc(Vec3::Y, direction.normalize());
            transform.scale = Vec3::new(0.095, direction.length(), 0.095);
        }
        for mut light in &mut flashes {
            light.intensity = if active { 26000.0 } else { 0.0 };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atmosphere_assets_and_particles_leave_with_the_scene() {
        let mut world = World::new();
        let mut scenes = SceneLibrary::new();
        scenes.current = scenes.index("moonlit-glade").unwrap();
        world.insert_resource(scenes);
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<GrassMaterial>>();
        world.init_resource::<AtmosphereVisuals>();
        sync(&mut world);
        assert!(world.resource::<AtmosphereVisuals>().blades.len() > 15_000);
        let mut fireflies = world.query::<&Firefly>();
        assert_eq!(fireflies.iter(&world).count(), 54 * 3);
        assert!(world.resource::<Assets<Mesh>>().len() >= 3);
        world.resource_mut::<SceneLibrary>().current = 0;
        sync(&mut world);
        assert_eq!(fireflies.iter(&world).count(), 0);
        assert_eq!(world.resource::<Assets<Mesh>>().len(), 0);
        assert_eq!(world.resource::<Assets<Image>>().len(), 0);
        assert_eq!(world.resource::<Assets<StandardMaterial>>().len(), 0);
        assert_eq!(world.resource::<Assets<GrassMaterial>>().len(), 0);
    }

    #[test]
    fn swept_bend_stays_visible_then_recovers() {
        let mut field = BendField::new();
        field.advance(Vec2::new(-0.7, 0.0), 0.016, 11.4, 0.42, 90.0);
        field.advance(Vec2::new(0.7, 0.0), 0.016, 11.4, 0.42, 90.0);
        let center = BEND_MAP_SIZE / 2 * BEND_MAP_SIZE + BEND_MAP_SIZE / 2;
        let initial = field.values[center].length();
        assert!(initial > 0.5, "the swept path should bend grass between positions");
        field.advance(Vec2::new(0.7, 0.0), 1.0, 11.4, 0.42, 90.0);
        assert!(field.values[center].length() > initial * 0.99);
        for _ in 0..360 { field.advance(Vec2::new(0.7, 0.0), 0.25, 11.4, 0.42, 90.0); }
        assert!(field.values[center].length() < initial * 0.4);
    }
}
