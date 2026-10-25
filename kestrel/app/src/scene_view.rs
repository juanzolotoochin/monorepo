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
    }
}

impl SceneView {
    pub fn scene(&self) -> &Scene {
        &self.pack.scene
    }

    pub fn has_feature(&self, feature: SceneFeature) -> bool {
        self.pack.scene.features.contains(&feature)
    }

    pub fn title(&self) -> &str {
        &self.pack.scene.name
    }

    pub fn extent(&self) -> f32 {
        crate::assets::extent(&self.pack)
    }
}
