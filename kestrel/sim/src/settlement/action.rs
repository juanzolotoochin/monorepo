//! Actions: what an agent can be told to do. A player's commands map onto
//! them (`Action::of`), and `act` applies one to its agent. The
//! simulation (`Settlement::act`) and client prediction (`AvatarReplay`)
//! both go through `act`, so a command moves an avatar the same way in both.
//!
//! An action writes only its own agent, reading the rest of the settlement
//! through a `View`. What it does to the world (ending a pile haul, reserving
//! or releasing depot stock, restarting the memory trial, releasing piano
//! keys) it emits as messages (`ActionMessages`), which the simulation
//! applies and the replay drops (`ABILITY_PREDICTION_HANDOFF.md`, D3).
//! A pile haul (`Haul`, the "interact" action) is the exception: its lab
//! part (`StartHaul`) still applies at once in stage 1, in the simulation
//! and, on its own copy of the lab, in the replay.
//!
//! A piano key is no action: it changes nothing on the avatar, only the
//! settlement's held notes, so it stays a world command
//! (`Settlement::play_piano`) that the replay ignores.
//!
//! Avatars act on their players' commands; haulers act on their brain's
//! picks (stage 2) and on the jobs stage 5 accepts, through the same `act`,
//! which dispatches by role (`HAULER_ACTIONS_HANDOFF.md`). Commands still
//! drive only avatars (`Settlement::act`). AI brains are to produce the same
//! vocabulary (`docs/character-definition.md`).

use std::collections::BTreeMap;

use content::character::CharacterDef;
use content::scenario::Weapon;
use fx::{Fx, FxVec2};

use super::avatar::{self, Intent};
use super::haulers;
use super::piles::{PileAction, PileLab};
use super::{AgentIx, AgentState, DepotIx, DepotState, ItemIx, Role, Settlement, Transfer};
use crate::messages::ActionMessages;
use crate::terrain::Terrain;
use crate::CommandKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Walk in a direction, walk to a point along a planned route, or stop.
    Move(Intent),
    /// Walk a route already planned: a hauler's accepted job, whose route
    /// stage 3 planned to check it is reachable. No command sends one
    /// (`Action::of` never makes it).
    Follow(Vec<FxVec2>),
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
    /// Interact with the pile lab: walk to a pile (or the collector, or a
    /// drop spot) and move units there, one every `pace` seconds (zero for
    /// all at once), standing at `approach` if given (`piles::haul`).
    Haul { action: PileAction, pace: Fx, approach: Option<FxVec2> },
    /// Climb into the nearest free vehicle in reach, or out of the one
    /// driven (`vehicle`).
    Board,
}

impl Action {
    /// The agent a command drives and what it tells it to do. `None` for
    /// commands that drive no agent, and for the piano, which acts on the
    /// world rather than on the avatar.
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
            CommandKind::Pile { agent, action, pace, approach } => (agent, Action::Haul { action, pace, approach }),
            CommandKind::Board { agent } => (agent, Action::Board),
            CommandKind::PlayPiano { .. }
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
            | CommandKind::Leave
            | CommandKind::Name { .. } => return None,
        };
        Some((agent, action))
    }
}

/// Applies an action to an agent, by its role: an avatar's part is
/// `avatar::act`, a hauler's `haulers::act`. Every route a command or a
/// brain gives an agent goes through here.
pub(super) fn act(a: &mut AgentState, action: &Action, view: &View, def: &CharacterDef, out: &mut ActionMessages) {
    match a.role {
        Role::Avatar(_) => avatar::act(a, action, view, def, out),
        Role::Hauler(ref hauler) => haulers::act(&mut a.body, hauler, action, view),
    }
}

/// What an acting agent reads of the world: the settlement as of the tick
/// the action applies on, without the agents (an action reads its own agent
/// only through the state it is given).
#[derive(Clone, Copy)]
pub struct View<'a> {
    pub me: AgentIx,
    pub tick: u64,
    pub terrain: &'a Terrain,
    pub weapons: &'a [Weapon],
    pub piles: Option<&'a PileLab>,
    pub depots: &'a [DepotState],
    pub transfers: &'a BTreeMap<u64, Transfer>,
    /// The depot items (`Settlement::depot_items`): minerals are left out.
    pub items: usize,
    pub spawn: FxVec2,
    pub spawn_height: Fx,
    /// The last piano key press's serial (`Settlement::piano_serial`).
    pub piano_serial: u64,
    /// The vehicles, and the characters they are: a driver climbs out
    /// beside theirs.
    pub vehicles: &'a [super::VehicleState],
    pub characters: &'a [CharacterDef],
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
            transfers: &self.transfers,
            items: self.depot_items(),
            spawn: self.spawn,
            spawn_height: self.spawn_height,
            piano_serial: self.piano_serial,
            vehicles: &self.vehicles,
            characters: &self.characters,
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

    /// See `Settlement::leaving_cargo`.
    pub fn leaving_cargo(&self) -> bool {
        super::leaving_cargo(self.transfers, self.me)
    }

    /// Whether a pile haul owns the agent's route.
    pub fn hauling(&self) -> bool {
        self.haul().is_some()
    }

    /// The agent's pile haul, by its `Hauler::job_serial`, if one runs.
    pub fn haul(&self) -> Option<u64> {
        let hauler = self.piles?.hauler(self.me)?;
        hauler.job.as_ref().map(|_| hauler.job_serial)
    }
}
