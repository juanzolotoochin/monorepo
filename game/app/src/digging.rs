//! Digging scenes: the deck's floor rests on layers of earth, and the
//! simulation's pits take them out a tile and a layer at a time. A bar over
//! each digging avatar fills over the dig, with a chip of dust now and then.

use std::collections::HashMap;

use bevy::prelude::*;
use voxel_assets::{ImpactResponse, SceneFeature};

use sim::settlement::avatar::Dig;
use sim::terrain::{LAYER, MAX_LAYERS};

use crate::assets::SceneLibrary;
use crate::effects::FxAssets;
use crate::gameplay::Player;
use crate::jetpack::Deck;
use crate::lockstep::Game;

/// One layer of earth under a deck tile, shown in digging scenes until it
/// is dug out. Bedrock, under everything, has no tile.
#[derive(Component)]
pub struct Layer {
    tile: Option<IVec2>,
    depth: u8,
}

/// A deck piece on a tile's surface (its floor plate and markings), gone
/// once the tile is dug.
#[derive(Component)]
pub struct Plate(IVec2);

/// The deck's body below the floor, hidden in digging scenes so pits show.
#[derive(Component)]
pub struct Underdeck;

#[derive(Component)]
pub struct DigBar {
    agent: u16,
}

#[derive(Component)]
pub struct DigFill;

/// The deck's tiles run from -`DECK` to `DECK` on both axes.
const DECK: i32 = 8;
/// The floor plates are this thick; the first layer starts under them.
const PLATE: f32 = 0.12;
/// Dust flies off every this many ticks of a dig.
const CHIP_TICKS: u64 = 15;
/// The bar's offset above the head, in avatar heights (before style scale).
const ABOVE: f32 = 2.15;

/// The simulation's pit tile at the arena's center.
fn center_tile(game: &Game) -> IVec2 {
    IVec2::new(game.center.x.round().floor_to_int() as i32, game.center.y.round().floor_to_int() as i32)
}

/// Arena tile of a simulation pit tile.
fn arena_tile(game: &Game, tile: (i32, i32)) -> IVec2 {
    IVec2::new(tile.0, tile.1) - center_tile(game)
}

/// How many layers are dug out of an arena tile.
fn dug(game: &Game, tile: IVec2) -> u8 {
    let Some(terrain) = game.world().map(sim::World::terrain) else { return 0 };
    let tile = tile + center_tile(game);
    terrain.pit((tile.x, tile.y))
}

pub fn update(
    mut commands: Commands,
    game: Res<Game>,
    library: Res<SceneLibrary>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    unsorted: Query<(Entity, &Transform), (With<Deck>, Without<Plate>, Without<Underdeck>)>,
    mut plates: Query<(&Plate, &mut Visibility), (Without<Layer>, Without<Underdeck>)>,
    mut under: Query<&mut Visibility, (With<Underdeck>, Without<Plate>, Without<Layer>)>,
    mut layers: Query<(&Layer, &mut Visibility), (Without<Plate>, Without<Underdeck>)>,
    mut built: Local<bool>,
) {
    // Sort the deck's pieces once: what lies on a tile goes with it, and
    // what's under the floor gives way to the earth.
    for (entity, transform) in &unsorted {
        let p = transform.translation;
        if p.y < -0.3 {
            commands.entity(entity).insert(Underdeck);
        } else if p.x.abs() <= DECK as f32 + 0.5 && p.z.abs() <= DECK as f32 + 0.5 {
            commands.entity(entity).insert(Plate(IVec2::new(p.x.round() as i32, p.z.round() as i32)));
        }
    }
    let digging = library.has_feature(SceneFeature::Digging);
    if digging && !*built {
        *built = true;
        spawn_layers(&mut commands, &mut meshes, &mut materials);
    }
    for (layer, mut visibility) in &mut layers {
        let shown = digging && layer.tile.is_none_or(|t| dug(&game, t) <= layer.depth);
        visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
    }
    // Elsewhere the deck is the scene's (see `jetpack::scene_changed`).
    if !digging {
        return;
    }
    for mut visibility in &mut under {
        visibility.set_if_neq(Visibility::Hidden);
    }
    for (plate, mut visibility) in &mut plates {
        let shown = dug(&game, plate.0) == 0;
        visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// Earth under every deck tile, in checkered layers that darken with
/// depth, on a bedrock slab.
fn spawn_layers(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    let cube = meshes.add(Cuboid::default());
    let earth = |m: &mut Assets<StandardMaterial>, shade: f32| {
        m.add(StandardMaterial { base_color: Color::srgb(0.36 * shade, 0.27 * shade, 0.21 * shade), perceptual_roughness: 0.95, ..default() })
    };
    let shades: Vec<[Handle<StandardMaterial>; 2]> =
        (0..MAX_LAYERS).map(|k| 1.0 - k as f32 * 0.22).map(|s| [earth(materials, s), earth(materials, s * 0.88)]).collect();
    let layer = LAYER.to_f32();
    for x in -DECK..=DECK {
        for z in -DECK..=DECK {
            for depth in 0..MAX_LAYERS {
                let top = if depth == 0 { -PLATE } else { -layer * depth as f32 };
                let bottom = -layer * (depth + 1) as f32;
                commands.spawn((
                    Layer { tile: Some(IVec2::new(x, z)), depth },
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(shades[depth as usize][((x + z + depth as i32) & 1) as usize].clone()),
                    Transform::from_xyz(x as f32, (top + bottom) / 2.0, z as f32).with_scale(Vec3::new(1.0, top - bottom, 1.0)),
                    Visibility::Hidden,
                ));
            }
        }
    }
    let bedrock = materials.add(StandardMaterial { base_color: Color::srgb(0.07, 0.09, 0.12), metallic: 0.5, ..default() });
    let floor = -layer * MAX_LAYERS as f32;
    let width = (2 * DECK + 1) as f32 + 0.5;
    commands.spawn((
        Layer { tile: None, depth: MAX_LAYERS },
        Mesh3d(cube),
        MeshMaterial3d(bedrock),
        Transform::from_xyz(0.0, floor - 0.2, 0.0).with_scale(Vec3::new(width, 0.4, width)),
        Visibility::Hidden,
    ));
}

/// Progress bars over digging avatars, dust while they dig, and a burst
/// when a layer comes out.
pub fn feedback(
    mut commands: Commands,
    game: Res<Game>,
    assets: Res<FxAssets>,
    sound: Res<crate::sound::Sound>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    player: Single<&GlobalTransform, With<Player>>,
    mut bars: Query<(Entity, &DigBar, &mut Node, &mut Visibility, &ComputedNode), Without<DigFill>>,
    mut fills: Query<(&ChildOf, &mut Node), With<DigFill>>,
    mut chips: Local<HashMap<u16, u64>>,
    mut pits: Local<Vec<((i32, i32), u8)>>,
) {
    let terrain = game.world().map(sim::World::terrain);
    let now = game.tick as f32 - 1.0 + game.alpha;
    let digs: Vec<(u16, Dig, Vec3)> = game.current.as_ref().map_or_else(Vec::new, |s| {
        s.agents
            .iter()
            .enumerate()
            .filter_map(|(i, a)| Some((i as u16, a.avatar.dig.clone()?, game.to_view(a.position).with_y(a.avatar.height.to_f32()))))
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
    // A layer came out: a burst where the floor was.
    if let Some(terrain) = terrain {
        if terrain.pits != *pits {
            for &(tile, depth) in &terrain.pits {
                if pits.iter().find(|(t, _)| *t == tile).is_none_or(|&(_, d)| d < depth) {
                    let center = arena_tile(&game, tile).as_vec2();
                    let floor = -(LAYER.to_f32() * depth as f32);
                    burst(&mut commands, &assets, Vec3::new(center.x, floor, center.y), depth as u32 * 977 + tile.0 as u32);
                    sound.play(crate::sound::Cue::Stone);
                }
            }
            *pits = terrain.pits.clone();
        }
    }
    let progress = |dig: &Dig| ((now - dig.started as f32) / (dig.until - dig.started) as f32).clamp(0.0, 1.0);
    let lift = if game.network.is_some() { 24.0 } else { 0.0 };
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

/// For the control snapshot: dug pits by arena tile, and whether the
/// player is digging.
pub fn snapshot(game: &Game) -> serde_json::Value {
    let Some(terrain) = game.world().map(sim::World::terrain) else { return serde_json::Value::Null };
    let pits: Vec<_> = terrain
        .pits
        .iter()
        .map(|&(tile, depth)| {
            let t = arena_tile(game, tile);
            serde_json::json!({"tile": [t.x, t.y], "depth": depth})
        })
        .collect();
    let digging = game.player().zip(game.current.as_ref()).is_some_and(|(i, s)| s.agents[i as usize].avatar.dig.is_some());
    serde_json::json!({"pits": pits, "digging": digging})
}
