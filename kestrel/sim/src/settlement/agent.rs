//! Agents: player avatars and haulers. Every agent has a body and a cargo
//! hold; what drives it is its role.

use content::character::{CharacterDef, CharacterIx};
use content::scenario;
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::avatar::AvatarState;
use super::haulers::HaulerState;
use super::{DepotIx, ItemIx};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentState {
    pub id: String,
    /// Player avatars: driven by commands, never by haul links.
    pub controlled: bool,
    /// The player whose commands drive this avatar.
    pub owner: Option<crate::PlayerId>,
    /// The character it is.
    pub character: CharacterIx,
    /// The name the owner goes by; empty until they give one.
    pub name: String,
    pub body: Body,
    pub carry: Carry,
    /// Haulers only.
    pub hauler: HaulerState,
    /// Player avatars only.
    pub avatar: AvatarState,
    /// Weapons are ready again on this tick.
    pub attack_ready: u64,
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

/// The cargo an agent carries: a hauler's load, or the units a player
/// moved by hand to or from a depot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Carry {
    /// Units carried, per item.
    pub cargo: Vec<u32>,
    pub capacity: u32,
    /// Units flying into / out of the cargo bed.
    pub pending_in: u32,
    pub pending_out: u32,
    /// Ticks to load or unload one unit (the character's `carry.transfer`).
    pub transfer_ticks: u64,
}

impl Carry {
    pub fn carrying(&self) -> bool {
        self.cargo.iter().any(|&u| u > 0)
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
        AgentState {
            id: def.id.clone(),
            controlled: def.controlled,
            // A scenario's avatar belongs to the host.
            owner: def.controlled.then_some(crate::PlayerId(0)),
            character,
            name: String::new(),
            body: Body {
                home: position,
                // Copied from the character, like a wave's tuning.
                speed: rules.movement.walk.div_int(content::TICKS_PER_SECOND as i64),
                base_radius: rules.body.radius,
                position,
                ..Body::default()
            },
            carry: Carry {
                cargo: vec![0; items],
                capacity: def.capacity,
                transfer_ticks: rules.carry.as_ref().expect("players and haulers carry").transfer,
                ..Carry::default()
            },
            hauler: HaulerState::new(def, all_links, item, depot),
            avatar: Default::default(),
            attack_ready: 0,
        }
    }

    /// The body radius; a hitched animal stands wider.
    pub fn radius(&self) -> Fx {
        if self.hauler.equipped {
            self.body.base_radius.max(Fx::from_ratio(7, 20))
        } else {
            self.body.base_radius
        }
    }

    pub fn equipped_item(&self) -> Option<ItemIx> {
        self.hauler.equipment.as_ref().filter(|_| self.hauler.equipped).map(|e| e.item)
    }
}
