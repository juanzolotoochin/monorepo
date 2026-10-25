//! Scenarios: the starting setup of a settlement scene (depots, haul links,
//! haulers, machines, defenders, enemy waves). A host turns a scenario into
//! the world at tick 0; from then on the state lives in saves.
//!
//! The file format is the original game's, so existing scene files load
//! unchanged. Simulation values parse into `Fx`; view-only values (labels,
//! skins, cargo layouts and anchors) stay `f32` and never reach the
//! simulation. Positions are arena coordinates: meters from the arena center.
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
    pub skin: String,
    pub position: Point,
    /// Meters per second. Unused for a `controlled` avatar, which moves at
    /// the simulation's walk and run speeds.
    pub speed: Fx,
    /// For a `controlled` avatar, only the size haulers steer around.
    pub radius: Fx,
    pub capacity: u32,
    pub links: Vec<String>,
    pub transfer_seconds: Fx,
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
    pub skin: String,
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
}

fn f(n: i64, d: i64) -> Fx {
    Fx::from_ratio(n, d)
}

fn within(v: Fx, min: Fx, max: Fx) -> bool {
    v >= min && v <= max
}

fn ident(id: &str) -> bool {
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
        let scenario: Scenario = ron::from_str(text).map_err(|e| e.to_string())?;
        scenario.validate()?;
        Ok(scenario)
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
            || self.agents.len() > 32
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
        ids(self.agents.iter().map(|x| x.id.as_str()))?;
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
                || !ident(&a.skin)
                || !within(a.speed, f(1, 10), Fx::from_int(8))
                || !within(a.radius, f(1, 20), Fx::ONE)
                || !(1..=64).contains(&a.capacity)
                || !within(a.transfer_seconds, f(1, 20), Fx::from_int(5))
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
        for w in &self.waves {
            if !point(w.position)
                || !point(w.target)
                || !ident(&w.skin)
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
