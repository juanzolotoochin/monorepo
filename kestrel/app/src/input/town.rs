//! The town's interact (World layer): in the Sacred Tree, E and A act on
//! the one target the sim's `interact_target` picks (`SACRED_TREE_HANDOFF.md`,
//! D5), instead of the controller's nearby targets (`targets`).
//!
//! Each frame [`refresh`] reads that target from the confirmed settlement,
//! for the highlight and its prompt (`settlement`, `hud`). [`interact`]
//! sends `Interact` naming it, which the sim checks again; a sprite target
//! opens the job menu instead, a radial panel (`Panel::Radial`) whose pick
//! sends `SetSpriteJob` ([`pick`]). The control protocol goes through the
//! same functions.
//!
//! Building (D1): the build key (keyboard B, controller X) opens a radial
//! menu of the building kinds ([`open_build_menu`]). Its pick starts
//! placing a blueprint ([`pick_build`]): a ghost stands on the ground a
//! short way in front of the elder and follows it ([`ghost`]), valid or
//! refused by the sim's own check (`Settlement::placement_check`), with the
//! reason. Interact puts it down (`PlaceBlueprint`, [`place`]); the build
//! key again, Escape or controller B backs out ([`cancel_build`]). While
//! placing there is no interact target.
//!
//! Dropping (D4): its own key, keyboard Q and controller B (B backs out of
//! a blueprint first, while placing), never an interact fallback, so a
//! press that just misses a target never drops the bundle. [`drop_bundle`]
//! sends `Drop`; the sim puts the bundle on a pile of its item at the
//! elder's feet or a new one there. A ground pile is then an interact
//! target like an output ("PICK UP 3 WOOD"). Nothing drops while placing.
//!
//! Possessing (D3): a built building's slot is a target empty-handed
//! ("POSSESS"), and while the elder possesses a building its slot is the
//! one target ("LEAVE"). The boarding key (G, the left stick) boards and
//! leaves as well; a sprite working inside is the target at its slot spot
//! (its job menu), so the boarding key is what possesses a staffed one.

use bevy::prelude::*;
use fx::{Fx, FxVec2};
use sim::settlement::{InteractTarget, Interaction, PlacementRefusal, SpriteJob};
use sim::CommandKind;

use super::gameplay::Dispatcher;
use super::InputService;
use crate::lockstep::SessionService;

/// What the highlight shows: the target, where it is and what interacting
/// does there.
#[derive(Clone, Debug, PartialEq)]
pub struct Highlight {
    pub target: InteractTarget,
    /// Arena coordinates, on the ground.
    pub position: Vec3,
    /// The ring's radius around `position`.
    pub radius: f32,
    /// What interacting does, in words: "PICK UP 3 WOOD", "SET JOB".
    pub prompt: String,
}

/// The job menu on a sprite: what each option sets.
#[derive(Clone, Debug, PartialEq)]
pub struct JobMenu {
    pub sprite: u16,
    pub options: Vec<String>,
    pub jobs: Vec<SpriteJob>,
}

/// A blueprint's ghost: where it would go, and whether it may.
#[derive(Clone, Debug, PartialEq)]
pub struct Ghost {
    pub kind: u16,
    /// World tiles, as `PlaceBlueprint` takes it.
    pub position: FxVec2,
    /// Arena coordinates on the ground: the footprint's center, and where
    /// its output pile and its slot spot would be.
    pub center: Vec3,
    pub output: Vec3,
    pub slot: Vec3,
    /// The footprint's radius in meters.
    pub radius: f32,
    /// The sim's verdict on it now (`Settlement::placement_check`).
    pub verdict: Result<(), PlacementRefusal>,
}

/// How far past its footprint in front of the elder a ghost stands, in
/// meters: clear of the elder's own body.
const GHOST_GAP: Fx = Fx::from_ratio(4, 5);

/// The town's interact state.
#[derive(Clone, Debug, Default)]
pub struct Town {
    /// This frame's target, if any.
    pub highlight: Option<Highlight>,
    /// The job menu open (or opening) on a sprite.
    pub menu: Option<JobMenu>,
    /// The build menu open (or opening): the building kind of each option.
    pub build_menu: Option<Vec<u16>>,
    /// The building kind being placed, whose ghost follows the elder.
    pub placing: Option<u16>,
    /// This frame's ghost, while placing.
    pub ghost: Option<Ghost>,
}

/// What an interact did.
#[derive(Clone, Debug, PartialEq)]
pub enum Used {
    /// Sent `Interact` on a bundle target: the depot items the cargo will
    /// hold once its units land, as `settlement::player_bundle` counts them.
    Bundle { expected: u32 },
    /// Opened the job menu on a sprite.
    Menu,
    /// Sent `Interact` to install a site's next part.
    Install,
    /// Sent `PlaceBlueprint` where the ghost stood.
    Placed,
    /// Sent `Interact` to board a building's slot, or to leave the one the
    /// elder possesses.
    Seat,
}

/// What a drop sent: the depot items the cargo holds once its units have
/// landed, as `Used::Bundle` counts them.
#[derive(Clone, Debug, PartialEq)]
pub struct Dropped {
    pub expected: u32,
}

/// Whether the scene is a town: its settlement has building kinds.
pub fn active(world: &World) -> bool {
    world.resource::<SessionService>().settlement().is_some_and(|s| !s.building_kinds.is_empty())
}

/// The player's target now and what using it does, by the sim's rule.
fn current(world: &World) -> Option<(InteractTarget, Interaction)> {
    let service = world.resource::<SessionService>();
    let (settlement, sim_world, agent) = (service.settlement()?, service.world()?, service.player()?);
    settlement.interact_target(agent, sim_world.tick(), sim_world.terrain())
}

/// Reads this frame's target for the highlight, or the ghost while
/// placing, when there is no target (interact puts the blueprint down).
pub fn refresh(world: &mut World) {
    let placing = world.resource::<Dispatcher>().town.placing;
    let (highlight, ghost) = match placing {
        Some(kind) => (None, ghost(world, kind)),
        None => (target(world), None),
    };
    let mut dispatcher = world.resource_mut::<Dispatcher>();
    dispatcher.town.highlight = highlight;
    dispatcher.town.ghost = ghost;
}

/// The ghost of a blueprint of `kind`: on the ground `GHOST_GAP` past its
/// footprint in front of the elder (its confirmed body, which the sim
/// checks against), with the sim's verdict.
pub fn ghost(world: &World, kind: u16) -> Option<Ghost> {
    let service = world.resource::<SessionService>();
    let (s, sim_world, agent) = (service.settlement()?, service.world()?, service.player()?);
    let k = s.building_kinds.get(kind as usize)?;
    let body = &s.agents.get(agent as usize)?.body;
    let position = body.position + sim::nav::heading_vector(body.heading) * (k.radius + GHOST_GAP);
    let (output, slot) = s.blueprint_spots(kind, position, sim_world.terrain())?;
    let verdict = s.placement_check(kind, position, sim_world.terrain()).map(|_| ());
    let ground = |p: FxVec2| service.to_view(p).with_y(0.0);
    Some(Ghost { kind, position, center: ground(position), output: ground(output), slot: ground(slot), radius: k.radius.to_f32(), verdict })
}

/// The player's target now, as the highlight shows it.
pub fn target(world: &World) -> Option<Highlight> {
    current(world).and_then(|(target, interaction)| highlight(world, target, &interaction))
}

/// The town's interact for the control protocol's state: the target and
/// its prompt, and the job menu if one is open.
pub fn snapshot(world: &World) -> serde_json::Value {
    let menu = world.get_resource::<Dispatcher>().and_then(|d| d.town.menu.as_ref()).filter(|_| {
        world.get_resource::<InputService>().is_some_and(|i| i.panel().is_some_and(|p| matches!(p, super::Panel::Radial { .. })) || i.radial_requested())
    });
    let target = target(world);
    let town = world.get_resource::<Dispatcher>().map(|d| d.town.clone()).unwrap_or_default();
    let open = world.get_resource::<InputService>().is_some_and(|i| i.panel().is_some() || i.radial_requested());
    let kind_id = |k: u16| world.resource::<SessionService>().settlement().and_then(|s| s.building_kinds.get(k as usize)).map(|k| k.id.clone());
    let ghost = town.placing.and_then(|kind| ghost(world, kind)).map(|g| {
        serde_json::json!({
            "kind": kind_id(g.kind),
            "position": [g.center.x, g.center.z],
            "valid": g.verdict.is_ok(),
            "reason": g.verdict.err().map(|r| r.message()),
        })
    });
    serde_json::json!({
        "target": target.as_ref().map(|h| h.target),
        "prompt": target.as_ref().map(|h| h.prompt.clone()),
        "menu": menu.map(|m| serde_json::json!({"sprite": m.sprite, "options": m.options})),
        "build_menu": town.build_menu.filter(|_| open).map(|kinds| kinds.into_iter().map(kind_id).collect::<Vec<_>>()),
        "placing": town.placing.and_then(kind_id),
        "ghost": ghost,
    })
}

fn highlight(world: &World, target: InteractTarget, interaction: &Interaction) -> Option<Highlight> {
    let service = world.resource::<SessionService>();
    let s = service.settlement()?;
    let item = |i: u16| s.items.get(i as usize).map_or_else(String::new, |id| id.to_uppercase());
    let (position, radius) = match target {
        InteractTarget::Output(b) | InteractTarget::Site(b) => (s.buildings.get(b as usize)?.output_position, 0.6),
        InteractTarget::Depot(d) => (s.depots.get(d as usize)?.position, 0.8),
        InteractTarget::Sprite(a) => (s.agents.get(a as usize)?.body.position, 0.45),
        InteractTarget::Pile(id) => {
            let pile = s.piles.as_ref()?.piles.get(&id)?;
            (pile.position, sim::settlement::piles::pile_radius(pile.units()).to_f32() + 0.15)
        }
        InteractTarget::Slot(b) => (s.buildings.get(b as usize)?.slot_position(s.building_kinds.get(s.buildings[b as usize].kind as usize)?), 0.45),
    };
    let prompt = match (target, interaction) {
        (InteractTarget::Output(_) | InteractTarget::Pile(_), Interaction::Bundle(job)) => format!("PICK UP {} {}", job.remaining, item(job.item)),
        (InteractTarget::Site(_), Interaction::Bundle(job)) => format!("DELIVER {} {}", job.remaining, item(job.item)),
        (_, Interaction::Bundle(job)) if job.direction == sim::settlement::Direction::Deposit => format!("DEPOSIT {} {}", job.remaining, item(job.item)),
        (_, Interaction::Bundle(job)) => format!("WITHDRAW {} {}", job.remaining, item(job.item)),
        (_, Interaction::JobMenu) => "SET JOB".to_string(),
        (_, Interaction::Install { building, part }) => format!("INSTALL {}", part_label(world, *building, *part)),
        (_, Interaction::Needs { needs, .. }) => {
            let needs: Vec<String> = needs.iter().map(|&(i, units)| format!("{units} {}", item(i))).collect();
            format!("NEEDS {}", needs.join(", "))
        }
        (_, Interaction::Board { .. }) => "POSSESS".to_string(),
        (_, Interaction::Leave { .. }) => "LEAVE".to_string(),
    };
    let position = service.to_view(position);
    Some(Highlight { target, position: position.with_y(0.0), radius, prompt })
}

/// Building `building`'s part `part`, as the scenario labels it
/// ("FOUNDATION").
fn part_label(world: &World, building: u16, part: u8) -> String {
    let service = world.resource::<SessionService>();
    let kind = service.settlement().and_then(|s| s.building_kinds.get(s.buildings.get(building as usize)?.kind as usize)).map(|k| k.id.clone());
    world
        .get_resource::<crate::settlement::Settlement>()
        .and_then(|s| s.simulation.as_ref())
        .and_then(|sim| sim.config().building_kinds.iter().find(|k| Some(&k.id) == kind.as_ref())?.parts.get(part as usize).map(|p| p.label.clone()))
        .unwrap_or_else(|| format!("PART {}", part + 1))
}

/// E or A: acts on the target, as the sim picks it now. A bundle target
/// (a site's delivery too), a site's install and a building's slot (board,
/// or leave while possessing it) get `Interact`; a sprite opens the job
/// menu. A site's needs send nothing: they fail with the
/// prompt's words, which show over the elder as a refusal does. While
/// placing, it puts the blueprint down instead.
pub fn interact(world: &mut World) -> Result<Used, String> {
    if world.resource::<Dispatcher>().town.placing.is_some() {
        return place(world).map(|_| Used::Placed);
    }
    let (target, interaction) = current(world).ok_or("Nothing in reach")?;
    let service = world.resource::<SessionService>();
    let agent = service.player().ok_or("No controlled actor")?;
    let s = service.settlement().ok_or("No settlement loaded")?;
    match interaction {
        Interaction::Bundle(job) => {
            let before: u32 = s.depot_cargo(&s.agents[agent as usize]).iter().sum();
            let expected = match job.direction {
                sim::settlement::Direction::Pickup => before + job.remaining,
                sim::settlement::Direction::Deposit => before - job.remaining,
            };
            world.resource_mut::<SessionService>().command(CommandKind::Interact { agent, target });
            Ok(Used::Bundle { expected })
        }
        Interaction::Install { .. } => {
            world.resource_mut::<SessionService>().command(CommandKind::Interact { agent, target });
            Ok(Used::Install)
        }
        Interaction::Board { .. } | Interaction::Leave { .. } => {
            world.resource_mut::<SessionService>().command(CommandKind::Interact { agent, target });
            Ok(Used::Seat)
        }
        Interaction::Needs { .. } => Err(highlight(world, target, &interaction).map_or_else(|| "Not ready".to_string(), |h| h.prompt)),
        Interaction::JobMenu => {
            let InteractTarget::Sprite(sprite) = target else { return Err("Not a sprite".into()) };
            let menu = job_menu(world, sprite);
            let title = format!("{}: SET JOB", sprite_label(world, sprite));
            world.resource_mut::<InputService>().open_radial(title, menu.options.clone());
            world.resource_mut::<Dispatcher>().town.menu = Some(menu);
            Ok(Used::Menu)
        }
    }
}

/// The drop key: sends `Drop` when the sim's own check (`drop_plan`) lets
/// the bundle go down now, or fails with its words ("Nothing carried"),
/// which show over the elder as the sim's refusals do. Nothing drops while
/// placing a blueprint.
pub fn drop_bundle(world: &mut World) -> Result<Dropped, String> {
    if world.resource::<Dispatcher>().town.placing.is_some() {
        return Err("Put the blueprint down first".into());
    }
    let service = world.resource::<SessionService>();
    let (s, sim_world, agent) = (service.settlement().ok_or("No settlement loaded")?, service.world().ok_or("No settlement loaded")?, service.player().ok_or("No controlled actor")?);
    let (_, job) = s.drop_plan(agent, sim_world.tick(), sim_world.terrain()).map_err(|r| r.message().to_string())?;
    let before: u32 = s.depot_cargo(&s.agents[agent as usize]).iter().sum();
    world.resource_mut::<SessionService>().command(CommandKind::Drop { agent });
    Ok(Dropped { expected: before - job.remaining })
}

/// The job menu's options: idle, then one per building kind, gathering its
/// product.
fn job_menu(world: &World, sprite: u16) -> JobMenu {
    let s = world.resource::<SessionService>().settlement().expect("a town");
    let mut options = vec!["IDLE".to_string()];
    let mut jobs = vec![SpriteJob::Idle];
    for (i, kind) in s.building_kinds.iter().enumerate() {
        let product = s.items.get(kind.product as usize).map_or_else(|| kind.id.to_uppercase(), |id| id.to_uppercase());
        options.push(format!("GATHER {product}"));
        jobs.push(SpriteJob::Staff(i as u16));
    }
    JobMenu { sprite, options, jobs }
}

/// A sprite's name, as the scenario labels it.
fn sprite_label(world: &World, sprite: u16) -> String {
    let id = world.resource::<SessionService>().settlement().and_then(|s| s.agents.get(sprite as usize)).map(|a| a.id.clone()).unwrap_or_default();
    world
        .get_resource::<crate::settlement::Settlement>()
        .and_then(|s| s.simulation.as_ref())
        .and_then(|sim| sim.config().sprite(&id).map(|s| s.label.clone()))
        .unwrap_or_else(|| id.to_uppercase())
}

/// The job menu's pick `index`: sends `SetSpriteJob` and closes the menu.
pub fn pick(world: &mut World, index: usize) -> Result<(), String> {
    let menu = world.resource_mut::<Dispatcher>().town.menu.take().ok_or("No job menu open")?;
    world.resource_mut::<InputService>().close_panel();
    let job = *menu.jobs.get(index).ok_or("No such job")?;
    world.resource_mut::<SessionService>().command(CommandKind::SetSpriteJob { agent: menu.sprite, job });
    Ok(())
}

/// The build key: opens the build menu, or backs out of placing a
/// blueprint.
pub fn build(world: &mut World) {
    if world.resource::<Dispatcher>().town.placing.is_some() {
        cancel_build(world);
    } else {
        open_build_menu(world);
    }
}

/// Opens the build menu: a radial panel with one option per building kind,
/// by its label.
pub fn open_build_menu(world: &mut World) {
    let Some(sim) = world.get_resource::<crate::settlement::Settlement>().and_then(|s| s.simulation.as_ref()) else { return };
    let options: Vec<String> = sim.config().building_kinds.iter().map(|k| k.label.clone()).collect();
    let kinds = (0..options.len() as u16).collect();
    world.resource_mut::<InputService>().open_radial("BUILD", options);
    let mut dispatcher = world.resource_mut::<Dispatcher>();
    dispatcher.town.menu = None;
    dispatcher.town.build_menu = Some(kinds);
}

/// The build menu's pick `index`: starts placing that kind's blueprint.
pub fn pick_build(world: &mut World, index: usize) -> Result<(), String> {
    let kinds = world.resource_mut::<Dispatcher>().town.build_menu.take().ok_or("No build menu open")?;
    world.resource_mut::<InputService>().close_panel();
    start_placing(world, *kinds.get(index).ok_or("No such building")?)
}

/// Starts placing a blueprint of `kind`: its ghost follows the elder.
pub fn start_placing(world: &mut World, kind: u16) -> Result<(), String> {
    let known = world.resource::<SessionService>().settlement().is_some_and(|s| (kind as usize) < s.building_kinds.len());
    if !known {
        return Err("No such building".into());
    }
    world.resource_mut::<Dispatcher>().town.placing = Some(kind);
    Ok(())
}

/// Backs out of placing a blueprint, sending nothing.
pub fn cancel_build(world: &mut World) {
    let mut dispatcher = world.resource_mut::<Dispatcher>();
    dispatcher.town.placing = None;
    dispatcher.town.ghost = None;
}

/// Interact while placing: sends `PlaceBlueprint` where the ghost stands
/// if the sim's check allows it now, and stops placing; otherwise fails
/// with the ghost's reason and keeps placing.
pub fn place(world: &mut World) -> Result<(), String> {
    let kind = world.resource::<Dispatcher>().town.placing.ok_or("Not placing a blueprint")?;
    let ghost = ghost(world, kind).ok_or("No ghost")?;
    ghost.verdict.map_err(|r| r.message().to_string())?;
    world.resource_mut::<SessionService>().command(CommandKind::PlaceBlueprint { kind, position: ghost.position });
    cancel_build(world);
    Ok(())
}
