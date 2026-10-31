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

use bevy::prelude::*;
use sim::settlement::{InteractTarget, Interaction, SpriteJob};
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

/// The town's interact state.
#[derive(Clone, Debug, Default)]
pub struct Town {
    /// This frame's target, if any.
    pub highlight: Option<Highlight>,
    /// The job menu open (or opening) on a sprite.
    pub menu: Option<JobMenu>,
}

/// What an interact did.
#[derive(Clone, Debug, PartialEq)]
pub enum Used {
    /// Sent `Interact` on a bundle target: the depot items the cargo will
    /// hold once its units land, as `settlement::player_bundle` counts them.
    Bundle { expected: u32 },
    /// Opened the job menu on a sprite.
    Menu,
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

/// Reads this frame's target for the highlight.
pub fn refresh(world: &mut World) {
    let highlight = target(world);
    world.resource_mut::<Dispatcher>().town.highlight = highlight;
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
    serde_json::json!({
        "target": target.as_ref().map(|h| h.target),
        "prompt": target.as_ref().map(|h| h.prompt.clone()),
        "menu": menu.map(|m| serde_json::json!({"sprite": m.sprite, "options": m.options})),
    })
}

fn highlight(world: &World, target: InteractTarget, interaction: &Interaction) -> Option<Highlight> {
    let service = world.resource::<SessionService>();
    let s = service.settlement()?;
    let item = |i: u16| s.items.get(i as usize).map_or_else(String::new, |id| id.to_uppercase());
    let (position, radius) = match target {
        InteractTarget::Output(b) => (s.buildings.get(b as usize)?.output_position, 0.6),
        InteractTarget::Depot(d) => (s.depots.get(d as usize)?.position, 0.8),
        InteractTarget::Sprite(a) => (s.agents.get(a as usize)?.body.position, 0.45),
    };
    let prompt = match (target, interaction) {
        (InteractTarget::Output(_), Interaction::Bundle(job)) => format!("PICK UP {} {}", job.remaining, item(job.item)),
        (_, Interaction::Bundle(job)) if job.direction == sim::settlement::Direction::Deposit => format!("DEPOSIT {} {}", job.remaining, item(job.item)),
        (_, Interaction::Bundle(job)) => format!("WITHDRAW {} {}", job.remaining, item(job.item)),
        (_, Interaction::JobMenu) => "SET JOB".to_string(),
    };
    let position = service.to_view(position);
    Some(Highlight { target, position: position.with_y(0.0), radius, prompt })
}

/// E or A: acts on the target, as the sim picks it now. A bundle target
/// gets `Interact`; a sprite opens the job menu.
pub fn interact(world: &mut World) -> Result<Used, String> {
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
