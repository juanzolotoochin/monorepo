//! Shared baked rigs. Animation is selected by asset metadata, independent of simulation roles.
use bevy::prelude::*;
use voxel_assets::{ActorPart, Motion, SkinMesh, StyleProfile};
pub struct Rig {
    pub root: Entity,
    bones: Vec<RigBone>,
    motion: Motion,
    step_local: f32,
    last_travel: Option<f32>,
    moving_hold: f32,
    gait_blend: f32,
}
pub struct RigTemplate {
    pub scale: f32,
    motion: Motion,
    parts: Vec<RigPart>,
}
struct RigPart {
    kind: ActorPart,
    rest: Transform,
    contact: Option<FootGeometry>,
    handles: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>,
}
struct RigBone {
    entity: Entity,
    kind: ActorPart,
    rest: Transform,
    contact: Option<FootGeometry>,
}
/// A biped's unarmed swing (a threat's attack): `windup` builds from 0 to 1
/// with the arms raised and the body leaning back, then `swing` drops from 1
/// to 0 as the arms come down in front and the body lunges.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Strike {
    pub windup: f32,
    pub swing: f32,
}
#[derive(Clone, Copy)]
pub(crate) struct FootGeometry {
    sole_y: f32,
    near_z: f32,
    far_z: f32,
}
impl FootGeometry {
    pub(crate) fn from_meshes(meshes: &[voxel_assets::MeshData]) -> Option<Self> {
        let vertices: Vec<_> = meshes.iter().flat_map(|mesh| mesh.positions.iter()).collect();
        let sole_y = vertices.iter().map(|p| p[1]).reduce(f32::min)?;
        if sole_y >= -0.1 { return None; }
        let sole = vertices.iter().filter(|p| p[1] <= sole_y + 0.06);
        let (near_z, far_z) = sole.fold((f32::INFINITY, f32::NEG_INFINITY), |(near, far), p| {
            (near.min(p[2]), far.max(p[2]))
        });
        (near_z.is_finite() && far_z.is_finite()).then_some(Self { sole_y, near_z, far_z })
    }

    pub(crate) fn center_z(self) -> f32 { (self.near_z + self.far_z) * 0.5 }
    pub(crate) fn reach(self) -> f32 { -self.sole_y }

    pub(crate) fn place(self, rest: Transform, offset: f32, lift: f32) -> Transform {
        let mut posed = rest;
        let center = self.center_z();
        let target = center + offset;
        // Rotate around the fixed hip while keeping the sole's center at its
        // distance-driven ground position. The outside sole edge determines
        // the leg stretch needed to avoid penetrating the floor.
        let center_at = |angle: f32| {
            let edge = if angle.sin() >= 0.0 { self.far_z } else { self.near_z };
            let stretched_sole_y = (lift - rest.translation.y + edge * angle.sin()) / angle.cos();
            stretched_sole_y * angle.sin() + center * angle.cos()
        };
        let (mut low, mut high) = (-1.2_f32, 1.2_f32);
        for _ in 0..14 {
            let middle = (low + high) * 0.5;
            if center_at(middle) > target { low = middle; } else { high = middle; }
        }
        let angle = (low + high) * 0.5;
        let edge = if angle.sin() >= 0.0 { self.far_z } else { self.near_z };
        posed.rotation = Quat::from_rotation_x(angle);
        // Keep the lowest sole edge on the floor while the hip stays attached.
        posed.scale.y = ((rest.translation.y - edge * angle.sin() - lift)
            / (-self.sole_y * angle.cos())).max(0.01);
        posed
    }
}

fn contact_phase(motion: &Motion, part: ActorPart) -> Option<f32> {
    use ActorPart::*;
    match (motion, part) {
        (Motion::Biped, RightLeg) | (Motion::Quadruped, RightLeg | LeftShin) => Some(0.0),
        (Motion::Biped, LeftLeg) | (Motion::Quadruped, LeftLeg | RightShin) => Some(std::f32::consts::PI),
        _ => None,
    }
}

fn contact_path(phase: f32, step: f32) -> (f32, f32) {
    let phase = phase.rem_euclid(std::f32::consts::TAU);
    let stance = 0.56;
    let span = step * stance;
    if phase < std::f32::consts::TAU * stance {
        (span - phase * step / std::f32::consts::PI, 0.0)
    } else {
        let progress = (phase / std::f32::consts::TAU - stance) / (1.0 - stance);
        let eased = progress * progress * (3.0 - 2.0 * progress);
        (-span + 2.0 * span * eased, step * 0.28 * (progress * std::f32::consts::PI).sin())
    }
}

/// Roll a cube around its next floor edge as distance advances. The body pivot
/// is at the cube center, `edge / 2` above the floor in its rest pose.
fn cube_roll_pose(rest: Transform, travel: f32, edge: f32, scale: f32) -> Transform {
    let angle = travel / (edge * scale.max(0.01)) * std::f32::consts::FRAC_PI_2;
    let quarter = std::f32::consts::FRAC_PI_2;
    let turn = (angle / quarter).floor();
    let local = angle - turn * quarter;
    let (sin, cos) = local.sin_cos();
    let center = turn * edge + edge * 0.5 * (1.0 - cos + sin);
    let mut posed = rest;
    posed.rotation = Quat::from_rotation_x(angle);
    posed.translation.y += edge * 0.5 * (sin + cos - 1.0);
    posed.translation.z += center - travel / scale.max(0.01);
    posed
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
                RigPart {
                    kind: part.part,
                    rest,
                    contact: contact_phase(&skin.motion, part.part)
                        .and_then(|_| FootGeometry::from_meshes(&part.meshes)),
                    handles,
                }
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
        for part in &self.parts {
            let bone = world.spawn((part.rest, Visibility::Inherited)).id();
            world.entity_mut(root).add_child(bone);
            for (mesh, material) in &part.handles {
                let e = world
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::default(),
                    ))
                    .id();
                world.entity_mut(bone).add_child(e);
            }
            bones.push(RigBone { entity: bone, kind: part.kind, rest: part.rest, contact: part.contact });
        }
        let step_local = bones.iter().filter_map(|bone| bone.contact.map(|contact| -contact.sole_y))
            .reduce(f32::min).map_or(0.3, |height| (height * 0.55).clamp(0.16, 0.35));
        Rig {
            root,
            bones,
            motion: self.motion.clone(),
            step_local,
            last_travel: None,
            moving_hold: 0.0,
            gait_blend: 0.0,
        }
    }
    pub fn spawn_part(
        &self,
        world: &mut World,
        part: ActorPart,
        transform: Transform,
    ) -> Option<Entity> {
        let handles = &self.parts.iter().find(|entry| entry.kind == part)?.handles;
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
    pub fn cycle_distance(&self, scale: f32) -> f32 {
        match self.motion {
            Motion::CubeRoll { edge } => 4.0 * edge * scale.max(0.01),
            _ => 2.0 * self.step_local * scale.max(0.01),
        }
    }
    pub fn part_world_transform(&self, world: &World, part: ActorPart) -> Option<Transform> {
        let bone = self.bones.iter().find(|bone| bone.kind == part)?.entity;
        let root = world.get::<Transform>(self.root)?;
        let local = world.get::<Transform>(bone)?;
        Some(Transform::from_matrix(root.to_matrix() * local.to_matrix()))
    }
    pub fn show_part(&self, world: &mut World, part: ActorPart, show: bool) {
        for bone in &self.bones {
            if bone.kind == part {
                let desired = if show {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                if world.get::<Visibility>(bone.entity) != Some(&desired) {
                    *world.get_mut::<Visibility>(bone.entity).unwrap() = desired;
                }
            }
        }
    }
    pub fn pose(
        &mut self,
        world: &mut World,
        position: Vec3,
        heading: f32,
        roll: f32,
        scale: f32,
        travel: f32,
        activity: f32,
        injury: f32,
        recoil: f32,
        strike: Strike,
        aim: Option<f32>,
        studio_moving: Option<bool>,
    ) -> (u64, u64) {
        // Arms come up early in the windup, so it reads from the start.
        let raise = 1.0 - (1.0 - strike.windup.clamp(0.0, 1.0)).powi(2);
        let lunge = strike.swing.clamp(0.0, 1.0);
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
        if let Some(moving) = studio_moving {
            self.gait_blend = if moving { 1.0 } else { 0.0 };
        } else {
            let dt = world.get_resource::<Time>().map_or(1.0 / 60.0, Time::delta_secs).clamp(0.0, 0.05);
            if self.last_travel.is_some_and(|last| (travel - last).abs() > 0.0001) {
                self.moving_hold = 0.12;
            } else {
                self.moving_hold = (self.moving_hold - dt).max(0.0);
            }
            let target = if self.moving_hold > 0.0 { 1.0 } else { 0.0 };
            self.gait_blend += (target - self.gait_blend) * (1.0 - (-20.0 * dt).exp());
        }
        self.last_travel = Some(travel);
        let phase = travel * std::f32::consts::PI / (self.step_local * scale.max(0.01));
        for bone in &self.bones {
            let part = bone.kind;
            let entity = bone.entity;
            let rest = bone.rest;
            let mut t = rest;
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
                    if bone.contact.is_none() && matches!(part, ActorPart::LeftLeg | ActorPart::RightLeg) {
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
                    if bone.contact.is_none() && matches!(part, ActorPart::LeftShin | ActorPart::RightShin | ActorPart::LeftKnee | ActorPart::RightKnee) {
                        let phase = (travel * (12.0 - injury * 3.0)).sin() * side;
                        t.rotation =
                            Quat::from_rotation_x(-phase.max(0.0) * (0.42 - injury * 0.18));
                    }
                    if matches!(part, ActorPart::LeftArm | ActorPart::RightArm) {
                        let swing = (travel * (12.0 - injury * 3.0)).sin() * side;
                        t.rotation =
                            Quat::from_rotation_x(-swing * (0.36 - injury * 0.12) - activity * 0.1
                                - raise * 2.6 - lunge * 1.3);
                    }
                    if matches!(part, ActorPart::LeftForearm | ActorPart::RightForearm) {
                        t.rotation = Quat::from_rotation_x(-0.3 - activity * 0.12 - raise * 0.5 + lunge * 0.2);
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
                        t.rotation =
                            Quat::from_rotation_x(-activity * 0.25 - injury * 0.26 + recoil * 0.28
                                - raise * 0.22 + lunge * 0.38)
                                * Quat::from_rotation_z(injury * 0.16);
                        t.translation.y -= injury * 0.06;
                        t.translation.z -= activity * 0.1 + injury * 0.08 + raise * 0.06 - lunge * 0.14;
                    }
                }
                Motion::Vehicle { wheel_radius } => {
                    if matches!(part, ActorPart::LeftLeg | ActorPart::RightLeg) {
                        t.rotation = Quat::from_rotation_x(travel / wheel_radius);
                    }
                }
                Motion::CubeRoll { edge } => {
                    if part == ActorPart::Body {
                        t = cube_roll_pose(rest, travel, *edge, scale);
                    }
                }
                Motion::Machine { axis } => {
                    if part == ActorPart::Weapon {
                        t.rotation = Quat::from_axis_angle(
                            Vec3::from_array(*axis).normalize(),
                            activity * std::f32::consts::TAU,
                        );
                    }
                }
                Motion::Turret => {
                    if part == ActorPart::Weapon {
                        t.rotation = Quat::from_rotation_y(aim.unwrap_or(0.) - heading);
                        t.translation.z -= activity * 0.12;
                    }
                }
                Motion::Static => {}
            }
            if let (Some(contact), Some(offset)) = (bone.contact, contact_phase(&self.motion, part)) {
                let (foot_offset, lift) = contact_path(phase + offset, self.step_local);
                let planted = contact.place(rest, foot_offset, lift);
                t.translation = t.translation.lerp(planted.translation, self.gait_blend);
                t.rotation = t.rotation.slerp(planted.rotation, self.gait_blend);
                t.scale = t.scale.lerp(planted.scale, self.gait_blend);
            }
            if *world.get::<Transform>(entity).unwrap() != t {
                *world.get_mut::<Transform>(entity).unwrap() = t;
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

#[cfg(test)]
mod contact_tests {
    use super::*;
    use crate::assets::SceneLibrary;

    #[test]
    fn cube_roll_keeps_its_corners_on_the_floor() {
        let edge = 0.6;
        let rest = Transform::from_xyz(0.0, edge * 0.5, 0.0);
        for frame in -80..=80 {
            let travel = frame as f32 * edge / 40.0;
            let pose = cube_roll_pose(rest, travel, edge, 1.0);
            let low = [-edge * 0.5, edge * 0.5].into_iter()
                .flat_map(|y| [-edge * 0.5, edge * 0.5].map(|z| {
                    (pose.rotation * Vec3::new(0.0, y, z)).y + pose.translation.y
                }))
                .fold(f32::INFINITY, f32::min);
            assert!(low.abs() < 0.0001, "frame {frame}: cube bottom at {low}");
        }
        let quarter = cube_roll_pose(rest, edge, edge, 1.0);
        assert!((quarter.rotation * Vec3::Y).distance(Vec3::Z) < 0.0001);
    }

    #[test]
    fn baked_biped_and_quadruped_soles_stay_above_ground() {
        let library = SceneLibrary::new();
        for id in ["worker", "ghoul", "ghoul_brute", "ox"] {
            let skin = library.scenes.iter().flat_map(|pack| &pack.skins)
                .find(|skin| skin.id == id).unwrap();
            let contacts: Vec<_> = skin.parts.iter()
                .filter(|part| contact_phase(&skin.motion, part.part).is_some())
                .collect();
            assert_eq!(contacts.len(), if id == "ox" { 4 } else { 2 });
            for part in contacts {
                let geometry = FootGeometry::from_meshes(&part.meshes).unwrap();
                let rest = Transform::from_translation(Vec3::from_array(part.pivot.unwrap()));
                let step = (-geometry.sole_y * 0.55).clamp(0.16, 0.35);
                for frame in 0..120 {
                    let phase = frame as f32 * std::f32::consts::TAU / 120.0;
                    let (offset, lift) = contact_path(phase, step);
                    let posed = geometry.place(rest, offset, lift);
                    let angle = posed.rotation.to_euler(EulerRot::XYZ).0;
                    let edge = if angle.sin() >= 0.0 { geometry.far_z } else { geometry.near_z };
                    let lowest = posed.translation.y + posed.scale.y * geometry.sole_y * angle.cos()
                        - edge * angle.sin();
                    assert!((lowest - lift).abs() < 0.002, "{id} {:?} frame {frame}: {lowest} vs {lift}", part.part);
                    let rendered_lowest = part.meshes.iter().flat_map(|mesh| &mesh.positions)
                        .map(|vertex| posed.translation.y + posed.scale.y * vertex[1] * angle.cos()
                            - vertex[2] * angle.sin())
                        .fold(f32::INFINITY, f32::min);
                    assert!(rendered_lowest >= lift - 0.01,
                        "{id} {:?} frame {frame}: rendered sole clips {:.3}m", part.part, lift - rendered_lowest);
                }
            }
        }
    }

    #[test]
    fn stance_cancels_root_travel() {
        let step = 0.3;
        let mut planted: Option<f32> = None;
        for frame in 0..60 {
            let phase = frame as f32 * std::f32::consts::TAU * 0.56 / 60.0;
            let (offset, lift) = contact_path(phase, step);
            assert_eq!(lift, 0.0);
            let world_foot = phase * step / std::f32::consts::PI + offset;
            if let Some(first) = planted { assert!((world_foot - first).abs() < 1e-5); }
            planted = Some(world_foot);
        }
    }

    #[test]
    fn baked_skin_contact_points_do_not_slide_through_stance() {
        let library = SceneLibrary::new();
        for id in ["worker", "ghoul", "ghoul_brute", "ox"] {
            let skin = library.scenes.iter().flat_map(|pack| &pack.skins)
                .find(|skin| skin.id == id).unwrap();
            for part in skin.parts.iter().filter(|part| contact_phase(&skin.motion, part.part).is_some()) {
                let geometry = FootGeometry::from_meshes(&part.meshes).unwrap();
                let rest = Transform::from_translation(Vec3::from_array(part.pivot.unwrap()));
                let step = (-geometry.sole_y * 0.55).clamp(0.16, 0.35);
                let mut first = None;
                let mut worst = 0.0_f32;
                for frame in 0..=56 {
                    let phase = frame as f32 / 100.0 * std::f32::consts::TAU;
                    let (offset, lift) = contact_path(phase, step);
                    assert_eq!(lift, 0.0);
                    let posed = geometry.place(rest, offset, lift);
                    let angle = posed.rotation.to_euler(EulerRot::XYZ).0;
                    let center = (geometry.near_z + geometry.far_z) * 0.5;
                    let root_travel = phase * step / std::f32::consts::PI;
                    let contact_z = root_travel + posed.translation.z
                        + posed.scale.y * geometry.sole_y * angle.sin() + center * angle.cos();
                    let first = *first.get_or_insert(contact_z);
                    worst = worst.max((contact_z - first).abs());
                }
                assert!(worst < 0.005, "{id} {:?} slides {worst:.3}m through stance", part.part);
            }
        }
    }
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
