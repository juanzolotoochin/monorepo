//! Pile-lab interaction, travel-to-transfer jobs and quantity-faithful voxel views.
use crate::{
    assets::SceneLibrary,
    gameplay::{cursor_ground, Controls, Obstacle, Player},
    navigation::{self, flat, Disc, RADIUS},
    pile_model::{pile_radius, unit_positions, PileId, Stock},
};
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
#[derive(Clone, Copy, Debug)]
enum Action {
    Pickup(PileId),
    Deposit(PileId),
    Drop(Vec3),
}
#[derive(Clone, Copy)]
struct Job {
    action: Action,
    center: Vec3,
    age: f32,
    stalled: f32,
    previous: Vec3,
}
#[derive(Resource)]
pub struct PileLab {
    scene: usize,
    stock: Option<Stock>,
    layout: Option<PileLayout>,
    assets: Option<LabAssets>,
    job: Option<Job>,
    notice: String,
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
}
#[derive(Component)]
pub struct CarryVisual {
    units: u32,
    material: Option<Mineral>,
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
mod view;
pub use demo::showcase;
pub use view::{captions, render, setup_scene, update_display};

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
fn approach_route(start: Vec3, center: Vec3, radius: f32, obstacles: &[Disc]) -> Option<Vec<Vec3>> {
    let distance = radius + RADIUS + 0.18;
    let mut candidates = Vec::new();
    let gap = flat(start - center).length();
    if gap >= radius + RADIUS + 0.04 && gap <= distance + 0.25 {
        return Some(vec![]);
    }
    for i in 0..24 {
        let angle = i as f32 * std::f32::consts::TAU / 24.0;
        let goal = center + Vec3::new(angle.cos(), 0.0, angle.sin()) * distance;
        let route = navigation::path(start, goal, obstacles);
        if !route.is_empty() {
            let mut previous = start;
            let cost: f32 = route
                .iter()
                .map(|&p| {
                    let d = previous.distance(p);
                    previous = p;
                    d
                })
                .sum();
            candidates.push((cost, route));
        }
    }
    candidates
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, route)| route)
}
fn free_drop(center: Vec3, units: u32, obstacles: &[Disc]) -> bool {
    let radius = pile_radius(units);
    center.x.abs() <= 6.5
        && center.z.abs() <= 6.5
        && obstacles
            .iter()
            .all(|o| flat(o.center - center).length() > o.radius + radius + 0.2)
}

pub fn input(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    window: Single<&Window>,
    player: Single<&Transform, With<Player>>,
    obstacles: Query<&Obstacle>,
    source_buttons: Query<(&Interaction, &SourceButton)>,
    panels: Query<&Interaction, With<LabPanel>>,
    mut lab: ResMut<PileLab>,
    mut controls: ResMut<Controls>,
) {
    if lab.stock.is_none() {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let manual = buttons.pressed(MouseButton::Right)
        || (buttons.just_pressed(MouseButton::Left) && shift)
        || [
            KeyCode::KeyW,
            KeyCode::KeyA,
            KeyCode::KeyS,
            KeyCode::KeyD,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::KeyR,
        ]
        .iter()
        .any(|key| keys.pressed(*key));
    if manual && lab.job.take().is_some() {
        controls.route.clear();
        lab.notice = "Haul canceled. Cargo is retained.".into();
    }
    if !buttons.just_pressed(MouseButton::Left) || shift {
        return;
    }
    let source_button = source_buttons
        .iter()
        .find(|(i, _)| **i == Interaction::Pressed)
        .map(|(_, b)| b.0);
    if let Some(index) = source_button {
        let result = lab.stock.as_mut().unwrap().add(index as PileId, 5);
        lab.notice = match result {
            Ok(n) => format!("Dispenser added {n} units to its output pile."),
            Err(e) => e.into(),
        };
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
    let source = source_button.or_else(|| {
        lab.layout
            .as_ref()
            .unwrap()
            .dispensers
            .iter()
            .enumerate()
            .filter_map(|(index, d)| {
                ray_sphere(
                    ray.origin,
                    *ray.direction,
                    Vec3::from_array(d.position) + Vec3::Y * 0.9,
                    0.9,
                )
                .map(|t| (t, index))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, index)| index)
    });
    if let Some(index) = source {
        let result = lab.stock.as_mut().unwrap().add(index as PileId, 5);
        lab.notice = match result {
            Ok(n) => format!("Dispenser added {n} units to its output pile."),
            Err(e) => e.into(),
        };
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
            ray_sphere(
                ray.origin,
                *ray.direction,
                point(p.position) + Vec3::Y * 0.3,
                radius.max(0.55),
            )
            .map(|t| (t, id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id);
    let carrying = stock.cargo.units > 0;
    let collect =
        !carrying || keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
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
            Action::Pickup(id)
        } else {
            Action::Deposit(id)
        }
    } else if carrying {
        Action::Drop(flat(ground))
    } else {
        lab.notice = "Click a pile to collect it, or right click to move.".into();
        return;
    };
    let discs: Vec<_> = obstacles.iter().map(|o| o.0).collect();
    if let Err(message) = begin_job(&mut lab, &mut controls, player.translation, action, &discs) {
        lab.notice = message.into();
    }
}

fn action_target(stock: &Stock, action: Action) -> Result<(Vec3, f32), &'static str> {
    match action {
        Action::Pickup(id) | Action::Deposit(id) => {
            let p = stock.piles.get(&id).ok_or("Pile no longer exists")?;
            let units = if matches!(action, Action::Deposit(_)) {
                (p.units + stock.cargo.units).min(voxel_assets::PILE_CAPACITY)
            } else {
                p.units
            };
            Ok((point(p.position), pile_radius(units)))
        }
        Action::Drop(p) => Ok((p, pile_radius(stock.cargo.units))),
    }
}
fn begin_job(
    lab: &mut PileLab,
    controls: &mut Controls,
    player: Vec3,
    action: Action,
    obstacles: &[Disc],
) -> Result<(), &'static str> {
    lab.job = None;
    controls.route.clear();
    let stock = lab.stock.as_ref().ok_or("Pile lab is inactive")?;
    let (center, radius) = action_target(stock, action)?;
    if matches!(action, Action::Drop(_)) && !free_drop(center, stock.cargo.units, obstacles) {
        return Err("Choose open floor away from piles, machines and the deck edge");
    }
    if matches!(action, Action::Pickup(_)) && stock.cargo.units == stock.cargo.capacity {
        return Err("Carrier is full. Deposit before collecting more");
    }
    let route = approach_route(player, center, radius, obstacles)
        .ok_or("No clear route to that pile location")?;
    controls.route = route;
    controls.attack = None;
    controls.queued = None;
    lab.job = Some(Job {
        action,
        center,
        age: 0.0,
        stalled: 0.0,
        previous: player,
    });
    lab.notice = if matches!(action, Action::Pickup(_)) {
        "Walking to collect mineral…"
    } else {
        "Walking to deposit cargo…"
    }
    .into();
    Ok(())
}

pub fn arrive(
    time: Res<Time>,
    player: Single<&Transform, With<Player>>,
    obstacles: Query<&Obstacle>,
    mut lab: ResMut<PileLab>,
    mut controls: ResMut<Controls>,
) {
    let Some(mut job) = lab.job else {
        return;
    };
    if controls.attack.is_some() {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    job.age += dt;
    job.stalled = if flat(player.translation - job.previous).length() < 0.003 {
        job.stalled + dt
    } else {
        0.0
    };
    job.previous = player.translation;
    let stock = lab.stock.as_ref().unwrap();
    let Ok((_, radius)) = action_target(stock, job.action) else {
        lab.job = None;
        controls.route.clear();
        lab.notice = "That pile no longer exists. Cargo retained.".into();
        return;
    };
    let distance = flat(player.translation - job.center).length();
    if distance >= radius + RADIUS + 0.02 && distance <= radius + RADIUS + 0.45 {
        let discs: Vec<_> = obstacles.iter().map(|o| o.0).collect();
        let stock = lab.stock.as_mut().unwrap();
        let result = match job.action {
            Action::Pickup(id) => stock.pickup(id).map(|n| {
                format!(
                    "Collected {n} units. Carrying {}/{}.",
                    stock.cargo.units, stock.cargo.capacity
                )
            }),
            Action::Deposit(id) => stock
                .deposit(id)
                .map(|n| format!("Deposited {n} units. {} still carried.", stock.cargo.units)),
            Action::Drop(p) => {
                if free_drop(p, stock.cargo.units, &discs) {
                    let n = stock.cargo.units;
                    stock
                        .drop_new([p.x, p.z])
                        .map(|_| format!("Placed a new pile of {n} units."))
                } else {
                    Err("Drop location is now blocked. Cargo retained.")
                }
            }
        };
        lab.notice = result.unwrap_or_else(str::to_string);
        lab.job = None;
        controls.route.clear();
        controls.velocity = Vec3::ZERO;
        return;
    }
    if job.age > 30.0 {
        lab.job = None;
        controls.route.clear();
        lab.notice = "Haul timed out. Cargo retained; choose another approach.".into();
        return;
    }
    if controls.route.is_empty() || job.stalled > 0.8 {
        let discs: Vec<_> = obstacles.iter().map(|o| o.0).collect();
        if let Some(route) = approach_route(player.translation, job.center, radius, &discs) {
            controls.route = route;
            job.stalled = 0.0;
        } else {
            lab.job = None;
            controls.route.clear();
            lab.notice = "Route blocked. Cargo retained.".into();
            return;
        }
    }
    lab.job = Some(job);
}
