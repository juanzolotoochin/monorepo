//! Discover playable characters and their capabilities from baked actor packs.
use std::{collections::HashSet, ops::Deref};
use voxel_assets::{ActorPart, ScenePack};

/// Character identity is authored by the actor style; the scene is only its
/// current asset loading location.
#[derive(Clone, Debug)]
pub struct CharacterEntry {
    pub scene_index: usize,
    pub id: String,
    pub name: String,
    pub can_locomote: bool,
    pub weapon: Option<content::scenario::Weapon>,
    pub can_jump: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CharacterCatalog(Vec<CharacterEntry>);

impl CharacterCatalog {
    pub fn discover(scenes: &[ScenePack]) -> Self {
        let mut ids = HashSet::new();
        Self(scenes.iter().enumerate().filter_map(|(scene_index, pack)| {
            let style = pack.scene.style.as_ref()?;
            if pack.actor.is_empty() || !ids.insert(style.id.clone()) {
                return None;
            }
            let has_weapon_mesh = pack.actor.iter().any(|part| {
                part.part == ActorPart::Weapon && !part.meshes.is_empty()
            });
            let can_locomote = [ActorPart::LeftLeg, ActorPart::RightLeg].into_iter()
                .all(|part| pack.actor.iter().any(|mesh| mesh.part == part && !mesh.meshes.is_empty()));
            Some(CharacterEntry {
                scene_index,
                id: style.id.clone(),
                name: style.actor_name.clone().unwrap_or_else(|| style.id.clone()),
                can_locomote,
                weapon: if has_weapon_mesh { style.actor_weapon } else { None },
                can_jump: can_locomote,
            })
        }).collect())
    }

    pub fn for_scene(&self, scene_index: usize) -> Option<&CharacterEntry> {
        self.iter().find(|character| character.scene_index == scene_index)
    }

    pub fn scene_for_id(&self, id: &str) -> Result<usize, String> {
        self.iter().find(|character| character.id == id)
            .map(|character| character.scene_index)
            .ok_or_else(|| format!("Unknown character {id}. Available characters: {}",
                self.iter().map(|character| character.id.as_str()).collect::<Vec<_>>().join(", ")))
    }
}

impl Deref for CharacterCatalog {
    type Target = [CharacterEntry];
    fn deref(&self) -> &Self::Target { &self.0 }
}
