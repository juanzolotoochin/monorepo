//! Resolved actions become gameplay commands (World layer). One path for
//! keyboard and controller: the shared handlers here, with mouse targeting
//! (cursor attacks, clicks on logistics) as pointer adapters
//! that run only while keyboard/mouse owns gameplay.
//!
//! The dispatcher runs in `PlayerControl`'s Input slot, in two parts around
//! the pointer adapters, so that keyboard/mouse keep their action order:
//! [`prepare`] (suspension, takeover release, weapon choice) runs before
//! them, [`dispatch`] (everything else) after.

use bevy::prelude::*;
use content::scenario::Weapon;
use service::{Service, WorldLayer};
use sim::CommandKind;
use voxel_assets::SceneFeature;

use super::targets::{self, TargetKey, Targets};
use super::{Device, Edge, InputService, SceneContext, Utility};
use crate::gameplay::Controls;
use crate::lockstep::{PlayerControl, SessionService, WorldMirror};
use crate::scene_view::SceneView;

/// The camera's isometric basis, shared by keys and stick: input x is
/// screen right, input y screen up, with the view turned `yaw` radians
/// counter-clockwise (seen from above) from the default (`town::ViewTurn`).
pub fn camera_relative(input: Vec2, yaw: f32) -> Vec3 {
    let turn = Quat::from_rotation_y(yaw);
    let right = turn * Vec3::new(1.0, 0.0, -1.0).normalize();
    let up = turn * Vec3::new(-1.0, 0.0, -1.0).normalize();
    right * input.x + up * input.y
}

/// The view's yaw now, for [`camera_relative`]: the town's turn, 0
/// elsewhere. Mid-turn it is the eased yaw the camera shows.
pub fn view_yaw(world: &World) -> f32 {
    world.get_resource::<Dispatcher>().map_or(0.0, |d| d.town.view.yaw())
}

/// A transfer waiting on the item chooser.
#[derive(Clone, Debug, PartialEq)]
struct PendingChoice {
    depot: String,
    deposit: bool,
    items: Vec<(String, u32)>,
}

/// The dispatcher's state (World layer).
#[derive(Resource, Default)]
pub struct Dispatcher {
    /// The jetpack is on because of a held lift.
    thrust: bool,
    repair_target: Option<u16>,
    /// The controller's world aim (flat unit vector), kept while the aim
    /// stick is neutral.
    pub aim: Option<Vec3>,
    /// Aim waits for the confirmed facing after this tick (a respawn).
    aim_after: Option<u64>,
    pub targets: Targets,
    choice: Option<PendingChoice>,
    /// The last controller action's outcome or refusal, for the HUD.
    pub notice: String,
    /// Where B would drop cargo, while carrying in a pile scene.
    pub drop_preview: Option<Vec3>,
    /// Releases sent (suspensions), for tests and the HUD.
    pub releases: u32,
    /// The town's highlighted target and job menu (`town`).
    pub town: super::town::Town,
}

impl Service for Dispatcher {
    type Layer = WorldLayer;
    type Deps = (SessionService, WorldMirror, SceneView);

    fn init(_world: &mut World) -> Self {
        Dispatcher::default()
    }
}

/// Pointer adapters run only while keyboard/mouse drives gameplay and no
/// UI context has it.
pub fn pointer_allowed(input: &InputService) -> bool {
    input.frame().owner == Device::KeyboardMouse && !input.frame().blocked
}

/// Suspension, takeover and weapon choice, before the pointer adapters.
pub fn prepare(world: &mut World) {
    let Some(agent) = world.resource::<SessionService>().player() else { return };
    publish_scene(world);
    let frame = world.resource::<InputService>().frame().clone();
    if frame.suspended.is_some() {
        release(world, agent);
    }
    if frame.blocked {
        return;
    }
    if frame.takeover && world.resource::<Dispatcher>().thrust && !frame.gameplay.lift_held {
        world.resource_mut::<SessionService>().command(CommandKind::Jetpack { agent, enabled: false });
        world.resource_mut::<Dispatcher>().thrust = false;
    }
    let weapons = world.resource::<SceneView>().scene().scenario.as_ref().map(|s| s.weapons.clone()).unwrap_or_default();
    for edge in &frame.gameplay.edges {
        let weapon = match *edge {
            Edge::SelectWeapon(w) => Some(w),
            Edge::NextWeapon => {
                let current = world.resource::<Controls>().weapon;
                let at = weapons.iter().position(|&w| w == current);
                weapons.get(at.map_or(0, |i| (i + 1) % weapons.len().max(1))).copied()
            }
            _ => None,
        };
        if let Some(weapon) = weapon.filter(|w| weapons.contains(w)) {
            world.resource_mut::<Controls>().weapon = weapon;
            world.resource_mut::<SessionService>().equip(weapon);
        }
    }
}

/// Tells the input layer what this scene offers.
fn publish_scene(world: &mut World) {
    let scene = world.resource::<SceneView>();
    let scenario = scene.scene().scenario.is_some();
    let diggable = scene.has_feature(SceneFeature::Digging) && scene.player().dig().is_some();
    let recording = world.get_resource::<crate::palimpsest::Memory>().is_some_and(|m| m.recording());
    let town = super::town::active(world);
    let mut utility = Vec::new();
    if diggable {
        utility.push(Utility::Dig);
    }
    if !scenario {
        utility.push(Utility::Record);
        utility.push(Utility::ClearRecording);
    }
    utility.push(Utility::Respawn);
    if logistics_active(world) {
        utility.push(Utility::CancelLogistics);
    }
    let placing = world.get_resource::<Dispatcher>().is_some_and(|d| d.town.placing.is_some());
    let possessing = town && super::town::possessing(world);
    let ballista = possessing && super::town::possessing_ballista(world);
    let mut input = world.resource_mut::<InputService>();
    input.set_scene(SceneContext { scenario, town, utility, recording });
    input.set_placing(placing);
    input.set_possessing(possessing, ballista);
}

/// An accepted pile haul or depot transfer is running for this player.
fn logistics_active(world: &World) -> bool {
    let haul = world.get_resource::<crate::piles::PileLab>().is_some_and(|lab| lab.job.is_some());
    let me = world.resource::<WorldMirror>().player_id();
    let transfer = world
        .get_resource::<crate::settlement::Settlement>()
        .and_then(|s| s.simulation.as_ref())
        .zip(me)
        .is_some_and(|(s, me)| s.manual_jobs.contains_key(&me));
    haul || transfer
}

/// Direct control ends: one semantic release (thrust off; manual and click
/// travel stop unless an accepted job owns the route), the local follow-up
/// attack goes, and the cached movement is forgotten so the next deliberate
/// input is never suppressed as a repeat.
fn release(world: &mut World, agent: u16) {
    world.resource_mut::<SessionService>().command(CommandKind::ReleaseDirectControl { agent });
    world.resource_mut::<PlayerControl>().forget_direct_input();
    world.resource_mut::<Controls>().queued = None;
    let mut dispatcher = world.resource_mut::<Dispatcher>();
    dispatcher.thrust = false;
    dispatcher.repair_target = None;
    dispatcher.releases += 1;
}

/// Everything but weapon choice: aim, attacks, the lift, utility actions,
/// logistics, and movement.
pub fn dispatch(world: &mut World) {
    let Some(agent) = world.resource::<SessionService>().player() else { return };
    let frame = world.resource::<InputService>().frame().clone();
    let seated = world.resource::<InputService>().seat().is_some();
    let now = world.resource::<Time>().elapsed_secs();
    // A town has one target for every device, the sim's (`town`), in place
    // of the controller's nearby targets.
    let town = super::town::active(world);
    if town {
        let dt = world.resource::<Time>().delta_secs();
        world.resource_mut::<Dispatcher>().town.view.advance(dt);
        super::town::refresh(world);
    } else {
        world.resource_mut::<Dispatcher>().town = default();
        if seated || frame.owner == Device::Gamepad {
            world.resource_scope(|world, mut dispatcher: Mut<Dispatcher>| dispatcher.targets.refresh(world, now));
        }
    }
    update_drop_preview(world);
    if frame.blocked {
        return;
    }
    // Z and C, or the D-pad's left and right, turn the town's view.
    if town {
        world.resource_mut::<Dispatcher>().town.view.turn(frame.gameplay.turn);
    }
    if frame.gameplay.repair_pressed && world.resource::<SessionService>().settlement()
        .and_then(|settlement| settlement.agents.get(agent as usize))
        .is_some_and(|avatar| avatar.body.max_health > 0 && avatar.body.health == 0)
    {
        respawn(world, agent);
        return;
    }
    let pad = frame.owner == Device::Gamepad;
    // A possessed ballista's reticle takes the aim (`town::aim_reticle`):
    // the mouse's ground point, or the right stick moving it.
    let ballista = town && super::town::possessing_ballista(world);
    if ballista {
        let cursor = if pad { None } else { cursor_ground(world) };
        let dt = world.resource::<Time>().delta_secs();
        super::town::aim_reticle(world, cursor, frame.gameplay.aim.filter(|_| pad), dt);
    } else if town {
        super::town::aim_reticle(world, None, None, 0.0);
    }
    if pad && !ballista {
        aim(world, frame.gameplay.aim);
    }
    if ballista && frame.gameplay.fire_held {
        let fired = super::town::fire(world, frame.gameplay.fire_pressed).map(|_| ());
        if fired.is_err() {
            town_feedback(world, fired);
        }
    }
    let jumping = world.resource::<SceneView>().player().jump().is_some();
    for &edge in &frame.gameplay.edges {
        match edge {
            Edge::Attack => attack(world),
            Edge::Utility(Utility::Respawn) => {
                respawn(world, agent);
                return;
            }
            Edge::Lift => {
                let kind = if jumping { CommandKind::Jump { agent } } else { CommandKind::Jetpack { agent, enabled: true } };
                world.resource_mut::<SessionService>().command(kind);
                world.resource_mut::<Dispatcher>().thrust = !jumping;
            }
            Edge::LiftRelease if !jumping => {
                world.resource_mut::<SessionService>().command(CommandKind::Jetpack { agent, enabled: false });
                world.resource_mut::<Dispatcher>().thrust = false;
            }
            Edge::Utility(Utility::Dig) => world.resource_mut::<SessionService>().dig(),
            Edge::Utility(Utility::Record) => {
                let recording = world.resource::<crate::palimpsest::Memory>().recording();
                let result = if recording { crate::palimpsest::deploy(world) } else { crate::palimpsest::record(world) };
                if let Err(e) = result {
                    world.resource_mut::<Dispatcher>().notice = e;
                }
            }
            Edge::Utility(Utility::ClearRecording) => crate::palimpsest::clear(world),
            Edge::Utility(Utility::CancelLogistics) => cancel_logistics(world, agent),
            Edge::MachineToggle => toggle_nearest_machine(world),
            Edge::Interact if town => town_interact(world),
            Edge::Interact => interact(world, false),
            Edge::Collect => interact(world, true),
            Edge::Drop if town => town_drop(world),
            Edge::Drop => drop_cargo(world),
            Edge::Burst if town => town_burst(world),
            Edge::Burst => {}
            Edge::TargetPrevious => world.resource_mut::<Dispatcher>().targets.cycle(-1),
            Edge::TargetNext => world.resource_mut::<Dispatcher>().targets.cycle(1),
            Edge::Choose(index) => choose(world, index),
            Edge::Board => world.resource_mut::<SessionService>().command(CommandKind::Board { agent }),
            Edge::Build if town => {
                let built = super::town::build(world);
                town_feedback(world, built);
            }
            Edge::CancelBuild if town => super::town::cancel_build(world),
            Edge::Build | Edge::CancelBuild => {}
            Edge::LiftRelease | Edge::SelectWeapon(_) | Edge::NextWeapon => {}
        }
    }
    movement(world, &frame.gameplay);
    repair(world, agent, frame.gameplay.repair_held);
}

fn repair(world: &mut World, agent: u16, held: bool) {
    let target = held.then(|| {
        let sim = world.resource::<SessionService>();
        let world = sim.world()?;
        let position = world.settlement()?.agents.get(agent as usize)?.body.position;
        world.terrain().repairable_asset_near(position)
    }).flatten();
    let mut dispatcher = world.resource_mut::<Dispatcher>();
    if dispatcher.repair_target != target {
        dispatcher.repair_target = target;
        world.resource_mut::<SessionService>().command(CommandKind::RepairCover { agent, asset: target });
    }
}

/// The ground under the mouse, in arena coordinates, unless the debug UI
/// has the pointer.
fn cursor_ground(world: &mut World) -> Option<Vec3> {
    let game = world.get_resource::<debug_ui::GameInput>()?;
    if game.capture().pointer || game.ui_owns_pointer() {
        return None;
    }
    let (camera, transform) = world.query::<(&Camera, &GlobalTransform)>().iter(world).next()?;
    let (camera, transform) = (camera.clone(), *transform);
    let window = world.query::<&Window>().iter(world).next()?.clone();
    crate::gameplay::cursor_ground(&camera, &transform, &window)
}

/// The controller's aim: explicit stick input, else the remembered aim,
/// first taken from the confirmed facing.
fn aim(world: &mut World, stick: Option<Vec2>) {
    let tick = world.resource::<WorldMirror>().tick;
    let waiting = world.resource::<Dispatcher>().aim_after.is_some_and(|after| tick <= after);
    if let Some(stick) = stick {
        let direction = crate::navigation::flat(camera_relative(stick, view_yaw(world))).normalize_or_zero();
        let mut dispatcher = world.resource_mut::<Dispatcher>();
        dispatcher.aim = Some(direction);
        dispatcher.aim_after = None;
    } else if world.resource::<Dispatcher>().aim.is_none() && !waiting {
        let facing = targets::confirmed_facing(world.resource::<WorldMirror>());
        world.resource_mut::<Dispatcher>().aim = facing;
    }
    let Some(direction) = world.resource::<Dispatcher>().aim else { return };
    let laser = world.resource::<Controls>().weapon == Weapon::Laser;
    if laser {
        world.resource_mut::<Controls>().aim = Some(direction);
        world.resource_scope(|world, mut control: Mut<PlayerControl>| {
            control.aim(&mut world.resource_mut::<SessionService>(), direction);
        });
    } else {
        world.resource_mut::<Controls>().aim = None;
    }
}

/// One attack toward the controller's aim. Targeting uses the confirmed
/// facing, never the predicted pose.
fn attack(world: &mut World) {
    let Some(direction) = world.resource::<Dispatcher>().aim else { return };
    let pack = world.resource::<SceneView>().pack.clone();
    world.resource_scope(|world, mut control: Mut<PlayerControl>| {
        world.resource_scope(|world, mut controls: Mut<Controls>| {
            control.strike(&mut world.resource_mut::<SessionService>(), &mut controls, &pack, direction);
        });
    });
}

fn respawn(world: &mut World, agent: u16) {
    world.resource_mut::<SessionService>().command(CommandKind::Respawn { agent });
    let attack_serial = world.resource::<Controls>().attack_serial;
    *world.resource_mut::<Controls>() = Controls { attack_serial, ..default() };
    let tick = world.resource::<SessionService>().world().map_or(0, |w| w.tick());
    let mut dispatcher = world.resource_mut::<Dispatcher>();
    dispatcher.aim = None;
    dispatcher.aim_after = Some(tick);
    dispatcher.thrust = false;
    dispatcher.repair_target = None;
}

/// Movement from the owning device. Neutral input sends Stop only after
/// this player's own directional walk; the other device's neutral never
/// does. Manual movement also cancels a depot transfer, from any binding.
fn movement(world: &mut World, gameplay: &super::Gameplay) {
    let direction = camera_relative(gameplay.movement, view_yaw(world));
    let walking = world.resource::<PlayerControl>().walking_direction();
    if gameplay.movement != Vec2::ZERO || walking {
        world.resource_scope(|world, mut control: Mut<PlayerControl>| {
            control.walk(&mut world.resource_mut::<SessionService>(), Vec2::new(direction.x, direction.z), gameplay.run);
        });
    }
    if gameplay.movement != Vec2::ZERO {
        let me = world.resource::<WorldMirror>().player_id();
        let busy = world
            .get_resource::<crate::settlement::Settlement>()
            .and_then(|s| s.simulation.as_ref())
            .zip(me)
            .is_some_and(|(s, me)| s.manual_jobs.contains_key(&me));
        let agent = world.resource::<SessionService>().player();
        if let (true, Some(agent)) = (busy, agent) {
            world.resource_mut::<SessionService>().command(CommandKind::CancelTransfer { agent });
            if let Some(mut s) = world.get_resource_mut::<crate::settlement::Settlement>() {
                s.notice = "Transfer canceled; airborne items still arrive.".into();
            }
        }
    }
}

/// Keyboard E with a supply chain: the nearest machine within reach.
fn toggle_nearest_machine(world: &mut World) {
    let Some(p) = world.resource::<SessionService>().player_view_position() else { return };
    let Some(sim) = world.get_resource::<crate::settlement::Settlement>().and_then(|s| s.simulation.as_ref()) else { return };
    let nearest = sim
        .config()
        .producers
        .iter()
        .map(|m| (Vec2::new(m.position[0] - p.x, m.position[1] - p.z).length(), m))
        .filter(|(d, _)| *d < targets::MACHINE_REACH)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, m)| m.id.clone());
    if let Some(id) = nearest {
        toggle_machine(world, &id);
    }
}

fn toggle_machine(world: &mut World, id: &str) {
    let Some(sim) = world.get_resource::<crate::settlement::Settlement>().and_then(|s| s.simulation.as_ref()) else { return };
    let enabled = !sim.producers[id].enabled;
    let producer = world.resource::<SessionService>().settlement().and_then(|s| s.producer_index(id));
    if let Some(producer) = producer {
        world.resource_mut::<SessionService>().command(CommandKind::SetMachine { producer, enabled });
    }
    world.resource_mut::<crate::settlement::Settlement>().notice = format!("{id} {}", if enabled { "enabled" } else { "paused" });
}

/// Explicit Cancel Logistics: a deliberate stop ends a haul and its walk,
/// and a depot transfer's unlaunched units are released. Airborne units
/// still land.
fn cancel_logistics(world: &mut World, agent: u16) {
    let mut service = world.resource_mut::<SessionService>();
    service.command(CommandKind::MoveAvatar { agent, intent: sim::settlement::avatar::Intent::Stop });
    service.command(CommandKind::CancelTransfer { agent });
    world.resource_mut::<PlayerControl>().forget_direct_input();
    world.resource_mut::<Dispatcher>().notice = "Logistics canceled; airborne units still land.".into();
}

/// The town's interact: a refusal before anything is sent (nothing in
/// reach) shows near the elder, as the sim's own refusals do.
fn town_interact(world: &mut World) {
    let used = super::town::interact(world).map(|_| ());
    town_feedback(world, used);
}

/// The town's drop key (keyboard Q, controller B): the bundle goes on the
/// ground at the elder's feet (`town::drop_bundle`).
fn town_drop(world: &mut World) {
    let dropped = super::town::drop_bundle(world).map(|_| ());
    town_feedback(world, dropped);
}

/// The town's Burst key (keyboard R, controller RT, while possessing a
/// building; R and RB in a ballista, its fireball): `town::burst`.
fn town_burst(world: &mut World) {
    let burst = super::town::burst(world).map(|_| ());
    town_feedback(world, burst);
}

/// A town action's refusal shows over the elder, as the sim's do.
fn town_feedback(world: &mut World, result: Result<(), String>) {
    match result {
        Ok(()) => world.resource_mut::<Dispatcher>().notice = String::new(),
        Err(reason) => {
            if let Some(mut s) = world.get_resource_mut::<crate::settlement::Settlement>() {
                s.refuse(&reason);
            }
        }
    }
}

/// The selected target's primary action (or, with `collect`, collecting
/// more while carrying). A refusal shows its reason; the selection never
/// silently moves to another target.
fn interact(world: &mut World, collect: bool) {
    let Some(target) = world.resource::<Dispatcher>().targets.selected().cloned() else {
        world.resource_mut::<Dispatcher>().notice = "Nothing nearby to use.".into();
        return;
    };
    let refuse = |world: &mut World, reason: String| world.resource_mut::<Dispatcher>().notice = format!("{}: {reason}", target.name);
    match &target.key {
        TargetKey::Pile(id) => {
            let Some(stock) = world.resource::<crate::piles::PileLab>().stock.clone() else { return };
            let Some(pile) = stock.piles.get(id) else { return };
            // A pile of a mineral carried takes it back; any other is
            // collected beside the cargo.
            let action = if collect || !stock.cargo.holds(pile.material) {
                if pile.units == 0 {
                    return refuse(world, "Empty output. Use its dispenser to add ore.".into());
                }
                sim::settlement::piles::PileAction::Pickup(*id)
            } else {
                sim::settlement::piles::PileAction::Deposit(*id)
            };
            crate::piles::haul(&mut world.resource_mut::<SessionService>(), action, 0.18, None);
        }
        TargetKey::Collector if collect => refuse(world, "The collector only takes deliveries.".into()),
        TargetKey::Collector => match target.verb {
            Ok(_) => crate::piles::haul(&mut world.resource_mut::<SessionService>(), sim::settlement::piles::PileAction::Deliver, 0.18, None),
            Err(reason) => refuse(world, reason),
        },
        TargetKey::Dispenser(index) if !collect => {
            world.resource_mut::<SessionService>().command(CommandKind::Dispense { dispenser: *index as u32, units: 5 });
        }
        TargetKey::Depot(depot) => {
            let service = world.resource::<SessionService>();
            let choices = if collect { targets::take_choices(service, depot) } else { targets::depot_choices(service, depot) };
            match choices {
                Err(reason) => refuse(world, reason),
                Ok(c) if c.items.len() == 1 => transfer(world, depot, &c.items[0].0, c.items[0].1, c.deposit),
                Ok(c) => {
                    let verb = if c.deposit { "DEPOSIT" } else { "COLLECT" };
                    let options = c.items.iter().map(|(item, units)| format!("{verb} {units} {}", item.to_uppercase())).collect();
                    world.resource_mut::<InputService>().open_chooser(target.name.clone(), options);
                    world.resource_mut::<Dispatcher>().choice = Some(PendingChoice { depot: depot.clone(), deposit: c.deposit, items: c.items });
                }
            }
        }
        TargetKey::Machine(id) if !collect => match target.verb {
            Ok(_) => toggle_machine(world, id),
            Err(reason) => refuse(world, reason),
        },
        TargetKey::Dispenser(_) | TargetKey::Machine(_) => refuse(world, "Nothing to collect here.".into()),
    }
}

fn transfer(world: &mut World, depot: &str, item: &str, units: u32, deposit: bool) {
    match crate::settlement::player_transfer(world, depot, item, units, deposit) {
        Ok(_) => world.resource_mut::<Dispatcher>().notice = String::new(),
        Err(e) => world.resource_mut::<Dispatcher>().notice = e,
    }
}

/// The chooser's pick, or the town's job menu's or build menu's.
fn choose(world: &mut World, index: usize) {
    if world.resource::<Dispatcher>().town.build_menu.is_some() {
        if let Err(e) = super::town::pick_build(world, index) {
            world.resource_mut::<Dispatcher>().notice = e;
        }
        return;
    }
    if world.resource::<Dispatcher>().town.menu.is_some() {
        if let Err(e) = super::town::pick(world, index) {
            world.resource_mut::<Dispatcher>().notice = e;
        }
        return;
    }
    let Some(choice) = world.resource_mut::<Dispatcher>().choice.take() else { return };
    if let Some((item, units)) = choice.items.get(index) {
        transfer(world, &choice.depot, item, *units, choice.deposit);
    }
}

/// Where B would drop the carried cargo: just ahead of the confirmed
/// facing.
fn update_drop_preview(world: &mut World) {
    let carrying = world.get_resource::<crate::piles::PileLab>().and_then(|lab| lab.stock.as_ref()).is_some_and(|s| s.cargo.units > 0);
    let at = carrying
        .then(|| {
            let origin = world.resource::<SessionService>().player_view_position()?;
            let facing = targets::confirmed_facing(world.resource::<WorldMirror>())?;
            Some(origin + facing * super::tuning::DROP_AHEAD)
        })
        .flatten();
    world.resource_mut::<Dispatcher>().drop_preview = at;
}

/// B: proposes the drop point; the simulation validates the space and
/// reports a refusal rather than searching elsewhere.
fn drop_cargo(world: &mut World) {
    let Some(at) = world.resource::<Dispatcher>().drop_preview else {
        world.resource_mut::<Dispatcher>().notice = "Nothing carried to drop.".into();
        return;
    };
    let point = world.resource::<SessionService>().to_sim(at);
    let action = world.get_resource::<crate::piles::PileLab>().and_then(|lab| lab.stock.as_ref()).and_then(|stock| crate::piles::drop_action(stock, point));
    let Some(action) = action else {
        world.resource_mut::<Dispatcher>().notice = "Nothing carried to drop.".into();
        return;
    };
    crate::piles::haul(&mut world.resource_mut::<SessionService>(), action, 0.18, None);
}
