//! Name tags over every player's avatar in a networked game: the name they
//! gave (`--name`), or "PLAYER n" until it arrives, in their slot's color.
//! Yours has a border in it too.

use bevy::prelude::*;

/// A color per player slot, so up to 16 players can tell each other apart:
/// the Okabe-Ito colors, which stay distinct for common color blindness,
/// lightened to read on the tags' dark backing, then paler versions of them.
const SLOT_COLORS: [(f32, f32, f32); 16] = [
    (0.45, 0.92, 1.0),
    (0.90, 0.62, 0.0),
    (0.94, 0.89, 0.26),
    (0.20, 0.80, 0.60),
    (0.84, 0.37, 0.0),
    (0.80, 0.47, 0.65),
    (0.40, 0.62, 0.95),
    (0.97, 0.93, 0.82),
    (0.70, 0.96, 1.0),
    (1.0, 0.80, 0.45),
    (1.0, 0.96, 0.60),
    (0.60, 0.95, 0.80),
    (1.0, 0.62, 0.42),
    (0.95, 0.72, 0.88),
    (0.68, 0.80, 1.0),
    (0.80, 0.80, 0.80),
];

/// The color of `player`'s slot.
pub fn slot_color(player: sim::PlayerId) -> Color {
    let (r, g, b) = SLOT_COLORS[player.0 as usize % SLOT_COLORS.len()];
    Color::srgb(r, g, b)
}

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
    mut tags: Query<(Entity, &NameTag, &mut Text, &mut TextColor, &mut BorderColor, &mut Node, &mut Visibility, &ComputedNode), (Without<HealthBar>, Without<HealthFill>)>,
    mut bars: Query<(Entity, &HealthBar, &mut Node, &mut Visibility), (Without<NameTag>, Without<HealthFill>)>,
    mut fills: Query<(&HealthFill, &mut Node, &mut BackgroundColor), (Without<NameTag>, Without<HealthBar>)>,
) {
    let me = mirror.player();
    let players: Vec<(u16, String, u32, u32, sim::PlayerId)> = match (&mirror.current, service.networked() || service.settlement().is_some_and(|s| s.player_health > 0)) {
        (Some(current), true) => current
            .agents
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                let avatar = a.avatar()?;
                let owner = avatar.owner.unwrap_or(sim::PlayerId(0));
                let name = if avatar.name.is_empty() { format!("PLAYER {}", owner.0 + 1) } else { avatar.name.clone() };
                Some((i as u16, name, a.body.health, a.body.max_health, owner))
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
    for (entity, tag, mut text, mut color, mut border, mut node, mut visibility, computed) in &mut tags {
        let Some((_, name, _, _, owner)) = players.iter().find(|(agent, ..)| *agent == tag.agent) else {
            commands.entity(entity).despawn();
            continue;
        };
        tagged.push(tag.agent);
        if text.0 != *name {
            text.0 = name.clone();
        }
        color.0 = slot_color(*owner);
        let mine = Some(tag.agent) == me;
        node.border = UiRect::all(px(if mine { 1.0 } else { 0.0 }));
        *border = BorderColor::all(slot_color(*owner));
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
        let Some((_, _, health, max_health, _)) = players.iter().find(|(agent, _, _, max, _)| *agent == bar.agent && *max > 0) else {
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
    for (agent, name, _, max_health, _) in players.into_iter().filter(|(agent, ..)| !tagged.contains(agent)) {
        commands.spawn((
            NameTag { agent },
            service::WorldScoped::default(),
            Text::new(name),
            TextFont { font_size: FontSize::Px(13.), ..default() },
            TextColor(Color::WHITE),
            BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.6)),
            BorderColor::all(Color::NONE),
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

#[cfg(test)]
mod tests {
    use super::SLOT_COLORS;

    /// Every slot's color is clearly unlike every other's, so 16 players can
    /// tell each other apart.
    #[test]
    fn slot_colors_are_distinct() {
        for (i, a) in SLOT_COLORS.iter().enumerate() {
            for (j, b) in SLOT_COLORS.iter().enumerate().skip(i + 1) {
                let d = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt();
                assert!(d > 0.15, "slots {i} and {j} are too alike ({d:.2})");
            }
        }
    }
}
