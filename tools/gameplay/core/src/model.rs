mod combat;
mod interaction;
mod production;
mod workers;
use crate::{
    navigation::{self, Disc, Point},
    *,
};
pub use interaction::ManualJob;
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
    pub fixed_steps: u64,
    pub navigation_requests: u64,
    pub navigation_failures: u64,
    pub navigation_waypoints: u64,
    pub assignments: u64,
    pub assignment_retries: u64,
    pub dock_wait_ticks: u64,
    pub empty_travel_meters: f64,
    pub loaded_travel_meters: f64,
    pub agent_phase_ticks: BTreeMap<String, BTreeMap<String, u64>>,
    pub producer_state_ticks: BTreeMap<String, BTreeMap<String, u64>>,
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
        target: u64,
        projectile: Option<u64>,
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
    pub manual_jobs: BTreeMap<String, ManualJob>,
    pub attack_ready: BTreeMap<String, u64>,
    #[serde(skip)]
    pub events: Vec<Event>,
}
impl Simulation {
    pub fn new(config: Scenario, mut obstacles: Vec<Disc>) -> Result<Self> {
        config.validate()?;
        obstacles.extend(
            config
                .producers
                .iter()
                .filter(|p| p.radius > 0.)
                .map(|p| Disc {
                    center: p.position,
                    radius: p.radius,
                }),
        );
        obstacles.extend(
            config
                .defenders
                .iter()
                .filter(|p| p.radius > 0.)
                .map(|p| Disc {
                    center: p.position,
                    radius: p.radius,
                }),
        );
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
            manual_jobs: BTreeMap::new(),
            attack_ready: BTreeMap::new(),
            events: vec![],
        };
        for a in &s.config.agents {
            if navigation::path_with_bounds(
                a.position,
                a.position,
                &s.obstacles,
                a.radius,
                s.config.extent,
                |_, _| true,
            )
            .is_empty()
            {
                return Err(format!("agent {} spawns inside an obstacle", a.id));
            }
        }
        for d in &s.config.depots {
            if navigation::path_with_bounds(
                d.position,
                d.position,
                &s.obstacles,
                0.15,
                s.config.extent,
                |_, _| true,
            )
            .is_empty()
            {
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
        let manual: u32 = self
            .manual_jobs
            .values()
            .filter(|j| !j.deposit && j.depot == depot && j.item == item)
            .map(|j| j.remaining)
            .sum();
        manual
            + self
                .agents
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
                .sum::<u32>()
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
        let manual: u32 = self
            .manual_jobs
            .values()
            .filter(|j| j.deposit && j.depot == depot && item.is_none_or(|i| i == j.item))
            .map(|j| j.remaining)
            .sum();
        hauling + flying + production + manual
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
        self.metrics.fixed_steps += 1;
        for a in self.agents.values() {
            *self
                .metrics
                .agent_phase_ticks
                .entry(a.id.clone())
                .or_default()
                .entry(format!("{:?}", a.phase))
                .or_default() += 1;
        }
        for p in &self.config.producers {
            let state = &self.producers[&p.id];
            let reason = if state.remaining > 0 {
                "processing"
            } else if !state.enabled {
                "disabled"
            } else if p.max_cycles.is_some_and(|limit| state.cycles >= limit) {
                "commission_complete"
            } else if self.free(&p.output) < p.products.iter().map(|s| s.units).sum::<u32>() {
                "output_blocked"
            } else if p
                .ingredients
                .iter()
                .any(|s| self.available(&p.input, &s.item) < s.units)
            {
                "input_starved"
            } else {
                "ready"
            };
            *self
                .metrics
                .producer_state_ticks
                .entry(p.id.clone())
                .or_default()
                .entry(reason.into())
                .or_default() += 1;
        }
        self.arrivals();
        self.sources();
        self.production();
        self.manual_transfers();
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
            let reserved = self
                .manual_jobs
                .get(&a.id)
                .filter(|j| !j.deposit)
                .map_or(0, |j| j.remaining);
            if a.cargo.total() + incoming + reserved > a.cargo.capacity {
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
