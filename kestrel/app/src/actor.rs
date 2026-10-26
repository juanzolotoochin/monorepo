//! Built-in actors: a player character's articulated body as data.
//!
//! A Player character's `view.actor` names one of these files. The player's
//! body is built from it as a tree of joints: each joint has a kind, a pivot
//! relative to its parent joint and the blocks drawn on it. The character
//! poser animates the joints by kind, and a scene's style may re-dress them
//! with baked meshes (`style::apply`).
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use voxel_assets::ActorPart;

use crate::character::{Part, PartKind};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    /// The root joint, the body at the hips. Its pivot is relative to the
    /// character's feet.
    pub root: Joint,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Joint {
    pub part: ActorPart,
    /// The pivot, relative to the parent joint's.
    #[serde(default)]
    pub at: [f32; 3],
    #[serde(default)]
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub joints: Vec<Joint>,
}

/// A box drawn on a joint, centered at `at` relative to the joint's pivot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub at: [f32; 3],
    pub size: [f32; 3],
    pub material: ArtMaterial,
    /// A jetpack exhaust plume: hidden until the flight system shows it.
    #[serde(default)]
    pub exhaust: bool,
}

/// The built-in art materials (`scene::SceneArt`). Styles recolor some of
/// them by role (`style::MaterialBindings`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtMaterial {
    Dark,
    Frame,
    Ivory,
    Orange,
    Gold,
    Cyan,
    White,
}

const BUILTIN: &[(&str, &str)] = &[("exosuit", include_str!("../actors/exosuit.ron"))];

/// A built-in actor by name. Panics on an unknown or invalid actor: they are
/// compiled in, and `builtin_actors_are_valid` checks every one.
pub fn builtin(name: &str) -> &'static Actor {
    static ACTORS: std::sync::LazyLock<Vec<(&str, Actor)>> = std::sync::LazyLock::new(|| {
        BUILTIN.iter().map(|&(name, text)| (name, parse(text).unwrap_or_else(|e| panic!("actor {name}: {e}")))).collect()
    });
    ACTORS.iter().find(|(n, _)| *n == name).map(|(_, actor)| actor).unwrap_or_else(|| panic!("unknown actor {name:?}"))
}

/// The actor a scene's player character is built from.
pub fn for_player(character: &str) -> &'static Actor {
    let view = crate::lockstep::builtin_character_view(character);
    builtin(view.actor.as_deref().unwrap_or_else(|| panic!("character {character:?} has no view.actor")))
}

fn parse(text: &str) -> Result<Actor, String> {
    let actor: Actor = ron::from_str(text).map_err(|e| e.to_string())?;
    if actor.root.part != ActorPart::Body {
        return Err("the root joint must be the Body".into());
    }
    let mut seen = Vec::new();
    let mut pending = vec![&actor.root];
    while let Some(joint) = pending.pop() {
        if kind(joint.part).is_none() {
            return Err(format!("{:?} is not a joint", joint.part));
        }
        if seen.contains(&joint.part) {
            return Err(format!("{:?} appears twice", joint.part));
        }
        seen.push(joint.part);
        let finite = |v: &[f32; 3]| v.iter().all(|x| x.is_finite() && x.abs() <= 4.0);
        if !finite(&joint.at) || joint.blocks.iter().any(|b| !finite(&b.at) || !finite(&b.size) || b.size.iter().any(|s| *s <= 0.0)) {
            return Err(format!("{:?} has an invalid pivot or block", joint.part));
        }
        pending.extend(&joint.joints);
    }
    Ok(actor)
}

/// The joint kind the poser animates for an actor part.
pub fn kind(part: ActorPart) -> Option<PartKind> {
    use ActorPart::*;
    let side = |left: bool| if left { -1.0 } else { 1.0 };
    Some(match part {
        Body => PartKind::Body,
        Chest => PartKind::Chest,
        Head => PartKind::Head,
        LeftArm | RightArm => PartKind::Arm(side(part == LeftArm)),
        LeftForearm | RightForearm => PartKind::Forearm(side(part == LeftForearm)),
        LeftHand | RightHand => PartKind::Hand(side(part == LeftHand)),
        LeftLeg | RightLeg => PartKind::Leg(side(part == LeftLeg)),
        LeftKnee | RightKnee => PartKind::Knee(side(part == LeftKnee)),
        LeftShin | RightShin => PartKind::Shin(side(part == LeftShin)),
        LeftFoot | RightFoot => PartKind::Foot(side(part == LeftFoot)),
        Weapon => PartKind::WeaponGrip,
        WoundPlug | Armor => return None,
    })
}

/// The material handle for a built-in art material.
pub(crate) trait Art {
    fn cube(&self) -> &Handle<Mesh>;
    fn material(&self, material: ArtMaterial) -> &Handle<StandardMaterial>;
}

/// Spawns `joint` and its descendants under `parent`.
pub(crate) fn spawn_joint(parent: &mut ChildSpawnerCommands, art: &impl Art, joint: &Joint) {
    let pivot = Transform::from_translation(Vec3::from_array(joint.at));
    let kind = kind(joint.part).expect("validated joint");
    let mut entity = parent.spawn((Part { kind, bind: pivot, rest: pivot }, pivot, Visibility::default()));
    if joint.part == ActorPart::Weapon {
        entity.insert(crate::character::LocalBlade);
    }
    entity.with_children(|children| {
        for block in &joint.blocks {
            let transform = Transform::from_translation(Vec3::from_array(block.at)).with_scale(Vec3::from_array(block.size));
            let mut spawned = children.spawn((Mesh3d(art.cube().clone()), MeshMaterial3d(art.material(block.material).clone()), transform));
            if block.exhaust {
                spawned.insert((crate::jetpack::Exhaust, Visibility::Hidden));
            }
        }
        for child in &joint.joints {
            spawn_joint(children, art, child);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_actors_are_valid() {
        for (name, _) in BUILTIN {
            builtin(name);
        }
        let content = crate::lockstep::builtin_content();
        for (def, view) in content.rules.characters.iter().zip(&content.views.characters) {
            if def.brain == content::character::Brain::Player {
                builtin(view.actor.as_deref().unwrap());
            }
        }
    }

    #[test]
    fn invalid_actors_are_rejected() {
        let joint = |part: &str, joints: &str| format!("(part: {part}, joints: [{joints}])");
        let actor = |root: String| format!("(root: {root})");
        assert!(parse(&actor(joint("Body", &joint("Chest", "")))).is_ok());
        assert!(parse(&actor(joint("Chest", ""))).unwrap_err().contains("root"));
        assert!(parse(&actor(joint("Body", &joint("Armor", "")))).unwrap_err().contains("not a joint"));
        assert!(parse(&actor(joint("Body", &format!("{}, {}", joint("Head", ""), joint("Head", ""))))).unwrap_err().contains("twice"));
        assert!(parse("(root: (part: Body, blocks: [(at: (0, 0, 0), size: (0, 1, 1), material: Dark)]))").unwrap_err().contains("invalid"));
    }
}
