//! Actions: what an agent can be told to do. A player's commands map onto
//! them (`Action::of`), and `avatar::act` applies one to its agent. The
//! simulation (`Settlement::act`) and client prediction (`AvatarReplay`)
//! both go through `act`, so a command moves an avatar the same way in both.
//!
//! An action writes only its own agent, reading the rest of the settlement
//! through a `View`. What a command also does to the world (cancelling a
//! pile job, reserving depot stock, the memory trial, piano notes) stays in
//! `Settlement::act` until it becomes a message
//! (`ABILITY_PREDICTION_HANDOFF.md`, D3).
//!
//! Only player avatars act for now. Step 5 has haulers and AI brains produce
//! the same vocabulary (`docs/character-definition.md`).

use content::scenario::Weapon;
use fx::{Fx, FxVec2};

use super::avatar::Intent;
use super::piles::PileLab;
use super::{AgentIx, DepotIx, DepotState, ItemIx, Settlement};
use crate::terrain::Terrain;
use crate::CommandKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Walk in a direction, walk to a point along a planned route, or stop.
    Move(Intent),
    /// Face the upper body this way, independently of travel.
    Aim(FxVec2),
    /// Attack toward `aim` (a direction on the ground), or buffer the press
    /// during a swing.
    Attack { weapon: Weapon, aim: FxVec2, muzzle: [Fx; 3] },
    /// Put a weapon the scene offers in hand.
    Equip(Weapon),
    /// Hold the jetpack on or off. Flying off stops a dig.
    Jetpack(bool),
    /// Start a grounded jump. Midair presses do nothing.
    Jump,
    /// Dig out a layer of the tile the agent stands on.
    DigDown,
    /// Take units from (or, with `deposit`, give units to) a depot; units
    /// move once the agent is at the dock.
    Transfer { depot: DepotIx, item: ItemIx, units: u32, deposit: bool },
    /// Drop the unlaunched part of a transfer. Units in the air still land.
    CancelTransfer,
    /// Hold or let go of repair on a cover asset.
    RepairCover(Option<u16>),
    /// The player stopped driving the avatar directly: see `avatar::release`.
    Release,
    /// Back to the spawn point with a fresh state.
    Respawn,
}

impl Action {
    /// The agent a command drives and what it tells it to do. `None` for
    /// commands that drive no agent, and for the pile lab and the piano,
    /// which act on the world rather than on the avatar.
    pub fn of(command: &CommandKind) -> Option<(AgentIx, Action)> {
        let (agent, action) = match *command {
            CommandKind::MoveAvatar { agent, ref intent } => (agent, Action::Move(intent.clone())),
            CommandKind::AimAvatar { agent, aim } => (agent, Action::Aim(aim)),
            CommandKind::Attack { agent, weapon, aim, muzzle } => (agent, Action::Attack { weapon, aim, muzzle }),
            CommandKind::Transfer { agent, depot, item, units, deposit } => (agent, Action::Transfer { depot, item, units, deposit }),
            CommandKind::CancelTransfer { agent } => (agent, Action::CancelTransfer),
            CommandKind::Equip { agent, weapon } => (agent, Action::Equip(weapon)),
            CommandKind::Jetpack { agent, enabled } => (agent, Action::Jetpack(enabled)),
            CommandKind::Jump { agent } => (agent, Action::Jump),
            CommandKind::DigDown { agent } => (agent, Action::DigDown),
            CommandKind::ReleaseDirectControl { agent } => (agent, Action::Release),
            CommandKind::RepairCover { agent, asset } => (agent, Action::RepairCover(asset)),
            CommandKind::Respawn { agent } => (agent, Action::Respawn),
            CommandKind::Pile { .. }
            | CommandKind::PlayPiano { .. }
            | CommandKind::SpawnMovers { .. }
            | CommandKind::Impulse { .. }
            | CommandKind::Dig { .. }
            | CommandKind::SetLink { .. }
            | CommandKind::SetMachine { .. }
            | CommandKind::SetDefender { .. }
            | CommandKind::Dispense { .. }
            | CommandKind::AddStock { .. }
            | CommandKind::MemoryRecord
            | CommandKind::MemoryDeploy
            | CommandKind::MemoryClear
            | CommandKind::Join
            | CommandKind::Name { .. } => return None,
        };
        Some((agent, action))
    }
}

/// What an acting agent reads of the world: the settlement as of the tick
/// the action applies on, without the agents (an action reads its own agent
/// only through the state it is given).
pub struct View<'a> {
    pub me: AgentIx,
    pub tick: u64,
    pub terrain: &'a Terrain,
    pub weapons: &'a [Weapon],
    pub piles: Option<&'a PileLab>,
    pub depots: &'a [DepotState],
    pub items: usize,
    pub spawn: FxVec2,
    pub spawn_height: Fx,
}

impl Settlement {
    /// What `me` reads when it acts on `tick`.
    pub(super) fn view<'a>(&'a self, me: AgentIx, tick: u64, terrain: &'a Terrain) -> View<'a> {
        View {
            me,
            tick,
            terrain,
            weapons: &self.weapons,
            piles: self.piles.as_ref(),
            depots: &self.depots,
            items: self.items.len(),
            spawn: self.spawn,
            spawn_height: self.spawn_height,
        }
    }
}

impl View<'_> {
    /// See `Settlement::has_weapon`.
    pub fn has_weapon(&self, weapon: Weapon) -> bool {
        super::combat::offers_weapon(self.weapons, weapon)
    }

    /// Whether the scene allows digging here: no pile or machine on the tile.
    pub fn can_dig_at(&self, position: FxVec2) -> bool {
        self.terrain.diggable && !self.piles.is_some_and(|lab| lab.blocks_dig(Terrain::pit_tile(position)))
    }

    /// Whether a pile haul owns the agent's route.
    pub fn hauling(&self) -> bool {
        self.piles.is_some_and(|lab| lab.hauler(self.me).is_some_and(|h| h.job.is_some()))
    }
}
