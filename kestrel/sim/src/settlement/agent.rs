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
use super::{DepotIx, Inventory, ItemIx, Kinds};

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
    /// bed. A unit flying out left `counts` when it launched, so only its
    /// `Transfer` shows it (`Settlement::leaving_cargo`).
    pub inventory: Inventory,
    /// Ticks to load or unload one unit (the character's `carry.transfer`).
    pub transfer_ticks: u64,
}

impl Carry {
    pub fn carrying(&self) -> bool {
        self.inventory.holds_any()
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
