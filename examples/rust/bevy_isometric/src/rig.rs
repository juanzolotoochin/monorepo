//! Shared baked rigs. Animation is selected by asset metadata, independent of simulation roles.
use bevy::prelude::*;
use voxel_assets::{ActorPart, Motion, SkinMesh, StyleProfile};
pub struct Rig {
    pub root: Entity,
    bones: Vec<(Entity, ActorPart, Transform)>,
    motion: Motion,
}
impl Rig {
    pub fn spawn(world: &mut World, skin: &SkinMesh, profile: &StyleProfile) -> Self {
        let root = world.spawn(Transform::default()).id();
        let mut bones = vec![];
        for part in &skin.parts {
            let default_pivot = match part.part {
                ActorPart::Body => [0., 0.94, 0.],
                ActorPart::LeftLeg => [-0.17, 0.58, 0.],
                ActorPart::RightLeg => [0.17, 0.58, 0.],
                ActorPart::Weapon => [0.4, 0.9, 0.],
            };
            let rest =
                Transform::from_translation(Vec3::from_array(part.pivot.unwrap_or(default_pivot)));
            let bone = world.spawn(rest).id();
            world.entity_mut(root).add_child(bone);
            let handles = crate::style::upload(
                world,
                voxel_assets::content_key(&(profile, part)),
                &part.meshes,
                profile,
            );
            for (mesh, material) in handles {
                let entity = world
                    .spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default()))
                    .id();
                world.entity_mut(bone).add_child(entity);
            }
            bones.push((bone, part.part, rest));
        }
        Self {
            root,
            bones,
            motion: skin.motion.clone(),
        }
    }
    pub fn pose(
        &self,
        world: &mut World,
        position: Vec3,
        heading: f32,
        scale: f32,
        travel: f32,
        activity: f32,
        aim: Option<f32>,
    ) {
        *world.get_mut::<Transform>(self.root).unwrap() = Transform::from_translation(position)
            .with_rotation(Quat::from_rotation_y(heading))
            .with_scale(Vec3::splat(scale));
        for (entity, part, rest) in &self.bones {
            let mut t = *rest;
            let side = if *part == ActorPart::LeftLeg { -1. } else { 1. };
            match &self.motion {
                Motion::Biped | Motion::Quadruped => {
                    if matches!(part, ActorPart::LeftLeg | ActorPart::RightLeg) {
                        t.rotation = Quat::from_rotation_x((travel * 12.).sin() * 0.5 * side);
                    }
                    if *part == ActorPart::Body {
                        t.translation.y += (travel * 24.).cos() * 0.018;
                    }
                }
                Motion::Vehicle { wheel_radius } => {
                    if matches!(part, ActorPart::LeftLeg | ActorPart::RightLeg) {
                        t.rotation = Quat::from_rotation_x(travel / wheel_radius);
                    }
                }
                Motion::Machine { axis } => {
                    if *part == ActorPart::Weapon {
                        t.rotation = Quat::from_axis_angle(
                            Vec3::from_array(*axis).normalize(),
                            activity * std::f32::consts::TAU,
                        );
                    }
                }
                Motion::Turret => {
                    if *part == ActorPart::Weapon {
                        t.rotation = Quat::from_rotation_y(aim.unwrap_or(0.) - heading);
                        t.translation.z -= activity * 0.12;
                    }
                }
                Motion::Static => {}
            }
            *world.get_mut::<Transform>(*entity).unwrap() = t;
        }
    }
}
