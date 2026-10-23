//! One-at-a-time transfers with a conserved visual unit flying between endpoints.
use super::*;
use crate::sound::{Cue, Sound};
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

pub(super) fn step(
    commands: &mut Commands,
    sound: &Sound,
    lab: &mut PileLab,
    job: &mut Job,
    player: &Transform,
    dt: f32,
    obstacles: &[Disc],
) -> Result<(), &'static str> {
    if job.remaining.is_none() {
        let stock = lab.stock.as_ref().unwrap();
        let amount = match job.action {
            Action::Pickup(id) => stock
                .piles
                .get(&id)
                .ok_or("Pile disappeared")?
                .units
                .min(stock.cargo.capacity - stock.cargo.units),
            Action::PickupAmount(id, n) => {
                let p = stock.piles.get(&id).ok_or("Pile disappeared")?;
                if p.units < n || stock.cargo.capacity - stock.cargo.units < n {
                    return Err("Exact pickup quantity is no longer available");
                }
                n
            }
            Action::Deposit(id) => stock.cargo.units.min(
                voxel_assets::PILE_CAPACITY - stock.piles.get(&id).ok_or("Pile disappeared")?.units,
            ),
            Action::Drop(p) => {
                if !free_drop(p, stock.cargo.units, obstacles) {
                    return Err("Drop location is blocked");
                }
                stock.cargo.units
            }
        };
        if amount == 0 {
            return Err("Nothing can be transferred");
        }
        job.remaining = Some(amount);
        job.planned = amount;
    }
    job.clock += dt;
    if job.clock < job.pace {
        return Ok(());
    }
    job.clock = 0.0;
    let assets = lab.assets.as_ref().unwrap().clone();
    let stock = lab.stock.as_mut().unwrap();
    let (material, start, destination) = match job.action {
        Action::Pickup(id) | Action::PickupAmount(id, _) => {
            let pile = stock.piles.get(&id).ok_or("Pile disappeared")?;
            let material = pile.material;
            let start = point(pile.position)
                + Vec3::from_array(
                    unit_positions()[pile.units.checked_sub(1).ok_or("Pile is empty")? as usize],
                );
            let slot = stock.cargo.units;
            stock.pickup_amount(id, 1)?;
            (material, start, Destination::Cargo(slot))
        }
        Action::Deposit(id) => {
            let material = stock.cargo.material.ok_or("Carrier is empty")?;
            let start = player.transform_point(backpack_slot(stock.cargo.units - 1));
            let slot = stock.piles.get(&id).ok_or("Pile disappeared")?.units;
            stock.deposit_amount(id, 1)?;
            (material, start, Destination::Pile(id, slot))
        }
        Action::Drop(p) => {
            let material = stock.cargo.material.ok_or("Carrier is empty")?;
            let start = player.transform_point(backpack_slot(stock.cargo.units - 1));
            let (id, slot) = if let Some(id) = job.destination {
                let slot = stock.piles.get(&id).ok_or("Pile disappeared")?.units;
                stock.deposit_amount(id, 1)?;
                (id, slot)
            } else {
                let id = stock.drop_new_amount([p.x, p.z], 1)?;
                job.destination = Some(id);
                (id, 0)
            };
            (material, start, Destination::Pile(id, slot))
        }
    };
    commands.spawn((
        LabEntity,
        MineralUnit,
        Transit {
            destination,
            start,
            age: 0.0,
            duration: job.pace.clamp(0.3, 1.0) * 0.85,
        },
        Mesh3d(assets.cube),
        MeshMaterial3d(assets.mineral[mineral_index(material)].clone()),
        Transform::from_translation(start).with_scale(Vec3::new(0.24, 0.21, 0.24)),
    ));
    lab.in_flight += 1;
    job.transferred += 1;
    *job.remaining.as_mut().unwrap() -= 1;
    lab.notice = format!("Tossing {} / {} units...", job.transferred, job.planned);
    sound.play(Cue::Toss);
    Ok(())
}
pub fn update_transit(
    mut commands: Commands,
    time: Res<Time>,
    sound: Res<Sound>,
    mut lab: ResMut<PileLab>,
    player: Single<&Transform, (With<Player>, Without<Transit>)>,
    mut transits: Query<(Entity, &mut Transit, &mut Transform), Without<Player>>,
) {
    let Some(stock) = &lab.stock else {
        return;
    };
    let mut landed = 0;
    for (entity, mut transit, mut transform) in &mut transits {
        transit.age += time.delta_secs().min(0.1);
        let t = (transit.age / transit.duration).min(1.0);
        let end = match transit.destination {
            Destination::Cargo(slot) => player.transform_point(backpack_slot(slot)),
            Destination::Pile(id, slot) => {
                let Some(pile) = stock.piles.get(&id) else {
                    commands.entity(entity).despawn();
                    landed += 1;
                    continue;
                };
                point(pile.position) + Vec3::from_array(unit_positions()[slot as usize])
            }
        };
        transform.translation =
            transit.start.lerp(end, t) + Vec3::Y * (std::f32::consts::PI * t).sin() * 1.5;
        transform.rotation = Quat::from_rotation_y(t * std::f32::consts::TAU)
            * Quat::from_rotation_z((t * std::f32::consts::PI).sin() * 0.6);
        if t >= 1.0 {
            commands.entity(entity).despawn();
            landed += 1;
            sound.play(Cue::Land);
        }
    }
    lab.in_flight = lab.in_flight.saturating_sub(landed);
}
