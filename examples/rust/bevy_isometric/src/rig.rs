//! Shared baked rigs. Animation is selected by asset metadata, independent of simulation roles.
use bevy::prelude::*;
use voxel_assets::{ActorPart, Motion, SkinMesh, StyleProfile};
pub struct Rig {
    pub root: Entity,
    bones: Vec<(Entity, ActorPart, Transform)>,
    motion: Motion,
}
pub struct RigTemplate {
    pub scale: f32,
    motion: Motion,
    parts: Vec<(
        ActorPart,
        Transform,
        Vec<(Handle<Mesh>, Handle<StandardMaterial>)>,
    )>,
}
impl RigTemplate {
    pub fn upload(world: &mut World, skin: &SkinMesh, profile: &StyleProfile) -> Self {
        let parts = skin
            .parts
            .iter()
            .map(|part| {
                let default_pivot = match part.part {
                    ActorPart::Body => [0., 0.94, 0.],
                    ActorPart::LeftLeg => [-0.17, 0.58, 0.],
                    ActorPart::RightLeg => [0.17, 0.58, 0.],
                    ActorPart::Weapon => [0.4, 0.9, 0.],
                };
                let rest = Transform::from_translation(Vec3::from_array(
                    part.pivot.unwrap_or(default_pivot),
                ));
                let handles = crate::style::upload(
                    world,
                    voxel_assets::content_key(&(profile, part)),
                    &part.meshes,
                    profile,
                );
                (part.part, rest, handles)
            })
            .collect();
        Self {
            scale: skin.scale,
            motion: skin.motion.clone(),
            parts,
        }
    }
    pub fn spawn(&self, world: &mut World) -> Rig {
        let root = world
            .spawn((Transform::default(), Visibility::Inherited))
            .id();
        let mut bones = vec![];
        for (part, rest, handles) in &self.parts {
            let bone = world.spawn(*rest).id();
            world.entity_mut(root).add_child(bone);
            for (mesh, material) in handles {
                let e = world
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::default(),
                    ))
                    .id();
                world.entity_mut(bone).add_child(e);
            }
            bones.push((bone, *part, *rest));
        }
        Rig {
            root,
            bones,
            motion: self.motion.clone(),
        }
    }
}
impl Rig {
    pub fn pose(
        &self,
        world: &mut World,
        position: Vec3,
        heading: f32,
        scale: f32,
        travel: f32,
        activity: f32,
        aim: Option<f32>,
    ) -> (u64, u64) {
        let mut writes = 0;
        let mut skips = 0;
        let root = Transform::from_translation(position)
            .with_rotation(Quat::from_rotation_y(heading))
            .with_scale(Vec3::splat(scale));
        if *world.get::<Transform>(self.root).unwrap() != root {
            *world.get_mut::<Transform>(self.root).unwrap() = root;
            writes += 1;
        } else {
            skips += 1;
        }
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
            if *world.get::<Transform>(*entity).unwrap() != t {
                *world.get_mut::<Transform>(*entity).unwrap() = t;
                writes += 1;
            } else {
                skips += 1;
            }
        }
        (writes, skips)
    }
}
