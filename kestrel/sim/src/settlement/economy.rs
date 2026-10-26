//! The settlement's single-threaded side: arrivals of flying units, sources,
//! recipes, and the handlers for every settlement message.

use std::collections::BTreeMap;

use fx::{Fx, FxVec2};
use rayon::prelude::*;

use super::haulers::{self, footprint, job_size, Assignment, Ctx, HaulerState, Phase, Trailer, ATTACH_TICKS, RETRY_TICKS};
use super::{AgentIx, AgentState, Body, Carry, CarryJob, DepotIx, Direction, InventoryRef, ItemIx, Role, Settlement, Side, Transfer, TRANSFER_FLIGHT_TICKS};
use crate::messages::{Attach, CarryUnit, ClaimDock, DockClaim, Equip, Messages, ReserveHaul, Retry};
use crate::terrain::Terrain;

impl Settlement {
    /// Stage 2: every agent and threat moves itself, reading last tick's bodies.
    pub(crate) fn self_update(&mut self, tick: u64, terrain: &Terrain) {
        self.events.clear();
        self.threats_self_update(tick, terrain);
        let footprints: Vec<_> = self.agents.iter().map(footprint).collect();
        let Settlement { agents, characters, dock_owner, depots, links, piles, transfers, .. } = self;
        let ctx = Ctx { tick, characters, terrain, piles: piles.as_ref(), footprints: &footprints, dock_owner, depots, links, transfers };
        agents.par_iter_mut().enumerate().for_each(|(i, a)| haulers::self_update(a, i as AgentIx, &ctx));
        self.release_distant_piano_notes();
    }

    /// Stage 3: each agent's messages, with the position that files them
    /// under a chunk.
    pub(crate) fn read(&self, tick: u64, terrain: &Terrain) -> Vec<(FxVec2, Messages)> {
        let mut result: Vec<(FxVec2, Messages)> = self
            .agents
            .par_iter()
            .enumerate()
            .map(|(i, a)| {
                let mut out = Messages::default();
                haulers::read(a, i as AgentIx, tick, self, terrain, &mut out);
                (a.body.position, out)
            })
            .collect();
        result.extend(self.read_combat(tick, terrain));
        result
    }

    /// The start of stage 5: units land, sources produce, recipes run.
    pub(crate) fn economy(&mut self, tick: u64) {
        self.arrivals(tick);
        self.sources(tick);
        self.production();
    }

    fn arrivals(&mut self, tick: u64) {
        let landed: Vec<u64> = self.transfers.values().filter(|t| t.arrival <= tick).map(|t| t.id).collect();
        for id in landed {
            let t = self.transfers.remove(&id).expect("listed transfer");
            self.land(t);
        }
    }

    /// A unit arrives at its end: a depot's stock or an agent's cargo. A
    /// unit leaving a cargo bed left its counts at launch, so landing is
    /// all that changes at that end.
    pub(super) fn land(&mut self, t: super::Transfer) {
        match t.to {
            InventoryRef::Depot(d) => {
                let depot = &mut self.depots[d as usize];
                depot.inventory.put(t.item, 1);
                self.metrics.delivered += 1;
                *self.metrics.delivered_items.entry(self.items[t.item as usize].clone()).or_default() += 1;
                if let InventoryRef::Agent(a) = t.from {
                    *self.metrics.deliveries_by_agent.entry(self.agents[a as usize].id.clone()).or_default() += 1;
                }
            }
            InventoryRef::Agent(a) => {
                self.agents[a as usize].carry.inventory.put(t.item, 1);
            }
        }
    }

    fn sources(&mut self, tick: u64) {
        for i in 0..self.sources.len() {
            let s = &self.sources[i];
            if tick == 0 || tick % s.every != 0 {
                continue;
            }
            let depot = &self.depots[s.depot as usize];
            let left = s.max_units.map_or(u32::MAX, |max| max.saturating_sub(s.total));
            let units = s.units.min(depot.inventory.free()).min(left);
            if units > 0 {
                let (d, item) = (s.depot as usize, s.item as usize);
                let added = self.depots[d].inventory.add(s.item, units);
                debug_assert!(added, "within the free space");
                self.accounts[item].generated += units as u64;
                self.sources[i].total += units;
            }
        }
    }

    fn production(&mut self) {
        for i in 0..self.producers.len() {
            let p = &mut self.producers[i];
            if p.remaining > 0 {
                p.remaining -= 1;
                if p.remaining == 0 {
                    p.cycles += 1;
                    let (output, products) = (p.output as usize, p.products.clone());
                    for (item, units) in products {
                        let depot = &mut self.depots[output];
                        depot.inventory.put(item, units);
                        self.accounts[item as usize].produced += units as u64;
                    }
                    self.metrics.cycles += 1;
                    self.events.push(super::Event::Produced { producer: i as u16 });
                }
                continue;
            }
            if !p.enabled || p.max_cycles.is_some_and(|limit| p.cycles >= limit) {
                continue;
            }
            let required: u32 = p.products.iter().map(|s| s.1).sum();
            // Ingredients leaving the same depot make room for the products.
            let freed: u32 = if p.input == p.output { p.ingredients.iter().map(|s| s.1).sum() } else { 0 };
            let (input, output) = (p.input as usize, p.output as usize);
            let fits = self.depots[output].inventory.free() + freed >= required;
            let stocked = p.ingredients.iter().all(|&(item, units)| self.depots[input].inventory.available(item) >= units);
            if !(fits && stocked) {
                continue;
            }
            let (ingredients, products, ticks) = (p.ingredients.clone(), p.products.clone(), p.ticks);
            for (item, units) in ingredients {
                self.depots[input].inventory.remove(item, units);
                self.accounts[item as usize].consumed += units as u64;
            }
            for (item, units) in products {
                let reserved = self.depots[output].inventory.reserve_in(item, units);
                debug_assert!(reserved, "checked that the products fit");
            }
            self.producers[i].remaining = ticks;
        }
    }

    /// Resolves every dock: holders keep it; otherwise the previous owner, if
    /// still approaching; otherwise the nearest approaching agent. The result
    /// doesn't depend on message order.
    pub(crate) fn resolve_docks(&mut self, claims: Vec<ClaimDock>) {
        let mut by_depot: BTreeMap<DepotIx, Vec<ClaimDock>> = BTreeMap::new();
        for claim in claims {
            by_depot.entry(claim.depot).or_default().push(claim);
        }
        let previous = std::mem::replace(&mut self.dock_owner, vec![None; self.depots.len()]);
        for (depot, claims) in by_depot {
            let holder = claims.iter().filter(|c| c.claim == DockClaim::Holding).map(|c| c.agent).min();
            let kept = previous[depot as usize].filter(|&o| claims.iter().any(|c| c.agent == o));
            let nearest = claims
                .iter()
                .filter_map(|c| match c.claim {
                    DockClaim::Approaching(d) => Some((d, c.agent)),
                    DockClaim::Holding => None,
                })
                .min()
                .map(|(_, agent)| agent);
            self.dock_owner[depot as usize] = holder.or(kept).or(nearest);
        }
    }

    /// The hauler a hauler's message came from.
    fn hauler(&self, agent: AgentIx) -> &HaulerState {
        self.agents[agent as usize].hauler().expect("hauler messages come from haulers")
    }

    fn hauler_mut(&mut self, agent: AgentIx) -> &mut HaulerState {
        self.agents[agent as usize].hauler_mut().expect("hauler messages come from haulers")
    }

    /// The body, cargo and state of the hauler a message came from.
    fn hauler_parts(&mut self, agent: AgentIx) -> (&mut Body, &mut Carry, &mut HaulerState) {
        match &mut self.agents[agent as usize] {
            AgentState { body, carry, role: Role::Hauler(hauler), .. } => (body, carry, hauler),
            _ => unreachable!("hauler messages come from haulers"),
        }
    }

    pub(crate) fn retry(&mut self, tick: u64, message: Retry) {
        self.hauler_mut(message.agent).retries = tick + RETRY_TICKS;
        self.metrics.assignment_retries += 1;
    }

    fn idle(&self, agent: AgentIx) -> bool {
        matches!(self.hauler(agent).phase, Phase::Idle | Phase::Returning)
    }

    pub(crate) fn equip(&mut self, tick: u64, message: Equip) {
        let h = self.hauler(message.agent);
        let e = h.equipment.as_ref().expect("equip comes from equippable agents");
        let (depot, item) = (e.depot as usize, e.item);
        if !self.idle(message.agent) || h.equipped || self.depots[depot].inventory.available(item) == 0 {
            return self.retry(tick, Retry { agent: message.agent });
        }
        let reserved = self.depots[depot].inventory.reserve_out(item, 1);
        debug_assert!(reserved, "checked that one is available");
        let (body, _, h) = self.hauler_parts(message.agent);
        h.phase = Phase::ToEquip;
        body.route = message.route;
    }

    /// Accepts a haul job if it still fits, possibly smaller than asked. It
    /// is two carry jobs: a load at the link's source, then an unload at its
    /// target. Both ends are held now, before the hauler walks to the source
    /// (`INVENTORY_HANDOFF.md`, "Why a haul reserves both ends"): the load
    /// on both its sides, the unload on the target only (`Carry::next`).
    /// Unlike a player's `ReserveStock`, which arrives with its job sized
    /// and its own side held, this sizes the job here and holds every side.
    pub(crate) fn reserve_haul(&mut self, tick: u64, message: ReserveHaul) {
        let a = &self.agents[message.agent as usize];
        let link = &self.links[message.link as usize];
        let units = if self.idle(message.agent) && link.enabled {
            message.units.min(job_size(a, link, &self.depots))
        } else {
            0
        };
        if units == 0 {
            return self.retry(tick, Retry { agent: message.agent });
        }
        let job = |other, direction| CarryJob { other: InventoryRef::Depot(other), item: link.item, direction, remaining: units, next_tick: tick };
        let (load, unload) = (job(link.from, Direction::Pickup), job(link.to, Direction::Deposit));
        // `job_size` checked both depots and the cargo's capacity, and an
        // idle hauler's cargo is empty.
        let held = load.reserve(self.inventory_mut(load.other), Side::Other) && unload.reserve(self.inventory_mut(unload.other), Side::Other);
        debug_assert!(held, "the job fits both depots");
        self.metrics.assignments += 1;
        let (body, carry, h) = self.hauler_parts(message.agent);
        let held = load.reserve(&mut carry.inventory, Side::Own);
        debug_assert!(held, "the job fits the cargo");
        carry.job = Some(load);
        carry.next = Some(unload);
        h.assignment = Some(Assignment { link: message.link });
        h.phase = Phase::ToPickup;
        body.route = message.route;
    }

    pub(crate) fn attach(&mut self, tick: u64, terrain: &Terrain, message: Attach) {
        let a = &self.agents[message.agent as usize];
        let h = self.hauler(message.agent);
        if h.phase != Phase::Attaching || h.equipped {
            return;
        }
        let e = h.equipment.as_ref().expect("attaching agents have equipment");
        let (depot, item) = (e.depot as usize, e.item);
        let (spot, heading) = h.trailer_spot(&a.body, terrain);
        self.depots[depot].inventory.take(item, 1);
        self.metrics.equipped += 1;
        self.events.push(super::Event::Equipped { agent: message.agent });
        let (body, _, h) = self.hauler_parts(message.agent);
        h.equipped = true;
        body.travel_heading = heading;
        h.trailer = Some(Trailer { position: spot, heading, travelled: Fx::ZERO });
        h.transfer_at = tick + ATTACH_TICKS;
    }

    /// Moves one unit of an agent's carry job, if it is due. The unit leaves
    /// the source, taking its share of what the job holds there; at the
    /// target the job's room passes to the unit in the air, until it lands.
    /// Stage 3 sends it only when the agent is at the job's dock.
    pub(crate) fn carry_unit(&mut self, tick: u64, message: CarryUnit) {
        let agent = message.agent as usize;
        let Some(job) = self.agents[agent].carry.job.clone() else { return };
        if tick < job.next_tick {
            return;
        }
        let (me, item) = (InventoryRef::Agent(message.agent), job.item);
        let (dock, at) = (self.position_of(job.other), self.agents[agent].body.position);
        match job.direction {
            Direction::Deposit => {
                self.agents[agent].carry.inventory.take(item, 1);
                self.launch(tick, item, me, job.other, at, dock);
            }
            Direction::Pickup => {
                self.inventory_mut(job.other).take(item, 1);
                self.metrics.picked += 1;
                self.launch(tick, item, job.other, me, dock, at);
            }
        }
        let carry = &mut self.agents[agent].carry;
        let job = carry.job.as_mut().expect("checked");
        job.remaining -= 1;
        job.next_tick = tick + carry.transfer_ticks;
        if job.remaining == 0 {
            carry.job = None;
        }
    }

    pub(crate) fn launch(&mut self, tick: u64, item: ItemIx, from: InventoryRef, to: InventoryRef, start: FxVec2, end: FxVec2) {
        let id = self.id();
        self.transfers.insert(id, Transfer { id, item, from, to, start, end, born: tick, arrival: tick + TRANSFER_FLIGHT_TICKS });
    }

    /// Commands: adds stock to a depot, within its unreserved space. For
    /// scripted demos and tests.
    pub fn add_stock(&mut self, depot: usize, item: usize, units: u32) {
        let (Some(d), true) = (self.depots.get_mut(depot), item < self.items.len()) else { return };
        // `add` refuses units that don't fit.
        if units == 0 || !d.inventory.add(item as ItemIx, units) {
            return;
        }
        self.accounts[item].generated += units as u64;
    }

    /// Commands: a player switches a haul link on or off.
    pub fn set_link(&mut self, link: usize, enabled: bool) {
        if let Some(l) = self.links.get_mut(link) {
            l.enabled = enabled;
        }
    }

    /// Commands: a player switches a machine on or off.
    pub fn set_machine(&mut self, producer: usize, enabled: bool) {
        if let Some(p) = self.producers.get_mut(producer) {
            p.enabled = enabled;
        }
    }
}
