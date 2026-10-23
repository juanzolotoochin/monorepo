use super::*;
impl Simulation {
    pub(super) fn combat(&mut self) {
        self.spent_projectiles.retain(|_, p| p.arrival > self.tick);
        let landed: Vec<_> = self
            .pinned
            .values()
            .filter(|p| p.land <= self.tick)
            .map(|p| p.target)
            .collect();
        for id in landed {
            let p = self.pinned.remove(&id).unwrap();
            self.events.push(Event::PinnedLanded {
                target: id,
                position: p.position,
            });
        }
        for p in self.pinned.values_mut() {
            let f = ((self.tick - p.born) as f32 / (p.land - p.born) as f32).min(1.);
            p.position[0] += p.heading.sin() * (1. - f) * 0.095;
            p.position[1] += p.heading.cos() * (1. - f) * 0.095;
        }
        let free: Vec<_> = self
            .projectiles
            .values()
            .filter(|p| p.target.is_none() && p.born < self.tick)
            .map(|p| p.id)
            .collect();
        for id in free {
            let p = &self.projectiles[&id];
            let duration = (p.arrival - p.born) as f32;
            let previous = ((self.tick - 1 - p.born) as f32 / duration).clamp(0., 1.);
            let current = ((self.tick - p.born) as f32 / duration).clamp(0., 1.);
            let start = [
                p.from[0] + (p.to[0] - p.from[0]) * previous,
                p.from[1] + (p.to[1] - p.from[1]) * previous,
            ];
            let end = [
                p.from[0] + (p.to[0] - p.from[0]) * current,
                p.from[1] + (p.to[1] - p.from[1]) * current,
            ];
            let obstacle = self
                .obstacles
                .iter()
                .filter_map(|o| segment_disc(start, end, o.center, o.radius + 0.04))
                .min_by(f32::total_cmp);
            let threat = self
                .threats
                .values()
                .filter_map(|t| {
                    segment_disc(
                        start,
                        end,
                        t.position,
                        if t.armor > 0 { 0.38 } else { 0.27 },
                    )
                    .map(|f| (f, t.id))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            if let Some((_, target)) = threat.filter(|(f, _)| obstacle.is_none_or(|rock| *f < rock))
            {
                let p = self.projectiles.get_mut(&id).unwrap();
                p.target = Some(target);
                p.arrival = self.tick;
            } else if let Some(f) = obstacle {
                let p = self.projectiles.remove(&id).unwrap();
                let impact = [
                    start[0] + (end[0] - start[0]) * f,
                    start[1] + (end[1] - start[1]) * f,
                ];
                let direction = [p.to[0] - p.from[0], p.to[1] - p.from[1]];
                let length = (direction[0] * direction[0] + direction[1] * direction[1])
                    .sqrt()
                    .max(0.001);
                self.events.push(Event::BlockedShot { position: impact });
                self.spent_projectiles.insert(
                    id,
                    SpentProjectile {
                        id,
                        item: p.item,
                        from: impact,
                        to: [
                            impact[0] - direction[0] / length * 0.5,
                            impact[1] - direction[1] / length * 0.5,
                        ],
                        born: self.tick,
                        arrival: self.tick + 12,
                    },
                );
            }
        }
        let arrived: Vec<_> = self
            .projectiles
            .values()
            .filter(|p| p.arrival <= self.tick)
            .map(|p| p.id)
            .collect();
        for id in arrived {
            let p = self.projectiles.remove(&id).unwrap();
            let Some(target_id) = p.target else {
                continue;
            };
            if let Some(target) = self.threats.get_mut(&target_id) {
                let impact = match p.kind {
                    ProjectileKind::Arrow if target.armor > 0 => {
                        target.armor -= 1;
                        self.metrics.deflections += 1;
                        if target.armor == 0 {
                            self.metrics.armor_breaks += 1;
                        }
                        Impact::Deflect
                    }
                    ProjectileKind::Arrow => {
                        target.hp = target.hp.saturating_sub(p.damage);
                        self.metrics.bow_hits += 1;
                        Impact::Arrow
                    }
                    ProjectileKind::Bolt
                        if (self.metrics.bolt_impales + self.metrics.bolt_pierces) % 3 == 2 =>
                    {
                        target.hp = target.hp.saturating_sub(1).max(1);
                        target.pierced = true;
                        target.stagger_until = self.tick + 42;
                        self.metrics.bolt_pierces += 1;
                        Impact::BoltPierce
                    }
                    ProjectileKind::Bolt => {
                        target.hp = 0;
                        self.metrics.bolt_impales += 1;
                        Impact::BoltImpale
                    }
                };
                let killed = target.hp == 0;
                let position = target.position;
                let wave = target.wave;
                let heading = (position[0] - p.from[0]).atan2(position[1] - p.from[1]);
                if matches!(impact, Impact::BoltPierce | Impact::Deflect) {
                    let side = if impact == Impact::Deflect { 1.2 } else { 0.0 };
                    self.spent_projectiles.insert(
                        p.id,
                        SpentProjectile {
                            id: p.id,
                            item: p.item.clone(),
                            from: position,
                            to: [
                                position[0] + heading.sin() * 2.3 + heading.cos() * side,
                                position[1] + heading.cos() * 2.3 - heading.sin() * side,
                            ],
                            born: self.tick,
                            arrival: self.tick + 18,
                        },
                    );
                }
                self.events.push(Event::Hit {
                    target: target_id,
                    projectile: Some(p.id),
                    position,
                    killed,
                    impact,
                });
                if killed {
                    self.threats.remove(&target_id);
                    self.metrics.kills += 1;
                    if impact == Impact::BoltImpale {
                        self.pinned.insert(
                            target_id,
                            Pinned {
                                target: target_id,
                                wave,
                                position,
                                heading,
                                born: self.tick,
                                land: self.tick + 36,
                            },
                        );
                    }
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
                    armor: w.armor,
                    pierced: false,
                    shocked_until: 0,
                    route,
                    stagger_until: 0,
                    knockback: [0.0, 0.0],
                },
            );
            self.wave_totals[i] += 1;
        }
        let mut breached = vec![];
        for threat in self.threats.values_mut() {
            let w = &self.config.waves[threat.wave];
            let previous = threat.position;
            let recoiling = self.tick < threat.stagger_until;
            if self.tick < threat.shocked_until {
                // The coil freezes the whole body, then the normal route resumes.
            } else if self.tick < threat.stagger_until {
                let next = [
                    threat.position[0] + threat.knockback[0] / HZ as f32,
                    threat.position[1] + threat.knockback[1] / HZ as f32,
                ];
                if navigation::clear_with_bounds(
                    threat.position,
                    next,
                    &self.obstacles,
                    0.2,
                    self.config.extent,
                ) {
                    threat.position = next;
                    self.metrics.knockback_meters += navigation::distance(previous, next) as f64;
                }
                threat.knockback[0] *= 0.85;
                threat.knockback[1] *= 0.85;
            } else {
                let wounded_speed = if threat.hp < w.hp { 0.55 } else { 1.0 };
                move_route(
                    &mut threat.position,
                    &mut threat.route,
                    w.speed * wounded_speed / HZ as f32,
                );
            }
            let moved = navigation::distance(previous, threat.position);
            threat.travelled += moved;
            if moved > 0.0 && !recoiling {
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
            if let Some(t) = projectile.target.and_then(|id| self.threats.get(&id)) {
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
                            .filter(|p| p.target == Some(t.id))
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
                        target: Some(target),
                        item: d.ammo.clone(),
                        from: d.position,
                        launch_height: d.muzzle_height,
                        to: position,
                        born: self.tick,
                        arrival: self.tick
                            + ticks(
                                navigation::distance(d.position, position) / d.projectile_speed,
                            ),
                        damage: d.damage,
                        kind: d.projectile,
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
pub(super) fn segment_disc(a: Point, b: Point, center: Point, radius: f32) -> Option<f32> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let f = [a[0] - center[0], a[1] - center[1]];
    let aa = d[0] * d[0] + d[1] * d[1];
    if aa < 1e-8 {
        return None;
    }
    let bb = 2.0 * (f[0] * d[0] + f[1] * d[1]);
    let cc = f[0] * f[0] + f[1] * f[1] - radius * radius;
    if cc <= 0.0 {
        return Some(0.0);
    }
    let disc = bb * bb - 4.0 * aa * cc;
    if disc < 0.0 {
        return None;
    }
    let t = (-bb - disc.sqrt()) / (2.0 * aa);
    (0.0..=1.0).contains(&t).then_some(t)
}
