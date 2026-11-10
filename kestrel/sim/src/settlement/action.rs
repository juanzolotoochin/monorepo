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
//! which dispatches by role (`HAULER_ACTIONS_HANDOFF.md`); sprites on their
//! brain's picks (`sprites`). Commands still
//! drive only avatars (`Settlement::act`). AI brains are to produce the same
//! vocabulary (`docs/character-definition.md`).

use std::collections::BTreeMap;

use content::character::CharacterDef;
use content::scenario::Weapon;
use fx::{Fx, FxVec2};

use super::avatar::{self, Intent};
use super::haulers;
use super::interact::InteractTarget;
use super::piles::{PileAction, PileLab};
use super::sprites;
use super::{AgentIx, AgentState, BuildingKind, BuildingState, DepotIx, DepotState, Direction, InventoryRef, ItemIx, Role, Settlement, Transfer};
use crate::messages::ActionMessages;
use crate::terrain::{DigRefusal, Terrain};
use crate::volume::DigDirection;
use crate::CommandKind;

/// How far a dig keeps from a tree pile's dock and a building's output pile
/// and slot spot (`View::dig_in_front`): a pile's ground radius.
const DIG_CLEARANCE: Fx = Fx::from_bits(1_503_238_554); // 0.35

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
    /// Dig out a layer of the tile the agent stands on; in sloped scenes,
    /// of the tile in front of it (`View::dig_in_front`).
    DigDown,
    /// In a volume scene, dig the tile in front, level, down or up
    /// (`View::dig_volume`); elsewhere as `DigDown`. `DigDown` in a volume
    /// scene is a level dig.
    DigFront(DigDirection),
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
    /// Climb into the nearest free vehicle or built building in reach, or
    /// out of the seat it has (`vehicle`, `Settlement::board`).
    Board,
    /// Pick up a bundle from (or, with `Deposit`, empty it into) a depot or
    /// a building output, as many units as fit (`avatar::bundle_job`);
    /// units move once the agent is there, as a transfer's do.
    Bundle { other: InventoryRef, direction: Direction },
    /// Act on a target the client highlighted (`interact`), if it is still
    /// in reach and usable: a bundle target's carry job (a site's delivery
    /// too), walking in to the dock, or a site's install. A sprite target,
    /// and a site's needs, do nothing here.
    Interact(InteractTarget),
    /// Drop the whole bundle on the ground at the agent's feet: onto a pile
    /// of its item there, or a new pile (`piles::drop_plan`). Stage 5 makes
    /// the pile and starts the carry job (`Settlement::drop_bundles`).
    Drop,
    /// Add the possessed building's Burst units to its output, if the
    /// cooldown allows (D3). Only a seated elder can; stage 5 adds them
    /// (`Settlement::bursts`).
    Burst,
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
            CommandKind::DigFront { agent, direction } => (agent, Action::DigFront(direction)),
            CommandKind::ReleaseDirectControl { agent } => (agent, Action::Release),
            CommandKind::RepairCover { agent, asset } => (agent, Action::RepairCover(asset)),
            CommandKind::Respawn { agent } => (agent, Action::Respawn),
            CommandKind::Pile { agent, action, pace, approach } => (agent, Action::Haul { action, pace, approach }),
            CommandKind::Board { agent } => (agent, Action::Board),
            CommandKind::Bundle { agent, other, direction } => (agent, Action::Bundle { other, direction }),
            CommandKind::Interact { agent, target } => (agent, Action::Interact(target)),
            CommandKind::Drop { agent } => (agent, Action::Drop),
            CommandKind::Burst { agent } => (agent, Action::Burst),
            CommandKind::PlayPiano { .. }
            | CommandKind::SpawnMovers { .. }
            | CommandKind::Impulse { .. }
            | CommandKind::Dig { .. }
            | CommandKind::SetLink { .. }
            | CommandKind::SetSpriteJob { .. }
            | CommandKind::PlaceBlueprint { .. }
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
/// `avatar::act`, a hauler's `haulers::act`, a sprite's `sprites::act`.
/// Every route a command or a brain gives an agent goes through here.
pub(super) fn act(a: &mut AgentState, action: &Action, view: &View, def: &CharacterDef, out: &mut ActionMessages) {
    match a.role {
        Role::Avatar(_) => avatar::act(a, action, view, def, out),
        Role::Hauler(ref hauler) => haulers::act(&mut a.body, hauler, action, view),
        Role::Sprite(ref sprite) => sprites::act(&mut a.body, sprite, action, view),
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
    /// The buildings and their kinds, which a sprite plans its job over.
    pub buildings: &'a [BuildingState],
    pub building_kinds: &'a [BuildingKind],
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
            buildings: &self.buildings,
            building_kinds: &self.building_kinds,
        }
    }

    /// The tile `agent` would dig in front of it now in a sloped scene, or
    /// why it can't (`View::dig_in_front`): for the app's refusal hint. It
    /// clones the terrain for the stranding check, so ask on a key press,
    /// not every frame.
    pub fn dig_in_front_of(&self, agent: AgentIx, tick: u64, terrain: &Terrain) -> Option<Result<(i32, i32), DigRefusal>> {
        let body = &self.agents.get(agent as usize)?.body;
        Some(self.view(agent, tick, terrain).dig_in_front(body.position, body.heading, body.base_radius))
    }

    /// The tile and first layer `agent` would dig in front of it now in a
    /// volume scene `direction`-wise, or why it can't
    /// (`View::dig_volume`): for the app's refusal hint.
    pub fn dig_volume_of(&self, agent: AgentIx, tick: u64, terrain: &Terrain, direction: DigDirection) -> Option<Result<((i32, i32), i32), DigRefusal>> {
        let a = self.agents.get(agent as usize)?;
        let height = a.avatar()?.height;
        Some(self.view(agent, tick, terrain).dig_volume(a.body.position, height, a.body.heading, a.body.base_radius, direction))
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

    /// The tile a digger of `radius` at `position` facing `heading` digs in a sloped
    /// scene (M3 of the mining design): the one in front
    /// (`Terrain::front_tile`), if the ground allows it
    /// (`Terrain::dig_refusal`) and nothing stands on it: no building,
    /// site, tree or prop (the terrain's obstacles), pile, tree pile's dock,
    /// or building's output pile or slot spot.
    pub fn dig_in_front(&self, position: FxVec2, heading: Fx, radius: Fx) -> Result<(i32, i32), DigRefusal> {
        let tile = Terrain::front_tile(position, crate::nav::heading_vector(heading));
        if let Some(refusal) = self.terrain.dig_refusal(Terrain::pit_tile(position), tile, radius) {
            return Err(refusal);
        }
        self.dig_blocked(tile).map_or(Ok(tile), Err)
    }

    /// The tile and first layer a digger of `radius` at `position`, its feet
    /// at `height`, facing `heading`, digs `direction`-wise in a volume
    /// scene (V5 of the verticality design): the tile in front
    /// (`Terrain::front_tile`), if the volume allows it
    /// (`Terrain::volume_dig`) and nothing stands on it (`dig_blocked`).
    pub fn dig_volume(&self, position: FxVec2, height: Fx, heading: Fx, radius: Fx, direction: DigDirection) -> Result<((i32, i32), i32), DigRefusal> {
        let tile = Terrain::front_tile(position, crate::nav::heading_vector(heading));
        let bottom = self.terrain.volume_dig(position, height, radius, tile, direction)?;
        self.dig_blocked(tile).map_or(Ok((tile, bottom)), Err)
    }

    /// What stands on `tile` that a dig in front refuses
    /// (`dig_in_front`), the ground aside: an obstacle (`Blocked`), or a
    /// pile, dock, output pile or slot spot (`Pile`). Cheap, unlike the
    /// ground's check, so a miner checks it on every plan (`miners`).
    pub fn dig_blocked(&self, tile: (i32, i32)) -> Option<DigRefusal> {
        // Whether a disc reaches into the tile's square.
        let center = Terrain::pit_center(tile);
        let touches = |p: FxVec2, radius: Fx| {
            let nearest = FxVec2::new(p.x.clamp(center.x - Fx::HALF, center.x + Fx::HALF), p.y.clamp(center.y - Fx::HALF, center.y + Fx::HALF));
            (p - nearest).length() < radius
        };
        if self.terrain.blocking_obstacles().any(|o| touches(o.center, o.radius)) {
            return Some(DigRefusal::Blocked);
        }
        let piles = self.piles.iter().flat_map(|lab| lab.piles.values()).map(|p| (p.position, super::piles::pile_radius(p.units())));
        let docks = self.depots.iter().map(|d| (d.position, DIG_CLEARANCE));
        let spots = self.buildings.iter().flat_map(|b| [b.output_position, b.slot_position(&self.building_kinds[b.kind as usize])]).map(|p| (p, DIG_CLEARANCE));
        if piles.chain(docks).chain(spots).any(|(p, radius)| touches(p, radius)) || self.piles.is_some_and(|lab| lab.blocks_dig(tile)) {
            return Some(DigRefusal::Pile);
        }
        None
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
