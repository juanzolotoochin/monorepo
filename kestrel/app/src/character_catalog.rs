//! Discover inspectable characters and their capabilities from baked scene packs.
use std::{collections::HashSet, ops::Deref};
use voxel_assets::{ActorPart, Motion, ScenePack};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CharacterSource {
    BuiltIn,
    Actor,
    Skin(String),
}

/// Character identity is authored by the actor style or baked skin; a scene
/// only determines where the required assets are loaded from.
#[derive(Clone, Debug)]
pub struct CharacterEntry {
    pub scene_index: usize,
    pub source: CharacterSource,
    pub id: String,
    pub name: String,
    pub can_locomote: bool,
    pub weapon: Option<content::scenario::Weapon>,
    pub can_jump: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CharacterCatalog(Vec<CharacterEntry>);

impl CharacterCatalog {
    pub fn discover(scenes: &[std::sync::Arc<ScenePack>]) -> Self {
        let mut ids = HashSet::new();
        let mut entries = Vec::new();
        for (scene_index, pack) in scenes.iter().enumerate() {
            if pack.scene.style.is_none() && pack.actor.is_empty()
                && ids.insert("builtin:exosuit".to_string()) {
                entries.push(CharacterEntry {
                    scene_index,
                    source: CharacterSource::BuiltIn,
                    id: "builtin:exosuit".into(),
                    name: "EXOSUIT".into(),
                    can_locomote: true,
                    weapon: Some(content::scenario::Weapon::Sword),
                    can_jump: true,
                });
            }
            let Some(style) = pack.scene.style.as_ref() else { continue };
            if !pack.actor.is_empty() && ids.insert(style.id.clone()) {
                let has_weapon_mesh = pack.actor.iter().any(|part| {
                    part.part == ActorPart::Weapon && !part.meshes.is_empty()
                });
                entries.push(CharacterEntry {
                    scene_index,
                    source: CharacterSource::Actor,
                    id: style.id.clone(),
                    name: style.actor_name.clone().unwrap_or_else(|| style.id.clone()),
                    // Actor styles dress the complete player rig. Missing
                    // authored meshes fall back to its procedural body, so
                    // mesh coverage does not determine motion capabilities.
                    can_locomote: true,
                    weapon: if has_weapon_mesh { style.actor_weapon } else { None },
                    can_jump: true,
                });
            }
            for skin in &pack.skins {
                if skin.parts.is_empty() || !matches!(skin.motion, Motion::Biped | Motion::Quadruped | Motion::CubeRoll { .. })
                    || !ids.insert(format!("skin:{}", skin.id)) {
                    continue;
                }
                entries.push(CharacterEntry {
                    scene_index,
                    source: CharacterSource::Skin(skin.id.clone()),
                    id: format!("skin:{}", skin.id),
                    name: skin.id.replace('_', " ").to_uppercase(),
                    can_locomote: true,
                    weapon: None,
                    can_jump: false,
                });
            }
        }
        Self(entries)
    }

    pub fn for_scene(&self, scene_index: usize) -> Option<&CharacterEntry> {
        self.iter().find(|character| character.scene_index == scene_index)
    }

    pub fn built_in(&self) -> Option<&CharacterEntry> {
        self.iter().find(|character| matches!(character.source, CharacterSource::BuiltIn))
    }

    pub fn index_for_id(&self, id: &str) -> Result<usize, String> {
        self.iter().position(|character| character.id == id)
            .ok_or_else(|| format!("Unknown character {id}. Available characters: {}",
                self.iter().map(|character| character.id.as_str()).collect::<Vec<_>>().join(", ")))
    }
}

impl Deref for CharacterCatalog {
    type Target = [CharacterEntry];
    fn deref(&self) -> &Self::Target { &self.0 }
}
