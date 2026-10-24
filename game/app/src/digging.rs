//! Digging scenes: the ground is drawn from the simulation's terrain, in
//! chunks of 16 × 16 tiles: terraces, pits, the layers their walls cut
//! through, and the ore in them. A chunk is rebuilt when a pit in or next to
//! it changes. The camera follows the player over the ground; a bar over
//! each digging avatar fills over the dig, with a chip of dust now and then
//! and a burst when a layer comes out.

use std::collections::{BTreeSet, HashMap};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use voxel_assets::{ImpactResponse, SceneFeature};

use fx::FxVec2;
use sim::mine::STEP;
use sim::settlement::avatar::Dig;
use sim::terrain::{Terrain, LAYER, MAX_LAYERS};

use crate::scene_view::SceneView;
use crate::effects::{CameraRig, FxAssets};
use crate::gameplay::Player;
use crate::lockstep::{SessionService, WorldMirror};
use service::{AppServices, Phase, Service, WorldLayer, WorldScoped, WorldState};

/// Tiles per chunk side.
const CHUNK: i32 = 16;
/// The arena's outer walls go down to here.
const BOTTOM: f32 = -3.0;
/// Dust flies off every this many ticks of a dig.
const CHIP_TICKS: u64 = content::millis_to_ticks(500);
/// The bar's offset above the head, in avatar heights (before style scale).
const ABOVE: f32 = 2.15;

/// A chunk of drawn ground, by its index from the arena's corner.
#[derive(Component)]
pub struct GroundChunk(IVec2);

#[derive(Component)]
pub struct DigBar {
    agent: u16,
}

#[derive(Component)]
pub struct DigFill;

/// The simulation's tile at the arena's center.
fn center_tile(center: FxVec2) -> IVec2 {
    IVec2::new(center.x.round().floor_to_int() as i32, center.y.round().floor_to_int() as i32)
}

/// Arena tile of a simulation tile.
fn arena_tile(center: FxVec2, tile: (i32, i32)) -> IVec2 {
    IVec2::new(tile.0, tile.1) - center_tile(center)
}

/// The dug ground of a digging scene (World layer): built when the world
/// starts, its chunks rebuilt as pits change.
#[derive(Resource)]
pub struct Digging {
    /// The pits the drawn ground shows.
    pits: Vec<((i32, i32), u8)>,
}

impl Service for Digging {
    type Layer = WorldLayer;
    type Deps = (WorldMirror, SceneView);

    fn init(world: &mut World) -> Self {
        if !world.resource::<SceneView>().has_feature(SceneFeature::Digging) {
            return Digging { pits: Vec::new() };
        }
        let center = center_tile(world.resource::<WorldMirror>().center);
        let terrain = world.resource::<SessionService>().world().map(|w| w.terrain().clone());
        let Some(terrain) = terrain else { return Digging { pits: Vec::new() } };
        let pits = terrain.pits.clone();
        world.run_system_cached_with(build_ground, (terrain, center)).expect("building the dug ground");
        Digging { pits }
    }

    fn systems(app: &mut App) {
        app.add_service_systems::<Self, _>(Phase::Mirror, (update, follow_camera).chain().run_if(in_state(WorldState::Active)));
    }
}

/// The ground's material, made once.
#[derive(Resource)]
struct GroundMaterial(Handle<StandardMaterial>);

fn build_ground(
    In((terrain, center)): In<(Terrain, IVec2)>,
    mut commands: Commands,
    ground: Option<Res<GroundMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let material = match ground {
        Some(ground) => ground.0.clone(),
        None => {
            let material = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.93, ..default() });
            commands.insert_resource(GroundMaterial(material.clone()));
            material
        }
    };
    let extent = terrain.extent.round().floor_to_int() as i32;
    let count = (2 * extent + 1 + CHUNK - 1) / CHUNK;
    for y in 0..count {
        for x in 0..count {
            let key = IVec2::new(x, y);
            let mesh = meshes.add(chunk_mesh(&terrain, center, extent, key));
            commands.spawn((GroundChunk(key), WorldScoped::default(), Mesh3d(mesh), MeshMaterial3d(material.clone()), Transform::default()));
        }
    }
}

/// Rebuilds the chunks whose pits changed.
pub fn update(
    mut commands: Commands,
    service: Res<SessionService>,
    mirror: Res<WorldMirror>,
    mut digging: ResMut<Digging>,
    mut meshes: ResMut<Assets<Mesh>>,
    chunks: Query<(Entity, &GroundChunk)>,
) {
    let Some(terrain) = service.world().map(sim::World::terrain) else { return };
    if chunks.is_empty() || digging.pits == terrain.pits {
        return;
    }
    let center = center_tile(mirror.center);
    let extent = terrain.extent.round().floor_to_int() as i32;
    // A pit changes its own tile's top and its neighbors' walls.
    let mut dirty = BTreeSet::new();
    let changed = digging.pits.iter().chain(&terrain.pits).filter(|p| !(digging.pits.contains(p) && terrain.pits.contains(p)));
    for &(tile, _) in changed {
        let a = arena_tile(mirror.center, tile);
        for offset in [IVec2::ZERO, IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            dirty.insert(((a + offset + IVec2::splat(extent)).div_euclid(IVec2::splat(CHUNK))).to_array());
        }
    }
    digging.pits = terrain.pits.clone();
    for (entity, chunk) in &chunks {
        if dirty.contains(&chunk.0.to_array()) {
            let mesh = meshes.add(chunk_mesh(terrain, center, extent, chunk.0));
            commands.entity(entity).insert(Mesh3d(mesh));
        }
    }
}

/// The ground of one chunk: a top for every tile, and walls down to every
/// lower neighbor, banded by terrace step and colored by the layer (or ore)
/// they cut through.
fn chunk_mesh(terrain: &Terrain, center: IVec2, extent: i32, key: IVec2) -> Mesh {
    let mine = terrain.mine();
    let tile = |a: IVec2| ((a + center).x, (a + center).y);
    let inside = |a: IVec2| a.x.abs() <= extent && a.y.abs() <= extent;
    let ground = |a: IVec2| if inside(a) { terrain.height(Terrain::pit_center(tile(a))).to_f32() } else { BOTTOM };
    let ore = |a: IVec2, layer: u8| mine.and_then(|m| m.ore(tile(a), layer));
    let mut mesh = MeshBuilder::default();
    let first = key * CHUNK - IVec2::splat(extent);
    for y in first.y..(first.y + CHUNK).min(extent + 1) {
        for x in first.x..(first.x + CHUNK).min(extent + 1) {
            let a = IVec2::new(x, y);
            let top = ground(a);
            let steps = mine.and_then(|m| m.steps(tile(a))).unwrap_or(0);
            let base = steps as f32 * STEP.to_f32();
            let dug = terrain.pit(tile(a));
            let shade = if (x + y) & 1 == 0 { 1.0 } else { 0.94 };
            let color = if dug == 0 {
                mix(surface(steps), ore(a, 0), 0.3)
            } else {
                mix(layer(dug), ore(a, dug), 0.65)
            };
            let (cx, cz) = (x as f32, y as f32);
            mesh.quad(
                [Vec3::new(cx - 0.5, top, cz - 0.5), Vec3::new(cx + 0.5, top, cz - 0.5), Vec3::new(cx + 0.5, top, cz + 0.5), Vec3::new(cx - 0.5, top, cz + 0.5)],
                Vec3::Y,
                color * shade,
            );
            for side in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let below = ground(a + side);
                if below >= top - 0.001 {
                    continue;
                }
                let normal = Vec3::new(side.x as f32, 0.0, side.y as f32);
                let edge = Vec3::new(cx, 0.0, cz) + normal * 0.5;
                let along = Vec3::new(-normal.z, 0.0, normal.x) * 0.5;
                let mut wall = |from: f32, to: f32, color: Vec3| {
                    mesh.quad(
                        [edge - along + Vec3::Y * from, edge + along + Vec3::Y * from, edge + along + Vec3::Y * to, edge - along + Vec3::Y * to],
                        normal,
                        color,
                    );
                };
                // Quarter-meter bands through the diggable layers, then rock.
                let deepest = base - LAYER.to_f32() * MAX_LAYERS as f32;
                let mut upper = top;
                while upper > below + 0.001 && upper > deepest + 0.001 {
                    let band = ((base - upper) / STEP.to_f32()).round() as i32;
                    let lower = (upper - STEP.to_f32()).max(below);
                    let k = (band / 2) as u8;
                    let tone = if band % 2 == 0 { 1.0 } else { 0.9 };
                    wall(lower, upper, mix(layer(k), ore(a, k), 0.7) * tone);
                    upper = lower;
                }
                if upper > below + 0.001 {
                    wall(below, upper, layer(MAX_LAYERS));
                }
            }
        }
    }
    mesh.build()
}

/// Ground color by terrace height: dusty low ground, redder high up.
fn surface(steps: u8) -> Vec3 {
    let t = (steps as f32 / 12.0).min(1.0);
    Vec3::new(0.23, 0.18, 0.15).lerp(Vec3::new(0.38, 0.26, 0.19), t)
}

/// Earth by layer, darker deeper; below the diggable layers, rock.
fn layer(k: u8) -> Vec3 {
    if k >= MAX_LAYERS {
        Vec3::new(0.15, 0.14, 0.16)
    } else {
        Vec3::new(0.24, 0.17, 0.13) * (1.0 - 0.17 * k as f32)
    }
}

/// `color` tinted toward a mineral's color.
fn mix(color: Vec3, ore: Option<u8>, amount: f32) -> Vec3 {
    match ore {
        Some(m) => color.lerp(Vec3::from_array(crate::piles::mineral_color(crate::piles::mineral(m))), amount),
        None => color,
    }
}

#[derive(Default)]
struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    /// A flat quad facing `normal`, in sRGB `color`.
    fn quad(&mut self, mut corners: [Vec3; 4], normal: Vec3, color: Vec3) {
        if (corners[1] - corners[0]).cross(corners[2] - corners[0]).dot(normal) < 0.0 {
            corners.reverse();
        }
        let first = self.positions.len() as u32;
        let linear = Color::srgb(color.x, color.y, color.z).to_linear().to_f32_array();
        for corner in corners {
            self.positions.push(corner.to_array());
            self.normals.push(normal.to_array());
            self.colors.push(linear);
        }
        self.indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }

    fn build(self) -> Mesh {
        let uvs = vec![[0.0, 0.0]; self.positions.len()];
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

/// In digging scenes the camera follows the player over the big ground;
/// elsewhere it stays where the scene put it.
pub fn follow_camera(
    time: Res<Time>,
    scene: Res<SceneView>,
    player: Single<&Transform, With<Player>>,
    mut rig: Single<&mut CameraRig>,
    mut home: Local<Option<(Vec3, Vec3)>>,
) {
    let (base, target) = *home.get_or_insert((rig.base, rig.target));
    if !scene.has_feature(SceneFeature::Digging) {
        if rig.base != base || rig.target != target {
            rig.base = base;
            rig.target = target;
        }
        return;
    }
    let focus = player.translation;
    let blend = 1.0 - (-8.0 * time.delta_secs()).exp();
    rig.base = rig.base.lerp(base + focus, blend);
    rig.target = rig.target.lerp(target + focus, blend);
}

/// Where a ray (from the camera, through the cursor) meets the ground.
pub fn ground_hit(ray: Ray3d, service: &SessionService) -> Option<Vec3> {
    let terrain = service.world()?.terrain();
    let height = |p: Vec3| terrain.height(service.to_sim(p)).to_f32();
    let plane = InfinitePlane3d::new(Vec3::Y);
    let (start, end) = (ray.intersect_plane(Vec3::Y * 6.0, plane)?, ray.intersect_plane(Vec3::Y * BOTTOM, plane)?);
    let steps = ((end - start) / 0.05).ceil().max(1.0) as usize;
    let mut previous = start;
    for i in 1..=steps {
        let t = start + (end - start) * i as f32 / steps as f32;
        let p = ray.get_point(t);
        if p.y <= height(p) {
            let (mut above, mut below) = (previous, t);
            for _ in 0..12 {
                let middle = (above + below) / 2.0;
                let q = ray.get_point(middle);
                if q.y <= height(q) {
                    below = middle;
                } else {
                    above = middle;
                }
            }
            let p = ray.get_point(below);
            return Some(p.with_y(height(p)));
        }
        previous = t;
    }
    None
}

/// Progress bars over digging avatars, dust while they dig, and a burst
/// when a layer comes out.
pub fn feedback(
    mut commands: Commands,
    service: Res<SessionService>,
    mirror: Res<WorldMirror>,
    assets: Res<FxAssets>,
    sound: Res<crate::sound::Sound>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    player: Single<&GlobalTransform, With<Player>>,
    mut bars: Query<(Entity, &DigBar, &mut Node, &mut Visibility, &ComputedNode), Without<DigFill>>,
    mut fills: Query<(&ChildOf, &mut Node), With<DigFill>>,
    mut chips: Local<HashMap<u16, u64>>,
    mut pits: Local<Vec<((i32, i32), u8)>>,
) {
    let terrain = service.world().map(sim::World::terrain);
    let now = mirror.tick as f32 - 1.0 + mirror.alpha;
    let digs: Vec<(u16, Dig, Vec3)> = mirror.current.as_ref().map_or_else(Vec::new, |s| {
        s.agents
            .iter()
            .enumerate()
            .filter_map(|(i, a)| Some((i as u16, a.avatar.dig.clone()?, mirror.to_view(a.position).with_y(a.avatar.height.to_f32()))))
            .collect()
    });
    let scale = player.compute_transform().scale.y;
    for &(agent, ref dig, at) in &digs {
        let chip = (now.max(dig.started as f32) as u64 - dig.started) / CHIP_TICKS;
        if chips.insert(agent, chip) != Some(chip) && chip > 0 {
            burst(&mut commands, &assets, at, agent as u32 * 31 + chip as u32);
            sound.play(crate::sound::Cue::Step);
        }
    }
    chips.retain(|agent, _| digs.iter().any(|(a, ..)| a == agent));
    // A layer came out: a burst from the new floor.
    if let Some(terrain) = terrain {
        if terrain.pits != *pits {
            for &(tile, depth) in &terrain.pits {
                if pits.iter().find(|(t, _)| *t == tile).is_none_or(|&(_, d)| d < depth) {
                    let center = arena_tile(mirror.center, tile).as_vec2();
                    let floor = terrain.height(Terrain::pit_center(tile)).to_f32();
                    burst(&mut commands, &assets, Vec3::new(center.x, floor, center.y), depth as u32 * 977 + tile.0 as u32);
                    sound.play(crate::sound::Cue::Stone);
                }
            }
            *pits = terrain.pits.clone();
        }
    }
    let progress = |dig: &Dig| ((now - dig.started as f32) / (dig.until - dig.started) as f32).clamp(0.0, 1.0);
    let lift = if service.networked() { 24.0 } else { 0.0 };
    let mut shown = Vec::new();
    for (entity, bar, mut node, mut visibility, computed) in &mut bars {
        let Some((_, dig, at)) = digs.iter().find(|(a, ..)| *a == bar.agent) else {
            commands.entity(entity).despawn();
            continue;
        };
        shown.push(bar.agent);
        for (parent, mut fill) in &mut fills {
            if parent.parent() == entity {
                fill.width = percent(progress(dig) * 100.0);
            }
        }
        let head = *at + Vec3::Y * ABOVE * scale;
        let Ok(v) = camera.0.world_to_viewport(camera.1, head) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let size = computed.size() * computed.inverse_scale_factor();
        node.left = px(v.x - size.x / 2.0);
        node.top = px(v.y - size.y - lift);
        *visibility = if size.x > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
    }
    for (agent, ..) in digs.iter().filter(|(a, ..)| !shown.contains(a)) {
        commands
            .spawn((
                DigBar { agent: *agent },
                Node { position_type: PositionType::Absolute, width: px(46.), height: px(7.), padding: UiRect::all(px(1.5)), ..default() },
                BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.75)),
                Visibility::Hidden,
            ))
            .with_child((DigFill, Node { width: percent(0.), height: percent(100.), ..default() }, BackgroundColor(Color::srgb(0.93, 0.63, 0.20))));
    }
}

/// Dust and grit flying up from the ground at `at`.
fn burst(commands: &mut Commands, assets: &FxAssets, at: Vec3, seed: u32) {
    crate::effects::impact(commands, assets, at + Vec3::Y * 0.05, Vec3::Y, seed, ImpactResponse::Stone);
}

/// For the control snapshot: dug pits by arena tile, whether the player is
/// digging, what the collector holds by mineral, and the nearest tile
/// (within 8 m of the player) whose next layer down holds ore.
pub fn snapshot(world: &World) -> serde_json::Value {
    let (Some(service), Some(mirror)) = (world.get_resource::<SessionService>(), world.get_resource::<WorldMirror>()) else {
        return serde_json::Value::Null;
    };
    let Some(terrain) = service.world().map(sim::World::terrain) else { return serde_json::Value::Null };
    let pits: Vec<_> = terrain
        .pits
        .iter()
        .map(|&(tile, depth)| {
            let t = arena_tile(mirror.center, tile);
            serde_json::json!({"tile": [t.x, t.y], "depth": depth})
        })
        .collect();
    let digging = mirror.player().zip(mirror.current.as_ref()).is_some_and(|(i, s)| s.agents[i as usize].avatar.dig.is_some());
    let collected = mirror.current.as_ref().and_then(|s| s.piles.as_ref()?.collector.as_ref()).map(|c| c.stored.clone());
    let me = mirror.player().zip(mirror.current.as_ref()).map(|(i, s)| Terrain::pit_tile(s.agents[i as usize].position));
    let nearest_ore = me.zip(terrain.mine()).and_then(|(me, mine)| {
        (-8..=8)
            .flat_map(|y| (-8..=8).map(move |x| (me.0 + x, me.1 + y)))
            .filter(|&t| terrain.pit(t) < MAX_LAYERS && mine.ore(t, terrain.pit(t)).is_some())
            .min_by_key(|&t| ((t.0 - me.0).pow(2) + (t.1 - me.1).pow(2), t))
            .map(|t| {
                let a = arena_tile(mirror.center, t);
                serde_json::json!({"tile": [a.x, a.y], "mineral": mine.ore(t, terrain.pit(t))})
            })
    });
    serde_json::json!({"pits": pits, "digging": digging, "collected": collected, "nearest_ore": nearest_ore})
}
