//! The pile lab: loose mineral piles, dispensers that add to their output
//! pile, and a player who walks to a pile and tosses units one at a time.
//! Ported from the original Bevy app (`piles/` and `pile_model.rs`).
//!
//! Every unit is conserved: `created` equals the units in piles plus cargo,
//! checked by [`PileLab::conserved`]. Positions are world tiles.

use std::collections::BTreeMap;

use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::avatar::{Intent, BODY_RADIUS};
use super::haulers::AgentState;
use super::{AgentIx, Settlement};
use crate::messages::PileStep;
use crate::nav;
use crate::terrain::Terrain;

pub const PILE_CAPACITY: u32 = 96;
pub const MAX_PILES: usize = 32;
/// A walk-and-toss job gives up after 30 s, and replans after 0.8 s without moving.
const JOB_TIMEOUT_TICKS: u64 = 900;
const STALL_TICKS: u64 = 24;
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PileLab {
    pub piles: BTreeMap<PileId, Pile>,
    pub cargo: Cargo,
    pub created: u64,
    pub next_id: PileId,
    /// The arena center, for the drop limit.
    pub center: FxVec2,
    pub job: Option<PileJob>,
    /// Counts started jobs, so a script can tell its job from a later one.
    pub job_serial: u64,
    /// Landing ticks of units in the air.
    pub in_flight: Vec<u64>,
    pub notice: String,
    pub last_error: Option<String>,
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
            cargo: Cargo { material: None, units: 0, capacity },
            created: 0,
            next_id: 0,
            center,
            job: None,
            job_serial: 0,
            in_flight: Vec::new(),
            notice: String::new(),
            last_error: None,
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

    pub fn total(&self) -> u64 {
        self.piles.values().map(|p| p.units as u64).sum::<u64>() + self.cargo.units as u64
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
    pub fn pickup_amount(&mut self, id: PileId, amount: u32) -> Result<u32, &'static str> {
        let pile = self.piles.get_mut(&id).ok_or("Pile no longer exists")?;
        if self.cargo.material.is_some_and(|m| m != pile.material) {
            return Err("Unload before collecting a different mineral");
        }
        if amount == 0 {
            return Err("No units available or carrier full");
        }
        if amount > pile.units {
            return Err("Requested quantity exceeds pile stock");
        }
        if amount > self.cargo.capacity - self.cargo.units {
            return Err("Requested quantity exceeds free carrying capacity");
        }
        self.cargo.material = Some(pile.material);
        self.cargo.units += amount;
        pile.units -= amount;
        if pile.units == 0 && !pile.source {
            self.piles.remove(&id);
        }
        Ok(amount)
    }

    pub fn pickup(&mut self, id: PileId) -> Result<u32, &'static str> {
        let units = self.piles.get(&id).ok_or("Pile no longer exists")?.units;
        self.pickup_amount(id, units.min(self.cargo.capacity - self.cargo.units))
    }

    pub fn deposit_amount(&mut self, id: PileId, amount: u32) -> Result<u32, &'static str> {
        let material = self.cargo.material.ok_or("Carrier is empty")?;
        let pile = self.piles.get_mut(&id).ok_or("Pile no longer exists")?;
        if pile.material != material {
            return Err("Different minerals stay in separate piles");
        }
        if amount == 0 || amount > self.cargo.units || amount > PILE_CAPACITY - pile.units {
            return Err("Requested deposit does not fit cargo or pile capacity");
        }
        pile.units += amount;
        self.cargo.units -= amount;
        if self.cargo.units == 0 {
            self.cargo.material = None;
        }
        Ok(amount)
    }

    pub fn deposit(&mut self, id: PileId) -> Result<u32, &'static str> {
        let units = self.piles.get(&id).ok_or("Pile no longer exists")?.units;
        self.deposit_amount(id, self.cargo.units.min(PILE_CAPACITY - units))
    }

    pub fn drop_new_amount(&mut self, position: FxVec2, amount: u32) -> Result<PileId, &'static str> {
        let material = self.cargo.material.ok_or("Carrier is empty")?;
        let offset = position - self.center;
        if offset.x.abs() > DROP_LIMIT || offset.y.abs() > DROP_LIMIT {
            return Err("Drop position must be on the deck");
        }
        if self.piles.len() >= MAX_PILES {
            return Err("Pile limit reached; deposit into an existing pile");
        }
        if amount == 0 || amount > self.cargo.units {
            return Err("Requested quantity exceeds cargo");
        }
        let id = self.insert(material, position, amount, false);
        self.cargo.units -= amount;
        if self.cargo.units == 0 {
            self.cargo.material = None;
        }
        Ok(id)
    }

    pub fn drop_new(&mut self, position: FxVec2) -> Result<PileId, &'static str> {
        self.drop_new_amount(position, self.cargo.units)
    }

    /// Where an action happens and how big the pile there is (or will be).
    fn target(&self, action: PileAction) -> Result<(FxVec2, Fx), &'static str> {
        match action {
            PileAction::Pickup(id) | PileAction::PickupAmount(id, _) | PileAction::Deposit(id) => {
                let p = self.piles.get(&id).ok_or("Pile no longer exists")?;
                let units = if matches!(action, PileAction::Deposit(_)) {
                    (p.units + self.cargo.units).min(PILE_CAPACITY)
                } else {
                    p.units
                };
                Ok((p.position, pile_radius(units)))
            }
            PileAction::Drop(p) => Ok((p, pile_radius(self.cargo.units))),
        }
    }

    fn free_drop(&self, terrain: &Terrain, center: FxVec2, units: u32) -> bool {
        let radius = pile_radius(units);
        let offset = center - self.center;
        offset.x.abs() <= DROP_LIMIT
            && offset.y.abs() <= DROP_LIMIT
            && terrain.obstacles.iter().all(|o| nav::distance(o.center, center) > o.radius + radius + Fx::from_ratio(1, 5))
    }

    fn fail(&mut self, error: &str) {
        self.job = None;
        self.notice = error.to_string();
        self.last_error = Some(error.to_string());
    }
}

/// The best route to a standing spot within transfer reach of a pile, or
/// an empty route if already there.
fn approach_route(terrain: &Terrain, start: FxVec2, center: FxVec2, radius: Fx) -> Option<Vec<FxVec2>> {
    let stand = radius + BODY_RADIUS + Fx::from_ratio(9, 50);
    let gap = nav::distance(start, center);
    if gap >= radius + BODY_RADIUS + Fx::from_ratio(1, 25) && gap <= stand + Fx::from_ratio(1, 4) {
        return Some(Vec::new());
    }
    (0..24)
        .filter_map(|i| {
            let angle = Fx::TAU.mul_int(i).div_int(24);
            let (sin, cos) = angle.sin_cos();
            let goal = center + FxVec2::new(cos, sin) * stand;
            let route = nav::path(terrain, start, goal, BODY_RADIUS);
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

/// Whether the avatar stands within reach of the job's pile.
fn in_reach(position: FxVec2, job: &PileJob) -> bool {
    let distance = nav::distance(position, job.center);
    distance >= job.radius + BODY_RADIUS + Fx::from_ratio(1, 50) && distance <= job.radius + BODY_RADIUS + Fx::from_ratio(9, 20)
}

impl Settlement {
    /// Commands: walk to a pile and transfer. `pace` is seconds per unit
    /// (zero for all at once); `approach` picks the standing spot.
    pub fn pile_action(&mut self, tick: u64, terrain: &Terrain, agent: usize, action: PileAction, pace: Fx, approach: Option<FxVec2>) {
        let Some(lab) = self.piles.as_mut() else { return };
        let Some(a) = self.agents.get_mut(agent).filter(|a| a.controlled) else { return };
        let result = (|| {
            if pace < Fx::ZERO || pace > Fx::from_int(5) {
                return Err("Invalid transfer interval");
            }
            if !lab.in_flight.is_empty() {
                return Err("Wait for airborne units to land before starting another transfer");
            }
            lab.job_serial += 1;
            lab.job = None;
            lab.last_error = None;
            let (center, radius) = lab.target(action)?;
            if matches!(action, PileAction::Drop(_)) && !lab.free_drop(terrain, center, lab.cargo.units) {
                return Err("Choose open floor away from piles, machines and the deck edge");
            }
            if matches!(action, PileAction::Pickup(_) | PileAction::PickupAmount(..)) && lab.cargo.units == lab.cargo.capacity {
                return Err("Carrier is full. Deposit before collecting more");
            }
            let route = match approach {
                Some(stand) => {
                    let gap = nav::distance(stand, center);
                    if gap < radius + BODY_RADIUS + Fx::from_ratio(1, 25) || gap > radius + BODY_RADIUS + Fx::from_ratio(9, 20) {
                        return Err("Approach point must be within safe transfer reach");
                    }
                    let path = nav::path(terrain, a.position, stand, BODY_RADIUS);
                    if path.is_empty() {
                        return Err("Approach point is blocked");
                    }
                    path
                }
                None => approach_route(terrain, a.position, center, radius).ok_or("No clear route to that pile location")?,
            };
            let pace = if pace == Fx::ZERO { 0 } else { (pace * Fx::from_int(content::TICKS_PER_SECOND)).round().floor_to_int().max(1) as u64 };
            follow(a, route);
            lab.job = Some(PileJob {
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
            lab.notice = if matches!(action, PileAction::Pickup(_) | PileAction::PickupAmount(..)) {
                "Walking to collect mineral…"
            } else {
                "Walking to deposit cargo…"
            }
            .into();
            Ok(())
        })();
        if let Err(error) = result {
            lab.notice = error.into();
            lab.last_error = Some(error.into());
        }
    }

    /// Commands: a dispenser adds units to its output pile.
    pub fn dispense(&mut self, dispenser: PileId, units: u32) {
        let Some(lab) = self.piles.as_mut() else { return };
        let result = lab.add(dispenser, units);
        lab.notice = match result {
            Ok(n) => format!("Dispenser added {n} units to its output pile."),
            Err(e) => e.into(),
        };
    }

    /// The player moved or attacked: a running haul stops, cargo stays.
    pub(super) fn cancel_pile_job(&mut self) {
        if let Some(lab) = self.piles.as_mut() {
            if lab.job.take().is_some() {
                lab.notice = "Haul canceled. Cargo is retained.".into();
                lab.last_error = Some("Haul canceled by player input".into());
            }
        }
    }

    /// Stage 2, for the avatar: walk toward the pile, replanning when stuck.
    pub(super) fn pile_job_self_update(lab: &PileLab, a: &mut AgentState, terrain: &Terrain) -> Option<&'static str> {
        let job = lab.job.as_ref()?;
        if job.remaining.is_some() || in_reach(a.position, job) {
            a.route.clear();
            a.avatar.velocity = FxVec2::ZERO;
            return None;
        }
        if job.age > JOB_TIMEOUT_TICKS {
            return Some("Haul timed out. Cargo retained; choose another approach.");
        }
        if a.route.is_empty() || job.stalled > STALL_TICKS {
            match approach_route(terrain, a.position, job.center, job.radius) {
                Some(route) => follow(a, route),
                None => return Some("Route blocked. Cargo retained."),
            }
        }
        None
    }

    /// Housekeeping for the job's clocks: age, stalls, landings, completion.
    pub(crate) fn pile_housekeeping(&mut self, tick: u64) {
        let Some(player) = self.player() else { return };
        if let Some(error) = self.agents[player as usize].avatar.pile_failure.take() {
            self.fail_pile_job(&error);
        }
        let position = self.agents[player as usize].position;
        let Some(lab) = self.piles.as_mut() else { return };
        lab.in_flight.retain(|&landing| landing > tick);
        let Some(job) = lab.job.as_mut() else { return };
        job.age += 1;
        job.stalled = if nav::distance(position, job.previous) < Fx::from_ratio(3, 1000) { job.stalled + 1 } else { 0 };
        job.previous = position;
        if job.remaining == Some(0) && lab.in_flight.is_empty() {
            lab.notice = format!("Transferred {} units. All items landed.", job.transferred);
            lab.job = None;
        }
    }

    /// Stage 3: the avatar is in reach and a unit (or the whole amount) is due.
    pub(super) fn read_pile_job(&self, me: AgentIx, a: &AgentState, tick: u64) -> Option<PileStep> {
        let job = self.piles.as_ref()?.job.as_ref()?;
        let due = job.remaining != Some(0) && tick >= job.next_toss && (job.remaining.is_some() || in_reach(a.position, job));
        due.then_some(PileStep { agent: me })
    }

    /// Moves one unit of the job (or all of it at pace zero).
    pub(crate) fn pile_step(&mut self, tick: u64, terrain: &Terrain) {
        let Some(lab) = self.piles.as_mut() else { return };
        let Some(mut job) = lab.job.clone() else { return };
        let result = (|| -> Result<Option<String>, &'static str> {
            if job.pace == 0 {
                let message = match job.action {
                    PileAction::Pickup(id) => lab.pickup(id).map(|n| format!("Collected {n} units. Carrying {}/{}.", lab.cargo.units, lab.cargo.capacity)),
                    PileAction::PickupAmount(id, n) => {
                        lab.pickup_amount(id, n).map(|n| format!("Collected {n} units. Carrying {}/{}.", lab.cargo.units, lab.cargo.capacity))
                    }
                    PileAction::Deposit(id) => lab.deposit(id).map(|n| format!("Deposited {n} units. {} still carried.", lab.cargo.units)),
                    PileAction::Drop(p) => {
                        if !lab.free_drop(terrain, p, lab.cargo.units) {
                            return Err("Drop location is now blocked. Cargo retained.");
                        }
                        let n = lab.cargo.units;
                        lab.drop_new(p).map(|_| format!("Placed a new pile of {n} units."))
                    }
                }?;
                return Ok(Some(message));
            }
            if job.remaining.is_none() {
                let amount = match job.action {
                    PileAction::Pickup(id) => {
                        let pile = lab.piles.get(&id).ok_or("Pile disappeared")?;
                        pile.units.min(lab.cargo.capacity - lab.cargo.units)
                    }
                    PileAction::PickupAmount(id, n) => {
                        let pile = lab.piles.get(&id).ok_or("Pile disappeared")?;
                        if pile.units < n || lab.cargo.capacity - lab.cargo.units < n {
                            return Err("Exact pickup quantity is no longer available");
                        }
                        n
                    }
                    PileAction::Deposit(id) => {
                        let pile = lab.piles.get(&id).ok_or("Pile disappeared")?;
                        lab.cargo.units.min(PILE_CAPACITY - pile.units)
                    }
                    PileAction::Drop(p) => {
                        if !lab.free_drop(terrain, p, lab.cargo.units) {
                            return Err("Drop location is blocked");
                        }
                        lab.cargo.units
                    }
                };
                if amount == 0 {
                    return Err("Nothing can be transferred");
                }
                job.remaining = Some(amount);
                job.planned = amount;
            }
            match job.action {
                PileAction::Pickup(id) | PileAction::PickupAmount(id, _) => {
                    lab.pickup_amount(id, 1)?;
                }
                PileAction::Deposit(id) => {
                    lab.deposit_amount(id, 1)?;
                }
                PileAction::Drop(p) => match job.destination {
                    Some(id) => {
                        lab.deposit_amount(id, 1)?;
                    }
                    None => job.destination = Some(lab.drop_new_amount(p, 1)?),
                },
            }
            // The unit flies for 0.85 × the pace, clamped to 0.3–1.0 s.
            let flight = (job.pace.clamp(9, 30) * 85).div_ceil(100);
            lab.in_flight.push(tick + flight);
            job.transferred += 1;
            *job.remaining.as_mut().expect("set above") -= 1;
            job.next_toss = tick + job.pace;
            Ok(None)
        })();
        match result {
            Ok(Some(message)) => {
                lab.notice = message;
                lab.last_error = None;
                lab.job = None;
            }
            Ok(None) => {
                lab.notice = format!("Tossing {} / {} units...", job.transferred, job.planned);
                lab.job = Some(job);
            }
            Err(error) => lab.fail(error),
        }
    }

    /// Stops a job that failed while walking.
    pub(crate) fn fail_pile_job(&mut self, error: &str) {
        if let Some(lab) = self.piles.as_mut() {
            lab.fail(error);
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
        assert_eq!(s.pickup(0), Ok(12));
        assert_eq!(s.piles[&0].units, 8);
        let before = s.total();
        assert!(s.pickup(1).is_err());
        assert!(s.deposit(1).is_err());
        assert_eq!(s.total(), before);
        let destination = s.drop_new(FxVec2::new(Fx::ONE, Fx::from_int(2))).unwrap();
        assert_eq!(s.piles[&destination].units, 12);
        assert_eq!(s.cargo.units, 0);
        assert_eq!(s.pickup(0), Ok(8));
        assert_eq!(s.deposit(destination), Ok(8));
        assert_eq!(s.piles[&destination].units, 20);
        assert!(s.deposit(destination).is_err());
        assert!(s.conserved());
    }

    #[test]
    fn partial_transfers_and_empty_pile_removal_never_destroy_material() {
        let mut s = lab();
        let spot = FxVec2::new(Fx::ONE, Fx::from_int(2));
        s.add(0, 200).unwrap();
        assert_eq!(s.piles[&0].units, PILE_CAPACITY);
        s.pickup(0).unwrap();
        let id = s.drop_new(spot).unwrap();
        s.add(0, 200).unwrap();
        s.pickup(id).unwrap();
        assert!(!s.piles.contains_key(&id));
        assert!(s.deposit(0).is_err());
        assert_eq!(s.cargo.units, 12);
        let id = s.drop_new(spot).unwrap();
        s.pickup(0).unwrap();
        s.deposit(id).unwrap();
        s.pickup(id).unwrap();
        s.add(0, 7).unwrap();
        assert_eq!(s.deposit(0), Ok(5));
        assert_eq!(s.cargo.units, 7);
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
