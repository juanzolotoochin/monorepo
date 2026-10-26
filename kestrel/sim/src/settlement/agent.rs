//! Agents: player avatars and haulers. Every agent has a body and a cargo
//! hold; what drives it is its role, fixed when the agent is made: a
//! scenario's controlled agents and joining players are avatars, the rest
//! haulers.

use content::character::{CharacterDef, CharacterIx};
use content::scenario;
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::avatar::AvatarState;
use super::haulers::HaulerState;
use super::{AgentIx, DepotIx, Inventory, InventoryRef, ItemIx, Kinds};
use crate::messages::{ReserveStock, StockChange};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentState {
    pub id: String,
    /// The character it is.
    pub character: CharacterIx,
    pub body: Body,
    pub carry: Carry,
    pub role: Role,
}

/// What drives an agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// Haul links and the hauler state machine (`haulers`).
    Hauler(HaulerState),
    /// A player's commands (`avatar`), never haul links.
    Avatar(AvatarState),
}

/// What every agent has, whatever drives it: where it is, how it moves and
/// how healthy it is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Body {
    /// Where a hauler goes back to and an avatar respawns.
    pub home: FxVec2,
    /// Meters per tick.
    pub speed: Fx,
    pub base_radius: Fx,
    pub position: FxVec2,
    pub heading: Fx,
    pub travel_heading: Fx,
    pub travelled: Fx,
    /// Waypoints still to walk: a hauler's route, an avatar's `GoTo` or pile
    /// haul.
    pub route: Vec<FxVec2>,
    /// Meters per second. Haulers walk their route without one, so theirs
    /// stays zero.
    pub velocity: FxVec2,
    /// A threat's push on an avatar busy with a swing, in meters per
    /// second. The swing keeps the avatar's own velocity from moving it,
    /// so the push moves it on its own (`walk`), and what is left of it
    /// joins `velocity` when the swing ends.
    pub push: FxVec2,
    pub health: u32,
    /// 0: it can't be hurt. Haulers have no health yet.
    pub max_health: u32,
}

impl Body {
    /// Alive, or without health at all.
    pub fn alive(&self) -> bool {
        self.max_health == 0 || self.health > 0
    }
}

/// The cargo an agent carries: a hauler's load, or the units a player
/// moved by hand to or from a depot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Carry {
    /// The units carried. `reserved_in` counts units flying into the cargo
    /// bed and room a pickup job holds; `reserved_out` the units a deposit
    /// job holds. A unit flying out left `counts` when it launched, so only
    /// its `Transfer` shows it (`Settlement::leaving_cargo`).
    pub inventory: Inventory,
    /// Ticks to load or unload one unit (the character's `carry.transfer`).
    pub transfer_ticks: u64,
    /// Units still to move between this cargo and another inventory. Only a
    /// player's depot transfer has one so far.
    pub job: Option<CarryJob>,
}

impl Carry {
    pub fn carrying(&self) -> bool {
        self.inventory.holds_any()
    }

    /// Ends the job early, if there is one: the one way a job stops before
    /// its last unit. It gives back what the job holds on this cargo now,
    /// and returns the release of what it holds on the other side, for the
    /// caller to apply (`Settlement::reserve_stock`): an action sends it to
    /// stage 5, which can write the other side; stage 5 applies it at once.
    /// Units already in the air still land.
    pub(super) fn drop_carry(&mut self, agent: AgentIx) -> Option<ReserveStock> {
        let job = self.job.take()?;
        job.release(&mut self.inventory, Side::Own);
        Some(ReserveStock { agent, job, change: StockChange::Release })
    }
}

/// Which way a carry job moves its units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// From the other inventory into the agent's cargo.
    Pickup,
    /// From the agent's cargo into the other inventory.
    Deposit,
}

/// One side of a carry job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The agent's own cargo.
    Own,
    /// The inventory the job moves units to or from (`CarryJob::other`).
    Other,
}

/// Moves `remaining` units of `item` between the agent's cargo and `other`,
/// one every `Carry::transfer_ticks`, the next on `next_tick`. While it runs
/// it holds the units it hasn't moved yet on both sides: out on the source,
/// in on the target. Each unit moved takes its share of both; the unit in
/// the air then holds only the target's (`INVENTORY_HANDOFF.md`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarryJob {
    pub other: InventoryRef,
    pub item: ItemIx,
    pub direction: Direction,
    pub remaining: u32,
    pub next_tick: u64,
}

impl CarryJob {
    /// Whether `side` gives the units, so holds them out (else it takes
    /// them, so holds room for them).
    pub fn gives(&self, side: Side) -> bool {
        (side == Side::Own) == (self.direction == Direction::Deposit)
    }

    /// Holds the `remaining` units on `side`, whose inventory is
    /// `inventory`. Refuses (and changes nothing) when they don't fit.
    pub fn reserve(&self, inventory: &mut Inventory, side: Side) -> bool {
        if self.gives(side) {
            inventory.reserve_out(self.item, self.remaining)
        } else {
            inventory.reserve_in(self.item, self.remaining)
        }
    }

    /// Gives back what `reserve` held on `side`.
    pub fn release(&self, inventory: &mut Inventory, side: Side) {
        if self.gives(side) {
            inventory.release_out(self.item, self.remaining);
        } else {
            inventory.release_in(self.item, self.remaining);
        }
    }
}

impl AgentState {
    pub(super) fn new(
        def: &scenario::Agent,
        character: CharacterIx,
        rules: &CharacterDef,
        position: FxVec2,
        all_links: &[scenario::HaulLink],
        items: usize,
        item: &impl Fn(&str) -> ItemIx,
        depot: &impl Fn(&str) -> DepotIx,
    ) -> AgentState {
        let role = if def.controlled {
            // A scenario's avatar belongs to the host.
            Role::Avatar(AvatarState { owner: Some(crate::PlayerId(0)), ..AvatarState::default() })
        } else {
            Role::Hauler(HaulerState::new(def, all_links, item, depot))
        };
        AgentState {
            id: def.id.clone(),
            character,
            body: Body {
                home: position,
                // Copied from the character, like a wave's tuning.
                speed: rules.movement.walk.div_int(content::TICKS_PER_SECOND as i64),
                base_radius: rules.body.radius,
                position,
                ..Body::default()
            },
            carry: Carry {
                inventory: Inventory::new(items, def.capacity, Kinds::AnyMix),
                transfer_ticks: rules.carry.as_ref().expect("players and haulers carry").transfer,
                job: None,
            },
            role,
        }
    }

    /// A player's avatar; `None` for a hauler.
    pub fn avatar(&self) -> Option<&AvatarState> {
        match &self.role {
            Role::Avatar(avatar) => Some(avatar),
            Role::Hauler(_) => None,
        }
    }

    pub fn avatar_mut(&mut self) -> Option<&mut AvatarState> {
        match &mut self.role {
            Role::Avatar(avatar) => Some(avatar),
            Role::Hauler(_) => None,
        }
    }

    /// A hauler's state; `None` for a player's avatar.
    pub fn hauler(&self) -> Option<&HaulerState> {
        match &self.role {
            Role::Hauler(hauler) => Some(hauler),
            Role::Avatar(_) => None,
        }
    }

    pub fn hauler_mut(&mut self) -> Option<&mut HaulerState> {
        match &mut self.role {
            Role::Hauler(hauler) => Some(hauler),
            Role::Avatar(_) => None,
        }
    }

    /// The body radius; a hitched animal stands wider.
    pub fn radius(&self) -> Fx {
        match self.hauler() {
            Some(hauler) => hauler.radius(&self.body),
            None => self.body.base_radius,
        }
    }

    pub fn equipped_item(&self) -> Option<ItemIx> {
        self.hauler()?.equipped_item()
    }
}
