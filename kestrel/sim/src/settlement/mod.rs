//! The settlement: the original game's logistics and defense rules on the
//! lockstep architecture.
//!
//! - Depots, sources, links and producers are buildings. Their state changes
//!   only in single-threaded stages: the economy step at the start of the
//!   apply phase, and message handlers.
//! - Agents (haulers and player avatars) move in the parallel self-update
//!   stage, reading last tick's positions of everyone else. Anything that
//!   touches a depot, a dock or another entity is a message.
//! - Reservations are incremental counters on each depot (`out` per item,
//!   `incoming` per item), so checking availability is O(1). A debug
//!   invariant recomputes them from scratch.
//!
//! Positions are world tiles (1 tile = 1 m). Scenario files use arena
//! coordinates; [`Settlement::from_scenario`] offsets them by the arena center.

pub mod avatar;
pub mod combat;
mod economy;
mod haulers;
pub mod memory;
pub mod piano;
pub mod piles;

use std::collections::BTreeMap;

use content::character::{Brain, CharacterDef, CharacterIx};
use content::scenario::{self, Scenario, Weapon, WeaponTuning};
use content::Content;
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use crate::hash::{StateHash, StateHasher};
use crate::nav;
use crate::terrain::{Disc, Terrain};

pub use combat::{CombatMetrics, DefenderState, Event, Impact, Pinned, Projectile, SpentProjectile, Threat, TossEnd, WaveState};
pub use haulers::{AgentState, Phase};

pub type ItemIx = u16;
pub type DepotIx = u16;
pub type LinkIx = u16;
pub type AgentIx = u16;

/// Ticks an item spends in the air between a dock and a cargo bed (0.35 s).
pub const TRANSFER_FLIGHT_TICKS: u64 = content::millis_to_ticks(350);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepotState {
    pub id: String,
    pub position: FxVec2,
    pub capacity: u32,
    /// Units on hand, per item.
    pub stock: Vec<u32>,
    /// Units promised to haulers, recipes or the player, per item.
    pub out: Vec<u32>,
    /// Units on their way here (hauled, flying, being produced), per item.
    pub incoming: Vec<u32>,
}

impl DepotState {
    pub fn total(&self) -> u32 {
        self.stock.iter().sum()
    }

    /// Space not taken by stock or incoming units.
    pub fn free(&self) -> u32 {
        self.capacity.saturating_sub(self.total() + self.incoming.iter().sum::<u32>())
    }

    /// Stock not promised to anyone.
    pub fn available(&self, item: ItemIx) -> u32 {
        self.stock[item as usize].saturating_sub(self.out[item as usize])
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceState {
    pub depot: DepotIx,
    pub item: ItemIx,
    pub units: u32,
    pub every: u64,
    pub max_units: Option<u32>,
    pub total: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkState {
    pub id: String,
    pub from: DepotIx,
    pub to: DepotIx,
    pub item: ItemIx,
    pub target: u32,
    pub priority: u32,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProducerState {
    pub id: String,
    pub position: FxVec2,
    pub input: DepotIx,
    pub output: DepotIx,
    pub ingredients: Vec<(ItemIx, u32)>,
    pub products: Vec<(ItemIx, u32)>,
    pub ticks: u64,
    pub max_cycles: Option<u32>,
    pub enabled: bool,
    /// Ticks left in the current cycle; zero when idle.
    pub remaining: u64,
    pub cycles: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Endpoint {
    Depot(DepotIx),
    Cargo(AgentIx),
}

/// One unit in the air. It belongs to neither end until it arrives.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transfer {
    pub id: u64,
    pub item: ItemIx,
    pub from: Endpoint,
    pub to: Endpoint,
    pub start: FxVec2,
    pub end: FxVec2,
    pub born: u64,
    pub arrival: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accounting {
    pub initial: u64,
    pub generated: u64,
    pub produced: u64,
    pub consumed: u64,
    pub spent: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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

/// Counters for HUDs and tests. Changed only in single-threaded stages;
/// per-agent counters live on the agents and are summed here.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metrics {
    pub picked: u64,
    pub delivered: u64,
    pub equipped: u64,
    pub cycles: u64,
    pub assignments: u64,
    pub assignment_retries: u64,
    pub delivered_items: BTreeMap<String, u64>,
    pub deliveries_by_agent: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settlement {
    /// The content's characters, indexed by `CharacterIx`. Rules, not state:
    /// left out of saves and hashes and restored from content on load.
    #[serde(skip)]
    pub characters: Vec<CharacterDef>,
    pub scenario: String,
    pub items: Vec<String>,
    pub depots: Vec<DepotState>,
    pub sources: Vec<SourceState>,
    pub links: Vec<LinkState>,
    pub producers: Vec<ProducerState>,
    pub agents: Vec<haulers::AgentState>,
    /// In flight, by ID.
    pub transfers: BTreeMap<u64, Transfer>,
    /// The agent allowed to use each depot's dock, as resolved last tick.
    pub dock_owner: Vec<Option<AgentIx>>,
    pub accounts: Vec<Accounting>,
    pub metrics: Metrics,
    pub next_id: u64,

    pub base_health: u32,
    pub health: u32,
    #[serde(default)]
    pub player_health: u32,
    pub weapons: Vec<Weapon>,
    /// By `Weapon as usize`: the scenario's tuning or the defaults.
    pub tuning: Vec<WeaponTuning>,
    pub waves: Vec<WaveState>,
    /// Sorted by ID.
    pub threats: Vec<Threat>,
    pub defenders: Vec<DefenderState>,
    /// Sorted by ID.
    pub projectiles: Vec<Projectile>,
    pub spent: Vec<SpentProjectile>,
    pub pinned: Vec<Pinned>,
    pub combat: CombatMetrics,
    /// What happened during the last tick. Not part of the state.
    #[serde(skip)]
    pub events: Vec<Event>,

    /// Where the player starts and respawns.
    pub spawn: FxVec2,
    pub spawn_height: Fx,
    /// Piano locations and held notes are saved and hashed with the world so
    /// every peer renders and hears the same performance.
    #[serde(default)]
    pub pianos: Vec<FxVec2>,
    #[serde(default)]
    pub piano_notes: Vec<piano::PianoNote>,
    #[serde(default)]
    pub piano_serial: u64,
    pub piles: Option<piles::PileLab>,
    pub memory: Option<memory::Memory>,
}

/// Everything a scene contributes to a new world, in arena coordinates
/// (meters from the arena center).
#[derive(Clone, Debug, Default)]
pub struct SceneSetup {
    /// The settlement, if the scene has one. Without it the world holds just
    /// the player.
    pub scenario: Option<Scenario>,
    /// The character of every player avatar. A scenario's controlled
    /// agents must name the same one.
    pub player: String,
    /// Half-width of the arena when there is no scenario.
    pub extent: Fx,
    pub obstacles: Vec<Disc>,
    /// Independently damaged cover pieces, in stable scene order.
    pub destructibles: Vec<crate::terrain::Destructible>,
    /// Piano locations in scene coordinates, in the same order as the pack.
    pub pianos: Vec<FxVec2>,
    /// Obstacles that open when the memory trial's door unlocks.
    pub doors: Vec<Disc>,
    pub surfaces: Vec<crate::terrain::Surface>,
    pub platforms_only: bool,
    /// The ground can be dug, by characters with the Dig ability.
    pub diggable: bool,
    /// Generates the arena's ground as a mine from this seed.
    pub mine: Option<u64>,
    pub spawn: [Fx; 3],
    pub piles: Option<PileSetup>,
    pub memory_trial: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PileSetup {
    pub capacity: u32,
    /// Material and output position of each dispenser.
    pub dispensers: Vec<(u8, [Fx; 2])>,
    pub piles: Vec<(u8, [Fx; 2], u32)>,
    /// Where the collector stands, if there is one.
    pub collector: Option<[Fx; 2]>,
}

impl Settlement {
    /// Builds the tick-0 settlement and adds its terrain: the arena bounds,
    /// `obstacles` from the scene, and machine and defender footprints.
    pub fn from_scenario(scenario: &Scenario, obstacles: &[Disc], terrain: &mut Terrain, content: &Content) -> Result<Settlement, String> {
        scenario.validate()?;
        let character = |name: &str| -> Result<CharacterIx, String> {
            content.names.character_index(name).ok_or_else(|| format!("unknown character {name:?}"))
        };
        let center = terrain.center;
        let at = |p: scenario::Point| FxVec2::new(center.x + p[0], center.y + p[1]);
        terrain.extent = scenario.extent;
        terrain.obstacles.extend_from_slice(obstacles);
        let footprints = scenario
            .producers
            .iter()
            .map(|p| (p.position, p.radius))
            .chain(scenario.defenders.iter().map(|d| (d.position, d.radius)));
        for (position, radius) in footprints {
            if radius > Fx::ZERO {
                terrain.obstacles.push(Disc { center: at(position), radius });
            }
        }
        terrain.revision += 1;

        let items: Vec<String> = scenario.items.iter().map(|i| i.id.clone()).collect();
        let item = |id: &str| items.iter().position(|i| i == id).expect("validated item") as ItemIx;
        let depot = |id: &str| scenario.depots.iter().position(|d| d.id == id).expect("validated depot") as DepotIx;
        let stacks = |s: &[scenario::Stack]| s.iter().map(|s| (item(&s.item), s.units)).collect::<Vec<_>>();

        let mut accounts = vec![Accounting::default(); items.len()];
        let depots = scenario
            .depots
            .iter()
            .map(|d| {
                let mut stock = vec![0; items.len()];
                for s in &d.stock {
                    stock[item(&s.item) as usize] += s.units;
                    accounts[item(&s.item) as usize].initial += s.units as u64;
                }
                DepotState {
                    id: d.id.clone(),
                    position: at(d.position),
                    capacity: d.capacity,
                    stock,
                    out: vec![0; items.len()],
                    incoming: vec![0; items.len()],
                }
            })
            .collect();
        let sources = scenario
            .sources
            .iter()
            .map(|s| SourceState {
                depot: depot(&s.depot),
                item: item(&s.item),
                units: s.units,
                every: scenario::ticks(s.every),
                max_units: s.max_units,
                total: 0,
            })
            .collect();
        let links: Vec<LinkState> = scenario
            .links
            .iter()
            .map(|l| LinkState {
                id: l.id.clone(),
                from: depot(&l.from),
                to: depot(&l.to),
                item: item(&l.item),
                target: l.target,
                priority: l.priority,
                enabled: true,
            })
            .collect();
        let producers = scenario
            .producers
            .iter()
            .map(|p| ProducerState {
                id: p.id.clone(),
                position: at(p.position),
                input: depot(&p.input),
                output: depot(&p.output),
                ingredients: stacks(&p.ingredients),
                products: stacks(&p.products),
                ticks: scenario::ticks(p.seconds),
                max_cycles: p.max_cycles,
                enabled: true,
                remaining: 0,
                cycles: 0,
            })
            .collect();
        let mut agents = scenario
            .agents
            .iter()
            .map(|a| {
                let ix = character(&a.character)?;
                let def = &content.rules.characters[ix as usize];
                let wanted = if a.controlled { Brain::Player } else { Brain::Hauler };
                if def.brain != wanted {
                    return Err(format!("agent {} is {:?}, a {:?} character; it needs a {wanted:?} one", a.id, a.character, def.brain));
                }
                Ok(haulers::AgentState::new(a, ix, def, at(a.position), &scenario.links, items.len(), &item, &depot))
            })
            .collect::<Result<Vec<_>, String>>()?;
        for agent in agents.iter_mut().filter(|agent| agent.controlled) {
            agent.avatar.health = scenario.player_health;
            agent.avatar.max_health = scenario.player_health;
            agent.avatar.weapon = scenario.weapons.first().copied().unwrap_or_default();
        }

        for a in &agents {
            if nav::path(terrain, a.position, a.position, a.radius()).is_empty() {
                return Err(format!("agent {} spawns inside an obstacle", a.id));
            }
        }
        let per_second = Fx::from_int(content::TICKS_PER_SECOND);
        let waves = scenario
            .waves
            .iter()
            .map(|w| {
                let ix = character(&w.character)?;
                let def = &content.rules.characters[ix as usize];
                let health = def.health.as_ref().ok_or_else(|| format!("wave {} sends {:?}, which has no health", w.id, w.character))?;
                Ok(WaveState {
                    id: w.id.clone(),
                    character: ix,
                    position: at(w.position),
                    target: at(w.target),
                    first: (w.first * per_second).round().floor_to_int().max(1) as u64,
                    every: scenario::ticks(w.every),
                    speed: def.movement.walk / per_second,
                    hp: health.hp,
                    armor: health.armor.as_ref().map_or(0, |a| a.points),
                    damage: health.breach_damage,
                    count: w.count,
                    spawned: 0,
                    route: None,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let defenders = scenario
            .defenders
            .iter()
            .map(|d| DefenderState {
                id: d.id.clone(),
                position: at(d.position),
                radius: d.radius,
                depot: depot(&d.depot),
                ammo: item(&d.ammo),
                range: d.range,
                period: scenario::ticks(d.period),
                damage: d.damage,
                speed: d.projectile_speed,
                muzzle_height: d.muzzle_height,
                kind: d.projectile,
                enabled: true,
                ready_at: 0,
                aim: None,
                fired_at: 0,
            })
            .collect();
        let tuning = [Weapon::Sword, Weapon::Spear, Weapon::Bow, Weapon::ArcCoil, Weapon::Laser].map(|w| scenario.weapon_tuning(w)).to_vec();
        let settlement = Settlement {
            characters: content.rules.characters.clone(),
            base_health: scenario.base_health,
            health: scenario.base_health,
            player_health: scenario.player_health,
            weapons: scenario.weapons.clone(),
            tuning,
            waves,
            threats: Vec::new(),
            defenders,
            projectiles: Vec::new(),
            spent: Vec::new(),
            pinned: Vec::new(),
            combat: CombatMetrics::default(),
            events: Vec::new(),
            spawn: FxVec2::ZERO,
            spawn_height: Fx::ZERO,
            pianos: Vec::new(),
            piano_notes: Vec::new(),
            piano_serial: 0,
            piles: None,
            memory: None,
            scenario: scenario.id.clone(),
            dock_owner: vec![None; scenario.depots.len()],
            items,
            depots,
            sources,
            links,
            producers,
            agents,
            transfers: BTreeMap::new(),
            accounts,
            metrics: Metrics::default(),
            next_id: 1,
        };
        for d in &settlement.depots {
            if nav::path(terrain, d.position, d.position, Fx::from_ratio(3, 20)).is_empty() {
                return Err(format!("depot {} dock is blocked", d.id));
            }
        }
        Ok(settlement)
    }

    /// Builds the tick-0 settlement for a scene, with or without a supply
    /// scenario, and adds the scene to the terrain.
    pub fn from_scene(setup: &SceneSetup, terrain: &mut Terrain, content: &Content) -> Result<Settlement, String> {
        let center = terrain.center;
        let at = |p: [Fx; 2]| FxVec2::new(center.x + p[0], center.y + p[1]);
        let place = |d: &Disc| Disc { center: FxVec2::new(center.x + d.center.x, center.y + d.center.y), radius: d.radius };
        let obstacles: Vec<Disc> = setup.obstacles.iter().chain(&setup.doors).map(place).collect();
        terrain.destructibles = setup.destructibles.iter().map(|piece| crate::terrain::Destructible {
            asset: piece.asset, disc: place(&piece.disc), health: piece.health, max_health: piece.max_health,
            repairable: piece.repairable, repair_progress: piece.repair_progress,
        }).collect();
        let mut settlement = match &setup.scenario {
            Some(scenario) => {
                if let Some(a) = scenario.agents.iter().find(|a| a.controlled && a.character != setup.player) {
                    return Err(format!("agent {} is {:?}, but the scene's player is {:?}", a.id, a.character, setup.player));
                }
                Settlement::from_scenario(scenario, &obstacles, terrain, content)?
            }
            None => {
                terrain.extent = setup.extent;
                terrain.obstacles.extend_from_slice(&obstacles);
                Settlement::from_scenario(&Self::player_only(setup.extent, [setup.spawn[0], setup.spawn[2]], &setup.player), &[], terrain, content)?
            }
        };
        terrain.surfaces = setup
            .surfaces
            .iter()
            .map(|s| crate::terrain::Surface { min: at([s.min.x, s.min.y]), max: at([s.max.x, s.max.y]), height: s.height })
            .collect();
        terrain.platforms_only = setup.platforms_only;
        terrain.diggable = setup.diggable;
        if let Some(seed) = setup.mine {
            terrain.generate_mine(seed, at([setup.spawn[0], setup.spawn[2]]));
        }
        terrain.revision += 1;
        settlement.spawn = at([setup.spawn[0], setup.spawn[2]]);
        settlement.spawn_height = setup.spawn[1];
        settlement.pianos = setup.pianos.iter().map(|&position| at([position.x, position.y])).collect();
        if let Some(player) = settlement.player() {
            let a = &mut settlement.agents[player as usize];
            a.position = settlement.spawn;
            a.home = settlement.spawn;
            a.heading = Fx::PI;
            a.avatar.height = settlement.spawn_height;
        }
        settlement.piles = setup.piles.as_ref().map(|p| {
            let dispensers: Vec<(u8, FxVec2)> = p.dispensers.iter().map(|&(m, o)| (m, at(o))).collect();
            let piles: Vec<(u8, FxVec2, u32)> = p.piles.iter().map(|&(m, pos, units)| (m, at(pos), units)).collect();
            let mut lab = piles::PileLab::new(center, p.capacity, &dispensers, &piles);
            if setup.mine.is_some() {
                // Ore can be piled anywhere in a mine, and there's a lot of it.
                lab.limit = terrain.extent - Fx::ONE;
                lab.max_piles = piles::MAX_MINE_PILES;
            }
            if let Some(position) = p.collector {
                lab.collector = Some(piles::Collector::new(at(position)));
            }
            lab
        });
        if let Some(c) = settlement.piles.as_ref().and_then(|lab| lab.collector.as_ref()) {
            terrain.obstacles.push(Disc { center: c.position, radius: piles::COLLECTOR_RADIUS });
            terrain.revision += 1;
        }
        let doors = setup.doors.iter().map(place).collect();
        settlement.memory = Some(memory::Memory::new(setup.memory_trial, center, doors));
        Ok(settlement)
    }

    /// The scenario of a scene without a settlement: just the player.
    fn player_only(extent: Fx, spawn: [Fx; 2], player: &str) -> Scenario {
        Scenario {
            extent,
            id: "scene".into(),
            weapons: Vec::new(),
            weapon_tuning: Vec::new(),
            items: vec![scenario::Item { id: "none".into(), label: "NONE".into(), skin: "none".into(), scale: 1.0 }],
            depots: Vec::new(),
            sources: Vec::new(),
            links: Vec::new(),
            agents: vec![scenario::Agent {
                controlled: true,
                id: "player".into(),
                label: "YOU".into(),
                character: player.into(),
                position: spawn,
                capacity: 8,
                links: Vec::new(),
                cargo_anchor: [0.0, 0.65, -0.19],
                cargo_layout: scenario::CargoLayout::default(),
                equipment: None,
            }],
            producers: Vec::new(),
            defenders: Vec::new(),
            waves: Vec::new(),
            base_health: 1,
            player_health: 0,
        }
    }

    pub fn item_index(&self, id: &str) -> Option<ItemIx> {
        self.items.iter().position(|i| i == id).map(|i| i as ItemIx)
    }

    pub fn depot_index(&self, id: &str) -> Option<DepotIx> {
        self.depots.iter().position(|d| d.id == id).map(|i| i as DepotIx)
    }

    pub fn agent_index(&self, id: &str) -> Option<AgentIx> {
        self.agents.iter().position(|a| a.id == id).map(|i| i as AgentIx)
    }

    pub fn link_index(&self, id: &str) -> Option<LinkIx> {
        self.links.iter().position(|l| l.id == id).map(|i| i as LinkIx)
    }

    pub fn producer_index(&self, id: &str) -> Option<u16> {
        self.producers.iter().position(|p| p.id == id).map(|i| i as u16)
    }

    pub fn depot(&self, id: &str) -> &DepotState {
        &self.depots[self.depot_index(id).expect("known depot") as usize]
    }

    pub fn agent(&self, id: &str) -> &haulers::AgentState {
        &self.agents[self.agent_index(id).expect("known agent") as usize]
    }

    /// Units of `item` at `depot`, by name.
    pub fn stock(&self, depot: &str, item: &str) -> u32 {
        self.depot(depot).stock[self.item_index(item).expect("known item") as usize]
    }

    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Per item: everything supplied equals everything accounted for.
    pub fn balances(&self) -> BTreeMap<String, Balance> {
        self.items
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let b = &self.accounts[i];
                let stored = self.depots.iter().map(|d| d.stock[i] as u64).sum();
                let carried = self.agents.iter().map(|a| a.cargo[i] as u64).sum();
                let in_transit = self.transfers.values().filter(|t| t.item as usize == i).count() as u64;
                let equipped = self.agents.iter().filter(|a| a.equipped_item() == Some(i as ItemIx)).count() as u64;
                let supplied = b.initial + b.generated + b.produced;
                let balance = Balance {
                    supplied,
                    stored,
                    carried,
                    in_transit,
                    equipped,
                    consumed: b.consumed,
                    spent: b.spent,
                    conserved: supplied == stored + carried + in_transit + equipped + b.consumed + b.spent,
                };
                (id.clone(), balance)
            })
            .collect()
    }

    /// Checks conservation and recomputes every reservation from scratch.
    /// For tests and debug builds.
    pub fn validate(&self) -> Result<(), String> {
        if let Some((item, _)) = self.balances().into_iter().find(|(_, b)| !b.conserved) {
            return Err(format!("material accounting mismatch for {item}"));
        }
        let items = self.items.len();
        let mut out = vec![vec![0u32; items]; self.depots.len()];
        let mut incoming = vec![vec![0u32; items]; self.depots.len()];
        for a in &self.agents {
            if let Some(job) = &a.assignment {
                let link = &self.links[job.link as usize];
                out[link.from as usize][link.item as usize] += job.units - job.loaded;
                incoming[link.to as usize][link.item as usize] += job.units - job.unloaded;
            }
            if a.phase == Phase::ToEquip {
                let e = a.equipment.as_ref().expect("equipping agents have equipment");
                out[e.depot as usize][e.item as usize] += 1;
            }
        }
        // A hauled unit counts as incoming from assignment until arrival: first
        // as not yet unloaded, then as flying.
        for t in self.transfers.values() {
            if let Endpoint::Depot(d) = t.to {
                incoming[d as usize][t.item as usize] += 1;
            }
        }
        for p in &self.producers {
            if p.remaining > 0 {
                for &(item, units) in &p.products {
                    incoming[p.output as usize][item as usize] += units;
                }
            }
        }
        for a in &self.agents {
            if let Some(job) = &a.avatar.job {
                if job.deposit {
                    incoming[job.depot as usize][job.item as usize] += job.remaining;
                } else {
                    out[job.depot as usize][job.item as usize] += job.remaining;
                }
            }
        }
        for (i, d) in self.depots.iter().enumerate() {
            if d.out != out[i] {
                return Err(format!("depot {} outgoing reservations drifted: {:?} vs {:?}", d.id, d.out, out[i]));
            }
            if d.incoming != incoming[i] {
                return Err(format!("depot {} incoming reservations drifted: {:?} vs {:?}", d.id, d.incoming, incoming[i]));
            }
            if d.total() + d.incoming.iter().sum::<u32>() > d.capacity {
                return Err(format!("depot {} overbooked", d.id));
            }
            if (0..items).any(|item| d.out[item] > d.stock[item]) {
                return Err(format!("depot {} stock overbooked", d.id));
            }
        }
        for a in &self.agents {
            let cargo: u32 = a.cargo.iter().sum();
            if cargo + a.pending_in > a.cargo_capacity {
                return Err(format!("agent {} capacity exceeded", a.id));
            }
        }
        Ok(())
    }
}

impl Settlement {
    /// An agent's character definition.
    pub fn character_of(&self, agent: usize) -> Option<&CharacterDef> {
        let ix = self.agents.get(agent)?.character;
        self.characters.get(ix as usize)
    }

    /// Restores the character rules after loading, checking every saved
    /// reference against them.
    pub fn set_characters(&mut self, characters: &[CharacterDef]) -> Result<(), String> {
        let known = |ix: CharacterIx| (ix as usize) < characters.len();
        if let Some(a) = self.agents.iter().find(|a| !known(a.character)) {
            return Err(format!("agent {} has unknown character {}", a.id, a.character));
        }
        if let Some(w) = self.waves.iter().find(|w| !known(w.character)) {
            return Err(format!("wave {} has unknown character {}", w.id, w.character));
        }
        self.characters = characters.to_vec();
        Ok(())
    }

    /// A hash of what the settlement does: where agents, threats and
    /// projectiles are, what they carry and how healthy they are, stock and
    /// metrics. It leaves out definition references and tuning copied into
    /// state, so moving definitions around keeps it while `hash_into` changes.
    pub fn behaviour_digest(&self) -> u64 {
        let agents: Vec<_> = self
            .agents
            .iter()
            .map(|a| {
                (&a.id, a.position, a.heading, a.travelled, a.phase, &a.route, &a.cargo, &a.trailer)
            })
            .collect();
        let avatars: Vec<_> = self
            .agents
            .iter()
            .map(|a| (a.avatar.velocity, a.avatar.height, a.avatar.health, &a.avatar.swing, &a.avatar.jump))
            .collect();
        let threats: Vec<_> = self
            .threats
            .iter()
            .map(|t| (t.id, t.wave, t.position, t.heading, t.hp, t.armor, t.stagger_until, t.knockback))
            .collect();
        let spawned: Vec<_> = self.waves.iter().map(|w| w.spawned).collect();
        let observed = (
            agents,
            avatars,
            threats,
            spawned,
            &self.depots,
            &self.projectiles,
            &self.pinned,
            &self.transfers,
            &self.metrics,
            &self.combat,
            self.health,
        );
        let bytes = postcard::to_allocvec(&observed).expect("behaviour encodes");
        bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, &byte| (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3))
    }
}

impl StateHash for Settlement {
    fn hash_into(&self, h: &mut StateHasher) {
        // Settlements are small; hashing their canonical encoding covers every field.
        let bytes = postcard::to_allocvec(self).expect("settlement encodes");
        h.write_u64(bytes.len() as u64);
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            h.write_u64(u64::from_le_bytes(word));
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod combat_tests;
#[cfg(test)]
mod scene_tests;
