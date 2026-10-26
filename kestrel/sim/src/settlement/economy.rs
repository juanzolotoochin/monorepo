//! The settlement's single-threaded side: arrivals of flying units, sources,
//! recipes, and the handlers for every settlement message.

use std::collections::BTreeMap;

use fx::{Fx, FxVec2};
use rayon::prelude::*;

use super::haulers::{self, footprint, job_size, Assignment, Ctx, HaulerState, Phase, Trailer, ATTACH_TICKS, RETRY_TICKS};
use super::{AgentIx, AgentState, Body, Carry, DepotIx, Endpoint, ItemIx, Role, Settlement, Transfer, TRANSFER_FLIGHT_TICKS};
use crate::messages::{Attach, ClaimDock, DockClaim, Equip, Load, Messages, Reserve, Retry, Unload};
use crate::terrain::Terrain;

impl Settlement {
    /// Stage 2: every agent and threat moves itself, reading last tick's bodies.
    pub(crate) fn self_update(&mut self, tick: u64, terrain: &Terrain) {
        self.events.clear();
        self.threats_self_update(tick, terrain);
        let footprints: Vec<_> = self.agents.iter().map(footprint).collect();
        let Settlement { agents, characters, dock_owner, depots, links, piles, .. } = self;
        let ctx = Ctx { tick, characters, terrain, piles: piles.as_ref(), footprints: &footprints, dock_owner, depots, links };
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
            if let Endpoint::Cargo(a) = t.from {
                self.agents[a as usize].carry.pending_out -= 1;
            }
            match t.to {
                Endpoint::Depot(d) => {
                    let depot = &mut self.depots[d as usize];
                    depot.stock[t.item as usize] += 1;
                    depot.incoming[t.item as usize] -= 1;
                    self.metrics.delivered += 1;
                    *self.metrics.delivered_items.entry(self.items[t.item as usize].clone()).or_default() += 1;
                    if let Endpoint::Cargo(a) = t.from {
                        *self.metrics.deliveries_by_agent.entry(self.agents[a as usize].id.clone()).or_default() += 1;
                    }
                }
                Endpoint::Cargo(a) => {
                    let agent = &mut self.agents[a as usize];
                    agent.carry.cargo[t.item as usize] += 1;
                    agent.carry.pending_in -= 1;
                }
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
            let units = s.units.min(depot.free()).min(left);
            if units > 0 {
                let (d, item) = (s.depot as usize, s.item as usize);
                self.depots[d].stock[item] += units;
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
                        depot.stock[item as usize] += units;
                        depot.incoming[item as usize] -= units;
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
            let fits = self.depots[output].free() + freed >= required;
            let stocked = p.ingredients.iter().all(|&(item, units)| self.depots[input].available(item) >= units);
            if !(fits && stocked) {
                continue;
            }
            let (ingredients, products, ticks) = (p.ingredients.clone(), p.products.clone(), p.ticks);
            for (item, units) in ingredients {
                self.depots[input].stock[item as usize] -= units;
                self.accounts[item as usize].consumed += units as u64;
            }
            for (item, units) in products {
                self.depots[output].incoming[item as usize] += units;
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
        if !self.idle(message.agent) || h.equipped || self.depots[depot].available(item) == 0 {
            return self.retry(tick, Retry { agent: message.agent });
        }
        self.depots[depot].out[item as usize] += 1;
        let (body, _, h) = self.hauler_parts(message.agent);
        h.phase = Phase::ToEquip;
        body.route = message.route;
    }

    /// Accepts a haul job if it still fits, possibly smaller than asked.
    pub(crate) fn reserve(&mut self, tick: u64, message: Reserve) {
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
        let (from, to, item) = (link.from as usize, link.to as usize, link.item as usize);
        self.depots[from].out[item] += units;
        self.depots[to].incoming[item] += units;
        self.metrics.assignments += 1;
        let (body, _, h) = self.hauler_parts(message.agent);
        h.assignment = Some(Assignment { link: message.link, units, loaded: 0, unloaded: 0 });
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
        let (depot, item) = (e.depot as usize, e.item as usize);
        let (spot, heading) = h.trailer_spot(&a.body, terrain);
        self.depots[depot].stock[item] -= 1;
        self.depots[depot].out[item] -= 1;
        self.metrics.equipped += 1;
        self.events.push(super::Event::Equipped { agent: message.agent });
        let (body, _, h) = self.hauler_parts(message.agent);
        h.equipped = true;
        body.travel_heading = heading;
        h.trailer = Some(Trailer { position: spot, heading, travelled: Fx::ZERO });
        h.transfer_at = tick + ATTACH_TICKS;
    }

    pub(crate) fn load(&mut self, tick: u64, message: Load) {
        let h = self.hauler(message.agent);
        let Some(job) = &h.assignment else { return };
        if h.phase != Phase::Loading || job.loaded >= job.units {
            return;
        }
        let link = &self.links[job.link as usize];
        let (from, item) = (link.from, link.item);
        let depot = &mut self.depots[from as usize];
        depot.stock[item as usize] -= 1;
        depot.out[item as usize] -= 1;
        self.metrics.picked += 1;
        let start = depot.position;
        let (body, carry, h) = self.hauler_parts(message.agent);
        h.assignment.as_mut().expect("checked").loaded += 1;
        carry.pending_in += 1;
        h.transfer_at = tick + carry.transfer_ticks;
        let end = body.position;
        self.launch(tick, item, Endpoint::Depot(from), Endpoint::Cargo(message.agent), start, end);
    }

    pub(crate) fn unload(&mut self, tick: u64, message: Unload) {
        let h = self.hauler(message.agent);
        let Some(job) = &h.assignment else { return };
        if h.phase != Phase::Unloading || job.unloaded >= job.units {
            return;
        }
        let link = &self.links[job.link as usize];
        let (to, item) = (link.to, link.item);
        let end = self.depots[to as usize].position;
        let (body, carry, h) = self.hauler_parts(message.agent);
        carry.cargo[item as usize] -= 1;
        h.assignment.as_mut().expect("checked").unloaded += 1;
        carry.pending_out += 1;
        h.transfer_at = tick + carry.transfer_ticks;
        let start = body.position;
        self.launch(tick, item, Endpoint::Cargo(message.agent), Endpoint::Depot(to), start, end);
    }

    pub(crate) fn launch(&mut self, tick: u64, item: ItemIx, from: Endpoint, to: Endpoint, start: FxVec2, end: FxVec2) {
        let id = self.id();
        self.transfers.insert(id, Transfer { id, item, from, to, start, end, born: tick, arrival: tick + TRANSFER_FLIGHT_TICKS });
    }

    /// Commands: adds stock to a depot, within its unreserved space. For
    /// scripted demos and tests.
    pub fn add_stock(&mut self, depot: usize, item: usize, units: u32) {
        let (Some(d), true) = (self.depots.get_mut(depot), item < self.items.len()) else { return };
        if units == 0 || units > d.free() {
            return;
        }
        d.stock[item] += units;
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
