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
//!   A ballista's (its ammo store, B2) is a delivery instead, while
//!   carrying its ammo, as much as fits; nothing comes back out of it.
//! - **A tree pile** (a depot): a deposit while carrying, a withdrawal when
//!   empty-handed.
//! - **A sprite** standing outside buildings, empty-handed only: the app's
//!   job menu, which sends `SetSpriteJob`. The sim does nothing with it.
//! - **A site** (D4), at the spot where its output pile will be: a delivery
//!   while carrying an item its next part still needs (every unit it still
//!   takes, through `bundle_job`; the rest stays in the bundle); otherwise
//!   an install once the part's materials are all there, carrying or not;
//!   otherwise, empty-handed, a readout of what it still needs, which does
//!   nothing in the sim. Carrying something it doesn't need, it is no
//!   target. A site has no output to pick up from yet.
//! - **A ground pile** (a dropped bundle, `piles`): a pickup, as an
//!   output's, topping up a bundle of its item. A site never takes a
//!   delivery from one: deliveries come from the bundle only.
//! - **A building's slot** (D3), at its slot spot, empty-handed only and for
//!   built buildings only: boarding it, as `Board` does, putting out a
//!   sprite there. A sprite working inside is no target (its job menu is
//!   for sprites standing outside), so interacting possesses a staffed
//!   building as an empty one. One another elder possesses is no target.
//!   Seated in a building, the slot is the one target, and interacting
//!   leaves it; seated in a vehicle there is none.
//!
//! A bundle target is usable when `bundle_job` would start a job there, so
//! the highlight and the action never disagree about it. The reach is wider
//! than the dock's (0.85 m): the action walks the agent in to `stand_spot`,
//! and the units move once it is there, as a transfer's do.
//!
//! New kinds slot in as new `InteractTarget` variants, each with a position
//! (`position`) and a use (`interaction`). Dropping on the ground is no thing in reach and no interact: it is
//! its own key (`Action::Drop`, D4), so a press that just misses a target
//! never drops the bundle.

use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::action::View;
use super::avatar::{bundle_job, BundleRefusal};
use super::buildings::STAND_GAP;
use super::piles::{PileId, DROP_GAP};
use super::avatar::Seat;
use super::{AgentIx, AgentState, BuildingIx, Carry, CarryJob, DepotIx, Direction, InventoryRef, ItemIx, Occupant, Settlement, Slot};
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
    /// A site, delivered to, installed or read (D4).
    Site(BuildingIx),
    /// A ground pile (a dropped bundle), picked up from.
    Pile(PileId),
    /// A built building's slot, boarded (D3), or left by its elder.
    Slot(BuildingIx),
}

impl InteractTarget {
    /// The holder a bundle target moves units with; `None` for the kinds
    /// that aren't bundles.
    pub fn holder(self) -> Option<InventoryRef> {
        match self {
            InteractTarget::Output(b) => Some(InventoryRef::Output(b)),
            InteractTarget::Depot(d) => Some(InventoryRef::Depot(d)),
            InteractTarget::Site(b) => Some(InventoryRef::Site(b)),
            InteractTarget::Pile(id) => Some(InventoryRef::Pile(id)),
            InteractTarget::Sprite(_) | InteractTarget::Slot(_) => None,
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
    /// Installs a site's next part, `part`, whose materials are all there.
    Install { building: BuildingIx, part: u8 },
    /// What a site's next part, `part`, still needs (`BuildingState::needs`),
    /// for the view to show; the sim does nothing.
    Needs { building: BuildingIx, part: u8, needs: Vec<(ItemIx, u32)> },
    /// Boards a building's slot (`Board`): stage 5 seats the elder there.
    Board { building: BuildingIx },
    /// Leaves the building the elder possesses.
    Leave { building: BuildingIx },
}

/// The nearest target in reach that `me` can use now and what using it
/// does, or `None`. `sprites` are the sprites and where they stand: the
/// view reads no agents (see `action::View`). A dead agent, or one seated
/// in a vehicle, has no target; one possessing a building has its slot, to
/// leave.
pub fn interact_target(view: &View, me: &AgentState, sprites: impl IntoIterator<Item = (AgentIx, FxVec2)>) -> Option<(InteractTarget, Interaction)> {
    let avatar = me.avatar()?;
    if !me.body.alive() {
        return None;
    }
    match avatar.seat {
        Some(Seat::Building(building)) => return Some((InteractTarget::Slot(building), Interaction::Leave { building })),
        Some(Seat::Vehicle(_)) => return None,
        None => {}
    }
    let from = me.body.position;
    // A building's output once it is built, its site until then.
    let buildings = view.buildings.iter().enumerate().map(|(b, building)| if building.built() { InteractTarget::Output(b as BuildingIx) } else { InteractTarget::Site(b as BuildingIx) });
    let depots = (0..view.depots.len() as DepotIx).map(InteractTarget::Depot);
    let piles = view.piles.into_iter().flat_map(|lab| lab.piles.keys().copied().map(InteractTarget::Pile));
    let slots = view.buildings.iter().enumerate().filter(|(_, b)| b.built()).map(|(b, _)| InteractTarget::Slot(b as BuildingIx));
    let mut candidates: Vec<(Fx, InteractTarget, FxVec2)> = buildings
        .chain(depots)
        .chain(piles)
        .chain(slots)
        .filter_map(|t| position(view, t).map(|p| (t, p)))
        // A sprite working inside a building is no target: its slot is, so
        // interacting possesses the building and puts the sprite out. The
        // job menu is for sprites standing outside.
        .chain(sprites.into_iter().filter(|&(a, _)| !inside(view, a)).map(|(a, p)| (InteractTarget::Sprite(a), p)))
        .map(|(t, p)| (distance(from, p), t, p))
        .filter(|&(d, _, _)| d <= INTERACT_REACH)
        .collect();
    // Nearest first; a tie goes to the lower kind, then the lower index.
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    candidates.into_iter().find_map(|(_, t, _)| interaction(view, &me.carry, t).ok().map(|use_| (t, use_)))
}

/// Whether building `b`'s output is a ballista's ammo store, delivered to
/// rather than picked up from (B2).
pub(super) fn ammo_store(view: &View, b: BuildingIx) -> bool {
    view.buildings.get(b as usize).is_some_and(|b| view.building_kinds[b.kind as usize].defence.is_some())
}

/// Whether sprite `a` works inside a building.
fn inside(view: &View, a: AgentIx) -> bool {
    view.buildings.iter().any(|b| b.slot == Slot::Occupied(Occupant::Sprite(a)))
}

/// Where `target` is, for its reach, if the view has it: an output pile, a
/// depot's dock, the spot a site's output pile will take, a ground pile, or
/// a building's slot spot. A sprite stands among the agents, which the view
/// hasn't.
pub fn position(view: &View, target: InteractTarget) -> Option<FxVec2> {
    match target {
        InteractTarget::Output(b) | InteractTarget::Site(b) => view.buildings.get(b as usize).map(|b| b.output_position),
        InteractTarget::Depot(d) => view.depots.get(d as usize).map(|d| d.position),
        InteractTarget::Pile(id) => view.piles?.piles.get(&id).map(|p| p.position),
        InteractTarget::Slot(b) => view.buildings.get(b as usize).map(|b| b.slot_position(&view.building_kinds[b.kind as usize])),
        InteractTarget::Sprite(_) => None,
    }
}

/// What interacting with `target` would do for an agent carrying `carry`,
/// or why it can't: a bundle kind's `bundle_job` (a deposit into a depot
/// while carrying, else a pickup), a sprite's job menu while empty-handed,
/// a site's delivery, install or needs (see the module), and boarding a
/// built building's slot empty-handed (`board_refusal`). Reach is the
/// caller's to check.
pub fn interaction(view: &View, carry: &Carry, target: InteractTarget) -> Result<Interaction, BundleRefusal> {
    let carrying = carry.inventory.total() + carry.inventory.total_in() > 0;
    match target {
        InteractTarget::Output(_) | InteractTarget::Depot(_) | InteractTarget::Pile(_) => {
            let other = target.holder().expect("a bundle kind");
            // An output and a ground pile are only picked up from (a drop
            // is its own key); a depot takes the bundle, and so does a
            // ballista's ammo store, which nothing is picked up from (B2).
            let takes = match target {
                InteractTarget::Depot(_) => true,
                InteractTarget::Output(b) => ammo_store(view, b),
                _ => false,
            };
            let direction = if carrying && takes { Direction::Deposit } else { Direction::Pickup };
            bundle_job(carry, view, other, direction).map(Interaction::Bundle)
        }
        // D5: a sprite is a target only empty-handed.
        InteractTarget::Sprite(_) if carrying => Err(BundleRefusal::HandsFull),
        InteractTarget::Sprite(_) => Ok(Interaction::JobMenu),
        InteractTarget::Slot(b) => {
            let building = view.buildings.get(b as usize).filter(|b| b.built()).ok_or(BundleRefusal::NotAHolder)?;
            board_refusal(carry, building.slot, view.me).map_or(Ok(Interaction::Board { building: b }), Err)
        }
        InteractTarget::Site(b) => {
            let site = view.buildings.get(b as usize).filter(|b| !b.built()).ok_or(BundleRefusal::NotAHolder)?;
            let kind = &view.building_kinds[site.kind as usize];
            let part = site.site().map_or(0, |(part, _)| part);
            let delivery = if carrying { Some(bundle_job(carry, view, InventoryRef::Site(b), Direction::Deposit)) } else { None };
            match delivery {
                Some(Ok(job)) => Ok(Interaction::Bundle(job)),
                // Nothing to deliver: the part goes in once it is all there,
                // whatever the bundle holds.
                Some(Err(BundleRefusal::NotNeeded)) | None if site.ready(kind) => Ok(Interaction::Install { building: b, part }),
                Some(Err(reason)) => Err(reason),
                None => Ok(Interaction::Needs { building: b, part, needs: site.needs(kind) }),
            }
        }
    }
}

/// Why agent `me`, carrying `carry`, can't board a built building whose
/// slot is `slot`, or `None` when it can (D3): not while a carry job runs
/// (`Busy`) or anything is in the bundle (`Carrying`, which points to the
/// drop key), and not into a slot another elder possesses (`Possessed`). A
/// sprite in the slot, or on its way to it, is put out. Stage 5's boarding
/// checks the same (`Settlement::board`).
pub fn board_refusal(carry: &Carry, slot: Slot, me: AgentIx) -> Option<BundleRefusal> {
    if carry.job.is_some() {
        Some(BundleRefusal::Busy)
    } else if carry.inventory.total() + carry.inventory.total_in() > 0 {
        Some(BundleRefusal::Carrying)
    } else if matches!(slot, Slot::Occupied(Occupant::Elder(a)) if a != me) {
        Some(BundleRefusal::Possessed)
    } else {
        None
    }
}

/// Where an agent standing at `from` stands to move units with `holder`: a
/// depot's dock, just in front of an output pile (or the spot a site's will
/// take), away from its building (the pile is that close to the
/// footprint), or `PILE_STAND` from a ground pile on its side. Within the
/// dock's reach of the holder.
pub fn stand_spot(view: &View, holder: InventoryRef, from: FxVec2) -> Option<FxVec2> {
    match holder {
        InventoryRef::Depot(d) => view.depots.get(d as usize).map(|d| d.position),
        InventoryRef::Output(b) | InventoryRef::Site(b) => view.buildings.get(b as usize).map(|b| b.output_position + (b.output_position - b.position).normalize_or_zero() * STAND_GAP),
        InventoryRef::Pile(id) => view.piles?.piles.get(&id).map(|p| p.position + (from - p.position).normalize_or_zero() * PILE_STAND),
        InventoryRef::Agent(_) | InventoryRef::Collector => None,
    }
}

/// How far from a ground pile's center the agent stands to pick it up: as
/// far as a drop puts it (`piles::DROP_GAP`), well within the dock's reach.
const PILE_STAND: Fx = DROP_GAP;

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
