//! Nearby interaction targets for the controller (World layer): piles,
//! dispensers and the collector in pile scenes, depots and machines in
//! settlements.
//!
//! A target is a candidate while it is within `tuning::TARGET_RADIUS` of its
//! interaction surface, on the avatar's floor, and (for targets the avatar
//! walks to) reachable on foot. The nearest candidate is selected and kept
//! while it stays a candidate; D-pad left/right cycles candidates in a
//! stable order. Distances come from the confirmed avatar position, never
//! the predicted view. Local eligibility is a hint: the simulation still
//! decides whether an action succeeds.

use std::collections::BTreeMap;

use bevy::prelude::*;
use voxel_assets::Mineral;

use super::tuning;
use crate::lockstep::{SessionService, WorldMirror};
use crate::pile_model::{pile_radius, PileId};
use crate::piles::PileLab;

/// What a target is, ordered for stable cycling.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TargetKey {
    Pile(PileId),
    Dispenser(usize),
    Collector,
    Depot(String),
    Machine(String),
}

/// A candidate, with what the primary action would do or why it can't.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub key: TargetKey,
    pub name: String,
    /// Arena coordinates, on the ground.
    pub position: Vec3,
    /// The interaction surface's radius around `position`.
    pub surface: f32,
    /// Distance from the avatar to the surface.
    pub gap: f32,
    /// The primary action's verb, or why it is unavailable.
    pub verb: Result<String, String>,
}

/// The selection and its candidates.
#[derive(Default, Debug)]
pub struct Targets {
    pub candidates: Vec<Target>,
    pub selected: Option<TargetKey>,
    /// Approach checks: reachable, and when it was checked (seconds).
    approach: BTreeMap<TargetKey, (bool, f32)>,
}

impl Targets {
    pub fn selected(&self) -> Option<&Target> {
        let key = self.selected.as_ref()?;
        self.candidates.iter().find(|t| &t.key == key)
    }

    /// Moves the selection through the candidates (`step` is ±1).
    pub fn cycle(&mut self, step: i32) {
        if self.candidates.is_empty() {
            return;
        }
        let n = self.candidates.len() as i32;
        let at = self.selected.as_ref().and_then(|k| self.candidates.iter().position(|t| &t.key == k)).map_or(if step > 0 { -1 } else { 0 }, |i| i as i32);
        let next = (at + step).rem_euclid(n) as usize;
        self.selected = Some(self.candidates[next].key.clone());
    }

    /// Rebuilds the candidates from the confirmed world; keeps the
    /// selection while it is still a candidate.
    pub fn refresh(&mut self, world: &World, now: f32) {
        let mut candidates = gather(world);
        let service = world.resource::<SessionService>();
        let Some(origin) = service.player_view_position() else {
            self.candidates.clear();
            self.selected = None;
            return;
        };
        let floor = service.ground_height(origin).unwrap_or(0.0);
        candidates.retain(|t| t.gap <= tuning::TARGET_RADIUS);
        candidates.retain(|t| service.ground_height(t.position).is_none_or(|h| (h - floor).abs() <= tuning::TARGET_FLOOR));
        // Targets the avatar walks to need a way there on foot.
        let walks = |k: &TargetKey| matches!(k, TargetKey::Pile(_) | TargetKey::Collector | TargetKey::Depot(_));
        let mut approach = std::mem::take(&mut self.approach);
        approach.retain(|k, _| candidates.iter().any(|t| &t.key == k));
        candidates.retain(|t| {
            if !walks(&t.key) || t.gap <= 0.6 {
                return true;
            }
            let entry = approach.entry(t.key.clone()).or_insert((false, f32::NEG_INFINITY));
            if now - entry.1 >= tuning::TARGET_RECHECK {
                *entry = (reachable(service, origin, t), now);
            }
            entry.0
        });
        self.approach = approach;
        candidates.sort_by(|a, b| a.key.cmp(&b.key));
        self.candidates = candidates;
        let kept = self.selected.as_ref().is_some_and(|k| self.candidates.iter().any(|t| &t.key == k));
        if !kept {
            self.selected = self
                .candidates
                .iter()
                .min_by(|a, b| a.gap.total_cmp(&b.gap).then_with(|| a.key.cmp(&b.key)))
                .map(|t| t.key.clone());
        }
    }
}

/// Whether there is a way on foot to the target's surface.
fn reachable(service: &SessionService, origin: Vec3, target: &Target) -> bool {
    let Some(world) = service.world() else { return false };
    let away = crate::navigation::flat(origin - target.position).normalize_or(Vec3::Z);
    let stand = target.position + away * (target.surface + 0.5);
    let Some(radius) = service.player_radius() else { return false };
    !sim::nav::path(world.terrain(), service.to_sim(origin), service.to_sim(stand), radius).is_empty()
}

pub(crate) fn mineral_label(m: Mineral) -> &'static str {
    match m {
        Mineral::AmberOre => "AMBER ORE",
        Mineral::BlueCrystal => "BLUE CRYSTAL",
        Mineral::GreenMalachite => "GREEN MALACHITE",
    }
}

/// Every target in the scene, before the range, floor and approach rules.
fn gather(world: &World) -> Vec<Target> {
    let service = world.resource::<SessionService>();
    let Some(origin) = service.player_view_position() else { return Vec::new() };
    let gap = |p: Vec3, surface: f32| (crate::navigation::flat(p - origin).length() - surface).max(0.0);
    let mut out = Vec::new();
    if let Some(lab) = world.get_resource::<PileLab>() {
        if let Some(stock) = &lab.stock {
            let cargo = &stock.cargo;
            for (&id, pile) in &stock.piles {
                let position = Vec3::new(pile.position[0], pile.height, pile.position[1]);
                let surface = pile_radius(pile.units).max(0.55);
                let verb = if cargo.units > 0 && cargo.material != Some(pile.material) {
                    Err("Different minerals stay separate. Drop your cargo first.".into())
                } else if cargo.units > 0 {
                    Ok("DEPOSIT".into())
                } else if pile.units == 0 {
                    Err("Empty output. Use its dispenser to add ore.".into())
                } else {
                    Ok("COLLECT".into())
                };
                let name = format!("{} PILE ({})", mineral_label(pile.material), pile.units);
                out.push(Target { key: TargetKey::Pile(id), name, position, surface, gap: gap(position, surface), verb });
            }
            if let Some(layout) = &lab.layout {
                for (index, d) in layout.dispensers.iter().enumerate() {
                    let position = Vec3::from_array(d.position).with_y(0.0);
                    let name = format!("{} DISPENSER", mineral_label(d.material));
                    out.push(Target { key: TargetKey::Dispenser(index), name, position, surface: 0.9, gap: gap(position, 0.9), verb: Ok("DISPENSE".into()) });
                }
            }
            if let Some(position) = lab.collector {
                let verb = if cargo.units > 0 { Ok("DELIVER".into()) } else { Err("Carrier is empty. Collect ore before delivering.".into()) };
                let surface = sim::settlement::piles::COLLECTOR_RADIUS.to_f32();
                out.push(Target { key: TargetKey::Collector, name: "COLLECTOR".into(), position, surface, gap: gap(position, surface), verb });
            }
        }
    }
    let settlement = world.get_resource::<crate::settlement::Settlement>().and_then(|s| s.simulation.as_ref());
    if let Some(view) = settlement {
        for depot in &view.config().depots {
            let position = Vec3::new(depot.position[0], 0.0, depot.position[1]);
            let verb = depot_choices(service, &depot.id).map_or_else(|e| Err(e), |c| Ok(if c.deposit { "DEPOSIT" } else { "COLLECT" }.into()));
            out.push(Target { key: TargetKey::Depot(depot.id.clone()), name: depot.label.to_uppercase(), position, surface: 0.9, gap: gap(position, 0.9), verb });
        }
        for producer in &view.config().producers {
            let position = Vec3::new(producer.position[0], 0.0, producer.position[1]);
            let distance = crate::navigation::flat(position - origin).length();
            let enabled = view.producers.get(&producer.id).is_some_and(|p| p.enabled);
            let verb = if distance < MACHINE_REACH {
                Ok(if enabled { "PAUSE" } else { "ENABLE" }.into())
            } else {
                Err(format!("Move within {MACHINE_REACH} m to switch it."))
            };
            let surface = producer.radius.max(1.0);
            out.push(Target { key: TargetKey::Machine(producer.id.clone()), name: producer.label.to_uppercase(), position, surface, gap: gap(position, surface), verb });
        }
    }
    out
}

/// How far a machine toggle reaches (meters from its center), as keyboard E.
pub const MACHINE_REACH: f32 = 2.5;

/// What the primary action at a depot would move.
#[derive(Clone, Debug, PartialEq)]
pub struct DepotChoices {
    /// Depositing carried goods (else taking).
    pub deposit: bool,
    /// Items and how many units each would move.
    pub items: Vec<(String, u32)>,
}

/// Items the player could deposit at a depot (preferred while carrying
/// any it accepts) or take from it, at the click's quantities.
pub fn depot_choices(service: &SessionService, depot: &str) -> Result<DepotChoices, String> {
    let current = service.settlement().ok_or("No settlement loaded")?;
    let (Some(agent), Some(d)) = (service.player(), current.depot_index(depot)) else {
        return Err("Unknown depot".into());
    };
    let moving = |deposit: bool| -> Vec<(String, u32)> {
        current
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| {
                let limit = current.transfer_limit(agent as usize, d as usize, i, deposit);
                (limit > 0).then(|| (item.clone(), limit.min(if deposit { 64 } else { 4 })))
            })
            .collect()
    };
    let deposit = moving(true);
    if !deposit.is_empty() {
        return Ok(DepotChoices { deposit: true, items: deposit });
    }
    take_choices(service, depot)
}

/// Items the player could take from a depot.
pub fn take_choices(service: &SessionService, depot: &str) -> Result<DepotChoices, String> {
    let current = service.settlement().ok_or("No settlement loaded")?;
    let (Some(agent), Some(d)) = (service.player(), current.depot_index(depot)) else {
        return Err("Unknown depot".into());
    };
    let items: Vec<(String, u32)> = current
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let limit = current.transfer_limit(agent as usize, d as usize, i, false);
            (limit > 0).then(|| (item.clone(), limit.min(4)))
        })
        .collect();
    if items.is_empty() {
        return Err("Nothing here to collect, or your cargo is full.".into());
    }
    Ok(DepotChoices { deposit: false, items })
}

/// The confirmed facing of the local avatar, as a flat direction: its aim
/// if it has one, else its heading.
pub fn confirmed_facing(mirror: &WorldMirror) -> Option<Vec3> {
    let current = mirror.current.as_ref()?;
    let a = &current.agents[mirror.player()? as usize];
    let direction = match a.avatar()?.aim {
        Some(aim) => Vec3::new(aim.x.to_f32(), 0.0, aim.y.to_f32()),
        None => {
            let h = sim::nav::heading_vector(a.body.heading);
            Vec3::new(h.x.to_f32(), 0.0, h.y.to_f32())
        }
    };
    Some(direction.normalize_or(Vec3::Z))
}
