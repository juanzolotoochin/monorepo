//! Other players' avatars in a networked game: a copy of the local player's
//! exosuit (as the scene's style dresses it) per avatar this machine doesn't
//! control, placed where the simulation has it between ticks and posed like
//! the local player.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::gameplay::{Part, PartKind, Player, Pose, WALK_SPEED};
use crate::jetpack::Exhaust;
use crate::lockstep::Game;
use crate::style::ActiveStyle;

#[derive(Component)]
pub struct Peer {
    agent: u16,
    /// The style the copy was made in; a restyle makes a new copy.
    style: Option<usize>,
    /// Walk cycle phase, advanced by distance moved.
    stride: f32,
}

/// A copied part pivot. Not a `Part`, so the local player's animation and
/// styling leave it alone.
#[derive(Component)]
pub struct PeerPart {
    kind: PartKind,
    rest: Transform,
}

/// A copied jetpack exhaust, shown while that avatar flies.
#[derive(Component)]
pub struct PeerExhaust;

pub fn update(
    mut commands: Commands,
    game: Res<Game>,
    style: Res<ActiveStyle>,
    time: Res<Time>,
    player: Single<Entity, With<Player>>,
    mut peers: Query<(Entity, &mut Peer, &mut Transform), (Without<PeerPart>, Without<Player>)>,
    mut parts: Query<(Entity, &PeerPart, &mut Transform), Without<Peer>>,
    parents: Query<&ChildOf>,
    mut exhausts: Query<(&ChildOf, &mut Visibility), With<PeerExhaust>>,
) {
    let me = game.player();
    let others: Vec<u16> = game.current.as_ref().map_or_else(Vec::new, |s| {
        (0..s.agents.len() as u16).filter(|&i| s.agents[i as usize].controlled && Some(i) != me).collect()
    });
    let breathe = (time.elapsed_secs() * 2.0).sin() * 0.012;
    let mut poses = Vec::new();
    let mut shown = Vec::new();
    for (entity, mut peer, mut transform) in &mut peers {
        if !others.contains(&peer.agent) || peer.style != style.current() {
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
        let airborne = current.avatar.flight.airborne;
        let speed = if airborne { 0.0 } else { (current.avatar.velocity.length().to_f32() / WALK_SPEED).min(1.4) };
        poses.push((entity, Pose { stride: peer.stride, speed, breathe, airborne, attack: None }));
    }
    for (entity, part, mut transform) in &mut parts {
        // Part pivots hang off the root or the body: find the peer above.
        let pose = parents.iter_ancestors(entity).find_map(|e| poses.iter().find(|(p, _)| *p == e).map(|(_, pose)| pose));
        if let Some(pose) = pose {
            *transform = pose.place(part.kind, part.rest);
        }
    }
    for (parent, mut visibility) in &mut exhausts {
        if let Some((_, pose)) = poses.iter().find(|(e, _)| *e == parent.parent()) {
            *visibility = if pose.airborne { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
    // The player's suit is only complete once the scene's style dresses it.
    if style.current().is_none() {
        return;
    }
    let (player, style) = (*player, style.current());
    for agent in others.into_iter().filter(|a| !shown.contains(a)) {
        commands.queue(move |world: &mut World| {
            let root = copy(world, player, None);
            world.entity_mut(root).insert(Peer { agent, style, stride: 0.0 });
        });
    }
}

/// Copies an entity and its children: their meshes, materials, transforms
/// and visibility, with part pivots and exhausts as their peer versions.
/// Lights are left out, so each peer doesn't add shadow-casting lights.
fn copy(world: &mut World, source: Entity, parent: Option<Entity>) -> Entity {
    let e = world.entity(source);
    let transform = e.get::<Transform>().copied().unwrap_or_default();
    let visibility = e.get::<Visibility>().copied().unwrap_or_default();
    let mesh = e.get::<Mesh3d>().cloned();
    let material = e.get::<MeshMaterial3d<StandardMaterial>>().cloned();
    let part = e.get::<Part>().map(|p| PeerPart { kind: p.kind, rest: p.rest });
    let exhaust = e.contains::<Exhaust>();
    let no_shadow = e.contains::<NotShadowCaster>();
    let children: Vec<Entity> = e.get::<Children>().map_or_else(Vec::new, |c| c.iter().collect());
    let mut copied = world.spawn((transform, visibility));
    if let Some(mesh) = mesh {
        copied.insert(mesh);
    }
    if let Some(material) = material {
        copied.insert(material);
    }
    if let Some(part) = part {
        copied.insert(part);
    }
    if exhaust {
        copied.insert(PeerExhaust);
    }
    if no_shadow {
        copied.insert(NotShadowCaster);
    }
    if let Some(parent) = parent {
        copied.insert(ChildOf(parent));
    }
    let id = copied.id();
    for child in children {
        if world.entity(child).contains::<PointLight>() || world.entity(child).contains::<SpotLight>() {
            continue;
        }
        copy(world, child, Some(id));
    }
    id
}
