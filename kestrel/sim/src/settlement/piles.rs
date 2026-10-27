//! The pile lab: loose mineral piles, dispensers that add to their output
//! pile, and a player who walks to a pile and tosses units one at a time.
//! Ported from the original Bevy app (`piles/` and `pile_model.rs`). In a
//! mine, dug ore lands in piles beside the pit, and a collector takes any
//! mineral delivered to it.
//!
//! Minerals are settlement items (`ItemIx`), added after the scenario's in
//! mineral order (`crate::mine`), so `item(mineral)` and `mineral(item)` map
//! between the two where the scene's setup, events and views number them.
//! Piles, the collector and a player's cargo are `Inventory`s; a player's
//! minerals sit in its own `Carry` beside its depot items, any mix within
//! the cargo's one capacity.
//!
//! A haul is a carry job (`Carry::job`) with a pile or the collector, which
//! holds its units on both sides from the command on (`INVENTORY_HANDOFF.md`),
//! and the walk to it (`PileJob`), which stays in the lab.
//!
//! A unit flies between the cargo and the pile as a `Transfer`, like a depot
//! unit, held as room on its target until it lands.
//!
//! Minerals are accounted like any item (`Settlement::balances`): units in
//! piles, cargo, the collector and the air make up what was set up, dispensed
//! and dug, and `Settlement::validate` checks it. Positions are world tiles.

use std::collections::BTreeMap;

use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::avatar::{character_of, AvatarState, Intent};
use super::combat::{Event, TossEnd};
use super::{AgentState, Body, CarryJob, Direction, Inventory, InventoryRef, ItemIx, Kinds, Role, Side};
use super::{AgentIx, Settlement, TRANSFER_FLIGHT_TICKS};
use crate::messages::CarryUnit;
use crate::mine::MINERALS;
use crate::nav;
use crate::terrain::Terrain;

pub const PILE_CAPACITY: u32 = 96;
pub const MAX_PILES: usize = 32;
/// Mines leave a pile by every few pits.
pub const MAX_MINE_PILES: usize = 512;
/// The collector's footprint.
pub const COLLECTOR_RADIUS: Fx = Fx::from_bits(3_865_470_566); // 0.9
/// A walk-and-toss job gives up after 30 s, and replans after 0.8 s without moving.
const JOB_TIMEOUT_TICKS: u64 = content::millis_to_ticks(30_000);
const STALL_TICKS: u64 = content::millis_to_ticks(800);
/// Piles can only be dropped this far from the arena center (6.5 m).
const DROP_LIMIT: Fx = Fx::from_bits(27_917_287_424);

pub type PileId = u32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pile {
    pub position: FxVec2,
    /// One mineral at a time, up to `PILE_CAPACITY`.
    pub inventory: Inventory,
    /// A dispenser's output pile, and the mineral it makes: it stays when
    /// empty, and only it can grow from nothing.
    pub source: Option<ItemIx>,
}

impl Pile {
    pub fn units(&self) -> u32 {
        self.inventory.total()
    }

    /// The mineral it holds, or for an empty dispenser's pile the one it
    /// makes.
    pub fn item(&self) -> Option<ItemIx> {
        self.inventory.kind().or(self.source)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PileAction {
    Pickup(PileId),
    PickupAmount(PileId, u32),
    Deposit(PileId),
    /// Starts a new pile here, of the first mineral carried (in mineral
    /// order). Kept for input logs from before `DropMineral`.
    Drop(FxVec2),
    /// Empties the cargo into the collector, every mineral: the collector
    /// takes any mix, so there is nothing to choose.
    Deliver,
    /// Starts a new pile here of the mineral named (`crate::mine`'s
    /// numbering), all of it carried. Appended after every other action,
    /// so older input logs decode unchanged (input format 3).
    DropMineral(FxVec2, u8),
}

impl PileAction {
    /// Where a drop starts its pile.
    pub fn drop_position(self) -> Option<FxVec2> {
        match self {
            PileAction::Drop(p) | PileAction::DropMineral(p, _) => Some(p),
            _ => None,
        }
    }
}

/// Takes any mineral delivered to it, and keeps count.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collector {
    pub position: FxVec2,
    /// Units delivered, any mix, with no limit.
    pub inventory: Inventory,
}

impl Collector {
    pub fn new(position: FxVec2, items: usize) -> Collector {
        Collector { position, inventory: Inventory::new(items, u32::MAX, Kinds::AnyMix) }
    }
}

/// The walk to a haul's pile and its pace. Its units are the agent's carry
/// job (`Carry::job`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PileJob {
    pub action: PileAction,
    pub center: FxVec2,
    pub radius: Fx,
    pub age: u64,
    pub stalled: u64,
    pub previous: FxVec2,
    /// Ticks between units; zero moves the whole amount at once on arrival.
    pub pace: u64,
    /// Units have started moving: the avatar stays where it is.
    pub started: bool,
    pub transferred: u32,
    pub planned: u32,
}

/// One avatar's haul. What it carries is in its `Carry`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hauler {
    pub job: Option<PileJob>,
    /// Counts started jobs, so a script can tell its job from a later one.
    pub job_serial: u64,
    /// Landing ticks of units in the air.
    pub in_flight: Vec<u64>,
    pub notice: String,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PileLab {
    pub piles: BTreeMap<PileId, Pile>,
    pub created: u64,
    pub next_id: PileId,
    /// The arena center, for the drop limit.
    pub center: FxVec2,
    /// The settlement's item count, for new piles' inventories.
    pub items: usize,
    /// The first mineral's item; the others follow in mineral order.
    pub first_mineral: ItemIx,
    /// Each avatar's haul, made on its first haul.
    pub haulers: BTreeMap<AgentIx, Hauler>,
    /// Piles go down no farther than this from the center, on either axis.
    pub limit: Fx,
    pub max_piles: usize,
    pub collector: Option<Collector>,
}

/// Where unit `n` of a pile sits, in lattice cells (x, layer, z): a stable,
/// supported mound that grows outward and upward as units are added.
pub fn unit_cells() -> &'static [(i32, i32, i32)] {
    static CELLS: std::sync::OnceLock<Vec<(i32, i32, i32)>> = std::sync::OnceLock::new();
    CELLS.get_or_init(|| {
        let mut cells: Vec<(i32, i32, i32)> = (0..6).flat_map(|y| (-3..=3).flat_map(move |x| (-3..=3).map(move |z| (x, y, z)))).collect();
        cells.sort_by_key(|&(x, y, z)| (x.abs().max(z.abs()) + y, y, x * x + z * z, x, z));
        cells.truncate(PILE_CAPACITY as usize);
        cells
    })
}

/// The ground radius of a pile of `units`.
pub fn pile_radius(units: u32) -> Fx {
    let spacing = Fx::from_ratio(7, 25); // 0.28 m
    let edge = Fx::from_ratio(9, 50); // 0.18 m
    unit_cells()
        .iter()
        .take(units as usize)
        .map(|&(x, _, z)| Fx::from_int(x * x + z * z).sqrt() * spacing + edge)
        .fold(Fx::from_ratio(7, 20), Fx::max)
}

impl PileLab {
    /// A lab whose dispensers and piles are given by mineral (`crate::mine`),
    /// for a settlement of `items` items whose minerals start at
    /// `first_mineral`.
    pub fn new(center: FxVec2, items: usize, first_mineral: ItemIx, dispensers: &[(u8, FxVec2)], piles: &[(u8, FxVec2, u32)]) -> PileLab {
        let mut lab = PileLab {
            piles: BTreeMap::new(),
            created: 0,
            next_id: 0,
            center,
            items,
            first_mineral,
            haulers: BTreeMap::new(),
            limit: DROP_LIMIT,
            max_piles: MAX_PILES,
            collector: None,
        };
        for &(mineral, output) in dispensers {
            let item = lab.item(mineral);
            lab.insert(item, output, 0, Some(item));
        }
        for &(mineral, position, units) in piles {
            lab.insert(lab.item(mineral), position, units, None);
            lab.created += units as u64;
        }
        lab
    }

    /// A mineral's item.
    pub fn item(&self, mineral: u8) -> ItemIx {
        self.first_mineral + mineral as ItemIx
    }

    /// An item's mineral. Only minerals are in piles.
    pub fn mineral(&self, item: ItemIx) -> u8 {
        debug_assert!(self.is_mineral(item), "item {item} is not a mineral");
        (item - self.first_mineral) as u8
    }

    pub fn is_mineral(&self, item: ItemIx) -> bool {
        self.minerals().contains(&item)
    }

    /// The minerals' items.
    fn minerals(&self) -> std::ops::Range<ItemIx> {
        self.first_mineral..self.first_mineral + MINERALS as ItemIx
    }

    /// The mineral a pile holds or makes. A scene can't set up a pile with
    /// no units (`Settlement::from_scene`), and an empty pile goes
    /// (`prune`) unless a haul holds room in it, so every pile has one.
    pub fn pile_mineral(&self, pile: &Pile) -> Option<u8> {
        pile.item().map(|item| self.mineral(item))
    }

    /// Mineral units in a cargo, of every mineral. Depot items beside them
    /// don't count.
    pub fn carried(&self, cargo: &Inventory) -> u32 {
        self.minerals().map(|i| cargo.count(i)).sum()
    }

    /// Units in a cargo by mineral, in mineral order.
    pub fn carried_by_mineral(&self, cargo: &Inventory) -> Vec<u32> {
        self.minerals().map(|i| cargo.count(i)).collect()
    }

    /// The first mineral a cargo holds, in mineral order: the one a
    /// `Drop` that names none puts down, and the next a delivery empties.
    pub fn first_carried(&self, cargo: &Inventory) -> Option<ItemIx> {
        self.minerals().find(|&i| cargo.count(i) > 0)
    }

    /// Where the views put `item`'s units in a cargo: minerals in mineral
    /// order, so `item`'s last unit sits at this slot minus one and a new
    /// one at this slot.
    fn cargo_slot(&self, cargo: &Inventory, item: ItemIx) -> u32 {
        (self.first_mineral..=item).map(|i| cargo.count(i)).sum()
    }

    /// The mineral a drop puts down: the one it names, if carried, or the
    /// first carried.
    fn drop_item(&self, cargo: &Inventory, action: PileAction) -> Result<ItemIx, &'static str> {
        match action {
            PileAction::DropMineral(_, mineral) => {
                if (mineral as usize) >= MINERALS {
                    return Err("Unknown mineral");
                }
                let item = self.item(mineral);
                if cargo.count(item) == 0 {
                    return Err("That mineral isn't carried");
                }
                Ok(item)
            }
            _ => self.first_carried(cargo).ok_or("Carrier is empty"),
        }
    }

    /// Units a drop puts down: all of its mineral.
    fn drop_units(&self, cargo: &Inventory, action: PileAction) -> u32 {
        self.drop_item(cargo, action).map_or(0, |i| cargo.count(i))
    }

    /// Units delivered to the collector, by mineral (empty without one).
    pub fn collected(&self) -> Vec<u32> {
        self.collector.as_ref().map_or_else(Vec::new, |c| (0..MINERALS as u8).map(|m| c.inventory.count(self.item(m))).collect())
    }

    fn insert(&mut self, item: ItemIx, position: FxVec2, units: u32, source: Option<ItemIx>) -> PileId {
        let id = self.next_id;
        self.next_id += 1;
        let mut inventory = Inventory::new(self.items, PILE_CAPACITY, Kinds::OneKind);
        let added = inventory.add(item, units);
        debug_assert!(added, "a new pile holds up to {PILE_CAPACITY} units");
        self.piles.insert(id, Pile { position, inventory, source });
        id
    }

    /// An avatar's hauler, if it has hauled.
    pub fn hauler(&self, agent: AgentIx) -> Option<&Hauler> {
        self.haulers.get(&agent)
    }

    /// An avatar's hauler, made empty on first use.
    pub fn hauler_mut(&mut self, agent: AgentIx) -> &mut Hauler {
        self.haulers.entry(agent).or_default()
    }

    /// A dispenser adds up to `requested` units to its output pile, within
    /// the room not held for units on their way.
    pub fn add(&mut self, id: PileId, requested: u32) -> Result<u32, &'static str> {
        let pile = self.piles.get_mut(&id).ok_or("Pile no longer exists")?;
        let item = pile.source.ok_or("Only dispensers create mineral")?;
        let amount = requested.min(pile.inventory.free());
        if amount == 0 {
            return Err("Output pile is full (96 units)");
        }
        let added = pile.inventory.add(item, amount);
        debug_assert!(added, "a dispenser's pile holds only its mineral");
        self.created += amount as u64;
        Ok(amount)
    }

    /// What a haul moves: its other side (none for a drop, whose pile is
    /// made when it starts), its mineral, its direction and its units.
    /// Units promised to another haul don't count, so a second haul for
    /// the same units gets what is left, or is refused. A cargo takes any
    /// mix within its one capacity; a pile takes one mineral.
    pub fn plan(&self, cargo: &Inventory, action: PileAction) -> Result<(Option<InventoryRef>, ItemIx, Direction, u32), &'static str> {
        let pile = |id: PileId| self.piles.get(&id).ok_or("Pile no longer exists");
        match action {
            PileAction::Pickup(id) | PileAction::PickupAmount(id, _) => {
                let p = pile(id)?;
                let item = p.item().ok_or("No units available")?;
                let (available, free) = (p.inventory.available(item), cargo.free());
                let units = match action {
                    PileAction::PickupAmount(_, 0) => return Err("No units available or carrier full"),
                    PileAction::PickupAmount(_, n) if n > available => return Err("Requested quantity exceeds pile stock"),
                    PileAction::PickupAmount(_, n) if n > free => return Err("Requested quantity exceeds free carrying capacity"),
                    PileAction::PickupAmount(_, n) => n,
                    _ if free == 0 => return Err("Carrier is full. Deposit before collecting more"),
                    _ if available == 0 => return Err("No units available"),
                    _ => available.min(free),
                };
                Ok((Some(InventoryRef::Pile(id)), item, Direction::Pickup, units))
            }
            PileAction::Deposit(id) => {
                let p = pile(id)?;
                if self.carried(cargo) == 0 {
                    return Err("Carrier is empty");
                }
                let item = p.item().filter(|&i| cargo.available(i) > 0 && p.inventory.accepts(i)).ok_or("Different minerals stay in separate piles")?;
                let units = cargo.available(item).min(p.inventory.free());
                if units == 0 {
                    return Err("Requested deposit does not fit cargo or pile capacity");
                }
                Ok((Some(InventoryRef::Pile(id)), item, Direction::Deposit, units))
            }
            PileAction::Drop(position) | PileAction::DropMineral(position, _) => {
                if self.carried(cargo) == 0 {
                    return Err("Carrier is empty");
                }
                let item = self.drop_item(cargo, action)?;
                let offset = position - self.center;
                if offset.x.abs() > self.limit || offset.y.abs() > self.limit {
                    return Err("Drop position must be on the deck");
                }
                if self.piles.len() >= self.max_piles {
                    return Err("Pile limit reached; deposit into an existing pile");
                }
                Ok((None, item, Direction::Deposit, cargo.available(item)))
            }
            PileAction::Deliver => {
                let item = self.first_carried(cargo).ok_or("Carrier is empty")?;
                self.collector.as_ref().ok_or("There is no collector here")?;
                Ok((Some(InventoryRef::Collector), item, Direction::Deposit, cargo.available(item)))
            }
        }
    }

    /// The inventory of a pile or the collector, which must exist.
    pub(crate) fn holder_mut(&mut self, holder: InventoryRef) -> &mut Inventory {
        match holder {
            InventoryRef::Pile(id) => &mut self.piles.get_mut(&id).expect("a pile that exists").inventory,
            InventoryRef::Collector => &mut self.collector.as_mut().expect("a collector").inventory,
            InventoryRef::Depot(_) | InventoryRef::Agent(_) => unreachable!("not in the pile lab"),
        }
    }

    /// Removes piles left empty, but not a dispenser's, nor one a haul
    /// still holds units in or out of, nor one with units in the air from
    /// or to it (`busy`).
    pub(crate) fn prune(&mut self, busy: &std::collections::BTreeSet<PileId>) {
        self.piles.retain(|id, p| p.source.is_some() || p.inventory.kind().is_some() || busy.contains(id));
    }

    /// Dug ore: `units` of mineral `item` out of the pit at `tile`, onto a
    /// pile of the same ore beside it with room, or a new one on the nearest
    /// open tile around. Returns the pile and the slot the first unit lands
    /// in, or nothing if there is nowhere to put it (the ore stays buried).
    pub fn mined(&mut self, item: ItemIx, units: u32, tile: (i32, i32), terrain: &Terrain) -> Option<(PileId, u32)> {
        let pit = Terrain::pit_center(tile);
        let beside = self
            .piles
            .iter()
            .filter(|(_, p)| p.source.is_none() && p.item() == Some(item) && p.inventory.free() >= units)
            .map(|(&id, p)| (nav::distance(p.position, pit), id))
            .filter(|&(d, _)| d < Fx::from_ratio(11, 5))
            .min();
        if let Some((_, id)) = beside {
            let pile = self.piles.get_mut(&id).expect("found above");
            let slot = pile.units();
            let added = pile.inventory.add(item, units);
            debug_assert!(added, "checked the pile's mineral and room");
            self.created += units as u64;
            return Some((id, slot));
        }
        if self.piles.len() >= self.max_piles {
            return None;
        }
        let radius = pile_radius(units);
        let mut around: Vec<(i32, i32)> = (-2..=2).flat_map(|y| (-2..=2).map(move |x| (x, y))).filter(|&o| o != (0, 0)).collect();
        around.sort_by_key(|&(x, y)| (x * x + y * y, y, x));
        let spot = around.into_iter().map(|(x, y)| (tile.0 + x, tile.1 + y)).find(|&t| {
            let center = Terrain::pit_center(t);
            terrain.pit(t) == 0
                && self.free_drop(terrain, center, units)
                && self.piles.values().all(|p| nav::distance(p.position, center) > pile_radius(p.units()) + radius + Fx::from_ratio(1, 10))
        })?;
        let id = self.insert(item, Terrain::pit_center(spot), units, None);
        self.created += units as u64;
        Some((id, 0))
    }

    /// Whether digging `tile` would undermine a pile or the collector.
    pub fn blocks_dig(&self, tile: (i32, i32)) -> bool {
        let center = Terrain::pit_center(tile);
        let reach = COLLECTOR_RADIUS + Fx::from_ratio(71, 100);
        self.piles.values().any(|p| Terrain::pit_tile(p.position) == tile)
            || self.collector.as_ref().is_some_and(|c| nav::distance(c.position, center) < reach)
    }

    /// Where an avatar's action happens and how big the pile there is (or
    /// will be), for an avatar carrying `cargo`.
    fn target(&self, cargo: &Inventory, action: PileAction) -> Result<(FxVec2, Fx), &'static str> {
        match action {
            PileAction::Pickup(id) | PileAction::PickupAmount(id, _) | PileAction::Deposit(id) => {
                let p = self.piles.get(&id).ok_or("Pile no longer exists")?;
                let carried = p.item().map_or(0, |i| cargo.count(i));
                let units = if matches!(action, PileAction::Deposit(_)) { (p.units() + carried).min(PILE_CAPACITY) } else { p.units() };
                Ok((p.position, pile_radius(units)))
            }
            PileAction::Drop(p) | PileAction::DropMineral(p, _) => Ok((p, pile_radius(self.drop_units(cargo, action)))),
            PileAction::Deliver => Ok((self.collector.as_ref().ok_or("There is no collector here")?.position, COLLECTOR_RADIUS)),
        }
    }

    fn free_drop(&self, terrain: &Terrain, center: FxVec2, units: u32) -> bool {
        let radius = pile_radius(units);
        let offset = center - self.center;
        offset.x.abs() <= self.limit
            && offset.y.abs() <= self.limit
            && terrain.blocking_obstacles().all(|o| nav::distance(o.center, center) > o.radius + radius + Fx::from_ratio(1, 5))
    }
}

/// The best route to a standing spot within transfer reach of a pile, or
/// an empty route if already there.
/// `body` is the avatar's radius.
fn approach_route(terrain: &Terrain, start: FxVec2, center: FxVec2, radius: Fx, body: Fx) -> Option<Vec<FxVec2>> {
    let stand = radius + body + Fx::from_ratio(9, 50);
    let gap = nav::distance(start, center);
    if gap >= radius + body + Fx::from_ratio(1, 25) && gap <= stand + Fx::from_ratio(1, 4) {
        return Some(Vec::new());
    }
    (0..24)
        .filter_map(|i| {
            let angle = Fx::TAU.mul_int(i).div_int(24);
            let (sin, cos) = angle.sin_cos();
            let goal = center + FxVec2::new(cos, sin) * stand;
            let route = walk_path(terrain, start, goal, body);
            if route.is_empty() {
                return None;
            }
            let mut previous = start;
            let cost = route.iter().fold(Fx::ZERO, |sum, &p| {
                let d = nav::distance(previous, p);
                previous = p;
                sum + d
            });
            Some((cost, i, route))
        })
        .min_by_key(|(cost, i, _)| (*cost, *i))
        .map(|(_, _, route)| route)
}

/// A route on foot: around obstacles, pits and cliffs.
fn walk_path(terrain: &Terrain, start: FxVec2, goal: FxVec2, body: Fx) -> Vec<FxVec2> {
    nav::path_with(terrain, start, goal, body, &[], &|a, b| terrain.walkable(a, b, body))
}

/// Whether the avatar stands within reach of the job's pile.
fn in_reach(position: FxVec2, job: &PileJob, body: Fx) -> bool {
    let distance = nav::distance(position, job.center);
    distance >= job.radius + body + Fx::from_ratio(1, 50) && distance <= job.radius + body + Fx::from_ratio(9, 20)
}

impl Settlement {
    /// Commands: walk to a pile and transfer. `pace` is seconds per unit
    /// (zero for all at once); `approach` picks the standing spot. The haul
    /// holds its units on both sides now, as a carry job: a second haul for
    /// the same units gets what is left, or is refused here.
    pub fn pile_action(&mut self, tick: u64, terrain: &Terrain, agent: usize, action: PileAction, pace: Fx, approach: Option<FxVec2>) {
        if self.piles.is_none() || !self.agents.get(agent).is_some_and(|a| a.avatar().is_some()) {
            return;
        }
        if let Err(error) = self.start_pile_job(tick, terrain, agent, action, pace, approach) {
            let hauler = self.piles.as_mut().expect("checked").hauler_mut(agent as AgentIx);
            hauler.notice = error.into();
            hauler.last_error = Some(error.into());
        }
    }

    fn start_pile_job(&mut self, tick: u64, terrain: &Terrain, agent: usize, action: PileAction, pace: Fx, approach: Option<FxVec2>) -> Result<(), &'static str> {
        let me = agent as AgentIx;
        if pace < Fx::ZERO || pace > Fx::from_int(5) {
            return Err("Invalid transfer interval");
        }
        let hauler = self.piles.as_mut().expect("a pile scene").hauler_mut(me);
        if !hauler.in_flight.is_empty() {
            return Err("Wait for airborne units to land before starting another transfer");
        }
        hauler.job_serial += 1;
        hauler.job = None;
        hauler.last_error = None;
        // A new haul replaces the one running, which gives back its units.
        self.drop_pile_carry(agent);
        if self.agents[agent].carry.job.is_some() {
            return Err("Finish the depot transfer before hauling");
        }
        let Settlement { piles, agents, characters, .. } = self;
        let lab = piles.as_mut().expect("a pile scene");
        let a = &mut agents[agent];
        let body_radius = character_of(characters, a).body.radius;
        let AgentState { body, carry, role: Role::Avatar(avatar), .. } = a else { unreachable!("checked: an avatar") };
        let cargo = &carry.inventory;
        let (center, radius) = lab.target(cargo, action)?;
        if action.drop_position().is_some() && !lab.free_drop(terrain, center, lab.drop_units(cargo, action)) {
            return Err("Choose open floor away from piles, machines and the deck edge");
        }
        if action == PileAction::Deliver && lab.carried(cargo) == 0 {
            return Err("Carrier is empty. Collect ore before delivering");
        }
        let (other, item, direction, units) = lab.plan(cargo, action)?;
        let route = match approach {
            Some(stand) => {
                let gap = nav::distance(stand, center);
                if gap < radius + body_radius + Fx::from_ratio(1, 25) || gap > radius + body_radius + Fx::from_ratio(9, 20) {
                    return Err("Approach point must be within safe transfer reach");
                }
                let path = walk_path(terrain, body.position, stand, body_radius);
                if path.is_empty() {
                    return Err("Approach point is blocked");
                }
                path
            }
            None => approach_route(terrain, body.position, center, radius, body_radius).ok_or("No clear route there on foot")?,
        };
        let pace = if pace == Fx::ZERO { 0 } else { (pace * Fx::from_int(content::TICKS_PER_SECOND)).round().floor_to_int().max(1) as u64 };
        // A drop's pile is made now, empty, so the units can be held there.
        let other = other.unwrap_or_else(|| InventoryRef::Pile(lab.insert(item, center, 0, None)));
        let job = CarryJob { other, item, direction, remaining: units, next_tick: tick };
        let held = job.reserve(&mut carry.inventory, Side::Own) && job.reserve(lab.holder_mut(other), Side::Other);
        debug_assert!(held, "`plan` checked both sides");
        carry.job = Some(job);
        follow(body, avatar, route);
        // A haul let go of earlier this turn no longer holds this one back.
        avatar.dropped_haul = false;
        let hauler = lab.hauler_mut(me);
        hauler.job = Some(PileJob { action, center, radius, age: 0, stalled: 0, previous: body.position, pace, started: false, transferred: 0, planned: units });
        hauler.notice = if matches!(action, PileAction::Pickup(_) | PileAction::PickupAmount(..)) {
            "Walking to collect mineral…"
        } else if action == PileAction::Deliver {
            "Walking to the collector…"
        } else {
            "Walking to deposit cargo…"
        }
        .into();
        Ok(())
    }

    /// Commands: a dispenser adds units to its output pile. Everyone hears
    /// about it.
    pub fn dispense(&mut self, dispenser: PileId, units: u32) {
        let players: Vec<AgentIx> = (0..self.agents.len() as AgentIx).filter(|&i| self.agents[i as usize].avatar().is_some()).collect();
        let Some(lab) = self.piles.as_mut() else { return };
        let notice = match lab.add(dispenser, units) {
            Ok(n) => {
                let item = lab.piles[&dispenser].source.expect("a dispenser's pile");
                self.accounts[item as usize].generated += n as u64;
                format!("Dispenser added {n} units to its output pile.")
            }
            Err(e) => e.into(),
        };
        for agent in players {
            lab.hauler_mut(agent).notice = notice.clone();
        }
    }

    /// The avatar moved, attacked or respawned (`DropHaul`): its haul stops,
    /// its cargo stays. A haul started since (another `job_serial`) stays.
    pub(super) fn cancel_pile_job(&mut self, agent: usize, serial: u64) {
        let Some(hauler) = self.piles.as_ref().and_then(|lab| lab.haulers.get(&(agent as AgentIx))) else { return };
        if hauler.job_serial == serial {
            self.end_pile_job(agent, "Haul canceled. Cargo is retained.", "Haul canceled by player input");
        }
    }

    /// Stops the avatar's haul, if one runs, with this notice and error: the
    /// units it hasn't moved go back (`drop_carry`). Its cargo, and units in
    /// the air, stay.
    pub(super) fn end_pile_job(&mut self, agent: usize, notice: &str, error: &str) {
        let Some(hauler) = self.piles.as_mut().and_then(|lab| lab.haulers.get_mut(&(agent as AgentIx))) else { return };
        if hauler.job.take().is_some() {
            hauler.notice = notice.into();
            hauler.last_error = Some(error.into());
        }
        self.drop_pile_carry(agent);
    }

    /// Ends the avatar's carry job if it is a haul's, giving back what it
    /// holds on both sides at once.
    fn drop_pile_carry(&mut self, agent: usize) {
        let carry = &mut self.agents[agent].carry;
        if carry.job.as_ref().is_some_and(|j| j.other.in_pile_lab()) {
            let releases = carry.drop_carry(agent as AgentIx);
            self.reserve_stock(releases);
        }
    }

    /// Stage 2, for an avatar: walk toward its pile, replanning when stuck.
    pub(super) fn pile_job_self_update(lab: &PileLab, me: AgentIx, body: &mut Body, avatar: &mut AvatarState, terrain: &Terrain, radius: Fx) -> Option<&'static str> {
        let job = lab.hauler(me)?.job.as_ref()?;
        if job.started || in_reach(body.position, job, radius) {
            body.route.clear();
            body.velocity = FxVec2::ZERO;
            return None;
        }
        if job.age > JOB_TIMEOUT_TICKS {
            return Some("Haul timed out. Cargo retained; choose another approach.");
        }
        if body.route.is_empty() || job.stalled > STALL_TICKS {
            match approach_route(terrain, body.position, job.center, job.radius, radius) {
                Some(route) => follow(body, avatar, route),
                None => return Some("Route blocked. Cargo retained."),
            }
        }
        None
    }

    /// Housekeeping for every haul's clocks: failures, age, stalls,
    /// landings, completion; then piles left empty go.
    pub(crate) fn pile_housekeeping(&mut self, tick: u64) {
        let failures: Vec<(usize, String)> = self
            .agents
            .iter_mut()
            .enumerate()
            .filter_map(|(i, a)| Some((i, a.avatar_mut()?.pile_failure.take()?)))
            .collect();
        for (agent, error) in failures {
            self.end_pile_job(agent, &error, &error);
        }
        let Some(lab) = self.piles.as_mut() else { return };
        let first = lab.first_mineral;
        for (&agent, hauler) in lab.haulers.iter_mut() {
            hauler.in_flight.retain(|&landing| landing > tick);
            let Some(job) = hauler.job.as_mut() else { continue };
            let a = &self.agents[agent as usize];
            let position = a.body.position;
            job.age += 1;
            job.stalled = if nav::distance(position, job.previous) < Fx::from_ratio(3, 1000) { job.stalled + 1 } else { 0 };
            job.previous = position;
            if job.started && a.carry.job.is_none() && hauler.in_flight.is_empty() {
                hauler.notice = done_notice(job, &a.carry.inventory, first);
                hauler.job = None;
            }
        }
        let busy = self
            .transfers
            .values()
            .flat_map(|t| [t.from, t.to])
            .filter_map(|end| match end {
                InventoryRef::Pile(id) => Some(id),
                _ => None,
            })
            .collect();
        lab.prune(&busy);
    }

    /// Stage 3: the avatar is in reach and a unit (or the whole amount) is due.
    pub(super) fn read_pile_job(&self, me: AgentIx, a: &AgentState, tick: u64) -> Option<CarryUnit> {
        if a.avatar()?.dropped_haul {
            return None;
        }
        let job = self.piles.as_ref()?.hauler(me)?.job.as_ref()?;
        let units = a.carry.job.as_ref().filter(|j| j.other.in_pile_lab())?;
        let due = tick >= units.next_tick && (job.started || in_reach(a.body.position, job, character_of(&self.characters, a).body.radius));
        due.then_some(CarryUnit { agent: me })
    }

    /// Launches the next unit of an avatar's haul (or all of it at pace
    /// zero), between its cargo and a pile or the collector, out of what the
    /// carry job holds on both sides. It flies as a `Transfer`
    /// (`TRANSFER_FLIGHT_TICKS`), its toss shown on the way
    /// (`Event::PileToss`, `Hauler::in_flight`); the haul ends once every
    /// unit landed (`pile_housekeeping`).
    pub(crate) fn pile_unit(&mut self, tick: u64, terrain: &Terrain, message: CarryUnit) {
        let (me, agent) = (message.agent, message.agent as usize);
        let Some(mut job) = self.piles.as_ref().and_then(|lab| lab.hauler(me)).and_then(|h| h.job.clone()) else { return };
        let Some(units) = self.agents[agent].carry.job.as_ref().filter(|j| j.other.in_pile_lab()) else { return };
        if tick < units.next_tick {
            return;
        }
        if let (false, Some(p)) = (job.started, job.action.drop_position()) {
            let lab = self.piles.as_ref().expect("a pile scene");
            if !lab.free_drop(terrain, p, units.remaining) {
                let error = if job.pace == 0 { "Drop location is now blocked. Cargo retained." } else { "Drop location is blocked" };
                return self.end_pile_job(agent, error, error);
            }
        }
        job.started = true;
        let mut moved = 0;
        while let Some(units) = self.agents[agent].carry.job.clone().filter(|j| j.other.in_pile_lab()) {
            let (from, to) = self.pile_unit_launch(tick, me, &units);
            moved += 1;
            let lab = self.piles.as_mut().expect("a pile scene");
            lab.hauler_mut(me).in_flight.push(tick + TRANSFER_FLIGHT_TICKS);
            self.events.push(Event::PileToss { material: lab.mineral(units.item), from, to, ticks: TRANSFER_FLIGHT_TICKS });
            let carry = &mut self.agents[agent].carry;
            let current = carry.job.as_mut().expect("moving its units");
            current.remaining -= 1;
            current.next_tick = tick + job.pace;
            if current.remaining == 0 {
                carry.job = None;
                // A delivery empties every mineral, one after another.
                if job.action == PileAction::Deliver {
                    job.planned += self.next_delivery(agent, tick + job.pace);
                }
            }
            if job.pace > 0 {
                break;
            }
        }
        job.transferred += moved;
        let hauler = self.piles.as_mut().expect("a pile scene").hauler_mut(me);
        if job.pace > 0 {
            hauler.notice = format!("Tossing {} / {} units...", job.transferred, job.planned);
        }
        hauler.job = Some(job);
    }

    /// Launches one unit of a haul's carry job between the cargo and its
    /// pile or the collector, as `carry_unit` does a depot unit: it leaves
    /// the source with its share of the job, and the job's room at the
    /// target passes to it until it lands (`arrivals`). Returns where its
    /// toss starts and ends.
    fn pile_unit_launch(&mut self, tick: u64, me: AgentIx, job: &CarryJob) -> (TossEnd, TossEnd) {
        let own = InventoryRef::Agent(me);
        let (from, to) = match job.direction {
            Direction::Pickup => (job.other, own),
            Direction::Deposit => (own, job.other),
        };
        let ends = (self.toss_end(from, job.item, true), self.toss_end(to, job.item, false));
        self.inventory_mut(from).take(job.item, 1);
        let (start, end) = (self.position_of(from), self.position_of(to));
        self.launch(tick, job.item, from, to, start, end);
        ends
    }

    /// Where a unit of `item` leaves (`leaving`) `holder`, or will land in
    /// it: after what it holds and the units of `item` already flying there.
    fn toss_end(&self, holder: InventoryRef, item: ItemIx, leaving: bool) -> TossEnd {
        let lab = self.piles.as_ref().expect("a pile scene");
        let flying = if leaving { 0 } else { self.transfers.values().filter(|t| t.to == holder && t.item == item).count() as u32 };
        match holder {
            InventoryRef::Pile(id) => {
                let pile = &lab.piles[&id];
                TossEnd::Pile { id, slot: pile.units() + flying - u32::from(leaving), position: pile.position }
            }
            InventoryRef::Collector => TossEnd::Collector { position: self.position_of(holder) },
            InventoryRef::Agent(agent) => {
                let cargo = &self.agents[agent as usize].carry.inventory;
                TossEnd::Cargo { agent, slot: lab.cargo_slot(cargo, item) + flying - u32::from(leaving) }
            }
            InventoryRef::Depot(_) => unreachable!("hauls don't reach depots"),
        }
    }

    /// Starts delivering the next mineral carried, if there is one, as the
    /// avatar's carry job (due on `tick`). Returns its units.
    fn next_delivery(&mut self, agent: usize, tick: u64) -> u32 {
        let Settlement { piles, agents, .. } = self;
        let (lab, carry) = (piles.as_mut().expect("a pile scene"), &mut agents[agent].carry);
        let Some(item) = lab.first_carried(&carry.inventory) else { return 0 };
        let remaining = carry.inventory.available(item);
        let job = CarryJob { other: InventoryRef::Collector, item, direction: Direction::Deposit, remaining, next_tick: tick };
        let held = job.reserve(&mut carry.inventory, Side::Own) && job.reserve(lab.holder_mut(job.other), Side::Other);
        debug_assert!(held, "the units are carried and the collector has no limit");
        carry.job = Some(job);
        remaining
    }
}

/// What a finished haul says, once every unit landed: at pace zero what it
/// moved, otherwise that it is done. Minerals start at item `first`.
fn done_notice(job: &PileJob, cargo: &Inventory, first: ItemIx) -> String {
    let n = job.transferred;
    let carried: u32 = (first..first + MINERALS as ItemIx).map(|i| cargo.count(i)).sum();
    if job.pace > 0 {
        return format!("Transferred {n} units. All items landed.");
    }
    match job.action {
        PileAction::Pickup(_) | PileAction::PickupAmount(..) => format!("Collected {n} units. Carrying {}/{}.", cargo.total(), cargo.capacity),
        PileAction::Deposit(_) => format!("Deposited {n} units. {carried} still carried."),
        PileAction::Drop(_) | PileAction::DropMineral(..) => format!("Placed a new pile of {n} units."),
        PileAction::Deliver => format!("Delivered {n} units to the collector."),
    }
}

/// Points the avatar along a route.
fn follow(body: &mut Body, avatar: &mut AvatarState, route: Vec<FxVec2>) {
    let target = route.last().copied().unwrap_or(body.position);
    body.route = route;
    avatar.swing = None;
    avatar.queued = None;
    avatar.queued_at = None;
    avatar.intent = Some(Intent::GoTo { target, run: false });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hauling avatar.
    const A: AgentIx = 0;

    /// A lab with each avatar's cargo beside it, as a settlement holds them.
    /// Its items are the minerals alone.
    struct Lab {
        lab: PileLab,
        cargo: BTreeMap<AgentIx, Inventory>,
    }

    /// A player's cargo capacity in the pile lab.
    const CAPACITY: u32 = 12;

    /// The minerals an avatar carries: units by mineral, and their total.
    struct Carried {
        minerals: Vec<u32>,
        units: u32,
    }

    impl std::ops::Deref for Lab {
        type Target = PileLab;
        fn deref(&self) -> &PileLab {
            &self.lab
        }
    }

    impl Lab {
        /// The lab and an avatar's cargo, made empty on first use.
        fn parts(&mut self, agent: AgentIx) -> (&mut PileLab, &mut Inventory) {
            (&mut self.lab, self.cargo.entry(agent).or_insert_with(|| Inventory::new(MINERALS, CAPACITY, Kinds::AnyMix)))
        }

        fn add(&mut self, id: PileId, units: u32) -> Result<u32, &'static str> {
            self.lab.add(id, units)
        }

        /// Starts a haul as the command does: plans it and holds its units
        /// on both sides. Returns its carry job.
        fn start(&mut self, agent: AgentIx, action: PileAction) -> Result<CarryJob, &'static str> {
            let (lab, cargo) = self.parts(agent);
            let (other, item, direction, remaining) = lab.plan(cargo, action)?;
            let other = other.unwrap_or_else(|| match action.drop_position() {
                Some(p) => InventoryRef::Pile(lab.insert(item, p, 0, None)),
                None => unreachable!("only a drop makes its pile"),
            });
            let job = CarryJob { other, item, direction, remaining, next_tick: 0 };
            assert!(job.reserve(cargo, Side::Own) && job.reserve(lab.holder_mut(other), Side::Other));
            Ok(job)
        }

        /// A whole haul at once: started, its units moved, and piles left
        /// empty gone. Returns its other side and units.
        fn haul(&mut self, agent: AgentIx, action: PileAction) -> Result<(InventoryRef, u32), &'static str> {
            let job = self.start(agent, action)?;
            let (lab, cargo) = self.parts(agent);
            let (item, units) = (job.item, job.remaining);
            match job.direction {
                Direction::Pickup => {
                    lab.holder_mut(job.other).take(item, units);
                    cargo.put(item, units);
                }
                Direction::Deposit => {
                    cargo.take(item, units);
                    lab.holder_mut(job.other).put(item, units);
                }
            }
            lab.prune(&std::collections::BTreeSet::new());
            Ok((job.other, units))
        }

        fn pickup(&mut self, agent: AgentIx, id: PileId) -> Result<u32, &'static str> {
            self.haul(agent, PileAction::Pickup(id)).map(|(_, n)| n)
        }

        fn deposit(&mut self, agent: AgentIx, id: PileId) -> Result<u32, &'static str> {
            self.haul(agent, PileAction::Deposit(id)).map(|(_, n)| n)
        }

        fn drop_new(&mut self, agent: AgentIx, position: FxVec2) -> Result<PileId, &'static str> {
            match self.haul(agent, PileAction::Drop(position))?.0 {
                InventoryRef::Pile(id) => Ok(id),
                other => unreachable!("a drop makes a pile, not {other:?}"),
            }
        }

        fn cargo(&self, agent: AgentIx) -> Carried {
            let minerals = self.cargo.get(&agent).map_or(vec![0; MINERALS], |c| self.lab.carried_by_mineral(c));
            Carried { units: minerals.iter().sum(), minerals }
        }

        /// Mineral units in piles, cargo and the collector (nothing flies
        /// here).
        fn total(&self) -> u64 {
            let lab = &self.lab;
            let carried = self.cargo.values().map(|c| lab.carried(c) as u64).sum::<u64>();
            lab.piles.values().map(|p| p.units() as u64).sum::<u64>() + carried + lab.collected().iter().map(|&n| n as u64).sum::<u64>()
        }

        /// Every unit set up or dispensed is somewhere.
        fn conserved(&self) -> bool {
            self.total() == self.lab.created
        }
    }

    fn lab() -> Lab {
        let center = FxVec2::ZERO;
        let lab = PileLab::new(
            center,
            MINERALS,
            0,
            &[(0, FxVec2::new(Fx::from_int(-2), Fx::ZERO))],
            &[(1, FxVec2::new(Fx::from_int(3), Fx::ZERO), 24)],
        );
        Lab { lab, cargo: BTreeMap::new() }
    }

    /// From the original game's tests.
    #[test]
    fn transfers_conserve_units_respect_capacity_and_keep_materials_separate() {
        let mut s = lab();
        assert_eq!(s.add(0, 20), Ok(20));
        assert_eq!(s.pickup(A, 0), Ok(12));
        assert_eq!(s.piles[&0].units(), 8);
        let before = s.total();
        assert!(s.pickup(A, 1).is_err());
        assert!(s.deposit(A, 1).is_err());
        assert_eq!(s.total(), before);
        let destination = s.drop_new(A, FxVec2::new(Fx::ONE, Fx::from_int(2))).unwrap();
        assert_eq!(s.piles[&destination].units(), 12);
        assert_eq!(s.cargo(A).units, 0);
        assert_eq!(s.pickup(A, 0), Ok(8));
        assert_eq!(s.deposit(A, destination), Ok(8));
        assert_eq!(s.piles[&destination].units(), 20);
        assert!(s.deposit(A, destination).is_err());
        assert!(s.conserved());
    }

    #[test]
    fn partial_transfers_and_empty_pile_removal_never_destroy_material() {
        let mut s = lab();
        let spot = FxVec2::new(Fx::ONE, Fx::from_int(2));
        s.add(0, 200).unwrap();
        assert_eq!(s.piles[&0].units(), PILE_CAPACITY);
        s.pickup(A, 0).unwrap();
        let id = s.drop_new(A, spot).unwrap();
        s.add(0, 200).unwrap();
        s.pickup(A, id).unwrap();
        assert!(!s.piles.contains_key(&id));
        assert!(s.deposit(A, 0).is_err());
        assert_eq!(s.cargo(A).units, 12);
        let id = s.drop_new(A, spot).unwrap();
        s.pickup(A, 0).unwrap();
        s.deposit(A, id).unwrap();
        s.pickup(A, id).unwrap();
        s.add(0, 7).unwrap();
        assert_eq!(s.deposit(A, 0), Ok(5));
        assert_eq!(s.cargo(A).units, 7);
        assert!(s.conserved());
    }

    /// Two avatars carry their own loads: one can't take from or tip into
    /// the other's, and units stay conserved across both.
    #[test]
    fn every_avatar_carries_its_own_load() {
        let mut s = lab();
        const B: AgentIx = 1;
        s.add(0, 20).unwrap();
        assert_eq!(s.pickup(A, 0), Ok(12));
        assert_eq!(s.pickup(B, 1), Ok(12));
        assert_eq!((s.cargo(A).minerals, s.cargo(B).minerals), (vec![12, 0, 0], vec![0, 12, 0]));
        assert_eq!(s.pickup(B, 0), Err("Carrier is full. Deposit before collecting more"));
        assert_eq!(s.deposit(A, 0), Ok(12));
        assert_eq!((s.cargo(A).units, s.cargo(B).units), (0, 12));
        assert!(s.conserved());
    }

    /// A cargo takes a second mineral beside the first, up to its one
    /// capacity; each goes back only to a pile of its own mineral, and a
    /// drop puts down the first.
    #[test]
    fn a_cargo_mixes_minerals_within_its_capacity() {
        let mut s = lab();
        s.add(0, 5).unwrap();
        assert_eq!(s.pickup(A, 0), Ok(5));
        assert_eq!(s.pickup(A, 1), Ok(7), "the second mineral fills the rest");
        assert_eq!(s.cargo(A).minerals, vec![5, 7, 0]);
        assert_eq!(s.piles[&1].units(), 17);
        assert_eq!(s.deposit(A, 1), Ok(7), "only blue crystal goes on its pile");
        assert_eq!(s.cargo(A).minerals, vec![5, 0, 0]);
        let id = s.drop_new(A, FxVec2::new(Fx::ONE, Fx::from_int(2))).unwrap();
        assert_eq!((s.lab.pile_mineral(&s.piles[&id]), s.piles[&id].units()), (Some(0), 5));
        assert_eq!(s.cargo(A).units, 0);
        assert!(s.conserved());
    }

    /// A drop that names its mineral puts down that one, all of it, and
    /// leaves the rest; it is refused for a mineral not carried or unknown.
    #[test]
    fn a_drop_puts_down_the_mineral_it_names() {
        let mut s = lab();
        s.add(0, 5).unwrap();
        s.pickup(A, 0).unwrap();
        s.pickup(A, 1).unwrap();
        assert_eq!(s.cargo(A).minerals, vec![5, 7, 0]);
        let spot = FxVec2::new(Fx::ONE, Fx::from_int(2));
        assert_eq!(s.start(A, PileAction::DropMineral(spot, 2)), Err("That mineral isn't carried"));
        assert_eq!(s.start(A, PileAction::DropMineral(spot, MINERALS as u8)), Err("Unknown mineral"));
        let (other, units) = s.haul(A, PileAction::DropMineral(spot, 1)).unwrap();
        let InventoryRef::Pile(id) = other else { panic!("a drop makes a pile") };
        assert_eq!((s.lab.pile_mineral(&s.piles[&id]), units), (Some(1), 7));
        assert_eq!(s.cargo(A).minerals, vec![5, 0, 0]);
        // The old drop still puts down the first mineral carried.
        let id = s.drop_new(A, FxVec2::new(Fx::from_int(-1), Fx::from_int(2))).unwrap();
        assert_eq!((s.lab.pile_mineral(&s.piles[&id]), s.piles[&id].units()), (Some(0), 5));
        assert_eq!(s.start(A, PileAction::DropMineral(spot, 0)), Err("Carrier is empty"));
        assert!(s.conserved());
    }

    /// A haul holds its units when it starts: a second haul for the same
    /// units gets what is left, then nothing; giving a haul's units back
    /// offers them again. A drop's new pile holds room for its units.
    #[test]
    fn hauls_hold_their_units_when_they_start() {
        let mut s = lab();
        const B: AgentIx = 1;
        const C: AgentIx = 2;
        // Pile 1: 24 units of blue crystal.
        let a = s.start(A, PileAction::Pickup(1)).unwrap();
        assert_eq!(a.remaining, 12);
        assert_eq!(s.start(B, PileAction::PickupAmount(1, 13)), Err("Requested quantity exceeds pile stock"));
        assert_eq!(s.start(B, PileAction::Pickup(1)).unwrap().remaining, 12);
        assert_eq!(s.start(C, PileAction::Pickup(1)), Err("No units available"));
        assert_eq!(s.piles[&1].inventory.reserved_out[1], 24);
        let (l, cargo) = s.parts(A);
        a.release(cargo, Side::Own);
        a.release(l.holder_mut(a.other), Side::Other);
        assert_eq!(s.start(C, PileAction::PickupAmount(1, 5)).unwrap().remaining, 5, "A's units are offered again");
        for agent in [A, B, C] {
            s.cargo.get(&agent).map(|c| c.validate().unwrap());
        }
        s.piles[&1].inventory.validate().unwrap();

        // A drop's pile is made empty, holding room for the drop.
        let mut s = lab();
        s.pickup(A, 1).unwrap();
        let drop = s.start(A, PileAction::Drop(FxVec2::new(Fx::ONE, Fx::from_int(2)))).unwrap();
        let InventoryRef::Pile(id) = drop.other else { panic!("a drop makes a pile") };
        assert_eq!((s.piles[&id].units(), s.piles[&id].inventory.reserved_in[1]), (0, 12));
        s.lab.prune(&std::collections::BTreeSet::new());
        assert!(s.piles.contains_key(&id), "a pile holding room stays");
    }

    #[test]
    fn pile_cells_are_unique_and_supported_and_radius_grows() {
        let cells = unit_cells();
        assert_eq!(cells.len(), PILE_CAPACITY as usize);
        let mut occupied = std::collections::BTreeSet::new();
        for &(x, y, z) in cells {
            if y > 0 {
                assert!(occupied.contains(&(x, y - 1, z)));
            }
            assert!(occupied.insert((x, y, z)));
        }
        assert_eq!(pile_radius(0), Fx::from_ratio(7, 20));
        assert!(pile_radius(96) > pile_radius(12));
    }
}
