//! Tossed units: each simulated toss becomes a unit flying between endpoints.
use super::*;
use crate::sound::{Cue, Sound};
use sim::settlement::TossEnd;
#[derive(Clone, Copy)]
pub enum Destination {
    Cargo(u32),
    Pile(PileId, u32),
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

/// Spawns a flying unit for every toss the simulation made since last frame.
pub fn spawn_tosses(
    mut commands: Commands,
    game: Res<Game>,
    lab: Res<PileLab>,
    sound: Res<Sound>,
    player: Single<&Transform, With<Player>>,
) {
    let Some(assets) = lab.assets.clone() else {
        return;
    };
    for event in &game.events {
        let sim::settlement::Event::PileToss { material, from, to, ticks } = event else {
            continue;
        };
        let start = match *from {
            TossEnd::Pile { slot, position, .. } => {
                let p = game.to_view(position);
                p + Vec3::from_array(unit_positions()[slot as usize])
            }
            TossEnd::Cargo { slot } => player.transform_point(backpack_slot(slot)),
        };
        let destination = match *to {
            TossEnd::Pile { id, slot, .. } => Destination::Pile(id, slot),
            TossEnd::Cargo { slot } => Destination::Cargo(slot),
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
    player: Single<&Transform, (With<Player>, Without<Transit>)>,
    mut transits: Query<(Entity, &mut Transit, &mut Transform), Without<Player>>,
) {
    let Some(stock) = &lab.stock else {
        return;
    };
    for (entity, mut transit, mut transform) in &mut transits {
        transit.age += time.delta_secs().min(0.1);
        let t = (transit.age / transit.duration).min(1.0);
        let end = match transit.destination {
            Destination::Cargo(slot) => player.transform_point(backpack_slot(slot)),
            Destination::Pile(id, slot) => {
                let Some(pile) = stock.piles.get(&id) else {
                    commands.entity(entity).despawn();
                    continue;
                };
                point(pile.position) + Vec3::from_array(unit_positions()[slot as usize])
            }
        };
        transform.translation = transit.start.lerp(end, t) + Vec3::Y * (std::f32::consts::PI * t).sin() * 1.5;
        transform.rotation = Quat::from_rotation_y(t * std::f32::consts::TAU) * Quat::from_rotation_z((t * std::f32::consts::PI).sin() * 0.6);
        if t >= 1.0 {
            commands.entity(entity).despawn();
            sound.play(Cue::Land);
        }
    }
}
