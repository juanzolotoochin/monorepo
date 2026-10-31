//! Interact: one key acts on the nearest usable thing in reach
//! (`SACRED_TREE_HANDOFF.md`, D5).
//!
//! `interact_target` lists what is within `INTERACT_REACH` of the agent and
//! usable in its current state, and returns the nearest, lowest kind then
//! lowest index on a tie (`InteractTarget`'s order). The app highlights that
//! target and sends `Interact { agent, target }` naming it; the action
//! (`Action::Interact`) checks it again with `interaction`, since the world
//! may have moved on since the client looked, and acts by its kind.
//!
//! - **A building's output:** a bundle pickup (`avatar::bundle_job`).
//! - **A tree pile** (a depot): a deposit while carrying, a withdrawal when
//!   empty-handed.
//! - **A sprite**, empty-handed only: the app's job menu, which sends
//!   `SetSpriteJob`. The sim does nothing with it.
//!
//! A bundle target is usable when `bundle_job` would start a job there, so
//! the highlight and the action never disagree about it. The reach is wider
//! than the dock's (0.85 m): the action walks the agent in to `stand_spot`,
//! and the units move once it is there, as a transfer's do.
//!
//! New kinds slot in as new `InteractTarget` variants, each with a position
//! (`position`) and a use (`interaction`): sites to deliver to or install
//! (M2.3), ground piles (M2.4) and building slots to board (M3.1). Dropping
//! on the ground (M2.4) is no thing in reach: it is the fallback when no
//! target is.

use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::action::View;
use super::avatar::{bundle_job, BundleRefusal};
use super::{AgentIx, AgentState, BuildingIx, Carry, CarryJob, DepotIx, Direction, InventoryRef, Settlement};
use crate::nav::distance;
use crate::terrain::Terrain;

/// How far from a target's position an agent can interact with it: wider
/// than the dock's reach, so the target lights up a step before the agent
/// is on it, and narrower than the gap between the tree's two piles (3.4 m)
/// or an output pile and the nearest tree pile, so one stands out.
pub const INTERACT_REACH: Fx = Fx::from_ratio(3, 2);

/// What interact acts on. The order is the tie-break (kind, then index),
/// and input logs encode the variant index: new kinds go last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum InteractTarget {
    /// A building's output pile, picked up from.
    Output(BuildingIx),
    /// A depot (a tree pile), deposited into or withdrawn from.
    Depot(DepotIx),
    /// A sprite, whose job the app's menu sets.
    Sprite(AgentIx),
}

impl InteractTarget {
    /// The holder a bundle target moves units with; `None` for the kinds
    /// that aren't bundles.
    pub fn holder(self) -> Option<InventoryRef> {
        match self {
            InteractTarget::Output(b) => Some(InventoryRef::Output(b)),
            InteractTarget::Depot(d) => Some(InventoryRef::Depot(d)),
            InteractTarget::Sprite(_) => None,
        }
    }
}

/// What interacting with a target does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Interaction {
    /// Starts this carry job (`Action::Bundle`'s, sized by `bundle_job`).
    Bundle(CarryJob),
    /// Opens the app's job menu on a sprite; the sim does nothing.
    JobMenu,
}

/// The nearest target in reach that `me` can use now and what using it
/// does, or `None`. `sprites` are the sprites and where they stand: the
/// view reads no agents (see `action::View`). A seated or dead agent has no
/// target.
pub fn interact_target(view: &View, me: &AgentState, sprites: impl IntoIterator<Item = (AgentIx, FxVec2)>) -> Option<(InteractTarget, Interaction)> {
    let avatar = me.avatar()?;
    if !me.body.alive() || avatar.driving.is_some() {
        return None;
    }
    let from = me.body.position;
    let outputs = (0..view.buildings.len() as BuildingIx).map(InteractTarget::Output);
    let depots = (0..view.depots.len() as DepotIx).map(InteractTarget::Depot);
    let mut candidates: Vec<(Fx, InteractTarget, FxVec2)> = outputs
        .chain(depots)
        .filter_map(|t| position(view, t).map(|p| (t, p)))
        .chain(sprites.into_iter().map(|(a, p)| (InteractTarget::Sprite(a), p)))
        .map(|(t, p)| (distance(from, p), t, p))
        .filter(|&(d, _, _)| d <= INTERACT_REACH)
        .collect();
    // Nearest first; a tie goes to the lower kind, then the lower index.
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    candidates.into_iter().find_map(|(_, t, _)| interaction(view, &me.carry, t).ok().map(|use_| (t, use_)))
}

/// Where `target` is, for its reach, if the view has it: an output pile or
/// a depot's dock. A sprite stands among the agents, which the view hasn't.
pub fn position(view: &View, target: InteractTarget) -> Option<FxVec2> {
    match target {
        InteractTarget::Output(b) => view.buildings.get(b as usize).map(|b| b.output_position),
        InteractTarget::Depot(d) => view.depots.get(d as usize).map(|d| d.position),
        InteractTarget::Sprite(_) => None,
    }
}

/// What interacting with `target` would do for an agent carrying `carry`,
/// or why it can't: a bundle kind's `bundle_job` (a deposit into a depot
/// while carrying, else a pickup), and a sprite's job menu while
/// empty-handed. Reach is the caller's to check.
pub fn interaction(view: &View, carry: &Carry, target: InteractTarget) -> Result<Interaction, BundleRefusal> {
    let carrying = carry.inventory.total() + carry.inventory.total_in() > 0;
    match target {
        InteractTarget::Output(_) | InteractTarget::Depot(_) => {
            let other = target.holder().expect("a bundle kind");
            // An output is only picked up from; a depot takes the bundle.
            let direction = if carrying && matches!(target, InteractTarget::Depot(_)) { Direction::Deposit } else { Direction::Pickup };
            bundle_job(carry, view, other, direction).map(Interaction::Bundle)
        }
        // D5: a sprite is a target only empty-handed.
        InteractTarget::Sprite(_) if carrying => Err(BundleRefusal::HandsFull),
        InteractTarget::Sprite(_) => Ok(Interaction::JobMenu),
    }
}

/// Where an agent stands to move units with `holder`: a depot's dock, or
/// just in front of an output pile, away from its building (the pile is
/// that close to the footprint). Within the dock's reach of the holder.
pub fn stand_spot(view: &View, holder: InventoryRef) -> Option<FxVec2> {
    match holder {
        InventoryRef::Depot(d) => view.depots.get(d as usize).map(|d| d.position),
        InventoryRef::Output(b) => view.buildings.get(b as usize).map(|b| b.output_position + (b.output_position - b.position).normalize_or_zero() * Fx::from_ratio(2, 5)),
        InventoryRef::Agent(_) | InventoryRef::Pile(_) | InventoryRef::Collector => None,
    }
}

impl Settlement {
    /// `agent`'s interact target on `tick` and what using it does
    /// (`interact_target`): what the app highlights and scripts read, by
    /// the rule the action checks.
    pub fn interact_target(&self, agent: AgentIx, tick: u64, terrain: &Terrain) -> Option<(InteractTarget, Interaction)> {
        let me = self.agents.get(agent as usize)?;
        let sprites = self.agents.iter().enumerate().filter(|(_, a)| a.sprite().is_some()).map(|(i, a)| (i as AgentIx, a.body.position));
        interact_target(&self.view(agent, tick, terrain), me, sprites)
    }
}
