//! Other players' avatars in a networked game: a plain voxel figure per
//! avatar this machine doesn't control, tinted per player, placed where the
//! simulation has it between ticks.

use bevy::prelude::*;

use crate::gameplay::Player;
use crate::lockstep::Game;

#[derive(Component)]
pub struct Peer {
    agent: u16,
    /// Walk cycle phase, advanced by distance moved.
    stride: f32,
}

/// A leg's hip pivot; `1.0` or `-1.0` for which side swings forward first.
#[derive(Component)]
pub struct Leg(f32);

pub fn update(
    mut commands: Commands,
    game: Res<Game>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    player: Single<&Transform, (With<Player>, Without<Peer>, Without<Leg>)>,
    mut peers: Query<(Entity, &mut Peer, &mut Transform, &Children), Without<Leg>>,
    mut legs: Query<(&Leg, &mut Transform), Without<Peer>>,
) {
    let me = game.player();
    let others: Vec<u16> = game.current.as_ref().map_or_else(Vec::new, |s| {
        (0..s.agents.len() as u16).filter(|&i| s.agents[i as usize].controlled && Some(i) != me).collect()
    });
    let mut shown = Vec::new();
    for (entity, mut peer, mut transform, children) in &mut peers {
        if !others.contains(&peer.agent) {
            commands.entity(entity).despawn();
            continue;
        }
        shown.push(peer.agent);
        let current = &game.current.as_ref().unwrap().agents[peer.agent as usize];
        let before = game.previous.as_ref().and_then(|p| p.agents.get(peer.agent as usize)).unwrap_or(current);
        let (from, to) = (game.to_view(before.position), game.to_view(current.position));
        let alpha = if from.distance(to) > 1.5 { 1.0 } else { game.alpha };
        let mut position = from.lerp(to, alpha);
        position.y = before.avatar.height.to_f32() + (current.avatar.height - before.avatar.height).to_f32() * alpha;
        peer.stride += position.with_y(0.0).distance(transform.translation.with_y(0.0)).min(1.0) * 8.5;
        transform.translation = position;
        let turn = (current.heading - before.heading).to_f32();
        let turn = (turn + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        transform.rotation = Quat::from_rotation_y(before.heading.to_f32() + turn * alpha);
        transform.scale = player.scale;
        let moving = current.avatar.velocity.length().to_f32() > 0.05;
        for &child in children {
            if let Ok((leg, mut hip)) = legs.get_mut(child) {
                let swing = if moving { peer.stride.sin() * 0.6 * leg.0 } else { 0.0 };
                hip.rotation = Quat::from_rotation_x(swing);
            }
        }
    }
    for agent in others.into_iter().filter(|a| !shown.contains(a)) {
        let owner = game.current.as_ref().unwrap().agents[agent as usize].owner.map_or(0, |p| p.0);
        spawn(&mut commands, &mut meshes, &mut materials, agent, owner);
    }
}

fn spawn(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, agent: u16, owner: u32) {
    let cube = meshes.add(Cuboid::default());
    // Golden-angle hues, so each player's color is far from the last.
    let hue = (owner as f32 * 137.5 + 20.0) % 360.0;
    let suit = materials.add(StandardMaterial { base_color: Color::hsl(hue, 0.65, 0.5), perceptual_roughness: 0.7, ..default() });
    let dark = materials.add(StandardMaterial { base_color: Color::srgb(0.08, 0.09, 0.12), perceptual_roughness: 0.8, ..default() });
    let visor = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.97, 1.0),
        emissive: LinearRgba::rgb(0.6, 1.6, 1.8),
        ..default()
    });
    let block = |p: &mut ChildSpawnerCommands, material: &Handle<StandardMaterial>, position: Vec3, size: Vec3| {
        p.spawn((Mesh3d(cube.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(position).with_scale(size)));
    };
    commands
        .spawn((Peer { agent, stride: 0.0 }, Transform::default(), Visibility::default()))
        .with_children(|p| {
            block(p, &suit, Vec3::new(0.0, 1.05, 0.0), Vec3::new(0.62, 0.62, 0.38));
            block(p, &dark, Vec3::new(0.0, 0.76, 0.0), Vec3::new(0.66, 0.12, 0.42));
            block(p, &suit, Vec3::new(0.0, 1.62, 0.0), Vec3::new(0.48, 0.44, 0.46));
            block(p, &visor, Vec3::new(0.0, 1.64, 0.235), Vec3::new(0.4, 0.12, 0.03));
            for (side, swing) in [(-0.17, 1.0), (0.17, -1.0)] {
                p.spawn((Leg(swing), Transform::from_xyz(side, 0.72, 0.0), Visibility::default()))
                    .with_children(|hip| block(hip, &dark, Vec3::new(0.0, -0.36, 0.0), Vec3::new(0.2, 0.72, 0.24)));
            }
        });
}
