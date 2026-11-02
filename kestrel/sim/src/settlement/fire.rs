//! The fireball and burning ground (`SACRED_TREE_BALLISTA.md`, B4).
//!
//! An elder possessing a ballista whose kind has a fireball throws one with
//! the Burst key (`Action::Burst`, `Settlement::bursts`): it flies from the
//! ballista to the reticle (`Settlement::fireballs`), costs no ammo, and
//! starts the elder's own cooldown (`AvatarState::fireball_ready`), apart
//! from a gatherer's Burst.
//!
//! Each tick, in stage 5 after the shots (`Settlement::burn`):
//!
//! - a fireball that arrives hurts every threat within its radius of where
//!   it lands, and leaves a fire patch there (`Settlement::fires`) that
//!   burns out after its time;
//! - a threat standing in a patch has its burn refreshed: it burns until
//!   the linger after now, whatever burned it before, so burns never stack;
//! - a burning threat takes the burn's damage once a period, the first a
//!   period after it caught fire, until its burn runs out.
//!
//! Fire only ever touches threats: elders and sprites walk through it.
//! Every hit is an `Event::Hit` with `Impact::Fire`, and a kill counts as
//! any other (`CombatMetrics::kills`).

use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::buildings::BuildingIx;
use super::{AgentIx, Event, Impact, Settlement};
use crate::nav::distance;

/// A kind's fireball (`content::scenario::Fireball`), in ticks: rules,
/// copied into each fireball and patch so they keep them in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FireballDef {
    /// Ticks the elder waits before the next.
    pub cooldown: u64,
    /// Meters a second.
    pub speed: Fx,
    /// What it does to each threat within `radius` of where it lands.
    pub damage: u32,
    pub radius: Fx,
    /// The fire it leaves: this radius, for this many ticks.
    pub fire_radius: Fx,
    pub fire_ticks: u64,
    pub burn: BurnDef,
}

/// What a fire does to a threat in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BurnDef {
    pub damage: u32,
    /// Ticks between hits.
    pub period: u64,
    /// Ticks a threat keeps burning after it leaves the fire.
    pub linger: u64,
}

/// A fireball in flight, from a ballista to its elder's reticle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fireball {
    pub id: u64,
    pub agent: AgentIx,
    pub building: BuildingIx,
    pub from: FxVec2,
    /// Above the ground at `from`.
    pub launch_height: Fx,
    pub to: FxVec2,
    pub born: u64,
    /// The tick it lands on.
    pub arrival: u64,
    pub def: FireballDef,
}

/// Burning ground a fireball left.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirePatch {
    pub id: u64,
    pub position: FxVec2,
    pub radius: Fx,
    pub born: u64,
    /// The first tick it is out.
    pub expires: u64,
    pub burn: BurnDef,
}

impl FirePatch {
    /// Whether `point` is in the fire.
    pub fn covers(&self, point: FxVec2) -> bool {
        distance(self.position, point) <= self.radius
    }
}

/// A threat's burn (`Threat::burn`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Burn {
    /// The last tick it burns: the linger after it last stood in a fire.
    pub until: u64,
    /// The tick of its next hit.
    pub next: u64,
    pub damage: u32,
    pub period: u64,
}

impl Settlement {
    /// Stage 5 (`bursts`): throws a fireball of `def` from ballista
    /// `building` to `to` for elder `agent`, and starts the elder's
    /// cooldown. The caller has checked it may (`burst_plan`).
    pub(super) fn throw_fireball(&mut self, tick: u64, agent: AgentIx, building: BuildingIx, to: FxVec2, def: FireballDef, launch_height: Fx) {
        let from = self.buildings[building as usize].position;
        let arrival = tick + content::scenario::ticks(distance(from, to) / def.speed);
        let id = self.id();
        self.fireballs.push(Fireball { id, agent, building, from, launch_height, to, born: tick, arrival, def });
        if let Some(avatar) = self.agents[agent as usize].avatar_mut() {
            avatar.fireball_ready = tick + def.cooldown;
        }
        self.events.push(Event::FireballThrown { agent, building, to, arrival });
    }

    /// Stage 5, after the shots: fireballs land, fires burn out, threats in
    /// a fire catch or keep burning, and burning threats take their hits.
    pub(crate) fn burn(&mut self, tick: u64) {
        if self.fireballs.is_empty() && self.fires.is_empty() && self.threats.iter().all(|t| t.burn.is_none()) {
            return;
        }
        let (landed, flying): (Vec<_>, Vec<_>) = std::mem::take(&mut self.fireballs).into_iter().partition(|f| f.arrival <= tick);
        self.fireballs = flying;
        for f in landed {
            let hit: Vec<u64> = self.threats.iter().filter(|t| distance(t.position, f.to) <= f.def.radius).map(|t| t.id).collect();
            for id in hit {
                self.fire_hit(id, f.def.damage);
            }
            let id = self.id();
            let def = f.def;
            self.fires.push(FirePatch { id, position: f.to, radius: def.fire_radius, born: tick, expires: tick + def.fire_ticks, burn: def.burn });
            self.events.push(Event::FireballLanded { position: f.to });
        }
        self.fires.retain(|p| p.expires > tick);
        let fires = &self.fires;
        let mut due = Vec::new();
        for t in &mut self.threats {
            // The newest fire it stands in sets the burn.
            if let Some(fire) = fires.iter().rev().find(|p| p.covers(t.position)) {
                let until = tick + fire.burn.linger;
                match &mut t.burn {
                    Some(burn) => burn.until = burn.until.max(until),
                    None => t.burn = Some(Burn { until, next: tick + fire.burn.period, damage: fire.burn.damage, period: fire.burn.period }),
                }
            }
            let Some(burn) = &mut t.burn else { continue };
            if burn.next <= tick && burn.next <= burn.until {
                due.push((t.id, burn.damage));
                burn.next += burn.period;
            }
            if tick >= burn.until {
                t.burn = None;
            }
        }
        for (id, damage) in due {
            self.fire_hit(id, damage);
        }
    }

    /// Fire takes `damage` from threat `id`'s health, whatever its armor;
    /// at 0 it dies, counted as any kill.
    fn fire_hit(&mut self, id: u64, damage: u32) {
        let Some(i) = self.threats.binary_search_by_key(&id, |t| t.id).ok() else { return };
        if damage == 0 {
            return;
        }
        let t = &mut self.threats[i];
        t.hp = t.hp.saturating_sub(damage);
        let (killed, position) = (t.hp == 0, t.position);
        self.events.push(Event::Hit { target: id, agent: None, projectile: None, position, killed, impact: Impact::Fire });
        if killed {
            self.threats.remove(i);
            self.combat.kills += 1;
        }
    }
}
