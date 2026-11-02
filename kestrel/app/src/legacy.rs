
//! The original prototype's view of a settlement, rebuilt from the lockstep
//! simulation each frame. Bevy code reads these plain `f32` structs (arena
//! coordinates, names instead of indices), interpolated between the last two
//! ticks, and scripts and traces serialize them in the shape they always had.
//! Nothing here feeds back into the simulation.

use std::collections::BTreeMap;

use content::scenario::{self as def, CargoLayout, ProjectileKind, Weapon};
use fx::{Fx, FxVec2};
use serde::Serialize;
use sim::settlement::{self as s, InventoryRef as SimHolder, Settlement as SimSettlement};

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

/// A sprite as the view sees it: drawn by its character's skin.
#[derive(Clone, Debug, Serialize)]
pub struct Sprite {
    pub id: String,
    pub label: String,
    pub skin: String,
}

/// A building kind as the view sees it.
#[derive(Clone, Debug, Serialize)]
pub struct BuildingKind {
    pub id: String,
    pub label: String,
    pub skin: String,
    pub radius: f32,
    /// What a gatherer makes; a ballista makes nothing.
    pub product: Option<String>,
    /// Seconds per unit with a sprite inside.
    pub period: f32,
    pub cap: u32,
    /// A ballista's ammo item and how much its store holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ammo: Option<(String, u32)>,
    /// The parts it is put up from, in install order.
    pub parts: Vec<Part>,
}

/// A part of a building kind: its label ("FOUNDATION") and the items it
/// takes, by item id.
#[derive(Clone, Debug, Serialize)]
pub struct Part {
    pub label: String,
    pub materials: Vec<(String, u32)>,
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
    /// The town's sprites and building kinds; empty, and left out of
    /// traces, in every other scene.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sprites: Vec<Sprite>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub building_kinds: Vec<BuildingKind>,
}

impl Scenario {
    /// A sprite's definition, by agent id.
    pub fn sprite(&self, id: &str) -> Option<&Sprite> {
        self.sprites.iter().find(|s| s.id == id)
    }
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
            sprites: scenario
                .sprites
                .iter()
                .map(|s| Sprite {
                    id: s.id.clone(),
                    label: s.label.clone(),
                    skin: crate::lockstep::builtin_character_view(&s.character).skin.clone().unwrap_or_default(),
                })
                .collect(),
            building_kinds: scenario
                .building_kinds
                .iter()
                .map(|k| BuildingKind {
                    id: k.id.clone(),
                    label: k.label.clone(),
                    skin: k.skin.clone(),
                    radius: k.radius.to_f32(),
                    product: k.product.clone(),
                    period: k.period.to_f32(),
                    cap: k.cap,
                    ammo: k.defence.as_ref().map(|d| (d.ammo.clone(), d.ammo_cap)),
                    parts: k.parts.iter().map(|p| Part { label: p.label.clone(), materials: p.materials.iter().map(|m| (m.item.clone(), m.units)).collect() }).collect(),
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

/// What a sprite is doing, as the player reads it (A7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpriteActivity {
    /// No job: home, or standing there.
    Idle,
    /// Walking: to its building, or home.
    Moving,
    /// No building of its job's kind is free.
    Waiting,
    /// Inside its building.
    Working,
}

/// A sprite's job and what it is doing about it.
#[derive(Clone, Debug, Serialize)]
pub struct SpriteView {
    /// The building kind it staffs (its id), or none when idle.
    pub job: Option<String>,
    pub activity: SpriteActivity,
    /// The building it holds a slot in, claimed or inside.
    pub building: Option<usize>,
}

/// Who works a building.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Occupant {
    Sprite(String),
    Elder(String),
}

/// Why a building is or isn't producing, as the player reads it (A7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildingStatus {
    /// Still a site, being put up.
    Site,
    /// Built, with nobody inside (a sprite may be on its way).
    Idle,
    Working,
    /// Its output holds its occupant's cap: production waits for a pickup.
    Full,
    /// A ballista with someone inside and nothing in its ammo store: it
    /// can't shoot (B2).
    NoAmmo,
}

/// A building as the view sees it (`sim::settlement::BuildingState`).
#[derive(Clone, Debug, Serialize)]
pub struct Building {
    /// Its kind's id.
    pub kind: String,
    pub position: Point,
    pub output_position: Point,
    /// Where its occupant stands.
    pub slot_position: Point,
    pub status: BuildingStatus,
    pub occupant: Option<Occupant>,
    /// The sprite on its way in on a claim.
    pub claimed_by: Option<String>,
    /// How far the next unit is, 0 to 1.
    pub progress: f32,
    /// The produced units waiting to be picked up.
    pub output: Inventory,
    /// What the output holds before production pauses, for this occupant.
    pub cap: u32,
    /// While it is a site: the part going in next and its materials.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<Site>,
    /// A ballista's: where it last aimed and the tick it last shot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turret: Option<DefenderState>,
}

/// A site as the view sees it (`sim::settlement::Stage::Site`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Site {
    /// The next part to install, an index into its kind's `parts`, and how
    /// many parts there are.
    pub part: usize,
    pub parts: usize,
    /// The next part's materials: item id, units landed, units it takes.
    pub materials: Vec<(String, u32, u32)>,
    /// Whether they are all there, so the part can be installed.
    pub ready: bool,
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
    /// A building's output pile, by building index.
    Output(u16),
    /// A site's materials, by building index: delivered to where its output
    /// pile will be.
    Site(u16),
    /// A ground pile of a dropped bundle, by pile id.
    Ground(u32),
}

/// A pile of a dropped bundle on the ground (`sim::settlement::piles`, a
/// pile whose item is no mineral). Mineral piles are the pile view's.
#[derive(Clone, Debug, Serialize)]
pub struct GroundPile {
    pub id: u32,
    pub position: Point,
    /// What it holds: one item.
    pub stock: Inventory,
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
    /// Player `agent` loosed an arrow: `projectile`, unless it is already
    /// gone.
    BowFired { agent: u16, projectile: Option<u64> },
    /// `by` is the avatar that struck, for melee and arc hits.
    Hit { target: u64, by: Option<String>, projectile: Option<u64>, position: Point, killed: bool, impact: Impact },
    PinnedLanded { target: u64, position: Point },
    BlockedShot { position: Point },
    /// Player `agent` fired the arc coil from `from`; its arc reaches `to`.
    ArcFired { agent: u16, from: Point, to: Point },
    LaserFired { agent: u16, to: Point, hit_actor: bool, cover: Option<u16> },
    Breach { position: Point },
    /// A threat's swing landed on player `agent` (`hit`) or missed. `from`
    /// is where the threat stands, if it still does.
    ThreatSwing { threat: u64, agent: String, from: Option<Point>, hit: bool, killed: bool },
    /// Building `building` (an index into `Simulation::buildings`) put a
    /// unit in its output.
    BuildingProduced { building: usize },
    /// Player `agent`'s bundle action was refused; `reason.message()` says
    /// why, for the view to show (M2.2).
    BundleRefused { agent: String, reason: s::avatar::BundleRefusal },
    /// A blueprint became site `building`.
    BlueprintPlaced { building: usize },
    /// A blueprint couldn't go down; `reason.message()` says why.
    BlueprintRefused { reason: s::PlacementRefusal },
    /// A part of `building` went in; `built` when it was the last.
    PartInstalled { building: usize, built: bool },
    /// A Burst put `units` in building `building`'s output at once.
    BuildingBurst { building: usize, units: u32 },
    /// Player `agent`'s Burst was refused; `reason.message()` says why.
    BurstRefused { agent: String, reason: s::BurstRefusal },
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
    /// The town's buildings and sprites; empty, and left out of traces, in
    /// every other scene.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buildings: Vec<Building>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub sprites: BTreeMap<String, SpriteView>,
    /// Dropped bundles on the ground; empty, and left out of traces, until
    /// a town's elder drops one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ground_piles: Vec<GroundPile>,
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
        // Mineral piles and the collector are drawn by the pile view, not
        // here; a dropped bundle's pile is.
        let lab = current.piles.as_ref();
        let mineral = |i: s::ItemIx| lab.is_some_and(|lab| lab.is_mineral(i));
        // A unit of `item` to or from `e`.
        let endpoint = |e: SimHolder, item: s::ItemIx| match e {
            SimHolder::Depot(d) => Some(Endpoint::Depot(current.depots[d as usize].id.clone())),
            SimHolder::Agent(a) => Some(Endpoint::Cargo(agent_name(a))),
            SimHolder::Pile(id) if !mineral(item) => Some(Endpoint::Ground(id)),
            SimHolder::Pile(_) | SimHolder::Collector => None,
            SimHolder::Output(b) => Some(Endpoint::Output(b)),
            SimHolder::Site(b) => Some(Endpoint::Site(b)),
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
                    // Depot items only: a player's minerals are drawn by the
                    // pile view.
                    cargo: inventory(current.depot_cargo(a), a.carry.inventory.capacity),
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
                let attack = current.characters[t.character as usize].attack;
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
                    s::Event::BowFired { agent, from, to, height, .. } => Event::BowFired {
                        agent: *agent,
                        // The newest player arrow flying as it says.
                        projectile: current
                            .projectiles
                            .iter()
                            .rev()
                            .find(|p| p.defender.is_none() && p.from == *from && p.to == *to && p.launch_height == *height)
                            .map(|p| p.id),
                    },
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
                    s::Event::ArcFired { agent, from, to, .. } => Event::ArcFired { agent: *agent, from: p(*from), to: p(*to) },
                    s::Event::LaserFired { agent, to, hit_actor, cover, .. } => Event::LaserFired { agent: *agent, to: p(*to), hit_actor: *hit_actor, cover: *cover },
                    s::Event::Breach { position } => Event::Breach { position: p(*position) },
                    s::Event::PileToss { .. } => return None,
                    // Drawn from the threat's `windup` instead, which also
                    // covers a client that joins mid-windup.
                    s::Event::ThreatWindup { .. } => return None,
                    s::Event::BuildingProduced { building } => Event::BuildingProduced { building: *building as usize },
                    s::Event::BundleRefused { agent, reason } => Event::BundleRefused { agent: agent_name(*agent), reason: *reason },
                    s::Event::BlueprintPlaced { building } => Event::BlueprintPlaced { building: *building as usize },
                    s::Event::BlueprintRefused { reason, .. } => Event::BlueprintRefused { reason: *reason },
                    s::Event::PartInstalled { building, built, .. } => Event::PartInstalled { building: *building as usize, built: *built },
                    s::Event::BuildingBurst { building, units } => Event::BuildingBurst { building: *building as usize, units: *units },
                    // A ballista building's shot sounds as a defender's.
                    s::Event::BuildingShot { building } => Event::Shot { defender: format!("building/{building}") },
                    s::Event::BurstRefused { agent, reason } => Event::BurstRefused { agent: agent_name(*agent), reason: *reason },
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
        let buildings = current
            .buildings
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let kind = current.building_kind(i as s::BuildingIx);
                let occupant = b.slot.occupant().map(|o| match o {
                    s::Occupant::Sprite(a) => Occupant::Sprite(agent_name(a)),
                    s::Occupant::Elder(a) => Occupant::Elder(agent_name(a)),
                });
                let cap = kind.cap_for(b.slot.occupant());
                let ballista = kind.defence.is_some();
                let status = if !b.built() {
                    BuildingStatus::Site
                } else if occupant.is_none() {
                    BuildingStatus::Idle
                } else if ballista && b.output.total() == 0 {
                    BuildingStatus::NoAmmo
                } else if ballista {
                    BuildingStatus::Working
                } else if b.output.total() + b.output.total_in() >= cap {
                    BuildingStatus::Full
                } else {
                    BuildingStatus::Working
                };
                Building {
                    kind: kind.id.clone(),
                    position: local(b.position),
                    output_position: local(b.output_position),
                    slot_position: local(b.slot_position(kind)),
                    status,
                    occupant,
                    claimed_by: match b.slot {
                        s::Slot::Claimed(a) => Some(agent_name(a)),
                        _ => None,
                    },
                    progress: (b.progress.to_f32() / kind.period.max(1) as f32).clamp(0.0, 1.0),
                    output: inventory(&b.output.counts, cap),
                    cap,
                    site: b.site().map(|(part, materials)| Site {
                        part: part as usize,
                        parts: kind.parts.len(),
                        materials: kind.parts.get(part as usize).into_iter().flatten().map(|&(i, units)| (item(i), materials.count(i), units)).collect(),
                        ready: b.ready(kind),
                    }),
                    turret: ballista.then(|| DefenderState { enabled: true, aim: b.turret.aim.map(local), fired_at: b.turret.fired_at }),
                }
            })
            .collect();
        let sprites = current
            .agents
            .iter()
            .filter_map(|a| {
                let sprite = a.sprite()?;
                let job = match sprite.job {
                    s::SpriteJob::Idle => None,
                    s::SpriteJob::Staff(kind) => Some(current.building_kinds[kind as usize].id.clone()),
                };
                let activity = match sprite.current() {
                    _ if sprite.inside() => SpriteActivity::Working,
                    Some(s::Step::Wait) => SpriteActivity::Waiting,
                    // Walking, or about to: a claim is granted on the way.
                    Some(s::Step::GoTo(_) | s::Step::Claim(_) | s::Step::Enter(_)) => SpriteActivity::Moving,
                    // An idle sprite's plan is done once it is home; a job
                    // with no plan yet gets one on the next tick.
                    None if job.is_none() => SpriteActivity::Idle,
                    None => SpriteActivity::Waiting,
                };
                let view = SpriteView { job, activity, building: sprite.holds.map(|h| h.building() as usize) };
                Some((a.id.clone(), view))
            })
            .collect();
        // A pile of a depot item, or an empty one a unit of one still flies
        // from or to (its last, picked up).
        let flying = |id: u32| current.transfers.values().any(|t| !mineral(t.item) && (t.from == SimHolder::Pile(id) || t.to == SimHolder::Pile(id)));
        let ground_piles = lab
            .into_iter()
            .flat_map(|lab| lab.piles.iter())
            .filter(|&(&id, p)| p.source.is_none() && p.item().map_or_else(|| flying(id), |i| !mineral(i)))
            .map(|(&id, p)| GroundPile { id, position: local(p.position), stock: inventory(&p.inventory.counts, p.inventory.capacity) })
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
                .filter_map(|t| {
                    let transfer = Transfer { id: t.id, item: item(t.item), from: endpoint(t.from, t.item)?, to: endpoint(t.to, t.item)?, born: t.born, arrival: t.arrival };
                    Some((t.id, transfer))
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
                    // A player's depot transfer.
                    a.avatar()?;
                    let job = a.carry.job.as_ref()?;
                    let SimHolder::Depot(depot) = job.other else { return None };
                    let manual = ManualJob {
                        depot: current.depots[depot as usize].id.clone(),
                        item: item(job.item),
                        remaining: job.remaining,
                        deposit: job.direction == s::Direction::Deposit,
                    };
                    Some((a.id.clone(), manual))
                })
                .collect(),
            buildings,
            sprites,
            ground_piles,
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
