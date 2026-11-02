//! Scenarios: the starting setup of a settlement scene (depots, haul links,
//! haulers, machines, defenders, enemy waves, building kinds and the
//! buildings that start built, sprites). A host turns a scenario into
//! the world at tick 0; from then on the state lives in saves.
//!
//! The file format is the original game's, so existing scene files load
//! unchanged. Simulation values parse into `Fx`; view-only values (labels,
//! skins, cargo layouts and anchors) stay `f32` and never reach the
//! simulation. Agents and waves name a character instead of a skin; the
//! character's `view.skin` draws them. Positions are arena coordinates:
//! meters from the arena center.
//! Weapon tuning in files counts 60 Hz ticks, as the original game did; use
//! [`WeaponTuning::cooldown`] and friends for simulation ticks.

use std::collections::BTreeSet;

use fx::Fx;
use serde::{Deserialize, Serialize};

use crate::TICKS_PER_SECOND;

pub type Point = [Fx; 2];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub skin: String,
    pub scale: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stack {
    pub item: String,
    pub units: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CargoLayout {
    pub columns: u32,
    pub rows: u32,
    pub spacing: [f32; 3],
    pub scale: f32,
    pub roll: f32,
}

impl Default for CargoLayout {
    fn default() -> Self {
        CargoLayout { columns: 2, rows: 2, spacing: [0.10, 0.08, 0.08], scale: 0.65, roll: std::f32::consts::FRAC_PI_2 }
    }
}

impl CargoLayout {
    /// The layout depots use unless they author one.
    pub fn depot() -> CargoLayout {
        CargoLayout { columns: 4, rows: 3, spacing: [0.39, 0.12, 0.15], scale: 1.0, roll: 0.0 }
    }

    fn valid(&self) -> bool {
        (1..=16).contains(&self.columns)
            && (1..=16).contains(&self.rows)
            && self.spacing.iter().all(|v| v.is_finite() && *v > 0.0 && *v <= 2.0)
            && self.scale.is_finite()
            && self.scale > 0.0
            && self.scale <= 2.0
            && self.roll.is_finite()
    }

    /// Local offset of the `n`th unit.
    pub fn offset(&self, n: u32) -> [f32; 3] {
        [
            ((n % self.columns) as f32 - (self.columns - 1) as f32 * 0.5) * self.spacing[0],
            (n / (self.columns * self.rows)) as f32 * self.spacing[1],
            (((n / self.columns) % self.rows) as f32 - (self.rows - 1) as f32 * 0.5) * self.spacing[2],
        ]
    }
}

fn stock_anchor() -> [f32; 3] {
    [0.0, 0.08, -0.6]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Depot {
    pub id: String,
    pub label: String,
    pub position: Point,
    pub capacity: u32,
    pub stock: Vec<Stack>,
    /// The one item it takes, if it takes only one: the Sacred Tree's wood
    /// and stone piles. Its stock, sources, links, machines and defenders
    /// must use that item too.
    #[serde(default)]
    pub only: Option<String>,
    #[serde(default = "stock_anchor")]
    pub stock_anchor: [f32; 3],
    #[serde(default = "CargoLayout::depot")]
    pub stock_layout: CargoLayout,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub depot: String,
    pub item: String,
    pub units: u32,
    pub every: Fx,
    pub max_units: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HaulLink {
    pub id: String,
    pub from: String,
    pub to: String,
    pub item: String,
    pub target: u32,
    pub priority: u32,
}

fn trailer_radius() -> Fx {
    Fx::from_ratio(7, 10)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Equipment {
    pub depot: String,
    pub item: String,
    pub skin: String,
    pub capacity: u32,
    pub speed_factor: Fx,
    #[serde(default = "trailer_radius")]
    pub radius: Fx,
    /// The trailer's offset from the body; `offset[2]` is the hitch length.
    pub offset: [Fx; 3],
    pub cargo_anchor: [f32; 3],
    #[serde(default)]
    pub cargo_layout: CargoLayout,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    #[serde(default)]
    pub controlled: bool,
    pub id: String,
    pub label: String,
    /// The character it is, by short name: a `Player` character for a
    /// `controlled` avatar, a `Hauler` otherwise. Its speed, radius and
    /// transfer time come from there.
    pub character: String,
    pub position: Point,
    /// Units it carries. Set here, not on the character: haulers of one
    /// kind carry different loads by job.
    pub capacity: u32,
    /// Carries one item at a time, as a bundle: the Sacred Tree's elder.
    /// Without it a cargo holds any mix.
    #[serde(default)]
    pub one_kind: bool,
    pub links: Vec<String>,
    pub cargo_anchor: [f32; 3],
    #[serde(default)]
    pub cargo_layout: CargoLayout,
    pub equipment: Option<Equipment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Producer {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub radius: Fx,
    pub id: String,
    pub label: String,
    pub position: Point,
    pub skin: String,
    pub input: String,
    pub output: String,
    pub ingredients: Vec<Stack>,
    pub products: Vec<Stack>,
    pub seconds: Fx,
    pub max_cycles: Option<u32>,
}

fn output_distance() -> Fx {
    Fx::from_ratio(6, 5)
}

fn building_health() -> u32 {
    10
}

/// A kind of building players put up from parts: a gatherer that turns out
/// one item while someone occupies it, or a ballista that shoots the
/// nearest enemy while someone occupies it and it has ammo (`defence`).
/// Every rate and recipe is here, not in code.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildingKind {
    pub id: String,
    pub label: String,
    pub skin: String,
    /// Footprint radius in meters.
    pub radius: Fx,
    /// The item a gatherer produces, one unit at a time. A ballista has
    /// none, and none of the production fields after it.
    #[serde(default)]
    pub product: Option<String>,
    /// Seconds per unit while a sprite occupies it.
    #[serde(default)]
    pub period: Fx,
    /// Units its output holds; production pauses when it is full.
    #[serde(default)]
    pub cap: u32,
    /// The output's cap while an elder possesses it.
    #[serde(default)]
    pub possessed_cap: u32,
    /// A possessing elder divides the period by this.
    #[serde(default)]
    pub possessed_multiplier: Fx,
    #[serde(default)]
    pub burst: Burst,
    /// A ballista's shooting and its ammo store, which takes the place of a
    /// gatherer's output pile. A building kind has either this or a product.
    #[serde(default)]
    pub defence: Option<Defence>,
    /// Meters from the building's center to its output pile (a ballista's
    /// ammo pile), in front.
    #[serde(default = "output_distance")]
    pub output_distance: Fx,
    /// Hits an enemy's swings take to break it (B8): a built building goes
    /// back to a site, and a site loses what was delivered to it.
    #[serde(default = "building_health")]
    pub health: u32,
    /// Installed one after another; installing the last one finishes it.
    pub parts: Vec<Part>,
}

/// What a possessing elder's Burst does: `units` at once into the output,
/// then `cooldown` seconds before the next.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Burst {
    pub units: u32,
    pub cooldown: Fx,
}

/// A ballista building's shooting (`BuildingKind::defence`), as a
/// `Defender`'s: the nearest enemy in range, one unit of ammo a shot. Its
/// ammo store takes `ammo` only, up to `ammo_cap`, and is only delivered
/// to.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defence {
    /// The item it shoots, by id.
    pub ammo: String,
    pub ammo_cap: u32,
    pub range: Fx,
    /// Seconds between shots.
    pub period: Fx,
    pub damage: u32,
    pub projectile_speed: Fx,
    #[serde(default = "muzzle_height")]
    pub muzzle_height: Fx,
    #[serde(default)]
    pub projectile: ProjectileKind,
    /// What a possessing elder's Burst key throws instead
    /// (`SACRED_TREE_BALLISTA.md`, B4); `None` for no fireball.
    #[serde(default)]
    pub fireball: Option<Fireball>,
}

/// A possessed ballista's fireball (B4): it flies to the reticle at
/// `speed` meters a second, hurts every enemy within `radius` of where it
/// lands by `damage`, and sets the ground there alight: a fire of radius
/// `fire_radius` for `fire_seconds`. An enemy in the fire burns, taking
/// `burn_damage` every `burn_period` seconds until `burn_linger` seconds
/// after it leaves. The elder waits `cooldown` seconds before the next. It
/// costs no ammo.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fireball {
    pub cooldown: Fx,
    pub speed: Fx,
    pub damage: u32,
    pub radius: Fx,
    pub fire_radius: Fx,
    pub fire_seconds: Fx,
    pub burn_damage: u32,
    pub burn_period: Fx,
    pub burn_linger: Fx,
}

/// One part of a building, and the materials it takes to install.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub id: String,
    pub label: String,
    pub materials: Vec<Stack>,
}

/// A building that stands, built, when the scenario starts.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Building {
    /// Its kind, by id.
    pub kind: String,
    pub position: Point,
}

/// A sprite: a small worker that staffs buildings. It carries nothing, so it
/// has none of a hauler's cargo, links or equipment, and starts idle.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sprite {
    pub id: String,
    pub label: String,
    /// The character it is, by short name: a `Sprite` character.
    pub character: String,
    pub position: Point,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProjectileKind {
    #[default]
    Arrow,
    Bolt,
}

fn muzzle_height() -> Fx {
    Fx::from_ratio(8, 5)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defender {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub radius: Fx,
    pub id: String,
    pub position: Point,
    pub skin: String,
    pub depot: String,
    pub ammo: String,
    pub range: Fx,
    /// Seconds between shots.
    pub period: Fx,
    pub damage: u32,
    pub projectile_speed: Fx,
    #[serde(default = "muzzle_height")]
    pub muzzle_height: Fx,
    #[serde(default)]
    pub projectile: ProjectileKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wave {
    pub id: String,
    pub position: Point,
    pub target: Point,
    /// The enemy this wave sends, by short name (`ghoul` for
    /// `character/ghoul`): its speed, health, armor and breach damage.
    pub character: String,
    /// Seconds until the first enemy.
    pub first: Fx,
    pub every: Fx,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Weapon {
    #[default]
    Sword,
    Spear,
    Bow,
    ArcCoil,
    Laser,
}

/// Simulation-side weapon rules. Tick counts are 60 Hz, as authored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponTuning {
    pub weapon: Weapon,
    pub cooldown_ticks: u64,
    pub reach: Fx,
    pub cone: Fx,
    pub damage: u32,
    pub knockback: Fx,
    pub stagger_ticks: u64,
    pub projectile_speed: Fx,
    pub critical_one_in: u64,
}

/// Converts the files' 60 Hz tick counts to simulation ticks, rounding up.
pub const fn ticks_from_60hz(ticks: u64) -> u64 {
    (ticks * TICKS_PER_SECOND as u64).div_ceil(60)
}

impl WeaponTuning {
    pub fn defaults(weapon: Weapon) -> WeaponTuning {
        let f = |v: i64, d: i64| Fx::from_ratio(v, d);
        let (cooldown_ticks, reach, cone, damage, knockback, stagger_ticks, projectile_speed, critical_one_in) = match weapon {
            Weapon::Sword => (38, f(6, 5), f(35, 100), 2, f(11, 2), 15, Fx::ZERO, 4),
            Weapon::Spear => (42, f(12, 5), f(4, 5), 2, Fx::from_int(8), 24, Fx::ZERO, 0),
            Weapon::Bow => (28, Fx::from_int(9), f(91, 100), 1, Fx::ZERO, 0, Fx::from_int(28), 0),
            Weapon::ArcCoil => (65, Fx::from_int(4), f(55, 100), 1, Fx::ZERO, 45, Fx::ZERO, 0),
            Weapon::Laser => (24, Fx::from_int(12), Fx::from_int(99).div_int(100), 1, Fx::ZERO, 0, Fx::ZERO, 0),
        };
        WeaponTuning { weapon, cooldown_ticks, reach, cone, damage, knockback, stagger_ticks, projectile_speed, critical_one_in }
    }

    /// Cooldown in simulation ticks.
    pub fn cooldown(&self) -> u64 {
        ticks_from_60hz(self.cooldown_ticks)
    }

    /// Stagger in simulation ticks.
    pub fn stagger(&self) -> u64 {
        ticks_from_60hz(self.stagger_ticks)
    }

    fn valid(&self) -> bool {
        (1..=600).contains(&self.cooldown_ticks)
            && within(self.reach, f(1, 10), Fx::from_int(64))
            && within(self.cone, -Fx::ONE, Fx::ONE)
            && (1..=100).contains(&self.damage)
            && within(self.knockback, Fx::ZERO, Fx::from_int(40))
            && self.stagger_ticks <= 600
            && within(self.projectile_speed, Fx::ZERO, Fx::from_int(100))
            && (self.weapon != Weapon::Bow || self.projectile_speed > Fx::ZERO)
            && self.critical_one_in <= 1000
    }
}

fn default_extent() -> Fx {
    Fx::from_int(8)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    #[serde(default = "default_extent")]
    pub extent: Fx,
    pub id: String,
    #[serde(default)]
    pub weapons: Vec<Weapon>,
    #[serde(default)]
    pub weapon_tuning: Vec<WeaponTuning>,
    pub items: Vec<Item>,
    pub depots: Vec<Depot>,
    pub sources: Vec<Source>,
    pub links: Vec<HaulLink>,
    pub agents: Vec<Agent>,
    pub producers: Vec<Producer>,
    pub defenders: Vec<Defender>,
    pub waves: Vec<Wave>,
    pub base_health: u32,
    /// Hit points for player avatars; zero disables player combat.
    #[serde(default)]
    pub player_health: u32,
    #[serde(default)]
    pub building_kinds: Vec<BuildingKind>,
    /// The buildings standing, built, at the start.
    #[serde(default)]
    pub buildings: Vec<Building>,
    #[serde(default)]
    pub sprites: Vec<Sprite>,
}

fn f(n: i64, d: i64) -> Fx {
    Fx::from_ratio(n, d)
}

fn within(v: Fx, min: Fx, max: Fx) -> bool {
    v >= min && v <= max
}

pub(crate) fn ident(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn ids<'a>(values: impl Iterator<Item = &'a str>) -> Result<BTreeSet<&'a str>, String> {
    let mut ids = BTreeSet::new();
    for id in values {
        if !ident(id) || !ids.insert(id) {
            return Err(format!("invalid or duplicate identifier {id}"));
        }
    }
    Ok(ids)
}

impl Scenario {
    pub fn parse(text: &str) -> Result<Scenario, String> {
        let scenario = Scenario::from_ron(text)?;
        scenario.validate()?;
        Ok(scenario)
    }

    /// Reads a scenario file without validating it. Optional values may be
    /// written bare (`radius: 0.28`) or as `Some(...)`.
    pub fn from_ron(text: &str) -> Result<Scenario, String> {
        let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
        options.from_str(text).map_err(|e| e.to_string())
    }

    pub fn weapon_tuning(&self, weapon: Weapon) -> WeaponTuning {
        self.weapon_tuning.iter().find(|t| t.weapon == weapon).cloned().unwrap_or_else(|| WeaponTuning::defaults(weapon))
    }

    /// The same checks as the original game.
    pub fn validate(&self) -> Result<(), String> {
        // Up to 24 in the original game; mines are 128.
        if !within(self.extent, Fx::from_int(4), Fx::from_int(128)) {
            return Err("invalid scenario extent".into());
        }
        let point = |p: Point| p.iter().all(|v| v.abs() <= self.extent);
        let unique_weapons = self.weapons.iter().collect::<BTreeSet<_>>().len() == self.weapons.len();
        let unique_tuning = self.weapon_tuning.iter().map(|t| t.weapon).collect::<BTreeSet<_>>().len() == self.weapon_tuning.len();
        if !ident(&self.id)
            || self.items.is_empty()
            || self.items.len() > 32
            || self.depots.len() > 64
            || self.agents.len() + self.sprites.len() > 32
            || self.building_kinds.len() > 16
            || self.buildings.len() > 64
            || self.links.len() > 128
            || self.producers.len() > 32
            || self.defenders.len() > 16
            || self.waves.len() > 8
            || self.sources.len() > 32
            || self.base_health == 0
            || self.player_health > 100
            || self.weapons.len() > 8
            || !unique_weapons
            || !unique_tuning
            || self.weapon_tuning.iter().any(|t| !t.valid() || !self.weapons.contains(&t.weapon))
        {
            return Err("scenario counts exceed supported limits".into());
        }
        let items = ids(self.items.iter().map(|x| x.id.as_str()))?;
        let depots = ids(self.depots.iter().map(|x| x.id.as_str()))?;
        let links = ids(self.links.iter().map(|x| x.id.as_str()))?;
        // Sprites are agents too, so they share the agents' ids.
        ids(self.agents.iter().map(|x| x.id.as_str()).chain(self.sprites.iter().map(|x| x.id.as_str())))?;
        let kinds = ids(self.building_kinds.iter().map(|x| x.id.as_str()))?;
        ids(self.producers.iter().map(|x| x.id.as_str()))?;
        ids(self.defenders.iter().map(|x| x.id.as_str()))?;
        ids(self.waves.iter().map(|x| x.id.as_str()))?;
        let stack = |s: &Stack| items.contains(s.item.as_str()) && (1..=4096).contains(&s.units);
        let stacks = |ss: &[Stack]| {
            !ss.is_empty() && ss.len() <= 16 && ss.iter().all(stack) && ss.iter().map(|s| &s.item).collect::<BTreeSet<_>>().len() == ss.len()
        };
        let anchor = |a: &[f32; 3]| a.iter().all(|v| v.is_finite() && v.abs() <= 4.0);
        for i in &self.items {
            if !ident(&i.skin) || !(0.02..=2.0).contains(&i.scale) || i.label.is_empty() {
                return Err("invalid item appearance".into());
            }
        }
        for d in &self.depots {
            if !point(d.position)
                || !d.stock_layout.valid()
                || !anchor(&d.stock_anchor)
                || !(1..=4096).contains(&d.capacity)
                || d.stock.iter().any(|s| !stack(s))
                || d.stock.iter().map(|s| s.units as u64).sum::<u64>() > d.capacity as u64
                || d.stock.iter().map(|s| &s.item).collect::<BTreeSet<_>>().len() != d.stock.len()
            {
                return Err(format!("invalid depot {}", d.id));
            }
        }
        for s in &self.sources {
            if !depots.contains(s.depot.as_str())
                || !items.contains(s.item.as_str())
                || s.units == 0
                || s.units > 4096
                || !within(s.every, f(1, 10), Fx::from_int(3600))
            {
                return Err("invalid source".into());
            }
        }
        for l in &self.links {
            if !depots.contains(l.from.as_str())
                || !depots.contains(l.to.as_str())
                || l.from == l.to
                || !items.contains(l.item.as_str())
                || l.target == 0
                || l.target > 4096
            {
                return Err(format!("invalid haul link {}", l.id));
            }
        }
        for a in &self.agents {
            if !a.cargo_layout.valid()
                || !point(a.position)
                || !ident(&a.character)
                || !(1..=64).contains(&a.capacity)
                || (!a.controlled && a.links.is_empty())
                || (a.controlled && (!a.links.is_empty() || a.equipment.is_some()))
                || a.links.iter().any(|l| !links.contains(l.as_str()))
                || !anchor(&a.cargo_anchor)
            {
                return Err(format!("invalid agent {}", a.id));
            }
            if let Some(e) = &a.equipment {
                if !within(e.radius, f(1, 10), Fx::from_int(2))
                    || !e.cargo_layout.valid()
                    || !depots.contains(e.depot.as_str())
                    || !items.contains(e.item.as_str())
                    || !ident(&e.skin)
                    || !(a.capacity..=64).contains(&e.capacity)
                    || !within(e.speed_factor, f(1, 5), Fx::from_int(2))
                    || e.offset.iter().any(|v| v.abs() > Fx::from_int(4))
                    || !anchor(&e.cargo_anchor)
                {
                    return Err("invalid equipment upgrade".into());
                }
            }
        }
        for p in &self.producers {
            if p.description.len() > 512 || !within(p.radius, Fx::ZERO, Fx::from_int(3)) {
                return Err("invalid machine footprint".into());
            }
            if !point(p.position)
                || !ident(&p.skin)
                || !depots.contains(p.input.as_str())
                || !depots.contains(p.output.as_str())
                || !stacks(&p.ingredients)
                || !stacks(&p.products)
                || !within(p.seconds, f(1, 10), Fx::from_int(3600))
                || p.max_cycles == Some(0)
            {
                return Err(format!("invalid producer {}", p.id));
            }
        }
        for k in &self.building_kinds {
            let parts = ids(k.parts.iter().map(|p| p.id.as_str())).is_ok();
            let shared = k.label.is_empty()
                || k.label.len() > 128
                || !ident(&k.skin)
                || !within(k.radius, f(1, 10), Fx::from_int(3))
                || !within(k.output_distance, k.radius, k.radius + Fx::from_int(4))
                || !(1..=1000).contains(&k.health)
                || k.parts.is_empty()
                || k.parts.len() > 8
                || !parts
                || k.parts.iter().any(|p| p.label.is_empty() || p.label.len() > 128 || !stacks(&p.materials));
            // A gatherer produces; a ballista shoots and has no production.
            let role = match (&k.product, &k.defence) {
                (Some(product), None) => {
                    items.contains(product.as_str())
                        && within(k.period, f(1, 10), Fx::from_int(3600))
                        && (1..=4096).contains(&k.cap)
                        && (k.cap..=4096).contains(&k.possessed_cap)
                        && within(k.possessed_multiplier, Fx::ONE, Fx::from_int(16))
                        && (1..=k.possessed_cap).contains(&k.burst.units)
                        && within(k.burst.cooldown, f(1, 10), Fx::from_int(3600))
                }
                (None, Some(d)) => {
                    k.period == Fx::ZERO
                        && k.cap == 0
                        && k.possessed_cap == 0
                        && k.possessed_multiplier == Fx::ZERO
                        && k.burst.units == 0
                        && k.burst.cooldown == Fx::ZERO
                        && items.contains(d.ammo.as_str())
                        && (1..=4096).contains(&d.ammo_cap)
                        && within(d.range, f(1, 10), Fx::from_int(32))
                        && within(d.period, f(1, 10), Fx::from_int(3600))
                        && (1..=1000).contains(&d.damage)
                        && within(d.projectile_speed, Fx::HALF, Fx::from_int(100))
                        && within(d.muzzle_height, Fx::ZERO, Fx::from_int(5))
                        && d.fireball.as_ref().is_none_or(|b| {
                            within(b.cooldown, f(1, 10), Fx::from_int(3600))
                                && within(b.speed, Fx::HALF, Fx::from_int(100))
                                && (0..=1000).contains(&b.damage)
                                && within(b.radius, Fx::ZERO, Fx::from_int(8))
                                && within(b.fire_radius, f(1, 10), Fx::from_int(8))
                                && within(b.fire_seconds, f(1, 10), Fx::from_int(600))
                                && (1..=1000).contains(&b.burn_damage)
                                && within(b.burn_period, f(1, 10), Fx::from_int(60))
                                && within(b.burn_linger, Fx::ZERO, Fx::from_int(600))
                        })
                }
                _ => false,
            };
            if shared || !role {
                return Err(format!("invalid building kind {}", k.id));
            }
        }
        for b in &self.buildings {
            if !kinds.contains(b.kind.as_str()) || !point(b.position) {
                return Err(format!("invalid building of kind {}", b.kind));
            }
        }
        for s in &self.sprites {
            if !point(s.position) || !ident(&s.character) || s.label.is_empty() || s.label.len() > 128 {
                return Err(format!("invalid sprite {}", s.id));
            }
        }
        for d in &self.defenders {
            if d.description.len() > 512 || d.label.len() > 128 {
                return Err("invalid defender description".into());
            }
            if !within(d.radius, Fx::ZERO, Fx::from_int(3)) {
                return Err("invalid defender footprint".into());
            }
            if !point(d.position)
                || !ident(&d.skin)
                || !depots.contains(d.depot.as_str())
                || !items.contains(d.ammo.as_str())
                || !within(d.range, f(1, 10), Fx::from_int(16))
                || !within(d.period, f(1, 10), Fx::from_int(60))
                || d.damage == 0
                || !within(d.projectile_speed, f(1, 10), Fx::from_int(40))
                || !within(d.muzzle_height, f(1, 20), Fx::from_int(4))
            {
                return Err("invalid defender".into());
            }
        }
        // A depot that takes one item takes it from everything that fills
        // or draws on it.
        for d in &self.depots {
            let Some(only) = &d.only else { continue };
            let bad = |depot: &str, item: &str| depot == d.id && item != only;
            if !items.contains(only.as_str())
                || d.stock.iter().any(|s| s.item != *only)
                || self.sources.iter().any(|s| bad(&s.depot, &s.item))
                || self.links.iter().any(|l| bad(&l.from, &l.item) || bad(&l.to, &l.item))
                || self.producers.iter().any(|p| p.ingredients.iter().any(|s| bad(&p.input, &s.item)) || p.products.iter().any(|s| bad(&p.output, &s.item)))
                || self.defenders.iter().any(|x| bad(&x.depot, &x.ammo))
                || self.agents.iter().filter_map(|a| a.equipment.as_ref()).any(|e| bad(&e.depot, &e.item))
            {
                return Err(format!("depot {} takes only {only}, but something else goes there", d.id));
            }
        }
        for w in &self.waves {
            if !point(w.position)
                || !point(w.target)
                || !within(w.first, Fx::ZERO, Fx::from_int(3600))
                || !within(w.every, Fx::HALF, Fx::from_int(3600))
                || !ident(&w.character)
                || w.count > 256
            {
                return Err("invalid wave".into());
            }
        }
        Ok(())
    }
}

/// Seconds to simulation ticks, rounding up, at least one. Decimal seconds
/// aren't exact in fixed point (0.35 s is a hair over 21 ticks at 60 Hz), so
/// anything within a millionth of a tick of a whole tick counts as that tick.
pub fn ticks(seconds: Fx) -> u64 {
    const SLACK: Fx = Fx::from_bits(1 << 12);
    (seconds.mul_int(TICKS_PER_SECOND as i64) - SLACK).ceil().floor_to_int().max(1) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOREST: &str = include_str!("../../app/assets/levels/forest_supply.ron");
    const ARENA: &str = include_str!("../../app/assets/levels/combat_arena_supply.ron");
    const SACRED_TREE: &str = include_str!("../../app/assets/levels/sacred_tree_supply.ron");

    #[test]
    fn the_original_scene_files_load() {
        let forest = Scenario::parse(FOREST).unwrap();
        assert_eq!(forest.id, "brasswood");
        assert_eq!(forest.depots[3].position[0], "-4.485".parse().unwrap());
        let arena = Scenario::parse(ARENA).unwrap();
        let sword = arena.weapon_tuning(Weapon::Sword);
        assert_eq!(sword.cooldown(), ticks_from_60hz(38));
        assert_eq!(arena.weapon_tuning(Weapon::Bow).cooldown(), ticks_from_60hz(28));
    }

    #[test]
    fn validation_rejects_unknown_references() {
        let mut scenario = Scenario::parse(FOREST).unwrap();
        scenario.links[0].item = "unobtainium".into();
        assert!(scenario.validate().is_err());
        let mut scenario = Scenario::parse(FOREST).unwrap();
        scenario.depots[0].stock.push(Stack { item: "lumber".into(), units: 1 });
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn the_sacred_tree_supply_loads_its_tunables() {
        let tree = Scenario::parse(SACRED_TREE).unwrap();
        assert_eq!((tree.agents.len(), tree.sprites.len(), tree.buildings.len()), (1, 4, 1));
        assert!(tree.depots.iter().all(|d| d.stock.is_empty()));
        let kind = |id: &str| tree.building_kinds.iter().find(|k| k.id == id).unwrap();
        let wood = kind("wood_gatherer");
        assert_eq!((wood.product.as_deref(), wood.period, wood.cap, wood.possessed_cap), (Some("wood"), Fx::from_int(5), 10, 20));
        assert!(wood.defence.is_none());
        assert_eq!((wood.possessed_multiplier, wood.burst.units, wood.burst.cooldown), (Fx::from_int(4), 5, Fx::from_int(30)));
        assert_eq!(wood.output_distance, "1.2".parse().unwrap(), "the default");
        assert_eq!(wood.health, 10);
        fn needs(k: &BuildingKind) -> Vec<Vec<(&str, u32)>> {
            k.parts.iter().map(|p| p.materials.iter().map(|s| (s.item.as_str(), s.units)).collect()).collect()
        }
        assert_eq!(needs(wood), [vec![("wood", 2)], vec![("wood", 2), ("stone", 2)]]);
        let stone = kind("stone_gatherer");
        assert_eq!((stone.product.as_deref(), stone.period), (Some("stone"), Fx::from_int(8)));
        assert_eq!(needs(stone), [vec![("wood", 2)], vec![("wood", 2)]]);
        // The ballista shoots as the Woodland Ballista does, from a store of
        // stone, and produces nothing.
        let ballista = kind("ballista");
        let defence = ballista.defence.as_ref().unwrap();
        assert_eq!((ballista.product.as_deref(), ballista.cap, ballista.burst.units), (None, 0, 0));
        assert_eq!((defence.ammo.as_str(), defence.ammo_cap, defence.damage), ("stone", 10, 1));
        assert_eq!((defence.range, defence.period, defence.projectile_speed), ("3.8".parse().unwrap(), "0.7".parse().unwrap(), Fx::from_int(8)));
        assert_eq!(needs(ballista), [vec![("wood", 2)], vec![("wood", 2), ("stone", 2)]]);
        // And its fireball (B4): every 20 s, a fire of radius 1.5 m for 6 s
        // that burns 1 a second until 3 s after an enemy leaves it.
        let fireball = defence.fireball.as_ref().unwrap();
        assert_eq!((fireball.cooldown, fireball.damage, fireball.fire_radius, fireball.fire_seconds), (Fx::from_int(20), 2, "1.5".parse().unwrap(), Fx::from_int(6)));
        assert_eq!((fireball.burn_damage, fireball.burn_period, fireball.burn_linger), (1, Fx::ONE, Fx::from_int(3)));
        assert_eq!(tree.buildings[0].kind, "wood_gatherer");
        // The other scenes have none of it.
        let forest = Scenario::parse(FOREST).unwrap();
        assert!(forest.building_kinds.is_empty() && forest.buildings.is_empty() && forest.sprites.is_empty());
    }

    #[test]
    fn validation_rejects_bad_buildings() {
        let invalid = |change: fn(&mut Scenario)| {
            let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
            change(&mut scenario);
            scenario.validate().is_err()
        };
        assert!(invalid(|s| s.building_kinds[0].product = Some("unobtainium".into())));
        assert!(invalid(|s| s.building_kinds[0].product = None));
        assert!(invalid(|s| s.building_kinds[0].defence = s.building_kinds[2].defence.clone()));
        assert!(invalid(|s| s.building_kinds[2].defence = None));
        assert!(invalid(|s| s.building_kinds[2].cap = 10));
        assert!(invalid(|s| s.building_kinds[2].defence.as_mut().unwrap().ammo = "unobtainium".into()));
        assert!(invalid(|s| s.building_kinds[2].defence.as_mut().unwrap().ammo_cap = 0));
        assert!(invalid(|s| s.building_kinds[2].defence.as_mut().unwrap().period = Fx::ZERO));
        assert!(invalid(|s| s.building_kinds[2].defence.as_mut().unwrap().fireball.as_mut().unwrap().burn_period = Fx::ZERO));
        assert!(invalid(|s| s.building_kinds[2].defence.as_mut().unwrap().fireball.as_mut().unwrap().cooldown = Fx::ZERO));
        assert!(invalid(|s| s.building_kinds[0].parts[1].materials[1].item = "unobtainium".into()));
        assert!(invalid(|s| s.building_kinds[0].parts.clear()));
        assert!(invalid(|s| s.building_kinds[0].parts[0].materials.clear()));
        assert!(invalid(|s| s.building_kinds[0].parts[1].id = "foundation".into()));
        assert!(invalid(|s| s.building_kinds[0].possessed_cap = 5));
        assert!(invalid(|s| s.building_kinds[0].burst.units = 0));
        assert!(invalid(|s| s.building_kinds[0].output_distance = Fx::HALF));
        assert!(invalid(|s| s.building_kinds[0].health = 0));
        assert!(invalid(|s| s.buildings[0].kind = "castle".into()));
        assert!(invalid(|s| s.buildings[0].position = [Fx::from_int(20), Fx::ZERO]));
        assert!(invalid(|s| s.sprites[0].id = "elder".into()));
    }

    #[test]
    fn the_tree_piles_take_one_item_and_the_elder_carries_one() {
        let tree = Scenario::parse(SACRED_TREE).unwrap();
        let only: Vec<_> = tree.depots.iter().map(|d| (d.id.as_str(), d.only.as_deref())).collect();
        assert_eq!(only, [("tree_wood", Some("wood")), ("tree_stone", Some("stone"))]);
        assert!(tree.agents[0].one_kind && tree.agents[0].capacity == 5);
        // Other scenes' depots and cargo take any mix.
        let forest = Scenario::parse(FOREST).unwrap();
        assert!(forest.depots.iter().all(|d| d.only.is_none()) && forest.agents.iter().all(|a| !a.one_kind));
        let invalid = |change: fn(&mut Scenario)| {
            let mut scenario = Scenario::parse(SACRED_TREE).unwrap();
            change(&mut scenario);
            scenario.validate().is_err()
        };
        assert!(invalid(|s| s.depots[0].only = Some("unobtainium".into())));
        assert!(invalid(|s| s.depots[0].stock.push(Stack { item: "stone".into(), units: 1 })));
        assert!(invalid(|s| s.sources.push(Source { depot: "tree_wood".into(), item: "stone".into(), units: 1, every: Fx::ONE, max_units: None })));
        assert!(!invalid(|s| s.sources.push(Source { depot: "tree_wood".into(), item: "wood".into(), units: 1, every: Fx::ONE, max_units: None })));
    }

    #[test]
    fn seconds_round_up_to_ticks() {
        let hz = TICKS_PER_SECOND as u64;
        assert_eq!(ticks("0.35".parse().unwrap()), (35 * hz).div_ceil(100));
        assert_eq!(ticks(Fx::ZERO), 1);
        assert_eq!(ticks(Fx::ONE), hz);
        assert_eq!(ticks_from_60hz(13), (13 * hz).div_ceil(60));
    }

    #[test]
    fn code_constants_convert_at_the_tick_rate() {
        let hz = TICKS_PER_SECOND as u64;
        assert_eq!(crate::millis_to_ticks(2000), 2 * hz);
        assert_eq!(crate::millis_to_ticks(1), 1);
        assert_eq!(crate::millis_to_ticks(0), 0);
        assert_eq!(crate::per_tick(Fx::from_int(TICKS_PER_SECOND)), Fx::ONE);
    }
}
