//! Digging scenes: the ground is drawn from the simulation's terrain, in
//! chunks of 16 × 16 tiles: terraces, pits, the layers their walls cut
//! through, and the ore in them. A chunk is rebuilt when a pit in or next to
//! it changes. The camera follows the player over the ground; a bar over
//! each digging avatar or miner sprite fills over the dig, with a chip of
//! dust now and then and a burst when a layer comes out.
//!
//! Sloped scenes (the Sacred Tree) draw the simulation's sloped ground: a
//! tile's top from its corner heights (`Terrain::patch`), so pit edges,
//! ramps and terraces slope, and a steep tile (`Terrain::steep`) flat at
//! its own height with walls down to its lower neighbours, so the hill's
//! sides and deep pits read as cliffs. The Dig Lab keeps its flat tiles and
//! walls.

use std::collections::{BTreeSet, HashMap};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use voxel_assets::{ImpactResponse, SceneFeature};

use fx::FxVec2;
use sim::mine::{Mine, Veins, STEP, STONE};
use sim::settlement::avatar::Dig;
use sim::terrain::{DigRefusal, Terrain, LAYER, MAX_LAYERS, MAX_SLOPE};

use crate::scene_view::SceneView;
use crate::effects::{CameraRig, FxAssets};
use crate::character::Player;
use crate::lockstep::{SessionService, WorldMirror};
use service::{AppServices, Phase, Service, WorldLayer, WorldScoped, WorldState};

/// Tiles per chunk side.
pub(crate) const CHUNK: i32 = 16;
/// The arena's outer walls go down to here.
const BOTTOM: f32 = -3.0;
/// Dust flies off every this many ticks of a dig.
const CHIP_TICKS: u64 = content::millis_to_ticks(500);
/// The bar's offset above the head, in avatar heights (before style scale).
const ABOVE: f32 = 2.15;
/// A sprite's bar, in meters over its feet: over its flame, under its tag.
const SPRITE_ABOVE: f32 = 1.18;

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
#[derive(Resource, Default)]
pub struct Digging {
    /// The pits the drawn ground shows.
    pits: Vec<((i32, i32), u8)>,
    /// In a volume scene, the volume the drawn ground shows, and the
    /// terrain revision it was read at (a volume dig bumps it).
    volume: Option<sim::volume::Volume>,
    revision: u64,
    /// Where cells came out since `feedback` last looked: a burst each.
    cleared: Vec<Vec3>,
}

impl Service for Digging {
    type Layer = WorldLayer;
    type Deps = (WorldMirror, SceneView, crate::character::Avatar);

    fn init(world: &mut World) -> Self {
        if !world.resource::<SceneView>().has_feature(SceneFeature::Digging) {
            return Digging::default();
        }
        let center = center_tile(world.resource::<WorldMirror>().center);
        let terrain = world.resource::<SessionService>().world().map(|w| w.terrain().clone());
        let Some(terrain) = terrain else { return Digging::default() };
        let digging = Digging { pits: terrain.pits.clone(), volume: terrain.volume.clone(), revision: terrain.revision, cleared: Vec::new() };
        world.run_system_cached_with(build_ground, (terrain, center)).expect("building the dug ground");
        digging
    }

    fn systems(app: &mut App) {
        let active = in_state(WorldState::Active);
        app.init_resource::<crate::prediction::DrawnAvatar>()
            .add_service_systems::<Self, _>(Phase::Mirror, (update, follow_camera).chain().run_if(active.clone()))
            .add_systems(
                PostUpdate,
                (feedback, toss_stone, fly_stones).after(bevy::transform::TransformSystems::Propagate).run_if(crate::control::rendered).run_if(active),
            );
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
    let started = std::time::Instant::now();
    for y in 0..count {
        for x in 0..count {
            let key = IVec2::new(x, y);
            let mesh = meshes.add(ground_mesh(&terrain, center, extent, key));
            commands.spawn((GroundChunk(key), WorldScoped::default(), Mesh3d(mesh), MeshMaterial3d(material.clone()), Transform::default()));
        }
    }
    info!("Built the dug ground: {} chunks in {:.1} ms", count * count, started.elapsed().as_secs_f64() * 1e3);
}

/// A chunk's ground: a volume scene's cells (`volume_mesh`), sloped in
/// sloped scenes, the Dig Lab's flat tiles and walls elsewhere.
fn ground_mesh(terrain: &Terrain, center: IVec2, extent: i32, key: IVec2) -> Mesh {
    if let Some(volume) = &terrain.volume {
        crate::volume_mesh::chunk_mesh(volume, center, extent, key)
    } else if terrain.sloped {
        sloped_chunk_mesh(terrain, center, extent, key)
    } else {
        chunk_mesh(terrain, center, extent, key)
    }
}

/// The arena tiles whose drawing a pit changing at arena tile `at` changes.
/// On flat ground, its own top and its four neighbours' walls down to it.
/// On sloped ground its four corners move, which reshapes the 3 × 3 tiles
/// touching them (and can make them steep or gentle), and their walls
/// reach the tiles beside them: everything within two.
fn redrawn(at: IVec2, sloped: bool) -> Vec<IVec2> {
    if sloped {
        (-2..=2).flat_map(|y| (-2..=2).map(move |x| at + IVec2::new(x, y))).collect()
    } else {
        [IVec2::ZERO, IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y].into_iter().map(|offset| at + offset).collect()
    }
}

/// The chunks to rebuild for pits that changed at these arena tiles, in an
/// arena of `extent` tiles round its center.
pub(crate) fn dirty_chunks(changed: impl IntoIterator<Item = IVec2>, extent: i32, sloped: bool) -> BTreeSet<[i32; 2]> {
    changed
        .into_iter()
        .flat_map(|at| redrawn(at, sloped))
        .map(|a| (a + IVec2::splat(extent)).div_euclid(IVec2::splat(CHUNK)).to_array())
        .collect()
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
    let center = center_tile(mirror.center);
    let extent = terrain.extent.round().floor_to_int() as i32;
    if let Some(volume) = &terrain.volume {
        // A volume dig bumps the revision; the cells that changed (and the
        // tiles their corner rule reaches) say which chunks to re-mesh.
        if chunks.is_empty() || digging.revision == terrain.revision {
            return;
        }
        digging.revision = terrain.revision;
        let Some(before) = digging.volume.replace(volume.clone()) else { return };
        let changed = crate::volume_mesh::changed_tiles(&before, volume, center, extent);
        if changed.is_empty() {
            return;
        }
        for &a in &changed {
            let tile = ((a + center).x, (a + center).y);
            if let Some(k) = crate::volume_mesh::lowest_cleared(&before, volume, tile) {
                digging.cleared.push(Vec3::new(a.x as f32, sim::volume::height(k).to_f32(), a.y as f32));
            }
        }
        let dirty = dirty_chunks(changed, extent, true);
        let started = std::time::Instant::now();
        let rebuilt = rebuild(&mut commands, &mut meshes, &chunks, &dirty, terrain, center, extent);
        info!("Re-meshed {rebuilt} volume chunks in {:.2} ms", started.elapsed().as_secs_f64() * 1e3);
        return;
    }
    if chunks.is_empty() || digging.pits == terrain.pits {
        return;
    }
    // A pit changes its own tile's top and its neighbors' walls (`redrawn`).
    let changed = digging.pits.iter().chain(&terrain.pits).filter(|p| !(digging.pits.contains(p) && terrain.pits.contains(p)));
    let dirty = dirty_chunks(changed.map(|&(tile, _)| arena_tile(mirror.center, tile)), extent, terrain.sloped);
    digging.pits = terrain.pits.clone();
    let started = std::time::Instant::now();
    let rebuilt = rebuild(&mut commands, &mut meshes, &chunks, &dirty, terrain, center, extent);
    info!("Rebuilt {rebuilt} ground chunks in {:.2} ms", started.elapsed().as_secs_f64() * 1e3);
}

/// Rebuilds the `dirty` chunks' meshes; returns how many.
fn rebuild(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    chunks: &Query<(Entity, &GroundChunk)>,
    dirty: &BTreeSet<[i32; 2]>,
    terrain: &Terrain,
    center: IVec2,
    extent: i32,
) -> usize {
    let mut rebuilt = 0;
    for (entity, chunk) in chunks {
        if dirty.contains(&chunk.0.to_array()) {
            let mesh = meshes.add(ground_mesh(terrain, center, extent, chunk.0));
            commands.entity(entity).insert(Mesh3d(mesh));
            rebuilt += 1;
        }
    }
    rebuilt
}

/// The ground of one chunk: a top for every tile, and walls down to every
/// lower neighbor, banded by terrace step and colored by the layer (or ore)
/// they cut through.
fn chunk_mesh(terrain: &Terrain, center: IVec2, extent: i32, key: IVec2) -> Mesh {
    let mine = terrain.mine();
    let tile = |a: IVec2| ((a + center).x, (a + center).y);
    let inside = |a: IVec2| a.x.abs() <= extent && a.y.abs() <= extent;
    let ground = |a: IVec2| if inside(a) { terrain.height(Terrain::pit_center(tile(a))).to_f32() } else { BOTTOM };
    let ore = |a: IVec2, layer: u8| mine.and_then(|m| Some(tint(m, m.ore(tile(a), layer)?)));
    let mut mesh = MeshBuilder::default();
    let first = key * CHUNK - IVec2::splat(extent);
    for y in first.y..(first.y + CHUNK).min(extent + 1) {
        for x in first.x..(first.x + CHUNK).min(extent + 1) {
            let a = IVec2::new(x, y);
            let top = ground(a);
            let steps = mine.and_then(|m| m.steps(tile(a))).unwrap_or(0);
            // Undug ground, from the mine's datum (a hill's plateau).
            let base = mine.and_then(|m| m.height(tile(a))).map_or(0.0, |h| h.to_f32());
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

/// Tile heights over a square of arena tiles, read from the terrain once:
/// each tile's ground (`Terrain::tile_height`; nothing outside the arena or
/// over a drop) and its ground before digging.
struct Heights {
    min: IVec2,
    size: i32,
    ground: Vec<Option<f32>>,
    undug: Vec<Option<f32>>,
}

impl Heights {
    /// For the arena tiles from `min` to `min + size - 1` on both axes.
    fn read(terrain: &Terrain, center: IVec2, extent: i32, min: IVec2, size: i32) -> Heights {
        let cells = (size * size) as usize;
        let mut heights = Heights { min, size, ground: Vec::with_capacity(cells), undug: Vec::with_capacity(cells) };
        for y in 0..size {
            for x in 0..size {
                let a = min + IVec2::new(x, y);
                let tile = ((a + center).x, (a + center).y);
                let inside = a.x.abs() <= extent && a.y.abs() <= extent;
                let ground = inside.then(|| terrain.tile_height(tile)).flatten().map(|h| h.to_f32());
                heights.ground.push(ground);
                heights.undug.push(ground.map(|h| h + LAYER.to_f32() * terrain.pit(tile) as f32));
            }
        }
        heights
    }

    fn index(&self, a: IVec2) -> Option<usize> {
        let d = a - self.min;
        (d.x >= 0 && d.y >= 0 && d.x < self.size && d.y < self.size).then(|| (d.y * self.size + d.x) as usize)
    }

    fn ground(&self, a: IVec2) -> Option<f32> {
        self.ground[self.index(a)?]
    }

    fn undug(&self, a: IVec2) -> Option<f32> {
        self.undug[self.index(a)?]
    }
}

/// A tile's corner heights, at its -x -y, +x -y, -x +y and +x +y corners:
/// each the lowest of the four tiles touching it, drops aside, as
/// `Terrain::patch` makes them. Nothing for a drop.
fn corners(a: IVec2, height: impl Fn(IVec2) -> Option<f32>) -> Option<[f32; 4]> {
    let h = height(a)?;
    let corner = |dx: i32, dy: i32| {
        [IVec2::new(dx - 1, dy - 1), IVec2::new(dx, dy - 1), IVec2::new(dx - 1, dy), IVec2::new(dx, dy)]
            .into_iter()
            .filter_map(|offset| height(a + offset))
            .fold(h, f32::min)
    };
    Some([corner(0, 0), corner(1, 0), corner(0, 1), corner(1, 1)])
}

/// Whether ground with these corners is steeper than [`MAX_SLOPE`] along a
/// tile edge (`Terrain::steep`).
fn steep(corners: [f32; 4]) -> bool {
    let [c00, c10, c01, c11] = corners;
    [c10 - c00, c11 - c01, c01 - c00, c11 - c10].iter().any(|d| d.abs() > MAX_SLOPE.to_f32() + 0.001)
}

/// The top drawn over a tile, by corner, and whether it is flat for being
/// steep: its sloped ground, or if that is steep, flat at the tile's height,
/// so a cliff or a deep pit's side stands as a wall. A gentle tile's corners
/// are never above a neighbour's height (each is the lowest round it), so
/// walls only fall from steep tiles (and the arena's edge).
fn drawn(a: IVec2, height: impl Fn(IVec2) -> Option<f32>) -> Option<([f32; 4], bool)> {
    let c = corners(a, &height)?;
    if steep(c) {
        Some(([height(a)?; 4], true))
    } else {
        Some((c, false))
    }
}

/// Each side of a tile: its direction, the tile's two corners on it, and
/// the neighbour's same two corners.
pub(crate) const SIDES: [(IVec2, [usize; 2], [usize; 2]); 4] =
    [(IVec2::X, [1, 3], [0, 2]), (IVec2::NEG_X, [0, 2], [1, 3]), (IVec2::Y, [2, 3], [0, 1]), (IVec2::NEG_Y, [0, 1], [2, 3])];

/// Where a tile's corner `i` ([`corners`]' order) is, at height `y`.
pub(crate) fn corner_at(a: IVec2, i: usize, y: f32) -> Vec3 {
    Vec3::new(a.x as f32 - 0.5 + (i & 1) as f32, y, a.y as f32 - 0.5 + (i >> 1) as f32)
}

/// The ground of one chunk in a sloped scene: a top for every tile
/// ([`drawn`]), a gentle one's corners colored by how deep they are dug,
/// and walls down to every lower neighbour, banded by quarter meter
/// through the scene's layers and colored by the layer (or stone) they cut
/// through.
fn sloped_chunk_mesh(terrain: &Terrain, center: IVec2, extent: i32, key: IVec2) -> Mesh {
    let mine = terrain.mine();
    let layers = terrain.max_layers;
    let tile = |a: IVec2| ((a + center).x, (a + center).y);
    let ore = |a: IVec2, layer: u8| mine.and_then(|m| Some(tint(m, m.ore(tile(a), layer)?)));
    let first = key * CHUNK - IVec2::splat(extent);
    // A tile's walls read its neighbours' corners, which read theirs.
    let heights = Heights::read(terrain, center, extent, first - IVec2::splat(2), CHUNK + 4);
    let ground = |a: IVec2| heights.ground(a);
    let (step, layer_height) = (STEP.to_f32(), LAYER.to_f32());
    let mut mesh = MeshBuilder::default();
    for y in first.y..(first.y + CHUNK).min(extent + 1) {
        for x in first.x..(first.x + CHUNK).min(extent + 1) {
            let a = IVec2::new(x, y);
            let (Some((top, flat)), Some(base)) = (drawn(a, ground), heights.undug(a)) else { continue };
            let steps = mine.and_then(|m| m.steps(tile(a))).unwrap_or(0);
            let shade = if (x + y) & 1 == 0 { 1.0 } else { 0.94 };
            let color = |dug: u8| {
                if dug == 0 {
                    mix(surface(steps), ore(a, 0), 0.3)
                } else {
                    mix(layer_of(dug, layers), ore(a, dug), 0.65)
                }
            };
            // A flat top shows the tile's own layer; a slope, at each corner,
            // how far it is dug below the lowest undug ground round it, so
            // terraces' slopes stay surface.
            let colors: [Vec3; 4] = match corners(a, |b| heights.undug(b)) {
                Some(undug) if !flat => std::array::from_fn(|i| color(((undug[i] - top[i]) / layer_height).round().clamp(0.0, layers as f32) as u8) * shade),
                _ => [color(terrain.pit(tile(a))) * shade; 4],
            };
            let p: [Vec3; 4] = std::array::from_fn(|i| corner_at(a, i, top[i]));
            // Split along the diagonal whose ends are closer in height, so a
            // dipped corner folds and the rest stays flat.
            if (top[0] - top[3]).abs() <= (top[1] - top[2]).abs() {
                mesh.triangle([p[0], p[1], p[3]], [colors[0], colors[1], colors[3]]);
                mesh.triangle([p[0], p[3], p[2]], [colors[0], colors[3], colors[2]]);
            } else {
                mesh.triangle([p[0], p[1], p[2]], [colors[0], colors[1], colors[2]]);
                mesh.triangle([p[1], p[3], p[2]], [colors[1], colors[3], colors[2]]);
            }
            let deepest = base - layer_height * layers as f32;
            for (side, [i, j], [ti, tj]) in SIDES {
                // The arena's edge drops below the deepest layer.
                let below = drawn(a + side, ground).map_or([deepest - 1.0; 4], |(below, _)| below);
                let (low_i, low_j) = (below[ti].min(top[i]), below[tj].min(top[j]));
                if top[i] - low_i < 0.001 && top[j] - low_j < 0.001 {
                    continue;
                }
                let wall = [corner_at(a, i, top[i]), corner_at(a, j, top[j]), corner_at(a, j, low_j), corner_at(a, i, low_i)];
                let normal = Vec3::new(side.x as f32, 0.0, side.y as f32);
                // Quarter-meter bands through the diggable layers, then rock.
                for band in 0..2 * layers as i32 {
                    let upper = base - step * band as f32;
                    let k = (band / 2) as u8;
                    let tone = if band % 2 == 0 { 1.0 } else { 0.9 };
                    mesh.polygon(&slab(&wall, upper - step, upper), normal, mix(layer_of(k, layers), ore(a, k), 0.7) * tone);
                }
                mesh.polygon(&slab(&wall, f32::MIN, deepest), normal, layer_of(layers, layers));
            }
        }
    }
    mesh.build()
}

/// The part of a convex polygon between heights `lower` and `upper`.
pub(crate) fn slab(polygon: &[Vec3], lower: f32, upper: f32) -> Vec<Vec3> {
    // Keeps the part where `inside` is at least zero.
    let clip = |points: Vec<Vec3>, inside: &dyn Fn(f32) -> f32| {
        let mut kept = Vec::with_capacity(points.len() + 2);
        for (n, &p) in points.iter().enumerate() {
            let q = points[(n + 1) % points.len()];
            let (dp, dq) = (inside(p.y), inside(q.y));
            if dp >= 0.0 {
                kept.push(p);
            }
            if (dp >= 0.0) != (dq >= 0.0) {
                kept.push(p.lerp(q, dp / (dp - dq)));
            }
        }
        kept
    };
    let under = clip(polygon.to_vec(), &|y| upper - y);
    clip(under, &|y| y - lower)
}

/// Ground color by terrace height: dusty low ground, redder high up.
pub(crate) fn surface(steps: u8) -> Vec3 {
    let t = (steps as f32 / 12.0).min(1.0);
    Vec3::new(0.23, 0.18, 0.15).lerp(Vec3::new(0.38, 0.26, 0.19), t)
}

/// Earth by layer, darker deeper; below the diggable layers, rock.
fn layer(k: u8) -> Vec3 {
    if k >= MAX_LAYERS {
        ROCK
    } else {
        Vec3::new(0.24, 0.17, 0.13) * (1.0 - 0.17 * k as f32)
    }
}

/// Below the diggable layers.
pub(crate) const ROCK: Vec3 = Vec3::new(0.15, 0.14, 0.16);

/// [`layer`] in ground `layers` deep (a sloped scene's `max_layers`): as
/// dark at the bottom however many layers there are.
fn layer_of(k: u8, layers: u8) -> Vec3 {
    if k >= layers {
        ROCK
    } else {
        Vec3::new(0.24, 0.17, 0.13) * (1.0 - 0.51 * k as f32 / layers as f32)
    }
}

/// A town's stone, in the ground and pit walls: a cool grey against the
/// brown earth (the dig lab's minerals have their own colors).
const STONE_TINT: Vec3 = Vec3::new(0.54, 0.55, 0.6);

/// The color a mineral of `mine`'s veins tints the ground: stone grey in a
/// town's mine (`Veins::Stone`), else the dig lab's ore colors.
fn tint(mine: &Mine, mineral: u8) -> Vec3 {
    match mine.spec.veins {
        Veins::Stone if mineral == STONE => STONE_TINT,
        _ => Vec3::from_array(crate::piles::mineral_color(crate::piles::mineral(mineral))),
    }
}

/// `color` tinted toward a mineral's color.
pub(crate) fn mix(color: Vec3, tint: Option<Vec3>, amount: f32) -> Vec3 {
    match tint {
        Some(tint) => color.lerp(tint, amount),
        None => color,
    }
}

/// Builds a ground mesh of vertex-coloured faces. Each vertex's `UV_0.x`
/// is 1 if it was pushed while `cut` was set (a volume's walls and
/// ceilings, `volume_mesh`), else 0.
#[derive(Default)]
pub(crate) struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    /// Marks the faces pushed next as cut faces.
    pub(crate) cut: bool,
}

impl MeshBuilder {
    /// A flat quad facing `normal`, in sRGB `color`.
    pub(crate) fn quad(&mut self, mut corners: [Vec3; 4], normal: Vec3, color: Vec3) {
        if (corners[1] - corners[0]).cross(corners[2] - corners[0]).dot(normal) < 0.0 {
            corners.reverse();
        }
        let first = self.positions.len() as u32;
        let linear = Color::srgb(color.x, color.y, color.z).to_linear().to_f32_array();
        for corner in corners {
            self.positions.push(corner.to_array());
            self.normals.push(normal.to_array());
            self.colors.push(linear);
            self.uvs.push([self.cut as u8 as f32, 0.0]);
        }
        self.indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }

    /// A triangle facing up, its corners in sRGB `colors`, lit flat.
    pub(crate) fn triangle(&mut self, mut corners: [Vec3; 3], mut colors: [Vec3; 3]) {
        let mut normal = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        if normal.y < 0.0 {
            corners.swap(1, 2);
            colors.swap(1, 2);
            normal = -normal;
        }
        let first = self.positions.len() as u32;
        let normal = normal.normalize_or(Vec3::Y);
        for (corner, color) in corners.into_iter().zip(colors) {
            self.positions.push(corner.to_array());
            self.normals.push(normal.to_array());
            self.colors.push(Color::srgb(color.x, color.y, color.z).to_linear().to_f32_array());
            self.uvs.push([self.cut as u8 as f32, 0.0]);
        }
        self.indices.extend([first, first + 1, first + 2]);
    }

    /// A flat convex polygon facing `normal`, in sRGB `color`; nothing if
    /// it has no area.
    pub(crate) fn polygon(&mut self, points: &[Vec3], normal: Vec3, color: Vec3) {
        let mut corners: Vec<Vec3> = Vec::with_capacity(points.len());
        for &p in points {
            if corners.last().is_none_or(|q| q.distance_squared(p) > 1e-8) {
                corners.push(p);
            }
        }
        while corners.len() > 1 && corners[0].distance_squared(corners[corners.len() - 1]) <= 1e-8 {
            corners.pop();
        }
        if corners.len() < 3 {
            return;
        }
        let area = corners.windows(2).skip(1).map(|w| (w[0] - corners[0]).cross(w[1] - corners[0])).sum::<Vec3>();
        if area.length_squared() < 1e-10 {
            return;
        }
        if area.dot(normal) < 0.0 {
            corners.reverse();
        }
        let first = self.positions.len() as u32;
        let linear = Color::srgb(color.x, color.y, color.z).to_linear().to_f32_array();
        for corner in &corners {
            self.positions.push(corner.to_array());
            self.normals.push(normal.to_array());
            self.colors.push(linear);
            self.uvs.push([self.cut as u8 as f32, 0.0]);
        }
        for n in 1..corners.len() as u32 - 1 {
            self.indices.extend([first, first + n, first + n + 1]);
        }
    }

    pub(crate) fn build(self) -> Mesh {
        let uvs = self.uvs;
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

/// Follow the player in scenes that request it, preserving the authored offset.
pub fn follow_camera(
    time: Res<Time>,
    scene: Res<SceneView>,
    player: Single<&Transform, With<Player>>,
    mut rig: Single<&mut CameraRig>,
    mut home: Local<Option<(Vec3, Vec3)>>,
) {
    let (base, target) = *home.get_or_insert((rig.base, rig.target));
    if !scene.has_feature(SceneFeature::Digging) && !scene.has_feature(SceneFeature::CameraFollow) {
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
    if let Some(volume) = &terrain.volume {
        return crate::volume_mesh::ray_hit(volume, ray, |p| service.to_sim(p));
    }
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

/// A dig to show: an avatar or a miner sprite digging, where, and the
/// render time (ticks) its progress is shown at.
#[derive(Clone, Debug, PartialEq)]
pub struct ShownDig {
    pub agent: u16,
    pub dig: Dig,
    pub at: Vec3,
    pub now: f32,
    /// A sprite's: its bar stands lower, over a smaller body.
    pub sprite: bool,
}

/// The digs to show (`AgentState::digging`): each avatar's and miner's in
/// the confirmed world at render time, but the local player's as its avatar
/// is drawn (D5), at `player`, where its body is drawn. On a joined client
/// that one is predicted, so its bar and dust start and stop with the drawn
/// avatar's dig. A sprite stands on `terrain`'s ground.
pub fn shown_digs(mirror: &WorldMirror, drawn: Option<&crate::prediction::DrawnState>, player: Vec3, terrain: Option<&Terrain>) -> Vec<ShownDig> {
    let now = mirror.tick as f32 - 1.0 + mirror.alpha;
    let me = mirror.player();
    let drawn = drawn.filter(|drawn| Some(drawn.agent) == me);
    let others = mirror.current.as_ref().map_or_else(Vec::new, |s| {
        s.agents
            .iter()
            .enumerate()
            .filter(|&(i, _)| Some(i as u16) != me || drawn.is_none())
            .filter_map(|(i, a)| {
                let dig = a.digging()?.clone();
                let height = a.avatar().map_or_else(|| terrain.map_or(0.0, |t| t.height(a.body.position).to_f32()), |avatar| avatar.height.to_f32());
                let at = mirror.to_view(a.body.position).with_y(height);
                Some(ShownDig { agent: i as u16, dig, at, now, sprite: a.avatar().is_none() })
            })
            .collect()
    });
    let mine = drawn.and_then(|drawn| Some(ShownDig { agent: drawn.agent, dig: drawn.avatar().dig.clone()?, at: player, now: drawn.time, sprite: false }));
    others.into_iter().chain(mine).collect()
}

/// Progress bars over digging avatars, dust while they dig, and a burst
/// when a layer comes out.
pub fn feedback(
    mut commands: Commands,
    service: Res<SessionService>,
    mirror: Res<WorldMirror>,
    drawn: Res<crate::prediction::DrawnAvatar>,
    assets: Res<FxAssets>,
    sound: Res<crate::sound::Sound>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    player: Single<&GlobalTransform, With<Player>>,
    mut bars: Query<(Entity, &DigBar, &mut Node, &mut Visibility, &ComputedNode), Without<DigFill>>,
    mut fills: Query<(&ChildOf, &mut Node), With<DigFill>>,
    mut chips: Local<HashMap<u16, u64>>,
    mut pits: Local<Vec<((i32, i32), u8)>>,
    mut digging: ResMut<Digging>,
) {
    // A volume dig came out: a burst from each dug tile's new floor.
    for (n, at) in digging.cleared.drain(..).enumerate() {
        burst(&mut commands, &assets, at, n as u32 * 977 + at.x as i32 as u32);
        sound.play(crate::sound::Cue::Stone);
    }
    let terrain = service.world().map(sim::World::terrain);
    let digs = shown_digs(&mirror, drawn.0.as_ref(), player.translation(), terrain);
    let scale = player.compute_transform().scale.y;
    for ShownDig { agent, dig, at, now, .. } in &digs {
        let (agent, at) = (*agent, *at);
        let chip = (now.max(dig.started as f32) as u64 - dig.started) / CHIP_TICKS;
        if chips.insert(agent, chip) != Some(chip) && chip > 0 {
            burst(&mut commands, &assets, at, agent as u32 * 31 + chip as u32);
            sound.play(crate::sound::Cue::Step);
        }
    }
    chips.retain(|agent, _| digs.iter().any(|d| d.agent == *agent));
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
    let progress = |d: &ShownDig| ((d.now - d.dig.started as f32) / (d.dig.until - d.dig.started) as f32).clamp(0.0, 1.0);
    let lift = if service.networked() { 24.0 } else { 0.0 };
    let mut shown = Vec::new();
    for (entity, bar, mut node, mut visibility, computed) in &mut bars {
        let Some(dig) = digs.iter().find(|d| d.agent == bar.agent) else {
            commands.entity(entity).despawn();
            continue;
        };
        shown.push(bar.agent);
        for (parent, mut fill) in &mut fills {
            if parent.parent() == entity {
                fill.width = percent(progress(dig) * 100.0);
            }
        }
        let head = dig.at + Vec3::Y * if dig.sprite { SPRITE_ABOVE } else { ABOVE * scale };
        let Ok(v) = camera.0.world_to_viewport(camera.1, head) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let size = computed.size() * computed.inverse_scale_factor();
        node.left = px(v.x - size.x / 2.0);
        node.top = px(v.y - size.y - lift);
        *visibility = if size.x > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
    }
    for agent in digs.iter().map(|d| d.agent).filter(|a| !shown.contains(a)) {
        commands
            .spawn((
                DigBar { agent },
                Node { position_type: PositionType::Absolute, width: px(46.), height: px(7.), padding: UiRect::all(px(1.5)), ..default() },
                BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.75)),
                Visibility::Hidden,
            ))
            .with_child((DigFill, Node { width: percent(0.), height: percent(100.), ..default() }, BackgroundColor(Color::srgb(0.93, 0.63, 0.20))));
    }
}

/// A stone flying from a pit onto its pile. A town's dig drops its stone on
/// the pile at once (M6.2); this is only the look of it landing there.
#[derive(Component)]
pub struct TossedStone {
    from: Vec3,
    to: Vec3,
    /// Seconds; it waits in the pit while negative.
    age: f32,
}

/// How long a tossed stone flies.
const TOSS_SECONDS: f32 = 0.45;
/// Stones tossed per stone layer dug out, and the seconds between them.
const TOSSED: usize = 3;
const TOSS_GAP: f32 = 0.09;
/// The farthest a dug layer's pile lies from its pit (`PileLab::mined`
/// drops it beside the pit, or on a pile within 2.2 m).
const PILE_REACH: f32 = 3.5;

/// When a stone layer comes out of a town's mine, a few stones fly from the
/// pit onto the nearest ground pile.
pub fn toss_stone(
    mut commands: Commands,
    service: Res<SessionService>,
    mirror: Res<WorldMirror>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut stone: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    mut pits: Local<Option<Vec<((i32, i32), u8)>>>,
) {
    let Some(terrain) = service.world().map(sim::World::terrain) else { return };
    let Some(mine) = terrain.mine().filter(|m| m.spec.veins == Veins::Stone) else { return };
    // The pits there were when the world started came out before it.
    let Some(before) = pits.as_mut() else {
        *pits = Some(terrain.pits.clone());
        return;
    };
    if terrain.pits == *before {
        return;
    }
    let dug: Vec<_> = terrain
        .pits
        .iter()
        .filter(|&&(tile, depth)| before.iter().find(|(t, _)| *t == tile).is_none_or(|&(_, d)| d < depth) && mine.ore(tile, depth - 1) == Some(STONE))
        .map(|&(tile, _)| tile)
        .collect();
    *before = terrain.pits.clone();
    let ground = |p: FxVec2| mirror.to_view(p).with_y(terrain.height(p).to_f32());
    let piles: Vec<Vec3> = mirror.current.as_ref().and_then(|s| s.piles.as_ref()).map_or_else(Vec::new, |lab| lab.piles.values().map(|p| ground(p.position)).collect());
    for tile in dug {
        let from = ground(Terrain::pit_center(tile));
        let Some(to) = piles.iter().copied().filter(|p| p.xz().distance(from.xz()) <= PILE_REACH).min_by(|a, b| a.distance(from).total_cmp(&b.distance(from))) else {
            continue;
        };
        let (mesh, material) = stone
            .get_or_insert_with(|| {
                let color = Color::srgb(STONE_TINT.x, STONE_TINT.y, STONE_TINT.z);
                (meshes.add(Cuboid::new(0.2, 0.17, 0.2)), materials.add(StandardMaterial { base_color: color, perceptual_roughness: 0.9, ..default() }))
            })
            .clone();
        for n in 0..TOSSED {
            // Each lands a little apart on the pile's top.
            let spread = Vec3::new((n as f32 - 1.0) * 0.12, 0.25, ((n * 7 % 3) as f32 - 1.0) * 0.1);
            commands.spawn((
                TossedStone { from: from + Vec3::Y * 0.1, to: to + spread, age: -(n as f32) * TOSS_GAP },
                WorldScoped::default(),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(from),
                Visibility::Hidden,
            ));
        }
    }
}

/// Flies tossed stones in an arc, and lands them with a clack.
pub fn fly_stones(
    mut commands: Commands,
    time: Res<Time>,
    sound: Res<crate::sound::Sound>,
    mut stones: Query<(Entity, &mut TossedStone, &mut Transform, &mut Visibility)>,
) {
    for (entity, mut stone, mut transform, mut visibility) in &mut stones {
        stone.age += time.delta_secs().min(0.1);
        if stone.age < 0.0 {
            continue;
        }
        let t = (stone.age / TOSS_SECONDS).min(1.0);
        *visibility = Visibility::Inherited;
        transform.translation = stone.from.lerp(stone.to, t) + Vec3::Y * (std::f32::consts::PI * t).sin() * 0.9;
        transform.rotation = Quat::from_rotation_y(t * std::f32::consts::TAU) * Quat::from_rotation_z((t * std::f32::consts::PI).sin() * 0.6);
        if t >= 1.0 {
            commands.entity(entity).despawn();
            sound.play(crate::sound::Cue::Land);
        }
    }
}

/// What the town's dig hint says when a dig in front is refused
/// (`Settlement::dig_in_front_of`), and a volume scene's notice
/// (`Settlement::dig_volume_of`).
pub fn refusal_text(refusal: DigRefusal) -> &'static str {
    match refusal {
        DigRefusal::Undiggable => "Can't dig here",
        DigRefusal::OwnTile => "Face the ground to dig",
        DigRefusal::Bottom => "The pit is as deep as it goes",
        DigRefusal::TooDeep => "Too deep from here: step down first",
        DigRefusal::Steepens => "Your footing would get too steep",
        DigRefusal::Strands => "That would leave someone no way out",
        DigRefusal::Blocked => "Something stands there",
        DigRefusal::Pile => "A pile is in the way",
        DigRefusal::Clear => "Nothing to dig there",
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
    let digging = mirror.player().zip(mirror.current.as_ref()).is_some_and(|(i, s)| s.agents[i as usize].avatar().is_some_and(|avatar| avatar.dig.is_some()));
    let collected = mirror.current.as_ref().and_then(|s| s.piles.as_ref()).filter(|lab| lab.collector.is_some()).map(|lab| lab.collected());
    let me = mirror.player().zip(mirror.current.as_ref()).map(|(i, s)| Terrain::pit_tile(s.agents[i as usize].body.position));
    let nearest_ore = me.zip(terrain.mine()).and_then(|(me, mine)| {
        (-8..=8)
            .flat_map(|y| (-8..=8).map(move |x| (me.0 + x, me.1 + y)))
            .filter(|&t| terrain.pit(t) < terrain.max_layers && mine.ore(t, terrain.pit(t)).is_some())
            .min_by_key(|&t| ((t.0 - me.0).pow(2) + (t.1 - me.1).pow(2), t))
            .map(|t| {
                let a = arena_tile(mirror.center, t);
                serde_json::json!({"tile": [a.x, a.y], "mineral": mine.ore(t, terrain.pit(t))})
            })
    });
    serde_json::json!({"pits": pits, "digging": digging, "collected": collected, "nearest_ore": nearest_ore})
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx::Fx;

    /// A sloped six-layer terrain round (0, 0), so arena and simulation
    /// tiles are the same; with `terraces`, a generated mine's.
    fn sloped(terraces: bool) -> Terrain {
        let mut terrain = Terrain::flat(FxVec2::ZERO, Fx::from_int(24));
        terrain.sloped = true;
        terrain.diggable = true;
        terrain.max_layers = 6;
        if terraces {
            terrain.generate_mine(11, FxVec2::new(Fx::from_int(-22), Fx::from_int(-22)), Veins::Stone);
        }
        terrain
    }

    fn heights(terrain: &Terrain) -> Heights {
        Heights::read(terrain, IVec2::ZERO, 24, IVec2::splat(-26), 53)
    }

    /// A stair and a deep pit beside it, on level ground and on terraces.
    fn dug(terraces: bool) -> Terrain {
        let mut terrain = sloped(terraces);
        for (tile, depth) in [((0, 0), 1), ((1, 0), 2), ((2, 0), 3), ((2, 1), 6), ((-3, 2), 2), ((-3, 3), 1)] {
            for _ in 0..depth {
                terrain.deepen(tile);
            }
        }
        terrain
    }

    /// The drawn corners are the simulation's ground at the tile's corners,
    /// and a tile is drawn flat exactly where the simulation calls it steep.
    #[test]
    fn corners_and_steepness_follow_the_simulation() {
        for terraces in [false, true] {
            let terrain = dug(terraces);
            let heights = heights(&terrain);
            let mut steep_tiles = 0;
            for y in -8..=8 {
                for x in -8..=8 {
                    let a = IVec2::new(x, y);
                    let c = corners(a, |b| heights.ground(b)).unwrap();
                    for (i, &height) in c.iter().enumerate() {
                        let p = corner_at(a, i, 0.0);
                        // Just inside the tile, so the corner is this tile's.
                        let inward = Vec2::new(x as f32 - p.x, y as f32 - p.z) * 0.002;
                        let sim = terrain.height(FxVec2::new(Fx::quantize(p.x + inward.x), Fx::quantize(p.z + inward.y))).to_f32();
                        assert!((sim - height).abs() < 0.01, "tile {a} corner {i}: drawn {height}, simulated {sim}");
                    }
                    assert_eq!(steep(c), terrain.steep((x, y)), "tile {a}");
                    let (top, flat) = drawn(a, |b| heights.ground(b)).unwrap();
                    assert_eq!(flat, steep(c));
                    if flat {
                        steep_tiles += 1;
                        assert_eq!(top, [heights.ground(a).unwrap(); 4]);
                    }
                }
            }
            assert!(steep_tiles > 0, "the deep pit makes its neighbours steep");
        }
    }

    /// A gentle tile's corners are never above a neighbour's drawn top on
    /// their shared edge, so walls only fall one way and the ground has no
    /// holes.
    #[test]
    fn walls_close_every_edge() {
        for terraces in [false, true] {
            let terrain = dug(terraces);
            let heights = heights(&terrain);
            let ground = |b: IVec2| heights.ground(b);
            for y in -8..=8 {
                for x in -8..=8 {
                    let a = IVec2::new(x, y);
                    let (top, flat) = drawn(a, ground).unwrap();
                    for (side, [i, j], [ti, tj]) in SIDES {
                        let (theirs, their_flat) = drawn(a + side, ground).unwrap();
                        if !flat && !their_flat {
                            assert_eq!((top[i], top[j]), (theirs[ti], theirs[tj]), "tile {a} side {side}");
                        }
                        if !flat && their_flat {
                            assert!(top[i] <= theirs[ti] && top[j] <= theirs[tj], "tile {a} side {side}");
                        }
                    }
                }
            }
        }
    }

    /// Everything a dig changes in the drawing (a tile's top, or a
    /// neighbour's that its walls reach down to) lies within the tiles
    /// `redrawn` names.
    #[test]
    fn a_dig_redraws_only_the_tiles_round_it() {
        for terraces in [false, true] {
            let mut terrain = dug(terraces);
            for dig in [(0, 0), (1, 1), (2, 2), (-2, 2), (3, 0), (2, 1), (-4, 3)] {
                let before = heights(&terrain);
                if !terrain.deepen(dig) {
                    continue;
                }
                let after = heights(&terrain);
                let redrawn = redrawn(IVec2::new(dig.0, dig.1), true);
                for y in -8..=8 {
                    for x in -8..=8 {
                        let a = IVec2::new(x, y);
                        let looks = |h: &Heights| {
                            let ground = |b: IVec2| h.ground(b);
                            let around: Vec<_> = SIDES.iter().map(|&(side, _, _)| drawn(a + side, ground)).collect();
                            (drawn(a, ground), around, corners(a, |b| h.undug(b)))
                        };
                        if looks(&before) != looks(&after) {
                            assert!(redrawn.contains(&a), "digging {dig:?} changed tile {a}");
                        }
                    }
                }
            }
        }
    }

    /// A dig rebuilds its own chunk, and the next one if its redrawn tiles
    /// reach it: one tile on flat ground, two in sloped scenes.
    #[test]
    fn a_dig_dirties_the_chunks_it_reaches() {
        let extent = 128;
        // Arena tile -128 is the first chunk's first; -113 its last.
        let middle = IVec2::new(-120, -120);
        assert_eq!(dirty_chunks([middle], extent, false), BTreeSet::from([[0, 0]]));
        assert_eq!(dirty_chunks([middle], extent, true), BTreeSet::from([[0, 0]]));
        let last = IVec2::new(-113, -120);
        assert_eq!(dirty_chunks([last], extent, false), BTreeSet::from([[0, 0], [1, 0]]));
        let near = IVec2::new(-114, -120);
        assert_eq!(dirty_chunks([near], extent, false), BTreeSet::from([[0, 0]]));
        assert_eq!(dirty_chunks([near], extent, true), BTreeSet::from([[0, 0], [1, 0]]));
        let corner = IVec2::new(-114, -114);
        assert_eq!(dirty_chunks([corner], extent, false), BTreeSet::from([[0, 0]]));
        assert_eq!(dirty_chunks([corner], extent, true), BTreeSet::from([[0, 0], [1, 0], [0, 1], [1, 1]]));
        // Two digs, one set.
        assert_eq!(dirty_chunks([middle, IVec2::new(-119, -120)], extent, true).len(), 1);
    }

    /// A wall's bands cut it at quarter meters, keeping its sloped bottom.
    #[test]
    fn a_slab_keeps_the_wall_between_its_heights() {
        let wall = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, -1.0, 0.0), Vec3::new(0.0, -0.5, 0.0)];
        let band = slab(&wall, -0.75, -0.5);
        assert!(!band.is_empty() && band.iter().all(|p| (-0.75..=-0.5).contains(&p.y)));
        assert!(band.iter().any(|p| p.x == 1.0) && band.iter().any(|p| p.x < 0.6), "{band:?}");
        // Below the wall, only its lowest corner touches (no area to draw).
        assert!(slab(&wall, -2.0, -1.0).iter().all(|p| *p == Vec3::new(1.0, -1.0, 0.0)), "nothing below the wall");
        assert_eq!(slab(&wall, -0.25, 0.0).len(), 4);
    }

    /// A town's stone is grey; the dig lab's ore keeps its colors.
    #[test]
    fn stone_tints_grey() {
        let terrain = sloped(true);
        assert_eq!(tint(terrain.mine().unwrap(), STONE), STONE_TINT);
        let mut lab = Terrain::flat(FxVec2::ZERO, Fx::from_int(24));
        lab.generate_mine(11, FxVec2::ZERO, Veins::Ore);
        assert_eq!(tint(lab.mine().unwrap(), 0), Vec3::from_array(crate::piles::mineral_color(crate::piles::mineral(0))));
    }
}
