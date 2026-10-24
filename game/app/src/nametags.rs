//! Name tags over every player's avatar in a networked game: the name they
//! gave (`--name`), or "PLAYER n" until it arrives. Yours is tinted.

use bevy::prelude::*;

use crate::gameplay::Player;
use crate::lockstep::{SessionService, WorldMirror};
use crate::peers::Peer;

#[derive(Component)]
pub struct NameTag {
    agent: u16,
}

/// Height of a tag above the feet, in avatar heights (before style scale).
const ABOVE: f32 = 2.15;

pub fn update(
    mut commands: Commands,
    service: Res<SessionService>,
    mirror: Res<WorldMirror>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    player: Single<&GlobalTransform, With<Player>>,
    peers: Query<(&Peer, &GlobalTransform)>,
    mut tags: Query<(Entity, &NameTag, &mut Text, &mut TextColor, &mut Node, &mut Visibility, &ComputedNode)>,
) {
    let me = mirror.player();
    let players: Vec<(u16, String)> = match (&mirror.current, service.networked()) {
        (Some(current), true) => current
            .agents
            .iter()
            .enumerate()
            .filter(|(_, a)| a.controlled)
            .map(|(i, a)| {
                let number = a.owner.map_or(1, |p| p.0 + 1);
                let name = if a.name.is_empty() { format!("PLAYER {number}") } else { a.name.clone() };
                (i as u16, name)
            })
            .collect(),
        _ => Vec::new(),
    };
    let head = |agent: u16| {
        let body = if Some(agent) == me { Some(*player) } else { peers.iter().find(|(p, _)| p.agent() == agent).map(|(_, t)| t) }?;
        let body = body.compute_transform();
        Some(body.translation + Vec3::Y * ABOVE * body.scale.y)
    };
    let mut tagged = Vec::new();
    for (entity, tag, mut text, mut color, mut node, mut visibility, computed) in &mut tags {
        let Some((_, name)) = players.iter().find(|(agent, _)| *agent == tag.agent) else {
            commands.entity(entity).despawn();
            continue;
        };
        tagged.push(tag.agent);
        if text.0 != *name {
            text.0 = name.clone();
        }
        color.0 = if Some(tag.agent) == me { Color::srgb(0.45, 0.92, 1.0) } else { Color::srgb(0.97, 0.93, 0.82) };
        let Some(v) = head(tag.agent).and_then(|p| camera.0.world_to_viewport(camera.1, p).ok()) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        // Centered over the head, sitting on it.
        let size = computed.size() * computed.inverse_scale_factor();
        node.left = px(v.x - size.x / 2.0);
        node.top = px(v.y - size.y);
        *visibility = if size.x > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
    }
    for (agent, name) in players.into_iter().filter(|(agent, _)| !tagged.contains(agent)) {
        commands.spawn((
            NameTag { agent },
            Text::new(name),
            TextFont { font_size: FontSize::Px(13.), ..default() },
            TextColor(Color::WHITE),
            BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.6)),
            Node { position_type: PositionType::Absolute, padding: UiRect::axes(px(6.), px(2.)), ..default() },
            Visibility::Hidden,
        ));
    }
}
