//! Scene-authored mech views. Simulation owns the seat and travel; these
//! articulated parts follow the shared state on hosts, clients, and replays.

use bevy::prelude::*;
use bevy::ecs::relationship::RelatedSpawner;
use service::WorldScoped;
use sim::settlement::vehicle::VehiclePhase;
use voxel_assets::Scene;

use crate::character::Player;
use crate::lockstep::{SessionService, WorldMirror};
use crate::peers::Peer;

#[derive(Component)]
pub(crate) struct MechRoot(usize);

#[derive(Component)]
pub(crate) struct MechMuzzle(pub usize);

#[derive(Clone, Copy, Component)]
pub(crate) enum JointKind { Leg(f32), Hatch, Cannon }

#[derive(Component)]
pub(crate) struct MechJoint { vehicle: usize, kind: JointKind, rest: Transform }

fn part(parent: &mut RelatedSpawner<ChildOf>, cube: &Handle<Mesh>, material: &Handle<StandardMaterial>,
    translation: Vec3, size: Vec3) {
    parent.spawn((Mesh3d(cube.clone()), MeshMaterial3d(material.clone()),
        Transform::from_translation(translation).with_scale(size)));
}

pub fn spawn(world: &mut World, scene: &Scene) {
    if scene.vehicles.is_empty() { return; }
    let cube = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(1.0, 1.0, 1.0));
    let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
    let armor = materials.add(Color::srgb(0.19, 0.23, 0.29));
    let dark = materials.add(Color::srgb(0.075, 0.09, 0.12));
    let edge = materials.add(Color::srgb(0.64, 0.53, 0.28));
    let glow = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.8, 1.0),
        emissive: LinearRgba::new(0.15, 1.4, 2.2, 1.0),
        ..default()
    });
    drop(materials);
    for (index, vehicle) in scene.vehicles.iter().enumerate() {
        let at = Vec3::from_array(vehicle.position);
        let root = world.spawn((MechRoot(index), Transform::from_translation(at)
            .with_rotation(Quat::from_rotation_y(vehicle.yaw)), Visibility::Inherited, WorldScoped::default())).id();
        world.entity_mut(root).with_children(|body| {
            part(body, &cube, &dark, Vec3::new(0.0, 1.72, 0.0), Vec3::new(1.55, 0.62, 0.91));
            part(body, &cube, &armor, Vec3::new(0.0, 2.70, -0.10), Vec3::new(1.78, 1.60, 0.98));
            part(body, &cube, &dark, Vec3::new(0.0, 2.84, 0.43), Vec3::new(1.05, 1.15, 0.10));
            part(body, &cube, &glow, Vec3::new(0.0, 2.83, 0.50), Vec3::new(0.32, 0.40, 0.045));
            part(body, &cube, &edge, Vec3::new(0.0, 3.60, -0.12), Vec3::new(1.90, 0.16, 1.13));
            for side in [-1.0_f32, 1.0] {
                part(body, &cube, &armor, Vec3::new(side * 1.05, 3.20, -0.08), Vec3::new(0.66, 0.53, 1.02));
                part(body, &cube, &dark, Vec3::new(side * 1.15, 2.50, 0.0), Vec3::new(0.48, 0.98, 0.57));
                part(body, &cube, &edge, Vec3::new(side * 1.14, 2.04, 0.22), Vec3::new(0.50, 0.16, 0.73));
                let rest = Transform::from_translation(Vec3::new(side * 0.55, 1.55, 0.0));
                body.spawn((MechJoint { vehicle: index, kind: JointKind::Leg(side), rest }, rest, Visibility::Inherited))
                    .with_children(|leg| {
                        part(leg, &cube, &armor, Vec3::new(0.0, -0.38, 0.0), Vec3::new(0.49, 0.92, 0.59));
                        part(leg, &cube, &dark, Vec3::new(0.0, -0.91, 0.08), Vec3::new(0.40, 0.22, 0.48));
                        part(leg, &cube, &armor, Vec3::new(0.0, -1.27, 0.12), Vec3::new(0.47, 0.70, 0.57));
                        part(leg, &cube, &edge, Vec3::new(0.0, -1.55, 0.38), Vec3::new(0.63, 0.20, 1.06));
                    });
            }
            let hatch = Transform::from_translation(Vec3::new(0.0, 3.42, 0.56));
            body.spawn((MechJoint { vehicle: index, kind: JointKind::Hatch, rest: hatch }, hatch, Visibility::Inherited))
                .with_children(|hinge| {
                    part(hinge, &cube, &armor, Vec3::new(0.0, -0.58, 0.03), Vec3::new(1.12, 1.15, 0.14));
                    part(hinge, &cube, &edge, Vec3::new(0.0, -0.66, 0.12), Vec3::new(0.68, 0.13, 0.05));
                });
            let cannon = Transform::from_translation(Vec3::new(1.20, 2.55, 0.52));
            body.spawn((MechJoint { vehicle: index, kind: JointKind::Cannon, rest: cannon }, cannon, Visibility::Inherited))
                .with_children(|barrel| {
                    part(barrel, &cube, &dark, Vec3::new(0.0, 0.0, 0.45), Vec3::new(0.71, 0.62, 1.25));
                    part(barrel, &cube, &armor, Vec3::new(0.0, 0.0, 1.20), Vec3::new(0.43, 0.42, 0.85));
                    part(barrel, &cube, &glow, Vec3::new(0.0, 0.0, 1.65), Vec3::new(0.28, 0.28, 0.10));
                    barrel.spawn((MechMuzzle(index), Transform::from_xyz(0.0, 0.0, 1.72)));
                });
        });
    }
}

fn opening(phase: &VehiclePhase, tick: f32) -> f32 {
    match *phase {
        VehiclePhase::Parked => 0.0,
        VehiclePhase::Driving { .. } => 0.0,
        VehiclePhase::Boarding { started, .. } => {
            let t = ((tick - started as f32) / sim::settlement::vehicle::BOARD_TICKS as f32).clamp(0.0, 1.0);
            (t * 4.0).min(1.0) * ((1.0 - t) * 4.0).min(1.0)
        }
        VehiclePhase::Exiting { started, .. } => {
            let t = ((tick - started as f32) / sim::settlement::vehicle::EXIT_TICKS as f32).clamp(0.0, 1.0);
            (t * 4.0).min(1.0) * ((1.0 - t) * 4.0).min(1.0)
        }
    }
}

fn occupied(settlement: &sim::settlement::Settlement, agent: u16) -> bool {
    settlement.vehicle_of(agent).is_some_and(|(_, vehicle)| vehicle.phase.driving(agent))
}

pub fn update(session: Res<SessionService>, mirror: Res<WorldMirror>, time: Res<Time>,
    mut roots: Query<(&MechRoot, &mut Transform)>, mut joints: Query<(&MechJoint, &mut Transform), Without<MechRoot>>,
    mut player: Query<&mut Visibility, With<Player>>,
    mut peers: Query<(&Peer, &mut Visibility), Without<Player>>,
) {
    let Some(settlement) = session.settlement() else { return };
    let tick = mirror.tick as f32 + mirror.alpha;
    for (root, mut transform) in &mut roots {
        let Some(vehicle) = settlement.vehicles.get(root.0) else { continue };
        transform.translation = session.to_view(vehicle.position);
        transform.rotation = Quat::from_rotation_y(vehicle.heading.to_f32());
    }
    for (joint, mut transform) in &mut joints {
        let Some(vehicle) = settlement.vehicles.get(joint.vehicle) else { continue };
        let mut target = joint.rest;
        match joint.kind {
            JointKind::Hatch => target.rotate_local_x(-1.65 * opening(&vehicle.phase, tick)),
            JointKind::Leg(side) => if let VehiclePhase::Driving { agent } = vehicle.phase {
                let a = &settlement.agents[agent as usize];
                let speed = a.avatar.velocity.length().to_f32();
                target.rotate_local_x((a.travelled.to_f32() * 2.6 + side * 1.57).sin() * (speed / 4.0).clamp(0.0, 0.65));
            },
            JointKind::Cannon => if let VehiclePhase::Driving { agent } = vehicle.phase {
                let a = &settlement.agents[agent as usize];
                if let Some(swing) = &a.avatar.swing {
                    let age = (tick - swing.started as f32).max(0.0);
                    target.translation.z -= (1.0 - age / 12.0).max(0.0) * 0.22;
                }
            },
        }
        let blend = 1.0 - (-14.0 * time.delta_secs()).exp();
        transform.translation = transform.translation.lerp(target.translation, blend);
        transform.rotation = transform.rotation.slerp(target.rotation, blend);
    }
    if let Some(agent) = session.player() {
        for mut visibility in &mut player {
            *visibility = if occupied(settlement, agent) { Visibility::Hidden } else { Visibility::Inherited };
        }
    }
    for (peer, mut visibility) in &mut peers {
        *visibility = if occupied(settlement, peer.agent()) { Visibility::Hidden } else { Visibility::Inherited };
    }
}
