//! The scene a world shows (World layer): its boulders, industrial props,
//! style and the player's body, set up when the world starts and gone with
//! it. Views ask it which scene they are in.

use std::sync::Arc;

use bevy::prelude::*;
use service::{Service, WorldLayer, WorldState};
use voxel_assets::{Scene, SceneFeature, ScenePack};

use crate::assets::SceneLibrary;
use crate::lockstep::WorldMirror;

/// Sample an asset-authored body track at the current repaired fraction.
/// The fallback grows the body upward from its permanent floor base.
pub(crate) fn repair_transform(piece: &voxel_assets::DestructiblePiece, fraction: f32) -> Transform {
    let fraction = fraction.clamp(0.0, 1.0);
    let (scale, offset) = if let Some(track) = &piece.repair_animation {
        let upper = track.keyframes.partition_point(|key| key.at < fraction)
            .min(track.keyframes.len() - 1);
        let lower = upper.saturating_sub(1);
        let a = &track.keyframes[lower];
        let b = &track.keyframes[upper];
        let t = if a.at == b.at { 0.0 } else { (fraction - a.at) / (b.at - a.at) };
        (Vec3::from_array(a.scale).lerp(Vec3::from_array(b.scale), t),
            Vec3::from_array(a.offset).lerp(Vec3::from_array(b.offset), t))
    } else { (Vec3::new(1.0, fraction, 1.0), Vec3::ZERO) };
    let center = Vec3::from_array(piece.center);
    let floor = center.y - piece.size[1] * 0.5;
    Transform::from_translation(Vec3::new(
        -center.x + center.x * (1.0 - scale.x) + offset.x,
        -center.y + floor * (1.0 - scale.y) + offset.y,
        -center.z + center.z * (1.0 - scale.z) + offset.z,
    )).with_scale(scale)
}

#[derive(Resource)]
pub struct SceneView {
    /// The scene's index in the `SceneLibrary`.
    pub index: usize,
    pub pack: Arc<ScenePack>,
}

impl Service for SceneView {
    type Layer = WorldLayer;
    type Deps = (WorldMirror,);

    fn init(world: &mut World) -> Self {
        world.resource_mut::<crate::effects::CoverContacts>().clear();
        // The world's scene: the session's label, on the host and on clients.
        let index = match world.resource::<WorldMirror>().scene {
            usize::MAX => 0,
            index => index,
        };
        let pack = world.resource::<SceneLibrary>().scenes[index].clone();
        let art = world.resource::<crate::scene::SceneArt>().clone();
        {
            let mut commands = world.commands();
            crate::scene::spawn_player(&mut commands, &art, &pack.scene);
            if pack.scene.industrial {
                crate::scene::spawn_industrial(&mut commands, &art);
            }
        }
        world.flush();
        crate::assets::spawn_scene(world, &pack);
        if let Some(weapon) = pack.scene.scenario.as_ref().and_then(|scenario| {
            (scenario.weapons.len() == 1).then(|| scenario.weapons[0])
        }) {
            world.resource_mut::<crate::gameplay::Controls>().weapon = weapon;
        }
        crate::style::apply(world, &pack, false);
        // The built-in deck shows only where the scene brings no ground of
        // its own: terrain, an environment, or ground to dig.
        let own_ground = pack.scene.terrain.is_some()
            || pack.environment.is_some()
            || pack.scene.features.contains(&SceneFeature::Digging);
        for mut visibility in world.query_filtered::<&mut Visibility, With<crate::jetpack::Deck>>().iter_mut(world) {
            *visibility = if own_ground { Visibility::Hidden } else { Visibility::Inherited };
        }
        info!("Showing scene {}", pack.scene.name);
        SceneView { index, pack }
    }

    fn shutdown(self, world: &mut World) {
        world.resource_mut::<crate::effects::CoverContacts>().clear();
        crate::destruction::release_materials(world);
        crate::assets::release_scene(world);
        // The built-in deck shows only in worlds, not behind the menus.
        for mut visibility in world.query_filtered::<&mut Visibility, With<crate::jetpack::Deck>>().iter_mut(world) {
            *visibility = Visibility::Hidden;
        }
    }

    fn systems(app: &mut App) {
        app.add_systems(
            PostUpdate,
            crate::assets::update_captions
                .after(bevy::transform::TransformSystems::Propagate)
                .run_if(crate::control::rendered)
                .run_if(in_state(WorldState::Active)),
        );
        app.add_systems(PostUpdate, (update_destructibles, crate::destruction::animate).chain().run_if(in_state(WorldState::Active)));
    }
}

/// Project the authoritative cover state into both visuals and view hit tests.
fn update_destructibles(world: &mut World) {
    let Some(terrain) = world.resource::<crate::lockstep::SessionService>().world().map(|w| w.terrain()) else { return };
    let health: Vec<u16> = terrain.destructibles.iter().map(|piece| piece.health).collect();
    let progress: Vec<u16> = terrain.destructibles.iter().map(|piece| piece.repair_progress).collect();
    let repairable: Vec<bool> = world.resource::<SceneView>().pack.destructibles.iter().map(|piece| piece.repairable).collect();
    let waiting: Vec<bool> = (0..health.len()).map(|index|
        world.resource::<crate::effects::CoverContacts>().waiting(index)).collect();
    let mut query = world.query::<(Entity, &mut crate::style::DestructibleView, &mut Visibility,
        Option<&crate::combat::Obstacle>, Option<&crate::destruction::CoverFragment>, &Children)>();
    let mut changes = Vec::new();
    let mut tint = Vec::new();
    let mut hits = Vec::new();
    let mut body_visibility = Vec::new();
    let mut base_progress = Vec::new();
    for (entity, mut view, mut visibility, obstacle, falling, children) in query.iter_mut(world) {
        if falling.is_some() { continue; }
        let current = health.get(view.index).copied().unwrap_or(0);
        let alive = current > 0;
        let permanent_base = repairable.get(view.index).copied().unwrap_or(false);
        let waiting = waiting.get(view.index).copied().unwrap_or(false);
        *visibility = if alive || view.last_health > 0 || permanent_base { Visibility::Inherited } else { Visibility::Hidden };
        if current != view.last_health && !waiting {
            let factor = 0.55 + 0.45 * f32::from(current) / f32::from(view.max_health);
            tint.extend(children.iter().map(|child| (child, factor)));
            if current < view.last_health { hits.push((entity, view.index, current)); }
            view.last_health = current;
        }
        let progress = progress.get(view.index).copied().unwrap_or(0);
        let fraction = (f32::from(current) + f32::from(progress) / content::TICKS_PER_SECOND as f32)
            / f32::from(view.max_health);
        body_visibility.extend(children.iter().map(|child| (child, view.last_health > 0 || progress > 0,
            permanent_base.then(|| (view.index, fraction)))));
        base_progress.extend(children.iter().map(|child| (child, progress)));
        let blocking = alive || permanent_base;
        if blocking != obstacle.is_some() { changes.push((entity, view.index, blocking)); }
    }
    for (child, visible, repair) in body_visibility {
        if world.get::<crate::style::CoverBody>(child).is_some() {
            if let Some(mut state) = world.get_mut::<Visibility>(child) {
                *state = if visible { Visibility::Inherited } else { Visibility::Hidden };
            }
            if let Some((index, fraction)) = repair {
                let transform = repair_transform(&world.resource::<SceneView>().pack.destructibles[index], fraction);
                *world.get_mut::<Transform>(child).unwrap() = transform;
            }
        }
    }
    let base_updates: Vec<_> = base_progress.into_iter().filter_map(|(child, progress)| {
        let rest = world.get::<crate::style::RepairBaseMaterial>(child)?.0;
        let handle = world.get::<MeshMaterial3d<StandardMaterial>>(child)?.0.clone();
        Some((handle, rest, progress))
    }).collect();
    for (handle, rest, progress) in base_updates {
        if let Some(mut material) = world.resource_mut::<Assets<StandardMaterial>>().get_mut(&handle) {
            let glow = f32::from(progress) / content::TICKS_PER_SECOND as f32;
            material.emissive = rest + LinearRgba::rgb(0.8 * glow, 0.42 * glow, 0.1 * glow);
        }
    }
    let updates: Vec<_> = tint.into_iter().filter_map(|(child, factor)| {
        let handle = world.get::<MeshMaterial3d<StandardMaterial>>(child)?.0.clone();
        let base = world.get::<crate::style::DamageMaterial>(child)?.0.to_srgba();
        Some((handle, Color::srgba(base.red * factor, base.green * factor, base.blue * factor, base.alpha)))
    }).collect();
    for (handle, color) in updates {
        if let Some(mut material) = world.resource_mut::<Assets<StandardMaterial>>().get_mut(&handle) {
            material.base_color = color;
        }
    }
    for (entity, index, alive) in changes {
        if alive {
            let piece = &world.resource::<SceneView>().pack.destructibles[index];
            let disc = crate::navigation::Disc { center: Vec3::from_array(piece.center), radius: piece.radius };
            world.entity_mut(entity).insert(crate::combat::Obstacle(disc));
        } else {
            world.entity_mut(entity).remove::<crate::combat::Obstacle>();
        }
    }
    let tick = world.resource::<crate::lockstep::SessionService>().world().map_or(0, |w| w.tick());
    for (entity, index, current) in hits {
        let piece = world.resource::<SceneView>().pack.destructibles[index].clone();
        crate::destruction::hit(world, entity, &piece, index, current, tick);
    }
}

impl SceneView {
    pub fn scene(&self) -> &Scene {
        &self.pack.scene
    }

    pub fn has_feature(&self, feature: SceneFeature) -> bool {
        self.pack.scene.features.contains(&feature)
    }

    /// The character every player avatar here is.
    pub fn player(&self) -> &'static content::character::CharacterDef {
        crate::lockstep::builtin_character(&self.pack.scene.player)
    }

    /// The player's jetpack hover height in meters, or 0 without a jetpack.
    pub fn hover(&self) -> f32 {
        self.player().jetpack().map_or(0.0, |j| crate::lockstep::view_f32(j.hover))
    }

    pub fn title(&self) -> &str {
        &self.pack.scene.name
    }

    pub fn extent(&self) -> f32 {
        crate::assets::extent(&self.pack)
    }
}
