//! The scene a world shows (World layer): its boulders, industrial props,
//! style and the player's body, set up when the world starts and gone with
//! it. Views ask it which scene they are in.

use std::sync::Arc;

use bevy::prelude::*;
use service::{Service, WorldLayer, WorldState};
use voxel_assets::{Scene, SceneFeature, ScenePack};

use crate::assets::SceneLibrary;
use crate::lockstep::WorldMirror;

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
    let waiting: Vec<bool> = (0..health.len()).map(|index|
        world.resource::<crate::effects::CoverContacts>().waiting(index)).collect();
    let mut query = world.query::<(Entity, &mut crate::style::DestructibleView, &mut Visibility,
        Option<&crate::combat::Obstacle>, Option<&crate::destruction::CoverFragment>, &Children)>();
    let mut changes = Vec::new();
    let mut tint = Vec::new();
    let mut hits = Vec::new();
    for (entity, mut view, mut visibility, obstacle, falling, children) in query.iter_mut(world) {
        if falling.is_some() { continue; }
        let current = health.get(view.index).copied().unwrap_or(0);
        let alive = current > 0;
        let waiting = waiting.get(view.index).copied().unwrap_or(false);
        *visibility = if alive || view.last_health > 0 { Visibility::Inherited } else { Visibility::Hidden };
        if current != view.last_health && !waiting {
            let factor = 0.55 + 0.45 * f32::from(current) / f32::from(view.max_health);
            tint.extend(children.iter().map(|child| (child, factor)));
            hits.push((entity, view.index, current));
            view.last_health = current;
        }
        if alive != obstacle.is_some() { changes.push((entity, view.index, alive)); }
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
