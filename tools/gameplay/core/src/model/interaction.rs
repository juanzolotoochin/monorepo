//! Controlled actors use the same inventories, reservations, airborne items and cadence as workers.
use super::*;
#[derive(Clone, Debug, Serialize)]
pub struct ManualJob {
    pub depot: String,
    pub item: String,
    pub remaining: u32,
    pub deposit: bool,
    pub next_tick: u64,
}
impl Simulation {
    /// A controlled actor's forward melee strike uses the same threat health and death events.
    pub fn strike(&mut self, id: &str) -> Result<u32> {
        if !self
            .config
            .agents
            .iter()
            .any(|a| a.id == id && a.controlled)
        {
            return Err("actor is not controlled".into());
        }
        if self.attack_ready.get(id).is_some_and(|t| *t > self.tick) {
            return Ok(0);
        }
        self.attack_ready.insert(id.into(), self.tick + 38);
        let a = &self.agents[id];
        let position = a.position;
        let forward = [a.heading.sin(), a.heading.cos()];
        let targets: Vec<_> = self
            .threats
            .values()
            .filter(|t| {
                let d = [t.position[0] - position[0], t.position[1] - position[1]];
                let distance = navigation::distance(t.position, position);
                distance <= 1.75
                    && (distance < 0.2 || (d[0] * forward[0] + d[1] * forward[1]) / distance > 0.35)
            })
            .map(|t| t.id)
            .collect();
        let count = targets.len() as u32;
        for target in targets {
            let t = self.threats.get_mut(&target).unwrap();
            t.hp = t.hp.saturating_sub(1);
            let direction = [t.position[0] - position[0], t.position[1] - position[1]];
            let length = navigation::distance(t.position, position).max(0.001);
            t.knockback = [direction[0] / length * 5.5, direction[1] / length * 5.5];
            t.stagger_until = self.tick + 15;
            self.metrics.melee_hits += 1;
            let killed = t.hp == 0;
            let position = t.position;
            self.events.push(Event::Hit {
                target,
                projectile: None,
                position,
                killed,
            });
            if killed {
                self.threats.remove(&target);
                self.metrics.kills += 1;
                self.metrics.melee_kills += 1;
            }
        }
        Ok(count)
    }

    /// Controlled bodies are driven by an external motor (keyboard, gamepad, or commands).
    pub fn set_controlled_pose(&mut self, id: &str, position: Point, heading: f32) -> Result<()> {
        if !self
            .config
            .agents
            .iter()
            .any(|a| a.id == id && a.controlled)
        {
            return Err("actor is not externally controlled".into());
        }
        if position
            .iter()
            .any(|v| !v.is_finite() || v.abs() > self.config.extent)
            || !heading.is_finite()
        {
            return Err("invalid actor pose".into());
        }
        let a = self.agents.get_mut(id).unwrap();
        a.travelled += navigation::distance(a.position, position);
        a.position = position;
        a.heading = heading;
        Ok(())
    }
    pub fn transfer_limit(&self, id: &str, depot: &str, item: &str, deposit: bool) -> u32 {
        let (Some(a), Some(_)) = (self.agents.get(id), self.depots.get(depot)) else {
            return 0;
        };
        if !self.accounts.contains_key(item) {
            return 0;
        }
        if deposit {
            a.cargo.units(item).min(self.free(depot))
        } else {
            (a.cargo.capacity - a.cargo.total()).min(self.available(depot, item))
        }
    }
    pub fn cancel_transfer(&mut self, id: &str) -> Result<()> {
        if !self
            .config
            .agents
            .iter()
            .any(|a| a.id == id && a.controlled)
        {
            return Err("actor is not externally controlled".into());
        }
        self.manual_jobs.remove(id);
        Ok(())
    }
    /// Reserve a precise amount. The transfer waits for the actor to reach the dock.
    /// Cancellation releases unlaunched units; airborne units always finish their transfer.
    pub fn transfer(
        &mut self,
        id: &str,
        depot: &str,
        item: &str,
        units: u32,
        deposit: bool,
    ) -> Result<()> {
        if !self
            .config
            .agents
            .iter()
            .any(|a| a.id == id && a.controlled)
        {
            return Err("actor is not externally controlled".into());
        }
        if self.manual_jobs.contains_key(id) || self.pending_cargo(id) || self.pending_drop(id) {
            return Err("actor already has a transfer in progress".into());
        }
        if !self.depots.contains_key(depot) || !self.accounts.contains_key(item) || units == 0 {
            return Err("unknown stock or zero transfer".into());
        }
        let cargo = &self.agents[id].cargo;
        if deposit {
            if cargo.units(item) < units || self.free(depot) < units {
                return Err("cargo or unreserved destination space is insufficient".into());
            }
        } else if cargo.capacity - cargo.total() < units || self.available(depot, item) < units {
            return Err("unreserved stock or cargo space is insufficient".into());
        }
        self.manual_jobs.insert(
            id.into(),
            ManualJob {
                depot: depot.into(),
                item: item.into(),
                remaining: units,
                deposit,
                next_tick: self.tick,
            },
        );
        Ok(())
    }
    pub(super) fn manual_transfers(&mut self) {
        let ids: Vec<_> = self.manual_jobs.keys().cloned().collect();
        for id in ids {
            let job = self.manual_jobs[&id].clone();
            let p = self.agents[&id].position;
            let dock = self.depot_position(&job.depot);
            if self.tick < job.next_tick || navigation::distance(p, dock) > 0.85 {
                continue;
            }
            let (from, to) = if job.deposit {
                self.agents.get_mut(&id).unwrap().cargo.take(&job.item, 1);
                (
                    Endpoint::Cargo(id.clone()),
                    Endpoint::Depot(job.depot.clone()),
                )
            } else {
                self.depots.get_mut(&job.depot).unwrap().take(&job.item, 1);
                self.metrics.picked += 1;
                (
                    Endpoint::Depot(job.depot.clone()),
                    Endpoint::Cargo(id.clone()),
                )
            };
            self.launch(
                &job.item,
                from,
                to,
                if job.deposit { p } else { dock },
                if job.deposit { dock } else { p },
            );
            let remaining = job.remaining - 1;
            if remaining == 0 {
                self.manual_jobs.remove(&id);
            } else {
                let cadence = self
                    .config
                    .agents
                    .iter()
                    .find(|a| a.id == id)
                    .unwrap()
                    .transfer_seconds;
                let next = self.tick + ticks(cadence);
                let j = self.manual_jobs.get_mut(&id).unwrap();
                j.remaining = remaining;
                j.next_tick = next;
            }
        }
    }
}
