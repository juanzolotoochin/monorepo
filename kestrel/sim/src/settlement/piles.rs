//! The pile lab: loose mineral piles, dispensers that add to their output
//! pile, and a player who walks to a pile and tosses units one at a time.
//! Ported from the original Bevy app (`piles/` and `pile_model.rs`). In a
//! mine, dug ore lands in piles beside the pit, and a collector takes any
//! mineral delivered to it.
//!
//! Every unit is conserved: `created` equals the units in piles, cargo and
//! the collector, checked by [`PileLab::conserved`]. Positions are world tiles.

use std::collections::BTreeMap;

use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::avatar::{character_of, Intent};
use super::combat::{Event, TossEnd};
use super::haulers::AgentState;
use super::{AgentIx, Settlement};
use crate::messages::PileStep;
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
    pub material: u8,
    pub position: FxVec2,
    pub units: u32,
    /// A dispenser's output pile: it stays when empty, and only it can grow
    /// from nothing.
    pub source: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cargo {
    pub material: Option<u8>,
    pub units: u32,
    pub capacity: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PileAction {
    Pickup(PileId),
    PickupAmount(PileId, u32),
    Deposit(PileId),
    /// Starts a new pile here.
    Drop(FxVec2),
    /// Empties the cargo into the collector.
    Deliver,
}

/// Takes any mineral delivered to it, and keeps count.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collector {
    pub position: FxVec2,
    /// Units delivered, by mineral.
    pub stored: Vec<u32>,
}

impl Collector {
    pub fn new(position: FxVec2) -> Collector {
        Collector { position, stored: vec![0; crate::mine::MINERALS] }
    }
}

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
    pub next_toss: u64,
    /// Units left once the transfer has started.
    pub remaining: Option<u32>,
    pub transferred: u32,
    pub planned: u32,
    /// A drop's new pile, once its first unit landed.
    pub destination: Option<PileId>,
}

/// One avatar's carrier and haul.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hauler {
    pub cargo: Cargo,
    pub job: Option<PileJob>,
    /// Counts started jobs, so a script can tell its job from a later one.
    pub job_serial: u64,
    /// Landing ticks of units in the air.
    pub in_flight: Vec<u64>,
    pub notice: String,
    pub last_error: Option<String>,
}

impl Hauler {
    fn new(capacity: u32) -> Hauler {
        Hauler {
            cargo: Cargo { material: None, units: 0, capacity },
            job: None,
            job_serial: 0,
            in_flight: Vec::new(),
            notice: String::new(),
            last_error: None,
        }
    }

    fn fail(&mut self, error: &str) {
        self.job = None;
        self.notice = error.to_string();
        self.last_error = Some(error.to_string());
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PileLab {
    pub piles: BTreeMap<PileId, Pile>,
    pub created: u64,
    pub next_id: PileId,
    /// The arena center, for the drop limit.
    pub center: FxVec2,
    /// Units each avatar can carry.
    pub capacity: u32,
    /// Each avatar's carrier and haul, made on its first haul.
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
    pub fn new(center: FxVec2, capacity: u32, dispensers: &[(u8, FxVec2)], piles: &[(u8, FxVec2, u32)]) -> PileLab {
        let mut lab = PileLab {
            piles: BTreeMap::new(),
            created: 0,
            next_id: 0,
            center,
            capacity,
            haulers: BTreeMap::new(),
            limit: DROP_LIMIT,
            max_piles: MAX_PILES,
            collector: None,
        };
        for &(material, output) in dispensers {
            lab.insert(material, output, 0, true);
        }
        for &(material, position, units) in piles {
            lab.insert(material, position, units, false);
            lab.created += units as u64;
        }
        lab
    }

    fn insert(&mut self, material: u8, position: FxVec2, units: u32, source: bool) -> PileId {
        let id = self.next_id;
        self.next_id += 1;
        self.piles.insert(id, Pile { material, position, units, source });
        id
    }

    /// An avatar's hauler, if it has hauled.
    pub fn hauler(&self, agent: AgentIx) -> Option<&Hauler> {
        self.haulers.get(&agent)
    }

    /// An avatar's hauler, made empty on first use.
    pub fn hauler_mut(&mut self, agent: AgentIx) -> &mut Hauler {
        let capacity = self.capacity;
        self.haulers.entry(agent).or_insert_with(|| Hauler::new(capacity))
    }

    /// What an avatar carries (empty before its first haul).
    pub fn cargo(&self, agent: AgentIx) -> Cargo {
        self.hauler(agent).map_or(Cargo { material: None, units: 0, capacity: self.capacity }, |h| h.cargo.clone())
    }

    pub fn total(&self) -> u64 {
        let collected = self.collector.as_ref().map_or(0, |c| c.stored.iter().map(|&n| n as u64).sum::<u64>());
        let carried = self.haulers.values().map(|h| h.cargo.units as u64).sum::<u64>();
        self.piles.values().map(|p| p.units as u64).sum::<u64>() + carried + collected
    }

    pub fn conserved(&self) -> bool {
        self.total() == self.created
    }

    /// A dispenser adds up to `requested` units to its output pile.
    pub fn add(&mut self, id: PileId, requested: u32) -> Result<u32, &'static str> {
        let pile = self.piles.get_mut(&id).ok_or("Pile no longer exists")?;
        if !pile.source {
            return Err("Only dispensers create mineral");
        }
        let amount = requested.min(PILE_CAPACITY - pile.units);
        if amount == 0 {
            return Err("Output pile is full (96 units)");
        }
        pile.units += amount;
        self.created += amount as u64;
        Ok(amount)
    }

    /// Exact: a rejected request changes nothing.
    pub fn pickup_amount(&mut self, agent: AgentIx, id: PileId, amount: u32) -> Result<u32, &'static str> {
        let capacity = self.capacity;
        let cargo = &mut self.haulers.entry(agent).or_insert_with(|| Hauler::new(capacity)).cargo;
        let pile = self.piles.get_mut(&id).ok_or("Pile no longer exists")?;
        if cargo.material.is_some_and(|m| m != pile.material) {
            return Err("Unload before collecting a different mineral");
        }
        if amount == 0 {
            return Err("No units available or carrier full");
        }
        if amount > pile.units {
            return Err("Requested quantity exceeds pile stock");
        }
        if amount > cargo.capacity - cargo.units {
            return Err("Requested quantity exceeds free carrying capacity");
        }
        cargo.material = Some(pile.material);
        cargo.units += amount;
        pile.units -= amount;
        if pile.units == 0 && !pile.source {
            self.piles.remove(&id);
        }
        Ok(amount)
    }

    pub fn pickup(&mut self, agent: AgentIx, id: PileId) -> Result<u32, &'static str> {
        let units = self.piles.get(&id).ok_or("Pile no longer exists")?.units;
        let cargo = self.cargo(agent);
        self.pickup_amount(agent, id, units.min(cargo.capacity - cargo.units))
    }

    pub fn deposit_amount(&mut self, agent: AgentIx, id: PileId, amount: u32) -> Result<u32, &'static str> {
        let capacity = self.capacity;
        let cargo = &mut self.haulers.entry(agent).or_insert_with(|| Hauler::new(capacity)).cargo;
        let material = cargo.material.ok_or("Carrier is empty")?;
        let pile = self.piles.get_mut(&id).ok_or("Pile no longer exists")?;
        if pile.material != material {
            return Err("Different minerals stay in separate piles");
        }
        if amount == 0 || amount > cargo.units || amount > PILE_CAPACITY - pile.units {
            return Err("Requested deposit does not fit cargo or pile capacity");
        }
        pile.units += amount;
        unload(cargo, amount);
        Ok(amount)
    }

    pub fn deposit(&mut self, agent: AgentIx, id: PileId) -> Result<u32, &'static str> {
        let units = self.piles.get(&id).ok_or("Pile no longer exists")?.units;
        self.deposit_amount(agent, id, self.cargo(agent).units.min(PILE_CAPACITY - units))
    }

    pub fn drop_new_amount(&mut self, agent: AgentIx, position: FxVec2, amount: u32) -> Result<PileId, &'static str> {
        let cargo = self.cargo(agent);
        let material = cargo.material.ok_or("Carrier is empty")?;
        let offset = position - self.center;
        if offset.x.abs() > self.limit || offset.y.abs() > self.limit {
            return Err("Drop position must be on the deck");
        }
        if self.piles.len() >= self.max_piles {
            return Err("Pile limit reached; deposit into an existing pile");
        }
        if amount == 0 || amount > cargo.units {
            return Err("Requested quantity exceeds cargo");
        }
        let id = self.insert(material, position, amount, false);
        unload(&mut self.hauler_mut(agent).cargo, amount);
        Ok(id)
    }

    pub fn drop_new(&mut self, agent: AgentIx, position: FxVec2) -> Result<PileId, &'static str> {
        self.drop_new_amount(agent, position, self.cargo(agent).units)
    }

    /// Exact: puts `amount` carried units in the collector.
    pub fn deliver_amount(&mut self, agent: AgentIx, amount: u32) -> Result<u32, &'static str> {
        let capacity = self.capacity;
        let cargo = &mut self.haulers.entry(agent).or_insert_with(|| Hauler::new(capacity)).cargo;
        let material = cargo.material.ok_or("Carrier is empty")?;
        let collector = self.collector.as_mut().ok_or("There is no collector here")?;
        if amount == 0 || amount > cargo.units {
            return Err("Requested delivery exceeds cargo");
        }
        collector.stored[material as usize] += amount;
        unload(cargo, amount);
        Ok(amount)
    }

    /// Dug ore: `units` of `material` out of the pit at `tile`, onto a pile
    /// of the same ore beside it with room, or a new one on the nearest open
    /// tile around. Returns the pile and the slot the first unit lands in,
    /// or nothing if there is nowhere to put it (the ore stays buried).
    pub fn mined(&mut self, material: u8, units: u32, tile: (i32, i32), terrain: &Terrain) -> Option<(PileId, u32)> {
        let pit = Terrain::pit_center(tile);
        let beside = self
            .piles
            .iter()
            .filter(|(_, p)| !p.source && p.material == material && p.units + units <= PILE_CAPACITY)
            .map(|(&id, p)| (nav::distance(p.position, pit), id))
            .filter(|&(d, _)| d < Fx::from_ratio(11, 5))
            .min();
        if let Some((_, id)) = beside {
            let pile = self.piles.get_mut(&id).expect("found above");
            let slot = pile.units;
            pile.units += units;
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
                && self.piles.values().all(|p| nav::distance(p.position, center) > pile_radius(p.units) + radius + Fx::from_ratio(1, 10))
        })?;
        let id = self.insert(material, Terrain::pit_center(spot), units, false);
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
    /// will be).
    fn target(&self, agent: AgentIx, action: PileAction) -> Result<(FxVec2, Fx), &'static str> {
        let carried = self.cargo(agent).units;
        match action {
            PileAction::Pickup(id) | PileAction::PickupAmount(id, _) | PileAction::Deposit(id) => {
                let p = self.piles.get(&id).ok_or("Pile no longer exists")?;
                let units = if matches!(action, PileAction::Deposit(_)) { (p.units + carried).min(PILE_CAPACITY) } else { p.units };
                Ok((p.position, pile_radius(units)))
            }
            PileAction::Drop(p) => Ok((p, pile_radius(carried))),
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

/// Takes `amount` units out of a carrier.
fn unload(cargo: &mut Cargo, amount: u32) {
    cargo.units -= amount;
    if cargo.units == 0 {
        cargo.material = None;
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
    /// (zero for all at once); `approach` picks the standing spot.
    pub fn pile_action(&mut self, tick: u64, terrain: &Terrain, agent: usize, action: PileAction, pace: Fx, approach: Option<FxVec2>) {
        let Some(lab) = self.piles.as_mut() else { return };
        let Some(a) = self.agents.get_mut(agent).filter(|a| a.controlled) else { return };
        let body = character_of(&self.characters, a).body.radius;
        let me = agent as AgentIx;
        let carried = lab.cargo(me);
        let result = (|| {
            if pace < Fx::ZERO || pace > Fx::from_int(5) {
                return Err("Invalid transfer interval");
            }
            let hauler = lab.hauler_mut(me);
            if !hauler.in_flight.is_empty() {
                return Err("Wait for airborne units to land before starting another transfer");
            }
            hauler.job_serial += 1;
            hauler.job = None;
            hauler.last_error = None;
            let (center, radius) = lab.target(me, action)?;
            if matches!(action, PileAction::Drop(_)) && !lab.free_drop(terrain, center, carried.units) {
                return Err("Choose open floor away from piles, machines and the deck edge");
            }
            if matches!(action, PileAction::Pickup(_) | PileAction::PickupAmount(..)) && carried.units == carried.capacity {
                return Err("Carrier is full. Deposit before collecting more");
            }
            if action == PileAction::Deliver && carried.units == 0 {
                return Err("Carrier is empty. Collect ore before delivering");
            }
            let route = match approach {
                Some(stand) => {
                    let gap = nav::distance(stand, center);
                    if gap < radius + body + Fx::from_ratio(1, 25) || gap > radius + body + Fx::from_ratio(9, 20) {
                        return Err("Approach point must be within safe transfer reach");
                    }
                    let path = walk_path(terrain, a.position, stand, body);
                    if path.is_empty() {
                        return Err("Approach point is blocked");
                    }
                    path
                }
                None => approach_route(terrain, a.position, center, radius, body).ok_or("No clear route there on foot")?,
            };
            let pace = if pace == Fx::ZERO { 0 } else { (pace * Fx::from_int(content::TICKS_PER_SECOND)).round().floor_to_int().max(1) as u64 };
            follow(a, route);
            let hauler = lab.hauler_mut(me);
            hauler.job = Some(PileJob {
                action,
                center,
                radius,
                age: 0,
                stalled: 0,
                previous: a.position,
                pace,
                next_toss: tick,
                remaining: None,
                transferred: 0,
                planned: 0,
                destination: None,
            });
            hauler.notice = if matches!(action, PileAction::Pickup(_) | PileAction::PickupAmount(..)) {
                "Walking to collect mineral…"
            } else if action == PileAction::Deliver {
                "Walking to the collector…"
            } else {
                "Walking to deposit cargo…"
            }
            .into();
            Ok(())
        })();
        if let Err(error) = result {
            let hauler = lab.hauler_mut(me);
            hauler.notice = error.into();
            hauler.last_error = Some(error.into());
        }
    }

    /// Commands: a dispenser adds units to its output pile. Everyone hears
    /// about it.
    pub fn dispense(&mut self, dispenser: PileId, units: u32) {
        let players: Vec<AgentIx> = (0..self.agents.len() as AgentIx).filter(|&i| self.agents[i as usize].controlled).collect();
        let Some(lab) = self.piles.as_mut() else { return };
        let notice = match lab.add(dispenser, units) {
            Ok(n) => format!("Dispenser added {n} units to its output pile."),
            Err(e) => e.into(),
        };
        for agent in players {
            lab.hauler_mut(agent).notice = notice.clone();
        }
    }

    /// The avatar moved or attacked: its haul stops, its cargo stays.
    pub(super) fn cancel_pile_job(&mut self, agent: usize) {
        let Some(hauler) = self.piles.as_mut().and_then(|lab| lab.haulers.get_mut(&(agent as AgentIx))) else { return };
        if hauler.job.take().is_some() {
            hauler.notice = "Haul canceled. Cargo is retained.".into();
            hauler.last_error = Some("Haul canceled by player input".into());
        }
    }

    /// Stage 2, for an avatar: walk toward its pile, replanning when stuck.
    pub(super) fn pile_job_self_update(lab: &PileLab, me: AgentIx, a: &mut AgentState, terrain: &Terrain, body: Fx) -> Option<&'static str> {
        let job = lab.hauler(me)?.job.as_ref()?;
        if job.remaining.is_some() || in_reach(a.position, job, body) {
            a.route.clear();
            a.avatar.velocity = FxVec2::ZERO;
            return None;
        }
        if job.age > JOB_TIMEOUT_TICKS {
            return Some("Haul timed out. Cargo retained; choose another approach.");
        }
        if a.route.is_empty() || job.stalled > STALL_TICKS {
            match approach_route(terrain, a.position, job.center, job.radius, body) {
                Some(route) => follow(a, route),
                None => return Some("Route blocked. Cargo retained."),
            }
        }
        None
    }

    /// Housekeeping for every haul's clocks: age, stalls, landings, completion.
    pub(crate) fn pile_housekeeping(&mut self, tick: u64) {
        let failures: Vec<(AgentIx, String)> = self
            .agents
            .iter_mut()
            .enumerate()
            .filter_map(|(i, a)| Some((i as AgentIx, a.avatar.pile_failure.take()?)))
            .collect();
        let Some(lab) = self.piles.as_mut() else { return };
        for (agent, error) in failures {
            lab.hauler_mut(agent).fail(&error);
        }
        for (&agent, hauler) in lab.haulers.iter_mut() {
            hauler.in_flight.retain(|&landing| landing > tick);
            let Some(job) = hauler.job.as_mut() else { continue };
            let position = self.agents[agent as usize].position;
            job.age += 1;
            job.stalled = if nav::distance(position, job.previous) < Fx::from_ratio(3, 1000) { job.stalled + 1 } else { 0 };
            job.previous = position;
            if job.remaining == Some(0) && hauler.in_flight.is_empty() {
                hauler.notice = format!("Transferred {} units. All items landed.", job.transferred);
                hauler.job = None;
            }
        }
    }

    /// Stage 3: the avatar is in reach and a unit (or the whole amount) is due.
    pub(super) fn read_pile_job(&self, me: AgentIx, a: &AgentState, tick: u64) -> Option<PileStep> {
        let job = self.piles.as_ref()?.hauler(me)?.job.as_ref()?;
        let due = job.remaining != Some(0) && tick >= job.next_toss && (job.remaining.is_some() || in_reach(a.position, job, character_of(&self.characters, a).body.radius));
        due.then_some(PileStep { agent: me })
    }

    /// Moves one unit of an avatar's job (or all of it at pace zero).
    pub(crate) fn pile_step(&mut self, tick: u64, terrain: &Terrain, agent: AgentIx) {
        let mut tosses = Vec::new();
        self.pile_step_inner(tick, terrain, agent, &mut tosses);
        self.events.extend(tosses);
    }

    fn pile_step_inner(&mut self, tick: u64, terrain: &Terrain, me: AgentIx, tosses: &mut Vec<Event>) {
        let Some(lab) = self.piles.as_mut() else { return };
        let Some(mut job) = lab.hauler(me).and_then(|h| h.job.clone()) else { return };
        let result = (|| -> Result<Option<String>, &'static str> {
            let cargo = lab.cargo(me);
            if job.pace == 0 {
                let carrying = |lab: &PileLab| {
                    let c = lab.cargo(me);
                    format!("Carrying {}/{}.", c.units, c.capacity)
                };
                let message = match job.action {
                    PileAction::Pickup(id) => lab.pickup(me, id).map(|n| format!("Collected {n} units. {}", carrying(lab))),
                    PileAction::PickupAmount(id, n) => lab.pickup_amount(me, id, n).map(|n| format!("Collected {n} units. {}", carrying(lab))),
                    PileAction::Deposit(id) => lab.deposit(me, id).map(|n| format!("Deposited {n} units. {} still carried.", lab.cargo(me).units)),
                    PileAction::Drop(p) => {
                        if !lab.free_drop(terrain, p, cargo.units) {
                            return Err("Drop location is now blocked. Cargo retained.");
                        }
                        lab.drop_new(me, p).map(|_| format!("Placed a new pile of {} units.", cargo.units))
                    }
                    PileAction::Deliver => lab.deliver_amount(me, cargo.units).map(|n| format!("Delivered {n} units to the collector.")),
                }?;
                return Ok(Some(message));
            }
            if job.remaining.is_none() {
                let amount = match job.action {
                    PileAction::Pickup(id) => {
                        let pile = lab.piles.get(&id).ok_or("Pile disappeared")?;
                        pile.units.min(cargo.capacity - cargo.units)
                    }
                    PileAction::PickupAmount(id, n) => {
                        let pile = lab.piles.get(&id).ok_or("Pile disappeared")?;
                        if pile.units < n || cargo.capacity - cargo.units < n {
                            return Err("Exact pickup quantity is no longer available");
                        }
                        n
                    }
                    PileAction::Deposit(id) => {
                        let pile = lab.piles.get(&id).ok_or("Pile disappeared")?;
                        cargo.units.min(PILE_CAPACITY - pile.units)
                    }
                    PileAction::Drop(p) => {
                        if !lab.free_drop(terrain, p, cargo.units) {
                            return Err("Drop location is blocked");
                        }
                        cargo.units
                    }
                    PileAction::Deliver => cargo.units,
                };
                if amount == 0 {
                    return Err("Nothing can be transferred");
                }
                job.remaining = Some(amount);
                job.planned = amount;
            }
            let (material, carried) = (cargo.material, cargo.units);
            let into_cargo = TossEnd::Cargo { agent: me, slot: carried };
            let out_of_cargo = TossEnd::Cargo { agent: me, slot: carried.wrapping_sub(1) };
            let (from, to, material) = match job.action {
                PileAction::Pickup(id) | PileAction::PickupAmount(id, _) => {
                    let pile = lab.piles.get(&id).ok_or("Pile disappeared")?;
                    let (slot, material, position) = (pile.units.checked_sub(1).ok_or("Pile is empty")?, pile.material, pile.position);
                    lab.pickup_amount(me, id, 1)?;
                    (TossEnd::Pile { id, slot, position }, into_cargo, material)
                }
                PileAction::Deposit(id) => {
                    let pile = lab.piles.get(&id).ok_or("Pile disappeared")?;
                    let (slot, position) = (pile.units, pile.position);
                    lab.deposit_amount(me, id, 1)?;
                    (out_of_cargo, TossEnd::Pile { id, slot, position }, material.expect("deposited"))
                }
                PileAction::Drop(p) => {
                    let (id, slot) = match job.destination {
                        Some(id) => {
                            let slot = lab.piles.get(&id).ok_or("Pile disappeared")?.units;
                            lab.deposit_amount(me, id, 1)?;
                            (id, slot)
                        }
                        None => {
                            let id = lab.drop_new_amount(me, p, 1)?;
                            job.destination = Some(id);
                            (id, 0)
                        }
                    };
                    (out_of_cargo, TossEnd::Pile { id, slot, position: p }, material.expect("dropped"))
                }
                PileAction::Deliver => {
                    lab.deliver_amount(me, 1)?;
                    let position = lab.collector.as_ref().expect("delivered").position;
                    (out_of_cargo, TossEnd::Collector { position }, material.expect("delivered"))
                }
            };
            // The unit flies for 0.85 × the pace, clamped to 0.3–1.0 s.
            let flight = (job.pace.clamp(content::millis_to_ticks(300), content::millis_to_ticks(1000)) * 85).div_ceil(100);
            lab.hauler_mut(me).in_flight.push(tick + flight);
            tosses.push(Event::PileToss { material, from, to, ticks: flight });
            job.transferred += 1;
            *job.remaining.as_mut().expect("set above") -= 1;
            job.next_toss = tick + job.pace;
            Ok(None)
        })();
        let hauler = lab.hauler_mut(me);
        match result {
            Ok(Some(message)) => {
                hauler.notice = message;
                hauler.last_error = None;
                hauler.job = None;
            }
            Ok(None) => {
                hauler.notice = format!("Tossing {} / {} units...", job.transferred, job.planned);
                hauler.job = Some(job);
            }
            Err(error) => hauler.fail(error),
        }
    }
}

/// Points the avatar along a route.
fn follow(a: &mut AgentState, route: Vec<FxVec2>) {
    let target = route.last().copied().unwrap_or(a.position);
    a.route = route;
    a.avatar.swing = None;
    a.avatar.queued = None;
    a.avatar.intent = Some(Intent::GoTo { target, run: false });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hauling avatar.
    const A: AgentIx = 0;

    fn lab() -> PileLab {
        let center = FxVec2::ZERO;
        PileLab::new(
            center,
            12,
            &[(0, FxVec2::new(Fx::from_int(-2), Fx::ZERO))],
            &[(1, FxVec2::new(Fx::from_int(3), Fx::ZERO), 24)],
        )
    }

    /// From the original game's tests.
    #[test]
    fn transfers_conserve_units_respect_capacity_and_keep_materials_separate() {
        let mut s = lab();
        assert_eq!(s.add(0, 20), Ok(20));
        assert_eq!(s.pickup(A, 0), Ok(12));
        assert_eq!(s.piles[&0].units, 8);
        let before = s.total();
        assert!(s.pickup(A, 1).is_err());
        assert!(s.deposit(A, 1).is_err());
        assert_eq!(s.total(), before);
        let destination = s.drop_new(A, FxVec2::new(Fx::ONE, Fx::from_int(2))).unwrap();
        assert_eq!(s.piles[&destination].units, 12);
        assert_eq!(s.cargo(A).units, 0);
        assert_eq!(s.pickup(A, 0), Ok(8));
        assert_eq!(s.deposit(A, destination), Ok(8));
        assert_eq!(s.piles[&destination].units, 20);
        assert!(s.deposit(A, destination).is_err());
        assert!(s.conserved());
    }

    #[test]
    fn partial_transfers_and_empty_pile_removal_never_destroy_material() {
        let mut s = lab();
        let spot = FxVec2::new(Fx::ONE, Fx::from_int(2));
        s.add(0, 200).unwrap();
        assert_eq!(s.piles[&0].units, PILE_CAPACITY);
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
        assert_eq!((s.cargo(A).material, s.cargo(B).material), (Some(0), Some(1)));
        assert_eq!(s.pickup(B, 0), Err("Unload before collecting a different mineral"));
        assert_eq!(s.deposit(A, 0), Ok(12));
        assert_eq!((s.cargo(A).units, s.cargo(B).units), (0, 12));
        assert!(s.conserved());
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
