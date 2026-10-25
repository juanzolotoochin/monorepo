//! Pile-lab input and quantity-faithful voxel views. The piles, the carrier
//! and walk-and-toss jobs are simulated (`kestrel/sim::settlement::piles`); this
//! module mirrors them and turns clicks into commands.
use crate::{
    character::Player, combat::Obstacle, gameplay::cursor_ground,
    navigation::Disc,
    lockstep::{SessionService, WorldMirror},
    pile_model::{pile_radius, unit_positions, Pile, PileId, Stock},
};
use sim::settlement::piles::PileAction;
use sim::CommandKind;
use bevy::prelude::*;
use service::{AppServices, Phase, Service, WorldLayer, WorldScoped, WorldState};
use voxel_assets::{Mineral, PileLayout};

fn mineral_name(m: Mineral) -> &'static str {
    match m {
        Mineral::AmberOre => "AMBER ORE",
        Mineral::BlueCrystal => "BLUE CRYSTAL",
        Mineral::GreenMalachite => "GREEN MALACHITE",
    }
}
fn mineral_index(m: Mineral) -> usize {
    match m {
        Mineral::AmberOre => 0,
        Mineral::BlueCrystal => 1,
        Mineral::GreenMalachite => 2,
    }
}
pub(crate) fn mineral_color(m: Mineral) -> [f32; 3] {
    match m {
        Mineral::AmberOre => [1.0, 0.4, 0.06],
        Mineral::BlueCrystal => [0.08, 0.65, 1.0],
        Mineral::GreenMalachite => [0.12, 0.9, 0.35],
    }
}
const MINERALS: [Mineral; 3] = [Mineral::AmberOre, Mineral::BlueCrystal, Mineral::GreenMalachite];
fn point(p: [f32; 2]) -> Vec3 {
    Vec3::new(p[0], 0.0, p[1])
}
/// Where a pile sits, on the ground under it.
fn place(p: &Pile) -> Vec3 {
    Vec3::new(p.position[0], p.height, p.position[1])
}
/// The lab's mesh and materials, made once and shared by every world.
#[derive(Resource, Clone)]
struct LabAssets {
    cube: Handle<Mesh>,
    mineral: [Handle<StandardMaterial>; 3],
    dark: Handle<StandardMaterial>,
    trim: Handle<StandardMaterial>,
}
/// The view's copy of the simulated pile lab (World layer).
#[derive(Resource)]
pub struct PileLab {
    pub(crate) stock: Option<Stock>,
    layout: Option<PileLayout>,
    assets: Option<LabAssets>,
    /// The running job's serial, if a haul is in progress.
    pub(crate) job: Option<u64>,
    pub(crate) notice: String,
    pub(crate) last_error: Option<String>,
    pub(crate) in_flight: u32,
    pub(crate) job_serial: u64,
    /// Where the collector stands (on the ground), and what it holds by mineral.
    pub(crate) collector: Option<Vec3>,
    pub(crate) collected: Vec<u32>,
    /// This player's avatar, and what everyone else carries.
    pub(crate) me: Option<u16>,
    pub(crate) others: std::collections::BTreeMap<u16, crate::pile_model::Cargo>,
}
impl Default for PileLab {
    fn default() -> Self {
        Self {
            stock: None,
            layout: None,
            assets: None,
            job: None,
            notice: String::new(),
            last_error: None,
            in_flight: 0,
            job_serial: 0,
            collector: None,
            collected: Vec::new(),
            me: None,
            others: std::collections::BTreeMap::new(),
        }
    }
}
/// A piece of the lab's view; it goes with the world.
#[derive(Component)]
#[require(WorldScoped)]
pub struct LabEntity;

impl Service for PileLab {
    type Layer = WorldLayer;
    type Deps = (WorldMirror, crate::scene_view::SceneView);

    fn init(world: &mut World) -> Self {
        world.run_system_cached_with(view::build, PileLab::default()).expect("building the pile lab's view")
    }

    fn systems(app: &mut App) {
        let active = in_state(WorldState::Active);
        app.add_service_systems::<Self, _>(Phase::Input, input.run_if(crate::control::interactive).run_if(active.clone()))
            .add_service_systems::<Self, _>(Phase::Mirror, mirror.run_if(active.clone()))
            .add_service_systems::<Self, _>(
                Phase::Present,
                (
                    spawn_tosses,
                    update_transit,
                    render,
                    update_display.run_if(crate::control::rendered),
                    showcase.run_if(crate::control::interactive),
                )
                    .chain()
                    .run_if(active.clone()),
            )
            .add_systems(
                PostUpdate,
                captions.after(bevy::transform::TransformSystems::Propagate).run_if(crate::control::rendered).run_if(active),
            );
    }
}
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
    /// Whose backpack: another player's avatar, or (none) this player's.
    agent: Option<u16>,
    units: u32,
    material: Option<Mineral>,
    hidden: u32,
}
#[derive(Clone, Copy)]
enum CaptionKind {
    Dispenser(usize),
    Pile(PileId),
    Cargo,
    Collector,
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
mod transfer;
mod view;
pub use demo::showcase;
pub use transfer::{spawn_tosses, update_transit};
pub(crate) use transfer::{Destination, Transit};
pub use view::{captions, render, update_display};

pub(crate) fn mineral(material: u8) -> Mineral {
    match material {
        0 => Mineral::AmberOre,
        1 => Mineral::BlueCrystal,
        _ => Mineral::GreenMalachite,
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

/// Fills the view's piles, carriers, and this player's job state from the
/// simulation.
pub fn mirror(service: Res<SessionService>, mirror: Res<WorldMirror>, mut lab: ResMut<PileLab>) {
    let Some(sim) = mirror.current.as_ref().and_then(|s| s.piles.as_ref()) else {
        return;
    };
    let local = |p: fx::FxVec2| [(p.x - mirror.center.x).to_f32(), (p.y - mirror.center.y).to_f32()];
    let ground = |p: fx::FxVec2| service.world().map_or(0.0, |w| w.terrain().height(p).to_f32());
    let piles = sim
        .piles
        .iter()
        .map(|(&id, p)| {
            let pile = Pile { material: mineral(p.material), position: local(p.position), units: p.units, source: p.source, height: ground(p.position) };
            (id, pile)
        })
        .collect();
    lab.collector = sim.collector.as_ref().map(|c| mirror.to_view(c.position).with_y(ground(c.position)));
    lab.collected = sim.collector.as_ref().map_or_else(Vec::new, |c| c.stored.clone());
    let view = |c: sim::settlement::piles::Cargo| crate::pile_model::Cargo { material: c.material.map(mineral), units: c.units, capacity: c.capacity };
    let me = mirror.player();
    let mine = me.and_then(|m| sim.hauler(m));
    let cargo = view(me.map_or_else(|| sim.cargo(u16::MAX), |m| sim.cargo(m)));
    let others: u64 = sim.haulers.iter().filter(|(&a, _)| Some(a) != me).map(|(_, h)| u64::from(h.cargo.units)).sum();
    let collected: u64 = sim.collector.as_ref().map_or(0, |c| c.stored.iter().map(|&n| u64::from(n)).sum());
    lab.stock = Some(Stock { piles, cargo, created: sim.created, elsewhere: others + collected });
    lab.me = me;
    lab.others = sim.haulers.iter().filter(|(&a, _)| Some(a) != me).map(|(&a, h)| (a, view(h.cargo.clone()))).collect();
    lab.job = mine.and_then(|h| h.job.as_ref().map(|_| h.job_serial));
    lab.job_serial = mine.map_or(0, |h| h.job_serial);
    lab.in_flight = mine.map_or(0, |h| h.in_flight.len() as u32);
    if let Some(h) = mine.filter(|h| !h.notice.is_empty()) {
        lab.notice = h.notice.clone();
    }
    lab.last_error = mine.and_then(|h| h.last_error.clone());
}

/// Starts a haul: the player walks to the pile and transfers, `pace`
/// seconds per unit (zero: all at once).
pub(crate) fn haul(service: &mut SessionService, action: PileAction, pace: f32, approach: Option<Vec3>) {
    let Some(agent) = service.player() else { return };
    let approach = approach.map(|p| service.to_sim(p));
    service.command(CommandKind::Pile { agent, action, pace: fx::Fx::quantize(pace), approach });
}

pub fn input(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    window: Single<&Window>,
    scene: Res<crate::scene_view::SceneView>,
    source_buttons: Query<(&Interaction, &SourceButton)>,
    panels: Query<&Interaction, With<LabPanel>>,
    mut lab: ResMut<PileLab>,
    mut service: ResMut<SessionService>,
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
        service.command(CommandKind::Dispense { dispenser: index as u32, units: 5 });
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
        service.command(CommandKind::Dispense { dispenser: index as u32, units: 5 });
        return;
    }
    if cursor.y < 110.0 || cursor.y > window.height() - 135.0 {
        return;
    }
    let ground = if scene.has_feature(voxel_assets::SceneFeature::Digging) {
        crate::digging::ground_hit(ray, &service)
    } else {
        cursor_ground(camera.0, camera.1, &window)
    };
    let Some(ground) = ground else {
        return;
    };
    let stock = lab.stock.as_ref().unwrap();
    let pile = stock
        .piles
        .iter()
        .filter_map(|(&id, p)| {
            let radius = pile_radius(p.units);
            ray_sphere(ray.origin, *ray.direction, place(p) + Vec3::Y * 0.3, radius.max(0.55)).map(|t| (t, id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id);
    let carrying = stock.cargo.units > 0;
    let collector = lab.collector.and_then(|c| ray_sphere(ray.origin, *ray.direction, c + Vec3::Y * 1.0, 1.1));
    if collector.is_some() && pile.is_none() {
        if carrying {
            haul(&mut service, PileAction::Deliver, 0.18, None);
        } else {
            lab.notice = "Collect ore from a pile, then click the collector to deliver it.".into();
        }
        return;
    }
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
        PileAction::Drop(service.to_sim(ground))
    } else {
        lab.notice = "Click a pile to collect it, or right click to move.".into();
        return;
    };
    haul(&mut service, action, 0.18, None);
}
