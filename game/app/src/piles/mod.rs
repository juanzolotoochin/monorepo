//! Pile-lab input and quantity-faithful voxel views. The piles, the carrier
//! and walk-and-toss jobs are simulated (`game/sim::settlement::piles`); this
//! module mirrors them and turns clicks into commands.
use crate::{
    assets::SceneLibrary,
    gameplay::{cursor_ground, Player},
    lockstep::Game,
    pile_model::{pile_radius, unit_positions, Pile, PileId, Stock},
};
use sim::settlement::piles::PileAction;
use sim::CommandKind;
use bevy::prelude::*;
use voxel_assets::{Mineral, PileLayout};

fn mineral_name(m: Mineral) -> &'static str {
    match m {
        Mineral::AmberOre => "AMBER ORE",
        Mineral::BlueCrystal => "BLUE CRYSTAL",
    }
}
fn mineral_index(m: Mineral) -> usize {
    match m {
        Mineral::AmberOre => 0,
        Mineral::BlueCrystal => 1,
    }
}
fn mineral_color(m: Mineral) -> [f32; 3] {
    match m {
        Mineral::AmberOre => [1.0, 0.4, 0.06],
        Mineral::BlueCrystal => [0.08, 0.65, 1.0],
    }
}
fn point(p: [f32; 2]) -> Vec3 {
    Vec3::new(p[0], 0.0, p[1])
}
#[derive(Clone)]
struct LabAssets {
    cube: Handle<Mesh>,
    mineral: [Handle<StandardMaterial>; 2],
    dark: Handle<StandardMaterial>,
    trim: Handle<StandardMaterial>,
}
/// The view's copy of the simulated pile lab.
#[derive(Resource)]
pub struct PileLab {
    scene: usize,
    pub(crate) stock: Option<Stock>,
    layout: Option<PileLayout>,
    assets: Option<LabAssets>,
    /// The running job's serial, if a haul is in progress.
    pub(crate) job: Option<u64>,
    pub(crate) notice: String,
    pub(crate) last_error: Option<String>,
    pub(crate) in_flight: u32,
    pub(crate) job_serial: u64,
}
impl Default for PileLab {
    fn default() -> Self {
        Self {
            scene: usize::MAX,
            stock: None,
            layout: None,
            assets: None,
            job: None,
            notice: String::new(),
            last_error: None,
            in_flight: 0,
            job_serial: 0,
        }
    }
}
#[derive(Component)]
pub struct LabEntity;
#[derive(Component)]
pub struct MineralUnit;
#[derive(Component)]
pub struct PileVisual {
    id: PileId,
    units: u32,
    hidden: u32,
}
#[derive(Component)]
pub struct CarryVisual {
    units: u32,
    material: Option<Mineral>,
    hidden: u32,
}
#[derive(Clone, Copy)]
enum CaptionKind {
    Dispenser(usize),
    Pile(PileId),
    Cargo,
}
#[derive(Component)]
pub struct Caption {
    kind: CaptionKind,
    anchor: Vec3,
}
#[derive(Component)]
pub struct SourceButton(usize);
#[derive(Component)]
pub struct LabPanel;
#[derive(Component)]
pub struct LabStatus;
#[derive(Component)]
pub struct CapacityTick(u32);

mod demo;
#[cfg(test)]
mod tests;
mod transfer;
mod view;
pub use demo::showcase;
pub use transfer::{spawn_tosses, update_transit};
pub(crate) use transfer::{Destination, Transit};
pub use view::{captions, render, setup_scene, update_display};

pub(crate) fn mineral(material: u8) -> Mineral {
    if material == 0 {
        Mineral::AmberOre
    } else {
        Mineral::BlueCrystal
    }
}

fn ray_sphere(origin: Vec3, direction: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let offset = origin - center;
    let b = offset.dot(direction);
    let d = b * b - offset.length_squared() + radius * radius;
    if d < 0.0 {
        return None;
    }
    let t = -b - d.sqrt();
    (t >= 0.0).then_some(t)
}

/// Fills the view's piles, carrier and job state from the simulation.
pub fn mirror(game: Res<Game>, mut lab: ResMut<PileLab>) {
    let Some(sim) = game.current.as_ref().and_then(|s| s.piles.as_ref()) else {
        return;
    };
    let local = |p: fx::FxVec2| [(p.x - game.center.x).to_f32(), (p.y - game.center.y).to_f32()];
    let piles = sim
        .piles
        .iter()
        .map(|(&id, p)| (id, Pile { material: mineral(p.material), position: local(p.position), units: p.units, source: p.source }))
        .collect();
    let cargo = crate::pile_model::Cargo { material: sim.cargo.material.map(mineral), units: sim.cargo.units, capacity: sim.cargo.capacity };
    lab.stock = Some(Stock { piles, cargo, created: sim.created });
    lab.job = sim.job.as_ref().map(|_| sim.job_serial);
    lab.job_serial = sim.job_serial;
    lab.in_flight = sim.in_flight.len() as u32;
    if !sim.notice.is_empty() {
        lab.notice = sim.notice.clone();
    }
    lab.last_error = sim.last_error.clone();
}

/// Starts a haul: the player walks to the pile and transfers, `pace`
/// seconds per unit (zero: all at once).
pub(crate) fn haul(game: &mut Game, action: PileAction, pace: f32, approach: Option<Vec3>) {
    let Some(agent) = game.player() else { return };
    let approach = approach.map(|p| game.to_sim(p));
    game.command(CommandKind::Pile { agent, action, pace: fx::Fx::quantize(pace), approach });
}

pub fn input(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    window: Single<&Window>,
    source_buttons: Query<(&Interaction, &SourceButton)>,
    panels: Query<&Interaction, With<LabPanel>>,
    mut lab: ResMut<PileLab>,
    mut game: ResMut<Game>,
) {
    if lab.stock.is_none() {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if !buttons.just_pressed(MouseButton::Left) || shift {
        return;
    }
    let source_button = source_buttons.iter().find(|(i, _)| **i == Interaction::Pressed).map(|(_, b)| b.0);
    if let Some(index) = source_button {
        game.command(CommandKind::Dispense { dispenser: index as u32, units: 5 });
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if panels.iter().any(|i| *i != Interaction::None) {
        return;
    }
    let Ok(ray) = camera.0.viewport_to_world(camera.1, cursor) else {
        return;
    };
    let source = lab
        .layout
        .as_ref()
        .unwrap()
        .dispensers
        .iter()
        .enumerate()
        .filter_map(|(index, d)| ray_sphere(ray.origin, *ray.direction, Vec3::from_array(d.position) + Vec3::Y * 0.9, 0.9).map(|t| (t, index)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index)| index);
    if let Some(index) = source {
        game.command(CommandKind::Dispense { dispenser: index as u32, units: 5 });
        return;
    }
    if cursor.y < 110.0 || cursor.y > window.height() - 135.0 {
        return;
    }
    let Some(ground) = cursor_ground(camera.0, camera.1, &window) else {
        return;
    };
    let stock = lab.stock.as_ref().unwrap();
    let pile = stock
        .piles
        .iter()
        .filter_map(|(&id, p)| {
            let radius = pile_radius(p.units);
            ray_sphere(ray.origin, *ray.direction, point(p.position) + Vec3::Y * 0.3, radius.max(0.55)).map(|t| (t, id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id);
    let carrying = stock.cargo.units > 0;
    let collect = !carrying || keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let action = if let Some(id) = pile {
        let p = &stock.piles[&id];
        if carrying && stock.cargo.material != Some(p.material) {
            lab.notice = "Different minerals stay separate. Drop your cargo first.".into();
            return;
        }
        if !carrying && p.units == 0 {
            lab.notice = "Empty output. Click its dispenser to add ore.".into();
            return;
        }
        if collect {
            PileAction::Pickup(id)
        } else {
            PileAction::Deposit(id)
        }
    } else if carrying {
        PileAction::Drop(game.to_sim(ground))
    } else {
        lab.notice = "Click a pile to collect it, or right click to move.".into();
        return;
    };
    haul(&mut game, action, 0.18, None);
}
