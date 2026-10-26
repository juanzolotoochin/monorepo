//! Combat: enemy waves, defenders, projectiles and the player's weapons,
//! ported from the original game.
//!
//! - Threats move themselves in the parallel self update.
//! - The read phase decides: projectile collisions, breaches, defender shots,
//!   and which threats a player's strike reaches. These become messages.
//! - The apply phase applies damage against the threat's current armor and
//!   health, so two hits in one tick stack correctly in message order.
//! - Waves spawn, spent arrows fade and pinned bodies land in housekeeping.

use content::scenario::{ticks_from_60hz, ProjectileKind, Weapon, WeaponTuning};
use content::TICKS_PER_SECOND;
use fx::{Fx, FxVec2};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::{AgentIx, DepotIx, ItemIx, Settlement};
use crate::messages::{Breach, Fire, Messages, ProjectileBlocked, ProjectileExpired, ProjectileHit, Strike};
use crate::nav::{self, distance};
use crate::terrain::{segment_disc, Terrain};

/// Knockback decays by 0.85 per original (60 Hz) tick.
const KNOCKBACK_DECAY: Fx = match TICKS_PER_SECOND {
    30 => Fx::from_bits(3_103_113_871), // 0.85²
    60 => Fx::from_bits(3_650_722_202), // 0.85
    _ => panic!("add KNOCKBACK_DECAY for this tick rate"),
};
const SPENT_TICKS: u64 = ticks_from_60hz(12);
const DEFLECTED_TICKS: u64 = ticks_from_60hz(18);
const PINNED_TICKS: u64 = ticks_from_60hz(36);
const STAGGER_ON_PIERCE: u64 = ticks_from_60hz(42);

/// A stable counter-based roll, as in the original game: outcomes never
/// depend on how many unrelated hits happened elsewhere.
pub fn combat_roll(tick: u64, entity: u64, stream: u64) -> u64 {
    let mut x = tick ^ entity.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ stream;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveState {
    pub id: String,
    /// The enemy it sends.
    pub character: content::character::CharacterIx,
    pub position: FxVec2,
    pub target: FxVec2,
    pub first: u64,
    pub every: u64,
    /// Meters per tick.
    pub speed: Fx,
    pub hp: u32,
    pub armor: u32,
    pub damage: u32,
    pub count: u32,
    pub spawned: u32,
    /// Every enemy of a wave follows the same planned route.
    pub route: Option<Vec<FxVec2>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Threat {
    pub id: u64,
    pub wave: u16,
    pub position: FxVec2,
    pub heading: Fx,
    pub travelled: Fx,
    pub hp: u32,
    pub armor: u32,
    pub pierced: bool,
    pub shocked_until: u64,
    pub route: Vec<FxVec2>,
    pub stagger_until: u64,
    /// Meters per second while staggered.
    pub knockback: FxVec2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefenderState {
    pub id: String,
    pub position: FxVec2,
    pub radius: Fx,
    pub depot: DepotIx,
    pub ammo: ItemIx,
    pub range: Fx,
    pub period: u64,
    pub damage: u32,
    /// Projectile speed, meters per second.
    pub speed: Fx,
    pub muzzle_height: Fx,
    pub kind: ProjectileKind,
    pub enabled: bool,
    pub ready_at: u64,
    pub aim: Option<FxVec2>,
    pub fired_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Projectile {
    pub id: u64,
    /// `None` for the player's arrows.
    pub defender: Option<u16>,
    /// A hint for rendering and overkill checks; projectiles never home.
    pub target: Option<u64>,
    /// `None` for the player's arrows, which aren't stock.
    pub item: Option<ItemIx>,
    pub from: FxVec2,
    pub launch_height: Fx,
    pub to: FxVec2,
    pub born: u64,
    pub arrival: u64,
    pub damage: u32,
    pub kind: ProjectileKind,
}

impl Projectile {
    /// The segment flown during `tick`.
    fn segment(&self, tick: u64) -> (FxVec2, FxVec2) {
        let duration = Fx::from_int((self.arrival - self.born).max(1) as i32);
        let at = |t: u64| {
            let f = (Fx::from_int(t.saturating_sub(self.born) as i32) / duration).clamp(Fx::ZERO, Fx::ONE);
            self.from + (self.to - self.from) * f
        };
        (at(tick - 1), at(tick))
    }
}

/// A projectile after it stopped: deflected, blocked or through a body. Visual only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpentProjectile {
    pub id: u64,
    pub item: Option<ItemIx>,
    pub from: FxVec2,
    pub to: FxVec2,
    pub born: u64,
    pub arrival: u64,
}

/// A body carried by a ballista bolt, sliding to rest. Visual only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pinned {
    pub target: u64,
    pub wave: u16,
    pub position: FxVec2,
    pub heading: Fx,
    pub born: u64,
    pub land: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Impact {
    Sword,
    Decapitate,
    Spear,
    Arrow,
    Deflect,
    Arc,
    Laser,
    BoltImpale,
    BoltPierce,
}

/// What happened this tick, for sounds and effects. Not part of the state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Shot { defender: Option<u16> },
    /// `agent` is the avatar that struck, for melee and arc hits.
    Hit { target: u64, agent: Option<AgentIx>, projectile: Option<u64>, position: FxVec2, killed: bool, impact: Impact },
    PinnedLanded { target: u64, position: FxVec2 },
    BlockedShot { position: FxVec2 },
    ArcFired { from: FxVec2, to: FxVec2 },
    LaserFired { agent: AgentIx, from: FxVec2, to: FxVec2, hit_actor: bool, cover: Option<u16> },
    Breach { position: FxVec2 },
    Produced { producer: u16 },
    Equipped { agent: AgentIx },
    /// A pile-lab unit left `from` and lands at `to` after `ticks`.
    PileToss { material: u8, from: TossEnd, to: TossEnd, ticks: u64 },
}

/// One end of a pile-lab toss: a unit slot in a pile, or in the carrier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TossEnd {
    Pile { id: u32, slot: u32, position: FxVec2 },
    /// A slot in an avatar's backpack.
    Cargo { agent: AgentIx, slot: u32 },
    /// The floor of a pit, where dug ore comes from.
    Pit { position: FxVec2 },
    /// The collector's hopper.
    Collector { position: FxVec2 },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatMetrics {
    pub shots: u64,
    pub kills: u64,
    pub melee_hits: u64,
    pub melee_kills: u64,
    pub bow_hits: u64,
    pub spear_hits: u64,
    pub arc_hits: u64,
    pub decapitations: u64,
    pub deflections: u64,
    pub armor_breaks: u64,
    pub bolt_impales: u64,
    pub bolt_pierces: u64,
    pub breaches: u64,
    pub knockback_meters: Fx,
}

impl Settlement {
    fn laser_clear_ground(terrain: &Terrain, from: FxVec2, from_height: Fx, to: FxVec2, to_height: Fx) -> bool {
        let distance = (to - from).length();
        let steps = (distance / Fx::from_ratio(1, 4)).ceil().floor_to_int().max(1);
        for step in 1..steps {
            let fraction = Fx::from_ratio(step, steps);
            let point = from + (to - from) * fraction;
            let beam = from_height + (to_height - from_height) * fraction + Fx::from_ratio(4, 5);
            if terrain.ground(point).is_some_and(|ground| ground > beam) {
                return false;
            }
        }
        true
    }

    fn laser_ground_block(terrain: &Terrain, from: FxVec2, forward: FxVec2, reach: Fx, height: Fx) -> Fx {
        let steps = (reach / Fx::from_ratio(1, 4)).ceil().floor_to_int().max(1);
        for step in 1..=steps {
            let fraction = Fx::from_ratio(step, steps);
            if terrain.ground(from + forward * (reach * fraction))
                .is_some_and(|ground| ground > height + Fx::from_ratio(4, 5)) {
                return fraction;
            }
        }
        Fx::ONE
    }

    fn laser_player(&self, shooter: usize, origin: FxVec2, forward: FxVec2, reach: Fx, terrain: &Terrain) -> Option<(Fx, usize)> {
        let end = origin + forward * reach;
        let from_height = self.agents.get(shooter).map_or(Fx::ZERO, |a| a.avatar.height);
        self.agents.iter().enumerate().filter_map(|(index, agent)| {
            if index == shooter || !agent.controlled || agent.avatar.health == 0 || agent.avatar.max_health == 0 {
                return None;
            }
            let radius = super::avatar::character_of(&self.characters, agent).body.radius;
            let fraction = segment_disc(origin, end, agent.position, radius)?;
            (terrain.clear(origin, agent.position, Fx::from_ratio(1, 50))
                && Self::laser_clear_ground(terrain, origin, from_height, agent.position, agent.avatar.height))
                .then_some((fraction, index))
        }).min_by_key(|(fraction, index)| (*fraction, *index))
    }

    pub fn tuning(&self, weapon: Weapon) -> &WeaponTuning {
        &self.tuning[weapon as usize]
    }

    /// Whether the scenario offers this weapon. Scenes without weapons still
    /// allow the sword, as in the original game.
    pub fn has_weapon(&self, weapon: Weapon) -> bool {
        self.weapons.contains(&weapon) || (self.weapons.is_empty() && weapon == Weapon::Sword)
    }

    /// A threat's collision radius: its character's armor radius while
    /// plating is left, its body radius once it breaks.
    pub(super) fn threat_radius(&self, t: &Threat) -> Fx {
        let def = &self.characters[self.waves[t.wave as usize].character as usize];
        match def.health.as_ref().and_then(|h| h.armor.as_ref()) {
            Some(armor) if t.armor > 0 => armor.radius,
            _ => def.body.radius,
        }
    }

    pub fn threat(&self, id: u64) -> Option<&Threat> {
        self.threats.binary_search_by_key(&id, |t| t.id).ok().map(|i| &self.threats[i])
    }

    fn threat_index(&self, id: u64) -> Option<usize> {
        self.threats.binary_search_by_key(&id, |t| t.id).ok()
    }

    /// Stage 2: every threat moves itself.
    pub(crate) fn threats_self_update(&mut self, tick: u64, terrain: &Terrain) {
        let waves = &self.waves;
        let knockback_moved: Fx = self
            .threats
            .par_iter_mut()
            .map(|t| {
                let wave = &waves[t.wave as usize];
                let previous = t.position;
                let recoiling = tick < t.stagger_until;
                let mut knocked = Fx::ZERO;
                if tick < t.shocked_until {
                    // The coil freezes the whole body, then the route resumes.
                } else if recoiling {
                    let next = t.position + t.knockback.div_int(content::TICKS_PER_SECOND as i64);
                    if terrain.clear(t.position, next, Fx::from_ratio(1, 5)) {
                        t.position = next;
                        knocked = distance(previous, next);
                    }
                    t.knockback = t.knockback * KNOCKBACK_DECAY;
                } else {
                    let wounded = if t.hp < wave.hp { Fx::from_ratio(11, 20) } else { Fx::ONE };
                    super::haulers::move_route(&mut t.position, &mut t.route, wave.speed * wounded);
                }
                let moved = distance(previous, t.position);
                t.travelled += moved;
                if moved > Fx::ZERO && !recoiling {
                    t.heading = nav::bearing(previous, t.position);
                }
                knocked
            })
            .reduce(|| Fx::ZERO, |a, b| a + b);
        self.combat.knockback_meters += knockback_moved;
    }

    /// Stage 3: collisions, breaches, shots and strikes, each filed with a position.
    pub(crate) fn read_combat(&self, tick: u64, terrain: &Terrain) -> Vec<(FxVec2, Messages)> {
        let mut result: Vec<(FxVec2, Messages)> = self
            .projectiles
            .par_iter()
            .filter(|p| p.born < tick)
            .map(|p| {
                let mut out = Messages::default();
                self.fly(p, tick, terrain, &mut out);
                (p.from, out)
            })
            .collect();
        for t in &self.threats {
            if t.route.is_empty() {
                let mut out = Messages::default();
                out.breach.push(Breach { threat: t.id });
                result.push((t.position, out));
            }
        }
        for (i, d) in self.defenders.iter().enumerate() {
            let mut out = Messages::default();
            self.aim_defender(i as u16, d, tick, &mut out);
            result.push((d.position, out));
        }
        for (i, a) in self.agents.iter().enumerate() {
            if a.controlled {
                let mut out = Messages::default();
                self.read_avatar(i as AgentIx, a, tick, terrain, &mut out);
                result.push((a.position, out));
            }
        }
        result
    }

    fn fly(&self, p: &Projectile, tick: u64, terrain: &Terrain, out: &mut Messages) {
        let (start, end) = p.segment(tick);
        let own = p.defender.map(|d| self.defenders[d as usize].position);
        let margin = Fx::from_ratio(1, 25);
        let static_obstacle = terrain
            .obstacles.iter()
            .filter(|o| own != Some(o.center))
            .filter_map(|o| segment_disc(start, end, o.center, o.radius + margin))
            .min();
        let cover = terrain.destructibles.iter().enumerate()
            .filter(|(_, piece)| piece.health > 0 && own != Some(piece.disc.center))
            .filter_map(|(index, piece)| segment_disc(start, end, piece.disc.center,
                piece.disc.radius + margin).map(|fraction| (fraction, index as u16)))
            .min();
        let obstacle = static_obstacle.into_iter().map(|f| (f, None))
            .chain(cover.map(|(f, index)| (f, Some(index)))).min();
        let threat = self
            .threats
            .iter()
            .filter_map(|t| segment_disc(start, end, t.position, self.threat_radius(t)).map(|f| (f, t.id)))
            .min();
        match (threat, obstacle) {
            (Some((f, target)), rock) if rock.is_none_or(|rock| f < rock.0) => {
                out.projectile_hit.push(ProjectileHit { projectile: p.id, target });
            }
            (_, Some((f, cover))) => {
                out.projectile_blocked.push(ProjectileBlocked { projectile: p.id, at: start + (end - start) * f, cover });
            }
            _ if p.arrival <= tick => out.projectile_expired.push(ProjectileExpired { projectile: p.id }),
            _ => {}
        }
    }

    fn aim_defender(&self, index: u16, d: &DefenderState, tick: u64, out: &mut Messages) {
        if !d.enabled || d.ready_at > tick || self.depots[d.depot as usize].available(d.ammo) == 0 {
            return;
        }
        let target = self
            .threats
            .iter()
            .filter(|t| {
                let incoming: u32 = self.projectiles.iter().filter(|p| p.target == Some(t.id)).map(|p| p.damage).sum();
                distance(d.position, t.position) <= d.range && incoming < t.hp
            })
            .min_by_key(|t| (distance(d.position, t.position), t.id));
        if let Some(t) = target {
            out.fire.push(Fire { defender: index, target: t.id, to: t.position });
        }
    }

    /// The targets of a resolving strike, in the order they take damage.
    pub(super) fn strike_targets(&self, shooter: AgentIx, origin: FxVec2, forward: FxVec2, weapon: Weapon, terrain: &Terrain) -> (Vec<u64>, Option<FxVec2>, Option<u16>) {
        let tuning = self.tuning(weapon);
        let (reach, cone) = (tuning.reach, tuning.cone);
        let close = Fx::from_ratio(1, 5);
        let mut targets: Vec<(Fx, u64)> = self
            .threats
            .iter()
            .filter_map(|t| {
                let delta = t.position - origin;
                let d = delta.length();
                let in_cone = d < close || delta.dot(forward) / d > cone;
                let visible = weapon == Weapon::Bow || terrain.clear(origin, t.position, Fx::from_ratio(1, 50));
                (d <= reach && in_cone && visible).then_some((d, t.id))
            })
            .collect();
        targets.sort();
        let mut beam = None;
        let mut cover = None;
        match weapon {
            Weapon::ArcCoil => {
                let end = origin + forward * reach;
                let blocked = terrain
                    .blocking_obstacles()
                    .filter_map(|o| segment_disc(origin, end, o.center, o.radius + Fx::from_ratio(1, 25)))
                    .min()
                    .unwrap_or(Fx::ONE);
                let lane = Fx::from_ratio(21, 50);
                targets.retain(|&(_, id)| {
                    let t = self.threat(id).expect("listed threat");
                    let delta = t.position - origin;
                    let along = delta.dot(forward);
                    let sideways = delta.perp_dot(forward).abs();
                    along >= Fx::ZERO && along <= reach * blocked && sideways <= lane
                });
                beam = Some(match targets.first() {
                    Some(&(_, id)) => self.threat(id).expect("listed threat").position,
                    None => origin + forward * (reach * blocked),
                });
                if let Some(&(_, first)) = targets.first() {
                    let anchor = self.threat(first).expect("listed threat").position;
                    let mut chain: Vec<(Fx, u64)> = self
                        .threats
                        .iter()
                        .filter(|t| t.id != first && distance(t.position, anchor) <= Fx::from_ratio(11, 5))
                        .map(|t| (distance(t.position, anchor), t.id))
                        .collect();
                    chain.sort();
                    targets = std::iter::once((Fx::ZERO, first)).chain(chain.into_iter().take(2)).collect();
                }
            }
            Weapon::Spear => targets.truncate(2),
            Weapon::Bow => targets.clear(),
            Weapon::Laser => {
                let end = origin + forward * reach;
                let static_block = terrain.obstacles.iter()
                    .filter_map(|o| segment_disc(origin, end, o.center, o.radius + Fx::from_ratio(1, 25)))
                    .min().unwrap_or(Fx::ONE);
                let cover_block = terrain.destructibles.iter().enumerate()
                    .filter(|(_, piece)| piece.health > 0)
                    .filter_map(|(index, piece)| segment_disc(origin, end, piece.disc.center,
                        piece.disc.radius + Fx::from_ratio(1, 25)).map(|fraction| (fraction, index)))
                    .min();
                let ground_block = Self::laser_ground_block(terrain, origin, forward, reach, self.agents[shooter as usize].avatar.height);
                let blocked = static_block.min(cover_block.map_or(Fx::ONE, |(fraction, _)| fraction)).min(ground_block);
                let player = self.laser_player(shooter as usize, origin, forward, reach, terrain)
                    .filter(|(fraction, _)| *fraction < blocked);
                let threat = targets.iter().filter_map(|&(_, id)| {
                    let position = self.threat(id)?.position;
                    segment_disc(origin, end, position, Fx::from_ratio(3, 10))
                        .filter(|fraction| *fraction < blocked && Self::laser_clear_ground(terrain, origin, self.agents[shooter as usize].avatar.height, position, terrain.ground(position).unwrap_or_default()))
                        .map(|fraction| (fraction, id, position))
                }).min_by_key(|(fraction, id, _)| (*fraction, *id));
                if let Some((_, id, position)) = threat.filter(|(fraction, _, _)| player.is_none_or(|(p, _)| *fraction <= p)) {
                    targets.retain(|&(_, target)| target == id);
                    beam = Some(position);
                } else {
                    targets.clear();
                    beam = Some(player.map_or(origin + forward * (reach * blocked), |(_, index)| self.agents[index].position));
                    if player.is_none() {
                        cover = cover_block.and_then(|(fraction, index)|
                            (fraction == blocked && fraction < static_block && fraction < ground_block).then_some(index as u16));
                    }
                }
            }
            Weapon::Sword => {}
        }
        (targets.into_iter().map(|(_, id)| id).collect(), beam, cover)
    }

    /// Applies a resolving strike. Returns how many threats it hit.
    pub(crate) fn strike(&mut self, tick: u64, message: Strike) -> u32 {
        let agent = message.agent as usize;
        if self.agents[agent].attack_ready > tick {
            return 0;
        }
        let tuning = self.tuning(message.weapon).clone();
        self.agents[agent].attack_ready = tick + tuning.cooldown();
        let origin = message.origin;
        let forward = message.forward;
        if message.weapon == Weapon::Bow {
            let right = FxVec2::new(forward.y, -forward.x);
            let from = origin + right * message.muzzle[0] + forward * message.muzzle[2];
            let to = from + forward * tuning.reach;
            let id = self.id();
            let arrival = tick + flight_ticks(tuning.reach, tuning.projectile_speed);
            self.projectiles.push(Projectile {
                id,
                defender: None,
                target: None,
                item: None,
                from,
                launch_height: message.muzzle[1],
                to,
                born: tick,
                arrival,
                damage: tuning.damage,
                kind: ProjectileKind::Arrow,
            });
            self.combat.shots += 1;
            self.events.push(Event::Shot { defender: None });
            return 1;
        }
        if let Some(to) = message.beam_end {
            self.events.push(if message.weapon == Weapon::Laser {
                let hit_actor = !message.targets.is_empty() || self.agents.iter().enumerate().any(|(index, target)| {
                    index != agent && target.controlled && target.avatar.health > 0 && target.position == to
                });
                Event::LaserFired { agent: message.agent, from: origin, to, hit_actor, cover: message.cover }
            } else {
                Event::ArcFired { from: origin, to }
            });
        }
        if message.weapon == Weapon::Laser && message.targets.is_empty() && self.player_health > 0 {
            if let Some(to) = message.beam_end {
                if let Some((_, target)) = self.agents.iter_mut().enumerate().find(|(index, target)| {
                    *index != agent && target.controlled && target.avatar.health > 0 && target.position == to
                }) {
                    target.avatar.health = target.avatar.health.saturating_sub(tuning.damage);
                    if target.avatar.health == 0 {
                        target.avatar.respawn_at = tick + ticks_from_60hz(120);
                        target.avatar.swing = None;
                        target.avatar.queued = None;
                        target.avatar.velocity = FxVec2::ZERO;
                    }
                    return 1;
                }
            }
        }
        let mut hits = 0;
        for (index, target) in message.targets.into_iter().enumerate() {
            let Some(i) = self.threat_index(target) else { continue };
            let t = &mut self.threats[i];
            let armored = t.armor > 0;
            let mut impact = match message.weapon {
                Weapon::Sword => Impact::Sword,
                Weapon::Spear => Impact::Spear,
                Weapon::Laser => Impact::Laser,
                _ => Impact::Arc,
            };
            let damage = match message.weapon {
                Weapon::Sword if armored => {
                    t.armor -= 1;
                    impact = Impact::Deflect;
                    0
                }
                Weapon::Sword => {
                    if tuning.critical_one_in > 0 && combat_roll(tick, target, 0x53574f5244) % tuning.critical_one_in == 0 {
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
                Weapon::Spear => tuning.damage.saturating_sub(index as u32).max(1),
                _ => tuning.damage + armored as u32,
            };
            if armored && t.armor == 0 {
                self.combat.armor_breaks += 1;
            }
            match impact {
                Impact::Deflect => self.combat.deflections += 1,
                Impact::Decapitate => self.combat.decapitations += 1,
                _ => {}
            }
            t.hp = t.hp.saturating_sub(damage);
            let away = (t.position - origin).normalize_or_zero();
            t.knockback = away * tuning.knockback;
            t.stagger_until = tick + tuning.stagger();
            match message.weapon {
                Weapon::ArcCoil => {
                    t.shocked_until = t.stagger_until;
                    self.combat.arc_hits += 1;
                }
                Weapon::Spear => self.combat.spear_hits += 1,
                _ => self.combat.melee_hits += 1,
            }
            hits += 1;
            let (killed, position) = (t.hp == 0, t.position);
            self.events.push(Event::Hit { target, agent: Some(agent as AgentIx), projectile: None, position, killed, impact });
            if killed {
                self.threats.remove(i);
                self.combat.kills += 1;
                if message.weapon == Weapon::Sword {
                    self.combat.melee_kills += 1;
                }
            }
        }
        hits
    }

    pub(crate) fn projectile_hit(&mut self, tick: u64, message: ProjectileHit) {
        let Some(at) = self.projectiles.iter().position(|p| p.id == message.projectile) else { return };
        let p = self.projectiles.remove(at);
        let Some(i) = self.threat_index(message.target) else { return };
        let t = &mut self.threats[i];
        let impact = match p.kind {
            ProjectileKind::Arrow if t.armor > 0 => {
                t.armor -= 1;
                self.combat.deflections += 1;
                if t.armor == 0 {
                    self.combat.armor_breaks += 1;
                }
                Impact::Deflect
            }
            ProjectileKind::Arrow => {
                t.hp = t.hp.saturating_sub(p.damage);
                self.combat.bow_hits += 1;
                Impact::Arrow
            }
            ProjectileKind::Bolt if combat_roll(p.born, message.target, p.id ^ 0x424f4c5b) % 3 == 0 => {
                t.hp = t.hp.saturating_sub(1).max(1);
                t.pierced = true;
                t.stagger_until = tick + STAGGER_ON_PIERCE;
                self.combat.bolt_pierces += 1;
                Impact::BoltPierce
            }
            ProjectileKind::Bolt => {
                t.hp = 0;
                self.combat.bolt_impales += 1;
                Impact::BoltImpale
            }
        };
        let (killed, position, wave) = (t.hp == 0, t.position, t.wave);
        let heading = nav::bearing(p.from, position);
        if matches!(impact, Impact::BoltPierce | Impact::Deflect) {
            let side = if impact == Impact::Deflect { Fx::from_ratio(6, 5) } else { Fx::ZERO };
            let ahead = nav::heading_vector(heading);
            let right = FxVec2::new(ahead.y, -ahead.x);
            self.spent.push(SpentProjectile {
                id: p.id,
                item: p.item,
                from: position,
                to: position + ahead * Fx::from_ratio(23, 10) + right * side,
                born: tick,
                arrival: tick + DEFLECTED_TICKS,
            });
        }
        self.events.push(Event::Hit { target: message.target, agent: None, projectile: Some(p.id), position, killed, impact });
        if killed {
            self.threats.remove(i);
            self.combat.kills += 1;
            if impact == Impact::BoltImpale {
                self.pinned.push(Pinned { target: message.target, wave, position, heading, born: tick, land: tick + PINNED_TICKS });
            }
        }
    }

    pub(crate) fn projectile_blocked(&mut self, tick: u64, message: ProjectileBlocked) {
        let Some(at) = self.projectiles.iter().position(|p| p.id == message.projectile) else { return };
        let p = self.projectiles.remove(at);
        let back = (p.to - p.from).normalize_or_zero() * Fx::HALF;
        self.events.push(Event::BlockedShot { position: message.at });
        self.spent.push(SpentProjectile { id: p.id, item: p.item, from: message.at, to: message.at - back, born: tick, arrival: tick + SPENT_TICKS });
    }

    pub(crate) fn projectile_expired(&mut self, message: ProjectileExpired) {
        self.projectiles.retain(|p| p.id != message.projectile);
    }

    pub(crate) fn breach(&mut self, message: Breach) {
        let Some(i) = self.threat_index(message.threat) else { return };
        let t = self.threats.remove(i);
        self.health = self.health.saturating_sub(self.waves[t.wave as usize].damage);
        self.combat.breaches += 1;
        self.events.push(Event::Breach { position: t.position });
    }

    pub(crate) fn fire(&mut self, tick: u64, message: Fire) {
        let d = &self.defenders[message.defender as usize];
        let (depot, ammo) = (d.depot as usize, d.ammo);
        if !d.enabled || d.ready_at > tick || self.depots[depot].available(ammo) == 0 {
            return;
        }
        let (from, launch_height, damage, kind, period) = (d.position, d.muzzle_height, d.damage, d.kind, d.period);
        let arrival = tick + flight_ticks(distance(from, message.to), d.speed);
        self.depots[depot].stock[ammo as usize] -= 1;
        self.accounts[ammo as usize].spent += 1;
        self.combat.shots += 1;
        let id = self.id();
        self.projectiles.push(Projectile {
            id,
            defender: Some(message.defender),
            target: Some(message.target),
            item: Some(ammo),
            from,
            launch_height,
            to: message.to,
            born: tick,
            arrival,
            damage,
            kind,
        });
        let d = &mut self.defenders[message.defender as usize];
        d.aim = Some(message.to);
        d.ready_at = tick + period;
        d.fired_at = tick;
        self.events.push(Event::Shot { defender: Some(message.defender) });
    }

    /// Housekeeping: fading arrows, landing bodies, new enemies.
    pub(crate) fn combat_housekeeping(&mut self, tick: u64, terrain: &Terrain) {
        self.spent.retain(|p| p.arrival > tick);
        let (landed, flying): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pinned).into_iter().partition(|p| p.land <= tick);
        self.pinned = flying;
        for p in landed {
            self.events.push(Event::PinnedLanded { target: p.target, position: p.position });
        }
        for p in &mut self.pinned {
            let f = Fx::from_int((tick - p.born) as i32) / Fx::from_int((p.land - p.born) as i32);
            p.position += nav::heading_vector(p.heading) * ((Fx::ONE - f.min(Fx::ONE)) * Fx::from_ratio(19, 100));
        }
        for w in 0..self.waves.len() {
            let wave = &self.waves[w];
            if tick < wave.first || wave.spawned >= wave.count || (tick - wave.first) % wave.every != 0 {
                continue;
            }
            if wave.route.is_none() {
                let route = nav::path(terrain, wave.position, wave.target, Fx::from_ratio(1, 5));
                self.waves[w].route = Some(route);
            }
            let wave = &self.waves[w];
            let route = wave.route.clone().expect("planned");
            if route.is_empty() {
                continue;
            }
            let (position, hp, armor) = (wave.position, wave.hp, wave.armor);
            let id = self.id();
            self.threats.push(Threat {
                id,
                wave: w as u16,
                position,
                heading: Fx::ZERO,
                travelled: Fx::ZERO,
                hp,
                armor,
                pierced: false,
                shocked_until: 0,
                route,
                stagger_until: 0,
                knockback: FxVec2::ZERO,
            });
            self.waves[w].spawned += 1;
        }
    }

    /// Commands: a player switches a defender on or off.
    pub fn set_defender(&mut self, defender: usize, enabled: bool) {
        if let Some(d) = self.defenders.get_mut(defender) {
            d.enabled = enabled;
        }
    }
}

/// Ticks to cover `distance` at `speed` meters per second, at least one.
fn flight_ticks(distance: Fx, speed: Fx) -> u64 {
    content::scenario::ticks(distance / speed)
}
