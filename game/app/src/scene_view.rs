//! The scene a world shows (World layer): its boulders, industrial props,
//! style and the player's body, set up when the world starts and gone with
//! it. Views ask it which scene they are in.

use std::sync::Arc;

use bevy::prelude::*;
use service::{Service, WorldLayer};
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
        crate::style::apply(world, &pack);
        info!("Showing scene {}", pack.scene.name);
        SceneView { index, pack }
    }

    fn shutdown(self, world: &mut World) {
        crate::assets::release_scene(world);
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
