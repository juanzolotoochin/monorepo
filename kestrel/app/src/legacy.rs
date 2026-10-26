
//! The original prototype's view of a settlement, rebuilt from the lockstep
//! simulation each frame. Bevy code reads these plain `f32` structs (arena
//! coordinates, names instead of indices), interpolated between the last two
//! ticks, and scripts and traces serialize them in the shape they always had.
//! Nothing here feeds back into the simulation.

use std::collections::BTreeMap;

use content::scenario::{self as def, CargoLayout, ProjectileKind, Weapon};
use fx::{Fx, FxVec2};
use serde::Serialize;
use sim::settlement::{self as s, Endpoint as SimEndpoint, Settlement as SimSettlement};

pub type Point = [f32; 2];

/// Ticks per second of the simulation.
pub const HZ: f32 = content::TICKS_PER_SECOND as f32;

#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub skin: String,
    pub scale: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stack {
    pub item: String,
    pub units: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Depot {
    pub id: String,
    pub label: String,
    pub position: Point,
    pub capacity: u32,
    pub stock_anchor: [f32; 3],
    pub stock_layout: CargoLayout,
}

#[derive(Clone, Debug, Serialize)]
pub struct Equipment {
    pub depot: String,
    pub item: String,
    pub skin: String,
    pub capacity: u32,
    pub radius: f32,
    pub offset: [f32; 3],
    pub cargo_anchor: [f32; 3],
    pub cargo_layout: CargoLayout,
}

#[derive(Clone, Debug, Serialize)]
pub struct Agent {
    pub controlled: bool,
    pub id: String,
    pub label: String,
    pub skin: String,
    pub speed: f32,
    pub radius: f32,
    pub capacity: u32,
    pub transfer_seconds: f32,
    pub cargo_anchor: [f32; 3],
    pub cargo_layout: CargoLayout,
    pub equipment: Option<Equipment>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Producer {
    pub description: String,
    pub radius: f32,
    pub id: String,
    pub label: String,
    pub position: Point,
    pub skin: String,
    pub input: String,
    pub output: String,
    pub ingredients: Vec<Stack>,
    pub products: Vec<Stack>,
    pub seconds: f32,
    pub max_cycles: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Defender {
    pub description: String,
    pub label: String,
    pub radius: f32,
    pub id: String,
    pub position: Point,
    pub skin: String,
    pub depot: String,
    pub ammo: String,
    pub range: f32,
    pub period: f32,
    pub damage: u32,
    pub projectile: ProjectileKind,
}

#[derive(Clone, Debug, Serialize)]
pub struct Wave {
    pub id: String,
    pub skin: String,
    pub hp: u32,
}

/// The scenario as the view sees it: names, appearance and positions in
/// arena coordinates.
#[derive(Clone, Debug, Serialize)]
pub struct Scenario {
    pub id: String,
    pub extent: f32,
    pub weapons: Vec<Weapon>,
    pub items: Vec<Item>,
    pub depots: Vec<Depot>,
    pub agents: Vec<Agent>,
    pub producers: Vec<Producer>,
    pub defenders: Vec<Defender>,
    pub waves: Vec<Wave>,
}

fn point(p: [Fx; 2]) -> Point {
    [p[0].to_f32(), p[1].to_f32()]
}

fn stacks(stacks: &[def::Stack]) -> Vec<Stack> {
    stacks.iter().map(|s| Stack { item: s.item.clone(), units: s.units }).collect()
}

impl Scenario {
    pub fn new(scenario: &def::Scenario) -> Scenario {
        Scenario {
            id: scenario.id.clone(),
            extent: scenario.extent.to_f32(),
            weapons: scenario.weapons.clone(),
            items: scenario.items.iter().map(|i| Item { id: i.id.clone(), label: i.label.clone(), skin: i.skin.clone(), scale: i.scale }).collect(),
            depots: scenario
                .depots
                .iter()
                .map(|d| Depot {
                    id: d.id.clone(),
                    label: d.label.clone(),
                    position: point(d.position),
                    capacity: d.capacity,
                    stock_anchor: d.stock_anchor,
                    stock_layout: d.stock_layout.clone(),
                })
                .collect(),
            agents: scenario
                .agents
                .iter()
                .map(|a| {
                    let character = crate::lockstep::builtin_character(&a.character);
                    let transfer = character.carry.as_ref().map_or(0, |c| c.transfer);
                    Agent {
                        controlled: a.controlled,
                        id: a.id.clone(),
                        label: a.label.clone(),
                        // The controlled player is drawn by the scene's actor, not a skin.
                        skin: crate::lockstep::builtin_character_view(&a.character).skin.clone().unwrap_or_default(),
                        speed: character.movement.walk.to_f32(),
                        radius: character.body.radius.to_f32(),
                        capacity: a.capacity,
                        transfer_seconds: transfer as f32 / content::TICKS_PER_SECOND as f32,
                        cargo_anchor: a.cargo_anchor,
                        cargo_layout: a.cargo_layout.clone(),
                        equipment: a.equipment.as_ref().map(|e| Equipment {
                            depot: e.depot.clone(),
                            item: e.item.clone(),
                            skin: e.skin.clone(),
                            capacity: e.capacity,
                            radius: e.radius.to_f32(),
                            offset: e.offset.map(Fx::to_f32),
                            cargo_anchor: e.cargo_anchor,
                            cargo_layout: e.cargo_layout.clone(),
                        }),
                    }
                })
                .collect(),
            producers: scenario
                .producers
                .iter()
                .map(|p| Producer {
                    description: p.description.clone(),
                    radius: p.radius.to_f32(),
                    id: p.id.clone(),
                    label: p.label.clone(),
                    position: point(p.position),
                    skin: p.skin.clone(),
                    input: p.input.clone(),
                    output: p.output.clone(),
                    ingredients: stacks(&p.ingredients),
                    products: stacks(&p.products),
                    seconds: p.seconds.to_f32(),
                    max_cycles: p.max_cycles,
                })
                .collect(),
            defenders: scenario
                .defenders
                .iter()
                .map(|d| Defender {
                    description: d.description.clone(),
                    label: d.label.clone(),
                    radius: d.radius.to_f32(),
                    id: d.id.clone(),
                    position: point(d.position),
                    skin: d.skin.clone(),
                    depot: d.depot.clone(),
                    ammo: d.ammo.clone(),
                    range: d.range.to_f32(),
                    period: d.period.to_f32(),
                    damage: d.damage,
                    projectile: d.projectile,
                })
                .collect(),
            waves: scenario
                .waves
                .iter()
                .map(|w| {
                    let content = crate::lockstep::builtin_content();
                    let hp = content
                        .names
                        .character_index(&w.character)
                        .and_then(|ix| content.rules.characters[ix as usize].health.as_ref())
                        .map_or(1, |health| health.hp);
                    let skin = crate::lockstep::builtin_character_view(&w.character).skin.clone().unwrap_or_default();
                    Wave { id: w.id.clone(), skin, hp }
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Inventory {
    pub capacity: u32,
    pub items: BTreeMap<String, u32>,
}

impl Inventory {
    pub fn units(&self, item: &str) -> u32 {
        *self.items.get(item).unwrap_or(&0)
    }

    pub fn total(&self) -> u32 {
        self.items.values().sum()
    }
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
    pub phase: String,
    pub cargo: Inventory,
    pub trailer: Option<Trailer>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProducerState {
    pub enabled: bool,
    /// Ticks left in the current cycle.
    pub remaining: u64,
    pub cycles: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct DefenderState {
    pub enabled: bool,
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
    pub armor: u32,
    pub pierced: bool,
    pub shocked_until: u64,
    pub stagger_until: u64,
    pub knockback: Point,
    /// The swing it is winding up, drawn until it lands.
    pub windup: Option<Windup>,
    /// The tick its last swing landed, hit or miss.
    pub swung: Option<u64>,
}

/// A threat's swing being wound up (`sim::settlement::Windup`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Windup {
    /// The player it swings at.
    pub agent: String,
    /// The tick the swing lands.
    pub lands: u64,
    /// The windup's length: it started on tick `lands - ticks`.
    pub ticks: u64,
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
    pub born: u64,
    pub arrival: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Projectile {
    pub id: u64,
    pub item: String,
    pub from: Point,
    pub launch_height: f32,
    pub to: Point,
    pub born: u64,
    pub arrival: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SpentProjectile {
    pub id: u64,
    pub item: String,
    pub from: Point,
    pub to: Point,
    pub born: u64,
    pub arrival: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Pinned {
    pub target: u64,
    pub wave: usize,
    pub position: Point,
    pub heading: f32,
    pub born: u64,
    pub land: u64,
}

/// The original counters, by their original names.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Metrics {
    pub picked: u64,
    pub delivered: u64,
    pub equipped: u64,
    pub cycles: u64,
    pub assignments: u64,
    pub shots: u64,
    pub kills: u64,
    pub melee_hits: u64,
    pub melee_kills: u64,
    pub bow_hits: u64,
    pub spear_hits: u64,
    pub arc_hits: u64,
    pub decapitations: u64,
    pub deflections: u64,
    pub armor_breaks: u64,
    pub bolt_impales: u64,
    pub bolt_pierces: u64,
    pub knockback_meters: f64,
    pub breaches: u64,
    pub delivered_items: BTreeMap<String, u64>,
    pub deliveries_by_agent: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ManualJob {
    pub depot: String,
    pub item: String,
    pub remaining: u32,
    pub deposit: bool,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Impact {
    Sword,
    Decapitate,
    Spear,
    Arrow,
    Deflect,
    Arc,
    Laser,
    BoltImpale,
    BoltPierce,
}

/// This frame's events, in the original shapes.
#[derive(Clone, Debug)]
pub enum Event {
    Produced { producer: String },
    Equipped { agent: String },
    Shot { defender: String },
    /// An avatar loosed an arrow.
    BowFired,
    /// `by` is the avatar that struck, for melee and arc hits.
    Hit { target: u64, by: Option<String>, projectile: Option<u64>, position: Point, killed: bool, impact: Impact },
    PinnedLanded { target: u64, position: Point },
    BlockedShot { position: Point },
    ArcFired { from: Point, to: Point },
    LaserFired { agent: u16, to: Point, hit_actor: bool, cover: Option<u16> },
    Breach { position: Point },
    /// A threat's swing landed on player `agent` (`hit`) or missed. `from`
    /// is where the threat stands, if it still does.
    ThreatSwing { threat: u64, agent: String, from: Option<Point>, hit: bool, killed: bool },
}

/// One frame's settlement, interpolated between the last two ticks.
#[derive(Clone, Debug, Serialize)]
pub struct Simulation {
    #[serde(skip)]
    config: Scenario,
    /// The last tick that ran, plus how far render time is past it.
    pub tick: u64,
    #[serde(skip)]
    pub alpha: f32,
    pub paused: bool,
    pub health: u32,
    pub depots: BTreeMap<String, Inventory>,
    pub agents: BTreeMap<String, AgentState>,
    pub producers: BTreeMap<String, ProducerState>,
    pub defenders: BTreeMap<String, DefenderState>,
    pub links: BTreeMap<String, bool>,
    pub transfers: BTreeMap<u64, Transfer>,
    pub threats: BTreeMap<u64, Threat>,
    pub projectiles: BTreeMap<u64, Projectile>,
    pub spent_projectiles: BTreeMap<u64, SpentProjectile>,
    pub pinned: BTreeMap<u64, Pinned>,
    pub metrics: Metrics,
    pub manual_jobs: BTreeMap<String, ManualJob>,
    #[serde(skip)]
    pub events: Vec<Event>,
    #[serde(skip)]
    balances: BTreeMap<String, Balance>,
    #[serde(skip)]
    statuses: BTreeMap<String, &'static str>,
    #[serde(skip)]
    valid: Result<(), String>,
}

impl Simulation {
    pub fn config(&self) -> &Scenario {
        &self.config
    }

    /// Render time in ticks: the last tick plus the interpolation fraction.
    pub fn time(&self) -> f32 {
        self.tick as f32 + self.alpha
    }

    /// The tick interpolated positions show, between the previous tick
    /// (`alpha` 0) and the last one. Timing drawn on a moving entity, such
    /// as a threat's windup, uses this so it keeps pace with the entity.
    pub fn drawn_time(&self) -> f32 {
        self.tick as f32 - 1.0 + self.alpha
    }

    pub fn seconds(&self) -> f64 {
        self.tick as f64 / HZ as f64
    }

    pub fn balances(&self) -> BTreeMap<String, Balance> {
        self.balances.clone()
    }

    pub fn validate_state(&self) -> Result<(), String> {
        self.valid.clone()
    }

    pub fn production_status(&self, id: &str) -> Option<&'static str> {
        self.statuses.get(id).copied()
    }

    /// Builds the view of `current` (interpolated from `previous` by `alpha`).
    /// `center` is the arena center in world tiles; `events` are every tick's
    /// events since the last frame.
    pub fn new(
        config: &Scenario,
        previous: Option<&SimSettlement>,
        current: &SimSettlement,
        tick: u64,
        alpha: f32,
        center: FxVec2,
        events: &[s::Event],
    ) -> Simulation {
        let local = |p: FxVec2| [(p.x - center.x).to_f32(), (p.y - center.y).to_f32()];
        let blend = |before: Option<FxVec2>, now: FxVec2| -> Point {
            let now = local(now);
            match before {
                Some(b) => {
                    let b = local(b);
                    [b[0] + (now[0] - b[0]) * alpha, b[1] + (now[1] - b[1]) * alpha]
                }
                None => now,
            }
        };
        let angle = |before: Option<Fx>, now: Fx| -> f32 {
            let now = now.to_f32();
            match before {
                Some(b) => {
                    let b = b.to_f32();
                    let turn = (now - b + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                    b + turn * alpha
                }
                None => now,
            }
        };
        let item = |i: s::ItemIx| current.items[i as usize].clone();
        let agent_name = |a: s::AgentIx| current.agents[a as usize].id.clone();
        let endpoint = |e: SimEndpoint| match e {
            SimEndpoint::Depot(d) => Endpoint::Depot(current.depots[d as usize].id.clone()),
            SimEndpoint::Cargo(a) => Endpoint::Cargo(agent_name(a)),
        };
        let inventory = |units: &[u32], capacity: u32| Inventory {
            capacity,
            items: units.iter().enumerate().filter(|(_, &n)| n > 0).map(|(i, &n)| (current.items[i].clone(), n)).collect(),
        };
        let agents = current
            .agents
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let before = previous.and_then(|p| current.same_agent_in(i as s::AgentIx, p));
                let trailer = match (a.hauler().and_then(|h| h.trailer.as_ref()), before.and_then(|b| b.hauler()?.trailer.as_ref())) {
                    (Some(t), b) => Some(Trailer {
                        position: blend(b.map(|b| b.position), t.position),
                        heading: angle(b.map(|b| b.heading), t.heading),
                        travelled: t.travelled.to_f32(),
                    }),
                    (None, _) => None,
                };
                let state = AgentState {
                    id: a.id.clone(),
                    position: blend(before.map(|b| b.body.position), a.body.position),
                    heading: angle(before.map(|b| b.body.heading), a.body.heading),
                    travelled: a.body.travelled.to_f32(),
                    // A player shows as an idle hauler.
                    phase: format!("{:?}", a.hauler().map_or(s::Phase::Idle, |h| h.phase)),
                    cargo: inventory(&a.carry.inventory.counts, a.carry.inventory.capacity),
                    trailer,
                };
                (a.id.clone(), state)
            })
            .collect();
        let threats = current
            .threats
            .iter()
            .map(|t| {
                let before = previous.and_then(|p| p.threat(t.id));
                let attack = current.characters[current.waves[t.wave as usize].character as usize].attack;
                // Positions are drawn between the previous tick and this one,
                // so a windup that landed this tick is still drawn finishing.
                // One a stagger cancelled (landing later) just goes away.
                let windup = t.windup.or_else(|| before.and_then(|b| b.windup).filter(|w| w.lands <= tick));
                let windup = windup.zip(attack).map(|(w, a)| Windup { agent: agent_name(w.agent), lands: w.lands, ticks: a.windup });
                // A landed swing sets `ready_at` to the tick after it plus the
                // cooldown; it is 0 before the first one.
                let swung = attack.and_then(|a| t.ready_at.checked_sub(1 + a.cooldown));
                let threat = Threat {
                    id: t.id,
                    wave: t.wave as usize,
                    position: blend(before.map(|b| b.position), t.position),
                    heading: angle(before.map(|b| b.heading), t.heading),
                    travelled: t.travelled.to_f32(),
                    hp: t.hp,
                    armor: t.armor,
                    pierced: t.pierced,
                    shocked_until: t.shocked_until,
                    stagger_until: t.stagger_until,
                    knockback: [t.knockback.x.to_f32(), t.knockback.y.to_f32()],
                    windup,
                    swung,
                };
                (t.id, threat)
            })
            .collect();
        let projectile_item = |i: Option<s::ItemIx>| i.map_or_else(|| "arrow".to_string(), &item);
        let c = &current.combat;
        let metrics = Metrics {
            picked: current.metrics.picked,
            delivered: current.metrics.delivered,
            equipped: current.metrics.equipped,
            cycles: current.metrics.cycles,
            assignments: current.metrics.assignments,
            shots: c.shots,
            kills: c.kills,
            melee_hits: c.melee_hits,
            melee_kills: c.melee_kills,
            bow_hits: c.bow_hits,
            spear_hits: c.spear_hits,
            arc_hits: c.arc_hits,
            decapitations: c.decapitations,
            deflections: c.deflections,
            armor_breaks: c.armor_breaks,
            bolt_impales: c.bolt_impales,
            bolt_pierces: c.bolt_pierces,
            knockback_meters: c.knockback_meters.to_f64(),
            breaches: c.breaches,
            delivered_items: current.metrics.delivered_items.clone(),
            deliveries_by_agent: current.metrics.deliveries_by_agent.clone(),
        };
        let balances = current
            .balances()
            .into_iter()
            .map(|(id, b)| {
                let balance = Balance {
                    supplied: b.supplied,
                    stored: b.stored,
                    carried: b.carried,
                    in_transit: b.in_transit,
                    equipped: b.equipped,
                    consumed: b.consumed,
                    spent: b.spent,
                    conserved: b.conserved,
                };
                (id, balance)
            })
            .collect();
        let statuses = current
            .producers
            .iter()
            .map(|p| {
                let required: u32 = p.products.iter().map(|s| s.1).sum();
                let status = if p.remaining > 0 {
                    "processing"
                } else if !p.enabled {
                    "disabled"
                } else if p.max_cycles.is_some_and(|limit| p.cycles >= limit) {
                    "commission_complete"
                } else if current.depots[p.output as usize].inventory.free() < required {
                    "output_blocked"
                } else if p.ingredients.iter().any(|&(i, n)| current.depots[p.input as usize].inventory.available(i) < n) {
                    "input_starved"
                } else {
                    "ready"
                };
                (p.id.clone(), status)
            })
            .collect();
        let events = events
            .iter()
            .filter_map(|e| {
                let p = |v: FxVec2| local(v);
                Some(match e {
                    s::Event::Produced { producer } => Event::Produced { producer: current.producers[*producer as usize].id.clone() },
                    s::Event::Equipped { agent } => Event::Equipped { agent: agent_name(*agent) },
                    s::Event::Shot { defender } => Event::Shot { defender: current.defenders[*defender as usize].id.clone() },
                    s::Event::BowFired { .. } => Event::BowFired,
                    s::Event::Hit { target, agent, projectile, position, killed, impact } => Event::Hit {
                        target: *target,
                        by: agent.map(agent_name),
                        projectile: *projectile,
                        position: p(*position),
                        killed: *killed,
                        impact: match impact {
                            s::Impact::Sword => Impact::Sword,
                            s::Impact::Decapitate => Impact::Decapitate,
                            s::Impact::Spear => Impact::Spear,
                            s::Impact::Arrow => Impact::Arrow,
                            s::Impact::Deflect => Impact::Deflect,
                            s::Impact::Arc => Impact::Arc,
                            s::Impact::Laser => Impact::Laser,
                            s::Impact::BoltImpale => Impact::BoltImpale,
                            s::Impact::BoltPierce => Impact::BoltPierce,
                        },
                    },
                    s::Event::PinnedLanded { target, position } => Event::PinnedLanded { target: *target, position: p(*position) },
                    s::Event::BlockedShot { position } => Event::BlockedShot { position: p(*position) },
                    s::Event::ArcFired { from, to, .. } => Event::ArcFired { from: p(*from), to: p(*to) },
                    s::Event::LaserFired { agent, to, hit_actor, cover, .. } => Event::LaserFired { agent: *agent, to: p(*to), hit_actor: *hit_actor, cover: *cover },
                    s::Event::Breach { position } => Event::Breach { position: p(*position) },
                    s::Event::PileToss { .. } => return None,
                    // Drawn from the threat's `windup` instead, which also
                    // covers a client that joins mid-windup.
                    s::Event::ThreatWindup { .. } => return None,
                    s::Event::ThreatSwing { threat, agent, hit, killed, .. } => Event::ThreatSwing {
                        threat: *threat,
                        agent: agent_name(*agent),
                        from: current.threat(*threat).map(|t| p(t.position)),
                        hit: *hit,
                        killed: *killed,
                    },
                })
            })
            .collect();
        Simulation {
            config: config.clone(),
            tick,
            alpha,
            paused: false,
            health: current.health,
            depots: current.depots.iter().map(|d| (d.id.clone(), inventory(&d.inventory.counts, d.inventory.capacity))).collect(),
            agents,
            producers: current
                .producers
                .iter()
                .map(|p| (p.id.clone(), ProducerState { enabled: p.enabled, remaining: p.remaining, cycles: p.cycles }))
                .collect(),
            defenders: current
                .defenders
                .iter()
                .map(|d| (d.id.clone(), DefenderState { enabled: d.enabled, aim: d.aim.map(local), fired_at: d.fired_at }))
                .collect(),
            links: current.links.iter().map(|l| (l.id.clone(), l.enabled)).collect(),
            transfers: current
                .transfers
                .values()
                .map(|t| {
                    let transfer = Transfer { id: t.id, item: item(t.item), from: endpoint(t.from), to: endpoint(t.to), born: t.born, arrival: t.arrival };
                    (t.id, transfer)
                })
                .collect(),
            threats,
            projectiles: current
                .projectiles
                .iter()
                .map(|p| {
                    let projectile = Projectile {
                        id: p.id,
                        item: projectile_item(p.item),
                        from: local(p.from),
                        launch_height: p.launch_height.to_f32(),
                        to: local(p.to),
                        born: p.born,
                        arrival: p.arrival,
                    };
                    (p.id, projectile)
                })
                .collect(),
            spent_projectiles: current
                .spent
                .iter()
                .map(|p| {
                    let spent = SpentProjectile { id: p.id, item: projectile_item(p.item), from: local(p.from), to: local(p.to), born: p.born, arrival: p.arrival };
                    (p.id, spent)
                })
                .collect(),
            pinned: current
                .pinned
                .iter()
                .map(|p| {
                    let pinned = Pinned { target: p.target, wave: p.wave as usize, position: local(p.position), heading: p.heading.to_f32(), born: p.born, land: p.land };
                    (p.target, pinned)
                })
                .collect(),
            metrics,
            manual_jobs: current
                .agents
                .iter()
                .filter_map(|a| {
                    let job = a.avatar()?.job.as_ref()?;
                    let manual = ManualJob {
                        depot: current.depots[job.depot as usize].id.clone(),
                        item: item(job.item),
                        remaining: job.remaining,
                        deposit: job.deposit,
                    };
                    Some((a.id.clone(), manual))
                })
                .collect(),
            events,
            balances,
            statuses,
            valid: current.validate(),
        }
    }
}

/// A scenario agent's character, from the builtin content the sim also uses.
fn agent_character(name: &str) -> &'static content::character::CharacterDef {
    let content = crate::lockstep::builtin_content();
    let ix = content.names.character_index(name).unwrap_or_else(|| panic!("unknown character {name:?}"));
    &content.rules.characters[ix as usize]
}
