//! Name tags over every player's avatar in a networked game: the name they
//! gave (`--name`), or "PLAYER n" until it arrives. Yours is tinted.

use bevy::prelude::*;

use crate::character::Player;
use crate::lockstep::{SessionService, WorldMirror};
use crate::peers::Peer;

#[derive(Component)]
pub struct NameTag {
    agent: u16,
}
#[derive(Component)]
pub(crate) struct HealthBar { agent: u16 }
#[derive(Component)]
pub(crate) struct HealthFill { agent: u16 }

/// Height of a tag above the feet, in avatar heights (before style scale).
const ABOVE: f32 = 2.15;

pub fn update(
    mut commands: Commands,
    service: Res<SessionService>,
    mirror: Res<WorldMirror>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    player: Single<&GlobalTransform, With<Player>>,
    peers: Query<(&Peer, &GlobalTransform)>,
    mut tags: Query<(Entity, &NameTag, &mut Text, &mut TextColor, &mut Node, &mut Visibility, &ComputedNode), (Without<HealthBar>, Without<HealthFill>)>,
    mut bars: Query<(Entity, &HealthBar, &mut Node, &mut Visibility), (Without<NameTag>, Without<HealthFill>)>,
    mut fills: Query<(&HealthFill, &mut Node, &mut BackgroundColor), (Without<NameTag>, Without<HealthBar>)>,
) {
    let me = mirror.player();
    let players: Vec<(u16, String, u32, u32)> = match (&mirror.current, service.networked() || service.settlement().is_some_and(|s| s.player_health > 0)) {
        (Some(current), true) => current
            .agents
            .iter()
            .enumerate()
            .filter(|(_, a)| a.controlled)
            .map(|(i, a)| {
                let number = a.owner.map_or(1, |p| p.0 + 1);
                let name = if a.name.is_empty() { format!("PLAYER {number}") } else { a.name.clone() };
                (i as u16, name, a.avatar.health, a.avatar.max_health)
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
        let Some((_, name, _, _)) = players.iter().find(|(agent, ..)| *agent == tag.agent) else {
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
    for (entity, bar, mut node, mut visibility) in &mut bars {
        let Some((_, _, health, max_health)) = players.iter().find(|(agent, _, _, max)| *agent == bar.agent && *max > 0) else {
            commands.entity(entity).despawn();
            continue;
        };
        let Some(at) = head(bar.agent).and_then(|p| camera.0.world_to_viewport(camera.1, p).ok()) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        node.left = px(at.x - 34.0);
        node.top = px(at.y + 2.0);
        *visibility = Visibility::Inherited;
        for (fill, mut width, mut color) in &mut fills {
            if fill.agent == bar.agent {
                width.width = percent(100.0 * *health as f32 / *max_health as f32);
                color.0 = if *health * 3 <= *max_health { Color::srgb(0.95, 0.3, 0.2) } else { Color::srgb(0.25, 0.85, 0.42) };
            }
        }
    }
    let tagged_bars: Vec<u16> = bars.iter().map(|(_, bar, ..)| bar.agent).collect();
    for (agent, name, _, max_health) in players.into_iter().filter(|(agent, ..)| !tagged.contains(agent)) {
        commands.spawn((
            NameTag { agent },
            service::WorldScoped::default(),
            Text::new(name),
            TextFont { font_size: FontSize::Px(13.), ..default() },
            TextColor(Color::WHITE),
            BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.6)),
            Node { position_type: PositionType::Absolute, padding: UiRect::axes(px(6.), px(2.)), ..default() },
            Visibility::Hidden,
        ));
        if max_health > 0 && !tagged_bars.contains(&agent) {
            commands.spawn((
                HealthBar { agent },
                service::WorldScoped::default(),
                BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.8)),
                Node { position_type: PositionType::Absolute, width: px(68.0), height: px(7.0), padding: UiRect::all(px(1.0)), ..default() },
                Visibility::Hidden,
            )).with_children(|parent| {
                parent.spawn((HealthFill { agent }, BackgroundColor(Color::srgb(0.25, 0.85, 0.42)), Node { width: percent(100.0), height: percent(100.0), ..default() }));
            });
        }
    }
}
