use crate::{navigation::Point, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub skin: String,
    pub scale: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stack {
    pub item: String,
    pub units: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Depot {
    pub id: String,
    pub label: String,
    pub position: Point,
    pub capacity: u32,
    pub stock: Vec<Stack>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub depot: String,
    pub item: String,
    pub units: u32,
    pub every: f32,
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
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Equipment {
    pub depot: String,
    pub item: String,
    pub skin: String,
    pub capacity: u32,
    pub speed_factor: f32,
    pub offset: [f32; 3],
    pub cargo_anchor: [f32; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub id: String,
    pub label: String,
    pub skin: String,
    pub position: Point,
    pub speed: f32,
    pub radius: f32,
    pub capacity: u32,
    pub links: Vec<String>,
    pub transfer_seconds: f32,
    pub cargo_anchor: [f32; 3],
    pub equipment: Option<Equipment>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Producer {
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
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defender {
    pub id: String,
    pub position: Point,
    pub skin: String,
    pub depot: String,
    pub ammo: String,
    pub range: f32,
    pub period: f32,
    pub damage: u32,
    pub projectile_speed: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wave {
    pub id: String,
    pub position: Point,
    pub target: Point,
    pub skin: String,
    pub first: f32,
    pub every: f32,
    pub speed: f32,
    pub hp: u32,
    pub damage: u32,
    pub count: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub id: String,
    pub items: Vec<Item>,
    pub depots: Vec<Depot>,
    pub sources: Vec<Source>,
    pub links: Vec<HaulLink>,
    pub agents: Vec<Agent>,
    pub producers: Vec<Producer>,
    pub defenders: Vec<Defender>,
    pub waves: Vec<Wave>,
    pub base_health: u32,
}
fn ident(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn finite(p: Point) -> bool {
    p.iter().all(|v| v.is_finite() && v.abs() <= 8.0)
}
fn number(v: f32, min: f32, max: f32) -> bool {
    v.is_finite() && (min..=max).contains(&v)
}
fn ids<'a>(values: impl Iterator<Item = &'a str>) -> Result<BTreeSet<&'a str>> {
    let mut ids = BTreeSet::new();
    for id in values {
        if !ident(id) || !ids.insert(id) {
            return Err(format!("invalid or duplicate identifier {id}"));
        }
    }
    Ok(ids)
}
impl Scenario {
    pub fn validate(&self) -> Result<()> {
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
            !ss.is_empty()
                && ss.len() <= 16
                && ss.iter().all(stack)
                && ss.iter().map(|s| &s.item).collect::<BTreeSet<_>>().len() == ss.len()
        };
        for i in &self.items {
            if !ident(&i.skin) || !number(i.scale, 0.02, 2.0) || i.label.is_empty() {
                return Err("invalid item appearance".into());
            }
        }
        for d in &self.depots {
            if !finite(d.position)
                || !(1..=4096).contains(&d.capacity)
                || d.stock.iter().any(|s| !stack(s))
                || d.stock.iter().map(|s| s.units as u64).sum::<u64>() > d.capacity as u64
                || d.stock
                    .iter()
                    .map(|s| &s.item)
                    .collect::<BTreeSet<_>>()
                    .len()
                    != d.stock.len()
            {
                return Err(format!("invalid depot {}", d.id));
            }
        }
        for s in &self.sources {
            if !depots.contains(s.depot.as_str())
                || !items.contains(s.item.as_str())
                || s.units == 0
                || s.units > 4096
                || !number(s.every, 0.1, 3600.0)
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
            if !finite(a.position)
                || !ident(&a.skin)
                || !number(a.speed, 0.1, 8.0)
                || !number(a.radius, 0.05, 1.0)
                || !(1..=64).contains(&a.capacity)
                || !number(a.transfer_seconds, 0.05, 5.0)
                || a.links.is_empty()
                || a.links.iter().any(|l| !links.contains(l.as_str()))
                || a.cargo_anchor
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 4.0)
            {
                return Err(format!("invalid agent {}", a.id));
            }
            if let Some(e) = &a.equipment {
                if !depots.contains(e.depot.as_str())
                    || !items.contains(e.item.as_str())
                    || !ident(&e.skin)
                    || !(a.capacity..=64).contains(&e.capacity)
                    || !number(e.speed_factor, 0.2, 2.0)
                    || e.offset
                        .iter()
                        .chain(e.cargo_anchor.iter())
                        .any(|v| !v.is_finite() || v.abs() > 4.0)
                {
                    return Err("invalid equipment upgrade".into());
                }
            }
        }
        for p in &self.producers {
            if !finite(p.position)
                || !ident(&p.skin)
                || !depots.contains(p.input.as_str())
                || !depots.contains(p.output.as_str())
                || !stacks(&p.ingredients)
                || !stacks(&p.products)
                || !number(p.seconds, 0.1, 3600.0)
                || p.max_cycles == Some(0)
            {
                return Err(format!("invalid producer {}", p.id));
            }
        }
        for d in &self.defenders {
            if !finite(d.position)
                || !ident(&d.skin)
                || !depots.contains(d.depot.as_str())
                || !items.contains(d.ammo.as_str())
                || !number(d.range, 0.1, 16.0)
                || !number(d.period, 0.1, 60.0)
                || d.damage == 0
                || !number(d.projectile_speed, 0.1, 40.0)
            {
                return Err("invalid defender".into());
            }
        }
        for w in &self.waves {
            if !finite(w.position)
                || !finite(w.target)
                || !ident(&w.skin)
                || !number(w.first, 0.0, 3600.0)
                || !number(w.every, 0.5, 3600.0)
                || !number(w.speed, 0.1, 4.0)
                || w.hp == 0
                || w.damage == 0
                || w.count > 256
            {
                return Err("invalid wave".into());
            }
        }
        Ok(())
    }
}
