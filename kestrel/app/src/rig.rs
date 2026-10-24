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
                    ActorPart::Chest => [0., 1.12, 0.],
                    ActorPart::LeftLeg => [-0.17, 0.58, 0.],
                    ActorPart::RightLeg => [0.17, 0.58, 0.],
                    ActorPart::LeftArm => [-0.43, 1.27, 0.],
                    ActorPart::RightArm => [0.43, 1.27, 0.],
                    ActorPart::LeftShin => [-0.17, 0.31, 0.],
                    ActorPart::RightShin => [0.17, 0.31, 0.],
                    ActorPart::LeftKnee => [-0.17, 0.31, 0.],
                    ActorPart::RightKnee => [0.17, 0.31, 0.],
                    ActorPart::LeftFoot => [-0.17, 0.1, 0.],
                    ActorPart::RightFoot => [0.17, 0.1, 0.],
                    ActorPart::LeftForearm => [-0.43, 1.01, 0.],
                    ActorPart::RightForearm => [0.43, 1.01, 0.],
                    ActorPart::LeftHand => [-0.43, 0.75, 0.],
                    ActorPart::RightHand => [0.43, 0.75, 0.],
                    ActorPart::Weapon => [0.4, 0.9, 0.],
                    ActorPart::Head => [0., 1.49, 0.],
                    ActorPart::WoundPlug => [0., 0.94, 0.],
                    ActorPart::Armor => [0., 0.94, 0.],
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
            let bone = world.spawn((*rest, Visibility::Inherited)).id();
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
    pub fn spawn_part(
        &self,
        world: &mut World,
        part: ActorPart,
        transform: Transform,
    ) -> Option<Entity> {
        let (_, _, handles) = self.parts.iter().find(|(kind, _, _)| *kind == part)?;
        let root = world.spawn((transform, Visibility::Inherited)).id();
        for (mesh, material) in handles {
            let child = world
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                ))
                .id();
            world.entity_mut(root).add_child(child);
        }
        Some(root)
    }
}
impl Rig {
    pub fn part_world_transform(&self, world: &World, part: ActorPart) -> Option<Transform> {
        let (bone, _, _) = self.bones.iter().find(|(_, kind, _)| *kind == part)?;
        let root = world.get::<Transform>(self.root)?;
        let local = world.get::<Transform>(*bone)?;
        Some(Transform::from_matrix(root.to_matrix() * local.to_matrix()))
    }
    pub fn show_part(&self, world: &mut World, part: ActorPart, show: bool) {
        for (entity, kind, _) in &self.bones {
            if *kind == part {
                let desired = if show {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                if world.get::<Visibility>(*entity) != Some(&desired) {
                    *world.get_mut::<Visibility>(*entity).unwrap() = desired;
                }
            }
        }
    }
    pub fn pose(
        &self,
        world: &mut World,
        position: Vec3,
        heading: f32,
        roll: f32,
        scale: f32,
        travel: f32,
        activity: f32,
        injury: f32,
        recoil: f32,
        aim: Option<f32>,
    ) -> (u64, u64) {
        let mut writes = 0;
        let mut skips = 0;
        let root = Transform::from_translation(position)
            .with_rotation(Quat::from_rotation_y(heading) * Quat::from_rotation_z(roll))
            .with_scale(Vec3::splat(scale));
        if *world.get::<Transform>(self.root).unwrap() != root {
            *world.get_mut::<Transform>(self.root).unwrap() = root;
            writes += 1;
        } else {
            skips += 1;
        }
        for (entity, part, rest) in &self.bones {
            let mut t = *rest;
            let side = if matches!(
                part,
                ActorPart::LeftLeg
                    | ActorPart::LeftArm
                    | ActorPart::LeftShin
                    | ActorPart::LeftKnee
                    | ActorPart::LeftFoot
                    | ActorPart::LeftForearm
                    | ActorPart::LeftHand
            ) {
                -1.
            } else {
                1.
            };
            match &self.motion {
                Motion::Biped | Motion::Quadruped => {
                    if matches!(part, ActorPart::LeftLeg | ActorPart::RightLeg) {
                        let stride = (travel * (12.0 - injury * 3.0)).sin()
                            * (0.5 - injury * 0.23)
                            * side
                            * if recoil > 0.0 { -1.0 } else { 1.0 };
                        let drag = if side < 0.0 {
                            injury * 0.24
                        } else {
                            -injury * 0.07
                        };
                        t.rotation = Quat::from_rotation_x(stride + drag + recoil * 0.18);
                    }
                    if matches!(part, ActorPart::LeftShin | ActorPart::RightShin | ActorPart::LeftKnee | ActorPart::RightKnee) {
                        let phase = (travel * (12.0 - injury * 3.0)).sin() * side;
                        t.rotation =
                            Quat::from_rotation_x(-phase.max(0.0) * (0.42 - injury * 0.18));
                    }
                    if matches!(part, ActorPart::LeftArm | ActorPart::RightArm) {
                        let swing = (travel * (12.0 - injury * 3.0)).sin() * side;
                        t.rotation =
                            Quat::from_rotation_x(-swing * (0.36 - injury * 0.12) - activity * 0.1);
                    }
                    if matches!(part, ActorPart::LeftForearm | ActorPart::RightForearm) {
                        t.rotation = Quat::from_rotation_x(-0.3 - activity * 0.12);
                    }
                    if matches!(part, ActorPart::LeftHand | ActorPart::RightHand) {
                        t.rotation = Quat::from_rotation_x(activity * 0.08);
                    }
                    if matches!(
                        part,
                        ActorPart::Body
                            | ActorPart::Chest
                            | ActorPart::Armor
                            | ActorPart::WoundPlug
                            | ActorPart::Head
                    ) {
                        t.translation.y += (travel * 24.).cos() * 0.018;
                        t.rotation =
                            Quat::from_rotation_x(-activity * 0.25 - injury * 0.26 + recoil * 0.28)
                                * Quat::from_rotation_z(injury * 0.16);
                        t.translation.y -= injury * 0.06;
                        t.translation.z -= activity * 0.1 + injury * 0.08;
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

/// Temporary material overrides are explicitly restored before a pooled rig is reused.
pub struct Fade {
    materials: Vec<(
        Entity,
        Handle<StandardMaterial>,
        Handle<StandardMaterial>,
        bool,
    )>,
}
impl Fade {
    pub fn new(world: &mut World, root: Entity) -> Self {
        let mut pending = vec![root];
        let mut materials = vec![];
        while let Some(e) = pending.pop() {
            if let Some(children) = world.get::<Children>(e) {
                pending.extend(children.iter());
            }
            if let Some(handle) = world.get::<MeshMaterial3d<StandardMaterial>>(e) {
                let original = handle.0.clone();
                let mut material = world
                    .resource::<Assets<StandardMaterial>>()
                    .get(&original)
                    .unwrap()
                    .clone();
                material.alpha_mode = AlphaMode::Blend;
                let faded = world
                    .resource_mut::<Assets<StandardMaterial>>()
                    .add(material);
                let no_shadow = world.get::<bevy::light::NotShadowCaster>(e).is_some();
                world
                    .entity_mut(e)
                    .insert((MeshMaterial3d(faded.clone()), bevy::light::NotShadowCaster));
                materials.push((e, original, faded, no_shadow));
            }
        }
        Self { materials }
    }
    pub fn opacity(&self, world: &mut World, opacity: f32) {
        let mut assets = world.resource_mut::<Assets<StandardMaterial>>();
        for (_, _, handle, _) in &self.materials {
            assets
                .get_mut(handle)
                .unwrap()
                .base_color
                .set_alpha(opacity.clamp(0., 1.));
        }
    }
    pub fn restore(self, world: &mut World) {
        for (e, original, faded, no_shadow) in self.materials {
            world
                .resource_mut::<Assets<StandardMaterial>>()
                .remove(&faded);
            if let Ok(mut e) = world.get_entity_mut(e) {
                e.insert(MeshMaterial3d(original));
                if !no_shadow {
                    e.remove::<bevy::light::NotShadowCaster>();
                }
            }
        }
    }
}
