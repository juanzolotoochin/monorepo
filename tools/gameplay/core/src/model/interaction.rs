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
    /// Accept an attack command now; contact is resolved by the simulation on
    /// an integer tick, independent of Bevy's animation/frame cadence.
    pub fn queue_attack_aim(&mut self, id: &str, weapon: Weapon, muzzle: [f32; 3], aim: Point, windup_ticks: u64) -> Result<()> {
        if windup_ticks == 0 || windup_ticks > HZ
            || !self.config.agents.iter().any(|a| a.id == id && a.controlled)
            || (!self.config.weapons.contains(&weapon)
                && !(self.config.weapons.is_empty() && weapon == Weapon::Sword))
            || muzzle.iter().any(|v| !v.is_finite() || v.abs() > 4.)
            || aim.iter().any(|v| !v.is_finite())
            || !aim[0].hypot(aim[1]).is_finite()
            || aim[0].hypot(aim[1]) < 0.001
        {
            return Err("invalid scheduled attack".into());
        }
        self.pending_attacks.push(PendingAttack {
            actor: id.into(), weapon, muzzle, aim,
            resolve_at: self.tick + windup_ticks,
        });
        Ok(())
    }
    /// All controlled attacks resolve in the deterministic simulation, independent of rendering.
    pub fn strike(&mut self, id: &str) -> Result<u32> {
        self.attack(id, Weapon::Sword)
    }
    pub fn attack(&mut self, id: &str, weapon: Weapon) -> Result<u32> {
        self.attack_with_muzzle(id, weapon, [0.0, 0.6, 0.25])
    }
    pub fn attack_with_muzzle(
        &mut self,
        id: &str,
        weapon: Weapon,
        muzzle: [f32; 3],
    ) -> Result<u32> {
        let heading = self.agents.get(id).ok_or("unknown actor")?.heading;
        self.attack_aim(id, weapon, muzzle, [heading.sin(), heading.cos()])
    }
    pub fn attack_aim(
        &mut self,
        id: &str,
        weapon: Weapon,
        muzzle: [f32; 3],
        aim: Point,
    ) -> Result<u32> {
        if muzzle.iter().any(|v| !v.is_finite() || v.abs() > 4.0) {
            return Err("invalid weapon muzzle".into());
        }
        let magnitude = (aim[0] * aim[0] + aim[1] * aim[1]).sqrt();
        if !magnitude.is_finite() || magnitude < 0.001 {
            return Err("invalid attack aim".into());
        }
        if !self.config.weapons.contains(&weapon)
            && !(self.config.weapons.is_empty() && weapon == Weapon::Sword)
        {
            return Err("weapon is unavailable in this scenario".into());
        }
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
        let tuning = self.config.weapon_tuning.iter().find(|t| t.weapon == weapon)
            .cloned().unwrap_or_else(|| WeaponTuning::defaults(weapon));
        self.attack_ready.insert(id.into(), self.tick + tuning.cooldown_ticks);
        let actor = &self.agents[id];
        let origin = actor.position;
        let forward = [aim[0] / magnitude, aim[1] / magnitude];
        let (reach, cone) = (tuning.reach, tuning.cone);
        let mut targets: Vec<_> = self
            .threats
            .values()
            .filter_map(|t| {
                let delta = [t.position[0] - origin[0], t.position[1] - origin[1]];
                let distance = navigation::distance(t.position, origin);
                (distance <= reach
                    && (distance < 0.2
                        || (delta[0] * forward[0] + delta[1] * forward[1]) / distance > cone)
                    && (weapon == Weapon::Bow
                        || navigation::clear_with_bounds(
                            origin,
                            t.position,
                            &self.obstacles,
                            0.02,
                            self.config.extent,
                        )))
                    .then_some((t.id, distance))
            })
            .collect();
        targets.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        if weapon == Weapon::Bow {
            let id = self.id();
            let right = [forward[1], -forward[0]];
            let from = [
                origin[0] + right[0] * muzzle[0] + forward[0] * muzzle[2],
                origin[1] + right[1] * muzzle[0] + forward[1] * muzzle[2],
            ];
            let to = [from[0] + forward[0] * reach, from[1] + forward[1] * reach];
            self.projectiles.insert(
                id,
                Projectile {
                    id,
                    defender: "player".into(),
                    target: None,
                    item: "arrow".into(),
                    from,
                    launch_height: muzzle[1],
                    to,
                    born: self.tick,
                    arrival: self.tick + ticks(reach / tuning.projectile_speed),
                    damage: tuning.damage,
                    kind: ProjectileKind::Arrow,
                },
            );
            self.metrics.shots += 1;
            self.events.push(Event::Shot {
                defender: "player".into(),
            });
            return Ok(1);
        }
        if weapon == Weapon::ArcCoil {
            let ray_end = [origin[0] + forward[0] * reach, origin[1] + forward[1] * reach];
            let blocked = self
                .obstacles
                .iter()
                .filter_map(|obstacle| {
                    super::combat::segment_disc(
                        origin,
                        ray_end,
                        obstacle.center,
                        obstacle.radius + 0.04,
                    )
                })
                .min_by(f32::total_cmp)
                .unwrap_or(1.0);
            targets.retain(|(id, _)| {
                let t = &self.threats[id];
                let delta = [t.position[0] - origin[0], t.position[1] - origin[1]];
                let along = delta[0] * forward[0] + delta[1] * forward[1];
                let sideways = (delta[0] * forward[1] - delta[1] * forward[0]).abs();
                along >= 0.0 && along <= reach * blocked && sideways <= 0.42
            });
            let beam_end = targets
                .first()
                .map(|(id, _)| self.threats[id].position)
                .unwrap_or([
                    origin[0] + forward[0] * reach * blocked,
                    origin[1] + forward[1] * reach * blocked,
                ]);
            self.events.push(Event::ArcFired {
                from: origin,
                to: beam_end,
            });
            if let Some((first, _)) = targets.first().copied() {
                let anchor = self.threats[&first].position;
                let mut chain: Vec<_> = self
                    .threats
                    .values()
                    .filter(|t| t.id != first && navigation::distance(t.position, anchor) <= 2.2)
                    .map(|t| (t.id, navigation::distance(t.position, anchor)))
                    .collect();
                chain.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
                targets = std::iter::once((first, 0.0))
                    .chain(chain.into_iter().take(2))
                    .collect();
            }
        } else if weapon == Weapon::Spear {
            targets.truncate(2);
        }
        let count = targets.len() as u32;
        for (index, (target, _)) in targets.into_iter().enumerate() {
            let t = self.threats.get_mut(&target).unwrap();
            let mut impact = match weapon {
                Weapon::Sword => Impact::Sword,
                Weapon::Spear => Impact::Spear,
                Weapon::ArcCoil => Impact::Arc,
                Weapon::Bow => unreachable!(),
            };
            let armored = t.armor > 0;
            let damage = match weapon {
                Weapon::Sword if armored => {
                    t.armor -= 1;
                    impact = Impact::Deflect;
                    0
                }
                Weapon::Sword => {
                    if tuning.critical_one_in > 0 && combat_roll(self.tick, target, 0x53574f5244) % tuning.critical_one_in == 0 {
                        impact = Impact::Decapitate;
                        t.hp
                    } else {
                        tuning.damage
                    }
                }
                Weapon::Spear if armored => {
                    t.armor = t.armor.saturating_sub(2);
                    1
                }
                Weapon::Spear => {
                    tuning.damage.saturating_sub(index as u32).max(1)
                }
                Weapon::ArcCoil => {
                    if armored {
                        tuning.damage + 1
                    } else {
                        tuning.damage
                    }
                }
                Weapon::Bow => unreachable!(),
            };
            if armored && t.armor == 0 {
                self.metrics.armor_breaks += 1;
            }
            if impact == Impact::Deflect {
                self.metrics.deflections += 1;
            }
            if impact == Impact::Decapitate {
                self.metrics.decapitations += 1;
            }
            t.hp = t.hp.saturating_sub(damage);
            let direction = [t.position[0] - origin[0], t.position[1] - origin[1]];
            let length = navigation::distance(t.position, origin).max(0.001);
            let force = tuning.knockback;
            t.knockback = [direction[0] / length * force, direction[1] / length * force];
            t.stagger_until = self.tick + tuning.stagger_ticks;
            if weapon == Weapon::ArcCoil {
                t.shocked_until = t.stagger_until;
                self.metrics.arc_hits += 1;
            }
            if weapon == Weapon::Spear {
                self.metrics.spear_hits += 1;
            }
            if weapon == Weapon::Sword {
                self.metrics.melee_hits += 1;
            }
            let killed = t.hp == 0;
            let position = t.position;
            self.events.push(Event::Hit {
                target,
                projectile: None,
                position,
                killed,
                impact,
            });
            if killed {
                self.threats.remove(&target);
                self.metrics.kills += 1;
                if weapon == Weapon::Sword {
                    self.metrics.melee_kills += 1;
                }
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
