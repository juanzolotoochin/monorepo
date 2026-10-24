//! Tossed units: each simulated toss becomes a unit flying between endpoints.
use super::*;
use crate::sound::{Cue, Sound};
use sim::settlement::TossEnd;
#[derive(Clone, Copy)]
pub enum Destination {
    /// A slot in an avatar's backpack.
    Cargo(u16, u32),
    Pile(PileId, u32),
    /// Into the collector's hopper, here.
    Collector(Vec3),
}
#[derive(Component)]
pub struct Transit {
    pub destination: Destination,
    start: Vec3,
    age: f32,
    duration: f32,
}
pub fn backpack_slot(index: u32) -> Vec3 {
    Vec3::new(
        (index % 3) as f32 * 0.25 - 0.25,
        0.77 + ((index / 3) % 2) as f32 * 0.24,
        -0.44 - (index / 6) as f32 * 0.24,
    )
}

/// Where an avatar's body is: this player's, or another player's copy.
fn body<F: bevy::ecs::query::QueryFilter>(mirror: &WorldMirror, player: &Transform, peers: &Query<(&crate::peers::Peer, &Transform), F>, agent: u16) -> Option<Transform> {
    if mirror.player() == Some(agent) {
        return Some(*player);
    }
    peers.iter().find(|(p, _)| p.agent() == agent).map(|(_, t)| *t)
}

/// Spawns a flying unit for every toss the simulation made since last frame.
pub fn spawn_tosses(
    mut commands: Commands,
    service: Res<SessionService>,
    mirror: Res<WorldMirror>,
    lab: Res<PileLab>,
    sound: Res<Sound>,
    player: Single<&Transform, With<Player>>,
    peers: Query<(&crate::peers::Peer, &Transform), Without<Player>>,
) {
    let Some(assets) = lab.assets.clone() else {
        return;
    };
    let body = |agent: u16| body(&mirror, &player, &peers, agent);
    for event in &mirror.events {
        let sim::settlement::Event::PileToss { material, from, to, ticks } = event else {
            continue;
        };
        let ground = |p: fx::FxVec2| mirror.to_view(p).with_y(service.world().map_or(0.0, |w| w.terrain().height(p).to_f32()));
        let start = match *from {
            TossEnd::Pile { slot, position, .. } => ground(position) + Vec3::from_array(unit_positions()[slot as usize]),
            TossEnd::Cargo { agent, slot } => {
                let Some(body) = body(agent) else { continue };
                body.transform_point(backpack_slot(slot))
            }
            TossEnd::Pit { position } => ground(position) + Vec3::Y * 0.2,
            TossEnd::Collector { position } => ground(position) + Vec3::Y * 1.6,
        };
        let destination = match *to {
            TossEnd::Pile { id, slot, .. } => Destination::Pile(id, slot),
            TossEnd::Cargo { agent, slot } => Destination::Cargo(agent, slot),
            TossEnd::Collector { position } => Destination::Collector(ground(position) + Vec3::Y * 1.6),
            TossEnd::Pit { position } => Destination::Collector(ground(position)),
        };
        commands.spawn((
            LabEntity,
            MineralUnit,
            Transit { destination, start, age: 0.0, duration: *ticks as f32 / crate::legacy::HZ },
            Mesh3d(assets.cube.clone()),
            MeshMaterial3d(assets.mineral[mineral_index(mineral(*material))].clone()),
            Transform::from_translation(start).with_scale(Vec3::new(0.24, 0.21, 0.24)),
        ));
        sound.play(Cue::Toss);
    }
}

pub fn update_transit(
    mut commands: Commands,
    time: Res<Time>,
    sound: Res<Sound>,
    lab: Res<PileLab>,
    mirror: Res<WorldMirror>,
    player: Single<&Transform, (With<Player>, Without<Transit>)>,
    peers: Query<(&crate::peers::Peer, &Transform), (Without<Player>, Without<Transit>)>,
    mut transits: Query<(Entity, &mut Transit, &mut Transform), (Without<Player>, Without<crate::peers::Peer>)>,
) {
    let Some(stock) = &lab.stock else {
        return;
    };
    for (entity, mut transit, mut transform) in &mut transits {
        transit.age += time.delta_secs().min(0.1);
        let t = (transit.age / transit.duration).min(1.0);
        let end = match transit.destination {
            Destination::Cargo(agent, slot) => {
                let Some(body) = body(&mirror, &player, &peers, agent) else {
                    commands.entity(entity).despawn();
                    continue;
                };
                body.transform_point(backpack_slot(slot))
            }
            Destination::Pile(id, slot) => {
                let Some(pile) = stock.piles.get(&id) else {
                    commands.entity(entity).despawn();
                    continue;
                };
                place(pile) + Vec3::from_array(unit_positions()[slot as usize])
            }
            Destination::Collector(at) => at,
        };
        transform.translation = transit.start.lerp(end, t) + Vec3::Y * (std::f32::consts::PI * t).sin() * 1.5;
        transform.rotation = Quat::from_rotation_y(t * std::f32::consts::TAU) * Quat::from_rotation_z((t * std::f32::consts::PI).sin() * 0.6);
        if t >= 1.0 {
            commands.entity(entity).despawn();
            sound.play(Cue::Land);
        }
    }
}
