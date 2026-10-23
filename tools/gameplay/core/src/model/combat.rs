use super::*;
impl Simulation {
    pub(super) fn combat(&mut self) {
        let arrived: Vec<_> = self
            .projectiles
            .values()
            .filter(|p| p.arrival <= self.tick)
            .map(|p| p.id)
            .collect();
        for id in arrived {
            let p = self.projectiles.remove(&id).unwrap();
            if let Some(target) = self.threats.get_mut(&p.target) {
                target.hp = target.hp.saturating_sub(p.damage);
                let killed = target.hp == 0;
                let position = target.position;
                self.events.push(Event::Hit { position, killed });
                if killed {
                    self.threats.remove(&p.target);
                    self.metrics.kills += 1;
                }
            }
        }
        for i in 0..self.config.waves.len() {
            let w = &self.config.waves[i];
            let first = ((w.first as f64 * HZ as f64).round() as u64).max(1);
            if self.tick < first
                || self.wave_totals[i] >= w.count
                || (self.tick - first) % ticks(w.every) != 0
            {
                continue;
            }
            let w = w.clone();
            let route = self.plan(w.position, w.target, 0.2);
            if route.is_empty() {
                continue;
            }
            let id = self.id();
            self.threats.insert(
                id,
                Threat {
                    id,
                    wave: i,
                    position: w.position,
                    heading: 0.0,
                    travelled: 0.0,
                    hp: w.hp,
                    route,
                },
            );
            self.wave_totals[i] += 1;
        }
        let mut breached = vec![];
        for threat in self.threats.values_mut() {
            let w = &self.config.waves[threat.wave];
            let previous = threat.position;
            move_route(&mut threat.position, &mut threat.route, w.speed / HZ as f32);
            let moved = navigation::distance(previous, threat.position);
            threat.travelled += moved;
            if moved > 0.0 {
                threat.heading =
                    (threat.position[0] - previous[0]).atan2(threat.position[1] - previous[1]);
            }
            if threat.route.is_empty() {
                breached.push(threat.id);
            }
        }
        for id in breached {
            let t = self.threats.remove(&id).unwrap();
            self.health = self.health.saturating_sub(self.config.waves[t.wave].damage);
            self.metrics.breaches += 1;
            self.events.push(Event::Breach {
                position: t.position,
            });
        }
        for projectile in self.projectiles.values_mut() {
            if let Some(t) = self.threats.get(&projectile.target) {
                projectile.to = t.position;
            }
        }
        for d in self.config.defenders.clone() {
            let state = &self.defenders[&d.id];
            if !state.enabled
                || state.ready_at > self.tick
                || self.available(&d.depot, &d.ammo) == 0
            {
                continue;
            }
            let target = self
                .threats
                .values()
                .filter(|t| {
                    navigation::distance(d.position, t.position) <= d.range
                        && self
                            .projectiles
                            .values()
                            .filter(|p| p.target == t.id)
                            .map(|p| p.damage)
                            .sum::<u32>()
                            < t.hp
                })
                .min_by(|a, b| {
                    navigation::distance(d.position, a.position)
                        .total_cmp(&navigation::distance(d.position, b.position))
                        .then(a.id.cmp(&b.id))
                })
                .map(|t| (t.id, t.position));
            if let Some((target, position)) = target {
                self.depots.get_mut(&d.depot).unwrap().take(&d.ammo, 1);
                self.accounts.get_mut(&d.ammo).unwrap().spent += 1;
                self.metrics.shots += 1;
                let id = self.id();
                self.projectiles.insert(
                    id,
                    Projectile {
                        id,
                        defender: d.id.clone(),
                        target,
                        item: d.ammo.clone(),
                        from: d.position,
                        to: position,
                        born: self.tick,
                        arrival: self.tick
                            + ticks(
                                navigation::distance(d.position, position) / d.projectile_speed,
                            ),
                        damage: d.damage,
                    },
                );
                let state = self.defenders.get_mut(&d.id).unwrap();
                state.aim = Some(position);
                state.ready_at = self.tick + ticks(d.period);
                state.fired_at = self.tick;
                self.events.push(Event::Shot {
                    defender: d.id.clone(),
                });
            }
        }
    }
}
