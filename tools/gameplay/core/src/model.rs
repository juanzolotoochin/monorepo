use crate::{
    navigation::{self, Disc, Point},
    *,
};
use serde::Serialize;
use std::collections::BTreeMap;
pub const HZ: u64 = 60;
fn ticks(seconds: f32) -> u64 {
    (seconds as f64 * HZ as f64).ceil().max(1.0) as u64
}
#[derive(Clone, Debug, Serialize)]
pub struct Inventory {
    pub capacity: u32,
    pub items: BTreeMap<String, u32>,
}
impl Inventory {
    pub fn new(capacity: u32) -> Self {
        Self {
            capacity,
            items: BTreeMap::new(),
        }
    }
    pub fn units(&self, item: &str) -> u32 {
        *self.items.get(item).unwrap_or(&0)
    }
    pub fn total(&self) -> u32 {
        self.items.values().sum()
    }
    fn add(&mut self, item: &str, units: u32) {
        assert!(self.total() + units <= self.capacity);
        *self.items.entry(item.into()).or_default() += units;
    }
    fn take(&mut self, item: &str, units: u32) {
        let value = self.items.get_mut(item).expect("reserved stock");
        assert!(*value >= units);
        *value -= units;
        if *value == 0 {
            self.items.remove(item);
        }
    }
}
#[derive(Clone, Debug, Serialize, Default)]
pub struct Accounting {
    pub initial: u64,
    pub generated: u64,
    pub produced: u64,
    pub consumed: u64,
    pub spent: u64,
}
#[derive(Clone, Debug, Serialize, Default)]
pub struct Metrics {
    pub picked: u64,
    pub delivered: u64,
    pub equipped: u64,
    pub cycles: u64,
    pub shots: u64,
    pub kills: u64,
    pub breaches: u64,
    pub delivered_items: BTreeMap<String, u64>,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub enum Phase {
    Idle,
    ToPickup,
    Loading,
    ToDrop,
    Unloading,
    ToEquip,
    Attaching,
}
#[derive(Clone, Debug, Serialize)]
pub struct Assignment {
    pub link: usize,
    pub units: u32,
    pub loaded: u32,
    pub unloaded: u32,
}
#[derive(Clone, Debug, Serialize)]
pub struct Trailer {
    pub position: Point,
    pub heading: f32,
    pub travelled: f32,
}
#[derive(Clone, Debug, Serialize)]
pub struct AgentState {
    pub id: String,
    pub position: Point,
    pub heading: f32,
    pub travelled: f32,
    pub phase: Phase,
    pub route: Vec<Point>,
    pub cargo: Inventory,
    pub assignment: Option<Assignment>,
    pub equipment: Option<String>,
    pub trailer: Option<Trailer>,
    pub transfer_at: u64,
    pub retries: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct ProducerState {
    pub enabled: bool,
    pub remaining: u64,
    pub cycles: u32,
}
#[derive(Clone, Debug, Serialize)]
pub struct DefenderState {
    pub enabled: bool,
    pub ready_at: u64,
    pub aim: Option<Point>,
    pub fired_at: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Threat {
    pub id: u64,
    pub wave: usize,
    pub position: Point,
    pub heading: f32,
    pub travelled: f32,
    pub hp: u32,
    pub route: Vec<Point>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Projectile {
    pub id: u64,
    pub defender: String,
    pub target: u64,
    pub item: String,
    pub from: Point,
    pub to: Point,
    pub born: u64,
    pub arrival: u64,
    pub damage: u32,
}
#[derive(Clone, Debug, Serialize)]
pub enum Endpoint {
    Depot(String),
    Cargo(String),
}
#[derive(Clone, Debug, Serialize)]
pub struct Transfer {
    pub id: u64,
    pub item: String,
    pub from: Endpoint,
    pub to: Endpoint,
    pub start: Point,
    pub end: Point,
    pub born: u64,
    pub arrival: u64,
}
#[derive(Clone, Debug, Serialize)]
pub enum Event {
    TransferStarted(u64),
    TransferArrived(u64),
    Produced {
        producer: String,
        products: Vec<Stack>,
    },
    Equipped {
        agent: String,
        item: String,
    },
    Shot {
        defender: String,
    },
    Hit {
        position: Point,
        killed: bool,
    },
    Breach {
        position: Point,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct Balance {
    pub supplied: u64,
    pub stored: u64,
    pub carried: u64,
    pub in_transit: u64,
    pub equipped: u64,
    pub consumed: u64,
    pub spent: u64,
    pub conserved: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct Simulation {
    #[serde(skip)]
    config: Scenario,
    #[serde(skip)]
    obstacles: Vec<Disc>,
    #[serde(skip)]
    accumulator: f64,
    #[serde(skip)]
    next_id: u64,
    #[serde(skip)]
    source_totals: Vec<u32>,
    #[serde(skip)]
    wave_totals: Vec<u32>,
    pub tick: u64,
    pub paused: bool,
    pub rate: f32,
    pub health: u32,
    pub depots: BTreeMap<String, Inventory>,
    pub agents: BTreeMap<String, AgentState>,
    pub producers: BTreeMap<String, ProducerState>,
    pub defenders: BTreeMap<String, DefenderState>,
    pub links: BTreeMap<String, bool>,
    pub transfers: BTreeMap<u64, Transfer>,
    pub threats: BTreeMap<u64, Threat>,
    pub projectiles: BTreeMap<u64, Projectile>,
    pub accounts: BTreeMap<String, Accounting>,
    pub metrics: Metrics,
    #[serde(skip)]
    pub events: Vec<Event>,
}
impl Simulation {
    pub fn new(config: Scenario, obstacles: Vec<Disc>) -> Result<Self> {
        config.validate()?;
        let mut depots = BTreeMap::new();
        let mut accounts: BTreeMap<String, Accounting> = config
            .items
            .iter()
            .map(|i| (i.id.clone(), Accounting::default()))
            .collect();
        for d in &config.depots {
            let mut stock = Inventory::new(d.capacity);
            for s in &d.stock {
                stock.add(&s.item, s.units);
                accounts.get_mut(&s.item).unwrap().initial += s.units as u64;
            }
            depots.insert(d.id.clone(), stock);
        }
        let agents = config
            .agents
            .iter()
            .map(|a| {
                (
                    a.id.clone(),
                    AgentState {
                        id: a.id.clone(),
                        position: a.position,
                        heading: 0.0,
                        travelled: 0.0,
                        phase: Phase::Idle,
                        route: vec![],
                        cargo: Inventory::new(a.capacity),
                        assignment: None,
                        equipment: None,
                        trailer: None,
                        transfer_at: 0,
                        retries: 0,
                    },
                )
            })
            .collect();
        let producers = config
            .producers
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    ProducerState {
                        enabled: true,
                        remaining: 0,
                        cycles: 0,
                    },
                )
            })
            .collect();
        let defenders = config
            .defenders
            .iter()
            .map(|d| {
                (
                    d.id.clone(),
                    DefenderState {
                        enabled: true,
                        ready_at: 0,
                        aim: None,
                        fired_at: 0,
                    },
                )
            })
            .collect();
        let links = config.links.iter().map(|l| (l.id.clone(), true)).collect();
        let s = Self {
            source_totals: vec![0; config.sources.len()],
            wave_totals: vec![0; config.waves.len()],
            health: config.base_health,
            config,
            obstacles,
            accumulator: 0.0,
            next_id: 1,
            tick: 0,
            paused: false,
            rate: 1.0,
            depots,
            agents,
            producers,
            defenders,
            links,
            transfers: BTreeMap::new(),
            threats: BTreeMap::new(),
            projectiles: BTreeMap::new(),
            accounts,
            metrics: Metrics::default(),
            events: vec![],
        };
        for a in &s.config.agents {
            if !navigation::clear(a.position, a.position, &s.obstacles, a.radius) {
                return Err(format!("agent {} spawns inside an obstacle", a.id));
            }
        }
        for d in &s.config.depots {
            if !navigation::clear(d.position, d.position, &s.obstacles, 0.15) {
                return Err(format!("depot {} dock is blocked", d.id));
            }
        }
        Ok(s)
    }
    pub fn config(&self) -> &Scenario {
        &self.config
    }
    pub fn seconds(&self) -> f64 {
        self.tick as f64 / HZ as f64
    }
    pub fn set_rate(&mut self, rate: f32) -> Result<()> {
        if !rate.is_finite() || !(0.25..=4.0).contains(&rate) {
            return Err("simulation rate must be 0.25..4".into());
        }
        self.rate = rate;
        Ok(())
    }
    pub fn set_link(&mut self, id: &str, enabled: bool) -> Result<()> {
        *self.links.get_mut(id).ok_or("unknown haul link")? = enabled;
        Ok(())
    }
    pub fn set_machine(&mut self, id: &str, enabled: bool) -> Result<()> {
        self.producers
            .get_mut(id)
            .ok_or("unknown producer")?
            .enabled = enabled;
        Ok(())
    }
    pub fn set_defender(&mut self, id: &str, enabled: bool) -> Result<()> {
        self.defenders
            .get_mut(id)
            .ok_or("unknown defender")?
            .enabled = enabled;
        Ok(())
    }
    pub fn add_stock(&mut self, depot: &str, item: &str, units: u32) -> Result<()> {
        if units == 0 || !self.accounts.contains_key(item) || !self.depots.contains_key(depot) {
            return Err("unknown depot/item or zero amount".into());
        }
        if units > self.free(depot) {
            return Err("stock does not fit unreserved destination capacity".into());
        }
        self.depots.get_mut(depot).unwrap().add(item, units);
        self.accounts.get_mut(item).unwrap().generated += units as u64;
        Ok(())
    }
    pub fn advance(&mut self, seconds: f64) -> Result<()> {
        if !seconds.is_finite() || !(0.0..=1.0).contains(&seconds) {
            return Err("advance expects a finite duration in 0..1 seconds".into());
        }
        self.events.clear();
        if self.paused {
            return Ok(());
        }
        self.accumulator += seconds * self.rate as f64;
        while self.accumulator + 1e-9 >= 1.0 / HZ as f64 {
            self.accumulator -= 1.0 / HZ as f64;
            self.step();
        }
        Ok(())
    }
    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    fn depot_position(&self, id: &str) -> Point {
        self.config
            .depots
            .iter()
            .find(|d| d.id == id)
            .unwrap()
            .position
    }
    fn outgoing(&self, depot: &str, item: &str) -> u32 {
        self.agents
            .values()
            .map(|a| {
                let haul = a
                    .assignment
                    .as_ref()
                    .filter(|j| {
                        let l = &self.config.links[j.link];
                        l.from == depot && l.item == item
                    })
                    .map_or(0, |j| j.units - j.loaded);
                let equip = self
                    .config
                    .agents
                    .iter()
                    .find(|d| d.id == a.id)
                    .and_then(|d| d.equipment.as_ref())
                    .is_some_and(|e| {
                        e.depot == depot && e.item == item && a.phase == Phase::ToEquip
                    }) as u32;
                haul + equip
            })
            .sum()
    }
    fn incoming(&self, depot: &str, item: Option<&str>) -> u32 {
        let hauling: u32 = self
            .agents
            .values()
            .filter_map(|a| a.assignment.as_ref())
            .filter(|j| {
                let l = &self.config.links[j.link];
                l.to == depot && item.is_none_or(|i| i == l.item)
            })
            .map(|j| j.units - j.unloaded)
            .sum();
        let flying = self
            .transfers
            .values()
            .filter(|t| {
                matches!(&t.to,Endpoint::Depot(id) if id==depot) && item.is_none_or(|i| i == t.item)
            })
            .count() as u32;
        let production: u32 = self
            .config
            .producers
            .iter()
            .filter(|p| p.output == depot && self.producers[&p.id].remaining > 0)
            .flat_map(|p| p.products.iter())
            .filter(|s| item.is_none_or(|i| i == s.item))
            .map(|s| s.units)
            .sum();
        hauling + flying + production
    }
    fn free(&self, depot: &str) -> u32 {
        let d = &self.depots[depot];
        d.capacity
            .saturating_sub(d.total() + self.incoming(depot, None))
    }
    fn available(&self, depot: &str, item: &str) -> u32 {
        self.depots[depot]
            .units(item)
            .saturating_sub(self.outgoing(depot, item))
    }
    fn step(&mut self) {
        self.tick += 1;
        self.arrivals();
        self.sources();
        self.production();
        self.workers();
        self.combat();
    }
    fn arrivals(&mut self) {
        let ready: Vec<_> = self
            .transfers
            .values()
            .filter(|t| t.arrival <= self.tick)
            .map(|t| t.id)
            .collect();
        for id in ready {
            let t = self.transfers.remove(&id).unwrap();
            match &t.to {
                Endpoint::Depot(d) => {
                    self.depots.get_mut(d).unwrap().add(&t.item, 1);
                    self.metrics.delivered += 1;
                    *self
                        .metrics
                        .delivered_items
                        .entry(t.item.clone())
                        .or_default() += 1;
                }
                Endpoint::Cargo(a) => self.agents.get_mut(a).unwrap().cargo.add(&t.item, 1),
            }
            self.events.push(Event::TransferArrived(id));
        }
    }
    fn sources(&mut self) {
        for i in 0..self.config.sources.len() {
            let s = self.config.sources[i].clone();
            if self.tick % ticks(s.every) != 0 {
                continue;
            }
            let units = s.units.min(self.free(&s.depot)).min(
                s.max_units
                    .map_or(u32::MAX, |m| m.saturating_sub(self.source_totals[i])),
            );
            if units > 0 {
                self.depots.get_mut(&s.depot).unwrap().add(&s.item, units);
                self.accounts.get_mut(&s.item).unwrap().generated += units as u64;
                self.source_totals[i] += units;
            }
        }
    }
    fn production(&mut self) {
        for p in self.config.producers.clone() {
            let mut state = self.producers[&p.id].clone();
            if state.remaining > 0 {
                state.remaining -= 1;
                if state.remaining == 0 {
                    for s in &p.products {
                        self.depots
                            .get_mut(&p.output)
                            .unwrap()
                            .add(&s.item, s.units);
                        self.accounts.get_mut(&s.item).unwrap().produced += s.units as u64;
                    }
                    state.cycles += 1;
                    self.metrics.cycles += 1;
                    self.events.push(Event::Produced {
                        producer: p.id.clone(),
                        products: p.products.clone(),
                    });
                }
            } else if state.enabled && p.max_cycles.is_none_or(|limit| state.cycles < limit) {
                let required: u32 = p.products.iter().map(|s| s.units).sum();
                let freed = if p.input == p.output {
                    p.ingredients.iter().map(|s| s.units).sum()
                } else {
                    0
                };
                if self.free(&p.output) + freed >= required
                    && p.ingredients
                        .iter()
                        .all(|s| self.available(&p.input, &s.item) >= s.units)
                {
                    for s in &p.ingredients {
                        self.depots
                            .get_mut(&p.input)
                            .unwrap()
                            .take(&s.item, s.units);
                        self.accounts.get_mut(&s.item).unwrap().consumed += s.units as u64;
                    }
                    state.remaining = ticks(p.seconds);
                }
            }
            self.producers.insert(p.id, state);
        }
    }
    fn pending_cargo(&self, agent: &str) -> bool {
        self.transfers
            .values()
            .any(|t| matches!(&t.to,Endpoint::Cargo(id) if id==agent))
    }
    fn pending_drop(&self, agent: &str) -> bool {
        self.transfers
            .values()
            .any(|t| matches!(&t.from,Endpoint::Cargo(id) if id==agent))
    }
    fn radius(def: &Agent, state: &AgentState) -> f32 {
        if state.equipment.is_some() {
            def.radius.max(0.35)
        } else {
            def.radius
        }
    }
    fn plan(&self, from: Point, to: Point, radius: f32) -> Vec<Point> {
        navigation::path(from, to, &self.obstacles, radius)
    }
    fn assign(&mut self, def: &Agent, a: &mut AgentState) {
        if self.tick < a.retries {
            return;
        }
        let radius = Self::radius(def, a);
        if a.equipment.is_none() {
            if let Some(e) = &def.equipment {
                if self.available(&e.depot, &e.item) > 0 {
                    let route = self.plan(a.position, self.depot_position(&e.depot), radius);
                    if !route.is_empty() {
                        a.phase = Phase::ToEquip;
                        a.route = route;
                        return;
                    }
                }
            }
        }
        let mut links: Vec<_> = self
            .config
            .links
            .iter()
            .enumerate()
            .filter(|(_, l)| def.links.contains(&l.id) && self.links[&l.id])
            .map(|(i, l)| (i, l.clone()))
            .collect();
        links.sort_by_key(|(i, l)| (std::cmp::Reverse(l.priority), *i));
        for (index, l) in links {
            let demand = l.target.saturating_sub(
                self.depots[&l.to].units(&l.item) + self.incoming(&l.to, Some(&l.item)),
            );
            let units = self
                .available(&l.from, &l.item)
                .min(self.free(&l.to))
                .min(demand)
                .min(a.cargo.capacity);
            if units == 0 {
                continue;
            }
            let from = self.depot_position(&l.from);
            let to = self.depot_position(&l.to);
            let route = self.plan(a.position, from, radius);
            if route.is_empty() || self.plan(from, to, radius).is_empty() {
                continue;
            }
            a.assignment = Some(Assignment {
                link: index,
                units,
                loaded: 0,
                unloaded: 0,
            });
            a.phase = Phase::ToPickup;
            a.route = route;
            return;
        }
        a.retries = self.tick + 15;
    }
    fn launch(&mut self, item: &str, from: Endpoint, to: Endpoint, start: Point, end: Point) {
        let id = self.id();
        self.transfers.insert(
            id,
            Transfer {
                id,
                item: item.into(),
                from,
                to,
                start,
                end,
                born: self.tick,
                arrival: self.tick + 21,
            },
        );
        self.events.push(Event::TransferStarted(id));
    }
    fn workers(&mut self) {
        let mut docks: BTreeMap<String, String> = BTreeMap::new();
        for def in &self.config.agents {
            let a = &self.agents[&def.id];
            let dock = match a.phase {
                Phase::Loading => a
                    .assignment
                    .as_ref()
                    .map(|j| self.config.links[j.link].from.clone()),
                Phase::Unloading => a
                    .assignment
                    .as_ref()
                    .map(|j| self.config.links[j.link].to.clone()),
                Phase::Attaching => def.equipment.as_ref().map(|e| e.depot.clone()),
                _ => None,
            };
            if let Some(d) = dock {
                docks.insert(d, a.id.clone());
            }
        }
        for def in self.config.agents.clone() {
            let mut a = self.agents.remove(&def.id).unwrap();
            match a.phase {
                Phase::Idle => self.assign(&def, &mut a),
                Phase::ToPickup | Phase::ToDrop | Phase::ToEquip => {
                    let dock = if a.phase == Phase::ToEquip {
                        def.equipment.as_ref().unwrap().depot.clone()
                    } else {
                        let j = a.assignment.as_ref().unwrap();
                        let l = &self.config.links[j.link];
                        if a.phase == Phase::ToPickup {
                            l.from.clone()
                        } else {
                            l.to.clone()
                        }
                    };
                    let goal = self.depot_position(&dock);
                    let busy = docks.get(&dock).is_some_and(|id| id != &a.id);
                    if !(busy && navigation::distance(a.position, goal) < 0.75) {
                        let speed = def.speed
                            * if a.equipment.is_some() {
                                def.equipment.as_ref().map_or(1.0, |e| e.speed_factor)
                            } else {
                                1.0
                            };
                        let before = a.position;
                        move_route(&mut a.position, &mut a.route, speed / HZ as f32);
                        let delta = [a.position[0] - before[0], a.position[1] - before[1]];
                        let distance = navigation::distance(a.position, before);
                        a.travelled += distance;
                        if distance > 0.00001 {
                            a.heading = delta[0].atan2(delta[1]);
                        }
                        if let Some(trailer) = &mut a.trailer {
                            let hitch = def.equipment.as_ref().unwrap().offset[2].abs().max(0.6);
                            let d = navigation::distance(trailer.position, a.position);
                            if d > hitch {
                                let previous = trailer.position;
                                let direction = [
                                    (a.position[0] - trailer.position[0]) / d,
                                    (a.position[1] - trailer.position[1]) / d,
                                ];
                                trailer.position = [
                                    a.position[0] - direction[0] * hitch,
                                    a.position[1] - direction[1] * hitch,
                                ];
                                trailer.heading = direction[0].atan2(direction[1]);
                                trailer.travelled +=
                                    navigation::distance(previous, trailer.position);
                            }
                        }
                        if a.route.is_empty() && !busy {
                            docks.insert(dock.clone(), a.id.clone());
                            a.transfer_at = self.tick;
                            if a.phase == Phase::ToPickup {
                                a.phase = Phase::Loading;
                            } else if a.phase == Phase::ToDrop {
                                a.phase = Phase::Unloading;
                            } else {
                                let e = def.equipment.as_ref().unwrap();
                                self.depots.get_mut(&e.depot).unwrap().take(&e.item, 1);
                                a.equipment = Some(e.item.clone());
                                a.trailer = Some(Trailer {
                                    position: [
                                        a.position[0] - a.heading.sin() * e.offset[2].abs(),
                                        a.position[1] - a.heading.cos() * e.offset[2].abs(),
                                    ],
                                    heading: a.heading,
                                    travelled: 0.0,
                                });
                                a.phase = Phase::Attaching;
                                a.transfer_at = self.tick + 60;
                            }
                        }
                    }
                }
                Phase::Loading => {
                    let j = a.assignment.as_mut().unwrap();
                    let l = self.config.links[j.link].clone();
                    if j.loaded < j.units && self.tick >= a.transfer_at {
                        self.depots.get_mut(&l.from).unwrap().take(&l.item, 1);
                        j.loaded += 1;
                        self.metrics.picked += 1;
                        self.launch(
                            &l.item,
                            Endpoint::Depot(l.from.clone()),
                            Endpoint::Cargo(a.id.clone()),
                            self.depot_position(&l.from),
                            a.position,
                        );
                        a.transfer_at = self.tick + ticks(def.transfer_seconds);
                    }
                    if j.loaded == j.units && !self.pending_cargo(&a.id) {
                        a.route = self.plan(
                            a.position,
                            self.depot_position(&l.to),
                            Self::radius(&def, &a),
                        );
                        a.phase = Phase::ToDrop;
                    }
                }
                Phase::Unloading => {
                    let j = a.assignment.as_mut().unwrap();
                    let l = self.config.links[j.link].clone();
                    if j.unloaded < j.units && self.tick >= a.transfer_at {
                        a.cargo.take(&l.item, 1);
                        j.unloaded += 1;
                        self.launch(
                            &l.item,
                            Endpoint::Cargo(a.id.clone()),
                            Endpoint::Depot(l.to.clone()),
                            a.position,
                            self.depot_position(&l.to),
                        );
                        a.transfer_at = self.tick + ticks(def.transfer_seconds);
                    }
                    if j.unloaded == j.units && !self.pending_drop(&a.id) {
                        a.assignment = None;
                        a.phase = Phase::Idle;
                    }
                }
                Phase::Attaching => {
                    if self.tick >= a.transfer_at {
                        a.cargo.capacity = def.equipment.as_ref().unwrap().capacity;
                        a.phase = Phase::Idle;
                        self.metrics.equipped += 1;
                        self.events.push(Event::Equipped {
                            agent: a.id.clone(),
                            item: a.equipment.clone().unwrap(),
                        });
                    }
                }
            }
            self.agents.insert(a.id.clone(), a);
        }
    }
    fn combat(&mut self) {
        let arrived: Vec<_> = self
            .projectiles
            .values()
            .filter(|p| p.arrival <= self.tick)
            .map(|p| p.id)
            .collect();
        for id in arrived {
            let p = self.projectiles.remove(&id).unwrap();
            if let Some(target) = self.threats.get_mut(&p.target) {
                target.hp = target.hp.saturating_sub(p.damage);
                let killed = target.hp == 0;
                let position = target.position;
                self.events.push(Event::Hit { position, killed });
                if killed {
                    self.threats.remove(&p.target);
                    self.metrics.kills += 1;
                }
            }
        }
        for i in 0..self.config.waves.len() {
            let w = &self.config.waves[i];
            let first = ((w.first as f64 * HZ as f64).round() as u64).max(1);
            if self.tick < first
                || self.wave_totals[i] >= w.count
                || (self.tick - first) % ticks(w.every) != 0
            {
                continue;
            }
            let w = w.clone();
            let route = self.plan(w.position, w.target, 0.2);
            if route.is_empty() {
                continue;
            }
            let id = self.id();
            self.threats.insert(
                id,
                Threat {
                    id,
                    wave: i,
                    position: w.position,
                    heading: 0.0,
                    travelled: 0.0,
                    hp: w.hp,
                    route,
                },
            );
            self.wave_totals[i] += 1;
        }
        let mut breached = vec![];
        for threat in self.threats.values_mut() {
            let w = &self.config.waves[threat.wave];
            let previous = threat.position;
            move_route(&mut threat.position, &mut threat.route, w.speed / HZ as f32);
            let moved = navigation::distance(previous, threat.position);
            threat.travelled += moved;
            if moved > 0.0 {
                threat.heading =
                    (threat.position[0] - previous[0]).atan2(threat.position[1] - previous[1]);
            }
            if threat.route.is_empty() {
                breached.push(threat.id);
            }
        }
        for id in breached {
            let t = self.threats.remove(&id).unwrap();
            self.health = self.health.saturating_sub(self.config.waves[t.wave].damage);
            self.metrics.breaches += 1;
            self.events.push(Event::Breach {
                position: t.position,
            });
        }
        for projectile in self.projectiles.values_mut() {
            if let Some(t) = self.threats.get(&projectile.target) {
                projectile.to = t.position;
            }
        }
        for d in self.config.defenders.clone() {
            let state = &self.defenders[&d.id];
            if !state.enabled
                || state.ready_at > self.tick
                || self.available(&d.depot, &d.ammo) == 0
            {
                continue;
            }
            let target = self
                .threats
                .values()
                .filter(|t| {
                    navigation::distance(d.position, t.position) <= d.range
                        && self
                            .projectiles
                            .values()
                            .filter(|p| p.target == t.id)
                            .map(|p| p.damage)
                            .sum::<u32>()
                            < t.hp
                })
                .min_by(|a, b| {
                    navigation::distance(d.position, a.position)
                        .total_cmp(&navigation::distance(d.position, b.position))
                        .then(a.id.cmp(&b.id))
                })
                .map(|t| (t.id, t.position));
            if let Some((target, position)) = target {
                self.depots.get_mut(&d.depot).unwrap().take(&d.ammo, 1);
                self.accounts.get_mut(&d.ammo).unwrap().spent += 1;
                self.metrics.shots += 1;
                let id = self.id();
                self.projectiles.insert(
                    id,
                    Projectile {
                        id,
                        defender: d.id.clone(),
                        target,
                        item: d.ammo.clone(),
                        from: d.position,
                        to: position,
                        born: self.tick,
                        arrival: self.tick
                            + ticks(
                                navigation::distance(d.position, position) / d.projectile_speed,
                            ),
                        damage: d.damage,
                    },
                );
                let state = self.defenders.get_mut(&d.id).unwrap();
                state.aim = Some(position);
                state.ready_at = self.tick + ticks(d.period);
                state.fired_at = self.tick;
                self.events.push(Event::Shot {
                    defender: d.id.clone(),
                });
            }
        }
    }
    pub fn balances(&self) -> BTreeMap<String, Balance> {
        self.accounts
            .iter()
            .map(|(id, b)| {
                let stored = self.depots.values().map(|s| s.units(id) as u64).sum();
                let carried = self.agents.values().map(|a| a.cargo.units(id) as u64).sum();
                let in_transit = self.transfers.values().filter(|t| t.item == *id).count() as u64;
                let equipped = self
                    .agents
                    .values()
                    .filter(|a| a.equipment.as_ref() == Some(id))
                    .count() as u64;
                let supplied = b.initial + b.generated + b.produced;
                (
                    id.clone(),
                    Balance {
                        supplied,
                        stored,
                        carried,
                        in_transit,
                        equipped,
                        consumed: b.consumed,
                        spent: b.spent,
                        conserved: supplied
                            == stored + carried + in_transit + equipped + b.consumed + b.spent,
                    },
                )
            })
            .collect()
    }
    pub fn validate_state(&self) -> Result<()> {
        if self.balances().values().any(|b| !b.conserved) {
            return Err("material accounting mismatch".into());
        }
        for (id, d) in &self.depots {
            if d.total() + self.incoming(id, None) > d.capacity {
                return Err(format!("depot {id} overbooked"));
            }
            for item in self.accounts.keys() {
                if self.outgoing(id, item) > d.units(item) {
                    return Err(format!("depot {id} stock overbooked"));
                }
            }
        }
        for a in self.agents.values() {
            let incoming = self
                .transfers
                .values()
                .filter(|t| matches!(&t.to,Endpoint::Cargo(id) if id==&a.id))
                .count() as u32;
            if a.cargo.total() + incoming > a.cargo.capacity {
                return Err("agent capacity exceeded".into());
            }
        }
        Ok(())
    }
}
fn move_route(position: &mut Point, route: &mut Vec<Point>, mut distance: f32) {
    while let Some(&goal) = route.first() {
        let remaining = navigation::distance(*position, goal);
        if remaining <= distance + 0.00001 {
            *position = goal;
            route.remove(0);
            distance = (distance - remaining).max(0.0);
        } else {
            position[0] += (goal[0] - position[0]) / remaining * distance;
            position[1] += (goal[1] - position[1]) / remaining * distance;
            break;
        }
        if distance <= 0.0 {
            break;
        }
    }
}
