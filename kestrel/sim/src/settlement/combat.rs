//! Combat: enemy waves, defenders, projectiles and the player's weapons,
//! ported from the original game.
//!
//! - Threats move themselves in the parallel self update.
//! - The read phase decides: projectile collisions, breaches, defender shots,
//!   and which threats a player's strike reaches. These become messages.
//! - The apply phase applies damage against the threat's current armor and
//!   health, so two hits in one tick stack correctly in message order.
//! - Threats with an `attack` stop to swing at players in reach: stage 3
//!   finds the swings that start or land, stage 5 hurts and pushes players.
//! - Waves spawn, spent arrows fade and pinned bodies land in housekeeping.

use content::character::AttackDef;
use content::scenario::{ticks_from_60hz, ProjectileKind, Weapon, WeaponTuning};
use content::TICKS_PER_SECOND;
use fx::{Fx, FxVec2};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::{AgentIx, DepotIx, ItemIx, Settlement};
use crate::messages::{Breach, Fire, Messages, ProjectileBlocked, ProjectileExpired, ProjectileHit, StartSwing, Strike, SwingLands};
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
/// A threat's swing reaches a player whose feet are at most this high
/// above the ground under it, in meters. A jump (apex about 0.57 m) doesn't
/// dodge it; the jetpack's hover (1.8 m) does. Every attacking character is
/// a ghoul of one height, so it is a rule, not a character field.
pub const SWING_HEIGHT: Fx = Fx::ONE;

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
    /// A swing at a player being wound up (the character's `attack`). The
    /// threat stands still meanwhile; a stagger or shock cancels it.
    pub windup: Option<Windup>,
    /// The first tick a new windup can start: the tick after its last swing
    /// landed plus the attack's cooldown.
    pub ready_at: u64,
}

/// A threat's swing on its way (`Threat::windup`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Windup {
    /// The player it swings at.
    pub agent: AgentIx,
    /// The tick the swing lands: stage 3 checks reach then, against where
    /// the player is on that tick.
    pub lands: u64,
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
    /// A defender fired.
    Shot { defender: u16 },
    /// An avatar loosed an arrow. `started` is the tick its `Attack`
    /// applied on (`Swing::pressed`), as on the other strike events.
    BowFired { agent: AgentIx, started: u64 },
    /// `agent` is the avatar that struck, for melee and arc hits.
    Hit { target: u64, agent: Option<AgentIx>, projectile: Option<u64>, position: FxVec2, killed: bool, impact: Impact },
    PinnedLanded { target: u64, position: FxVec2 },
    BlockedShot { position: FxVec2 },
    ArcFired { agent: AgentIx, from: FxVec2, to: FxVec2, started: u64 },
    LaserFired { agent: AgentIx, from: FxVec2, to: FxVec2, hit_actor: bool, cover: Option<u16>, started: u64 },
    Breach { position: FxVec2 },
    Produced { producer: u16 },
    Equipped { agent: AgentIx },
    /// A pile-lab unit left `from` and lands at `to` after `ticks`.
    PileToss { material: u8, from: TossEnd, to: TossEnd, ticks: u64 },
    /// A threat started winding up a swing at player `agent`. It lands on
    /// tick `lands` unless a stagger or shock cancels it first, which shows
    /// as the threat's `windup` going away with no `ThreatSwing`.
    ThreatWindup { threat: u64, agent: AgentIx, lands: u64 },
    /// A threat's swing at player `agent` landed. A hit took `damage` health
    /// (`killed` if that was the last) and pushed the player; a miss (`hit`
    /// false, `damage` 0) means the player got out of reach or was dead.
    ThreatSwing { threat: u64, agent: AgentIx, hit: bool, damage: u32, killed: bool },
}

/// What a strike reaches (`Settlement::strike_targets`), as its `Strike`
/// message carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StrikeTargets {
    /// Threats, in the order they take damage.
    pub targets: Vec<u64>,
    /// Where an arc coil's or a laser's beam ends.
    pub beam_end: Option<FxVec2>,
    /// The cover piece a laser stops at.
    pub cover: Option<u16>,
    /// A laser stops at another live player, standing at `beam_end`.
    pub hit_player: bool,
}

/// Whether a client may show an event before its world confirms it
/// (`ABILITY_PREDICTION_HANDOFF.md`, D4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prediction {
    /// Shown when the confirmed world has it: it depends on stage 5 (damage,
    /// deaths, cover breaking, stock moving) or isn't an avatar's.
    Confirmed,
    /// Built from a stage 3 message alone (`strike_event`), so an
    /// `AvatarReplay` predicts it. `agent` and `started`, the tick its
    /// action's command applied on, match it to the confirmed one (D7).
    Predict { agent: AgentIx, started: u64, on_miss: OnMiss },
}

/// What a client does with a predicted event that never confirms, or
/// confirms differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnMiss {
    /// A one-shot (a beam, a sound): leave it, apply what the confirmed
    /// event says, and log it.
    Keep,
    /// It started something lasting: stop it.
    Cancel,
}

impl Event {
    /// This kind's prediction policy. Exactly the events `strike_event`
    /// builds are predicted.
    pub fn prediction(&self) -> Prediction {
        match *self {
            Event::BowFired { agent, started } | Event::ArcFired { agent, started, .. } | Event::LaserFired { agent, started, .. } => {
                Prediction::Predict { agent, started, on_miss: OnMiss::Keep }
            }
            _ => Prediction::Confirmed,
        }
    }
}

/// The event a resolving strike shows, from the message alone: stage 5
/// pushes it as it applies the strike, and `AvatarReplay` predicts it.
/// Sword and spear strikes show nothing until stage 5 says what they hit
/// (`Hit`).
pub(super) fn strike_event(message: &Strike) -> Option<Event> {
    let (agent, started, from) = (message.agent, message.started, message.origin);
    match message.weapon {
        Weapon::Bow => Some(Event::BowFired { agent, started }),
        Weapon::ArcCoil => message.beam_end.map(|to| Event::ArcFired { agent, from, to, started }),
        Weapon::Laser => message.beam_end.map(|to| Event::LaserFired {
            agent,
            from,
            to,
            hit_actor: !message.targets.is_empty() || message.hit_player,
            cover: message.cover,
            started,
        }),
        Weapon::Sword | Weapon::Spear => None,
    }
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

    fn laser_player(&self, shooter: usize, from_height: Fx, origin: FxVec2, forward: FxVec2, reach: Fx, terrain: &Terrain) -> Option<(Fx, usize)> {
        let end = origin + forward * reach;
        self.agents.iter().enumerate().filter_map(|(index, agent)| {
            let avatar = agent.avatar()?;
            if index == shooter || agent.body.health == 0 || agent.body.max_health == 0 {
                return None;
            }
            let radius = super::avatar::character_of(&self.characters, agent).body.radius;
            let fraction = segment_disc(origin, end, agent.body.position, radius)?;
            (terrain.clear(origin, agent.body.position, Fx::from_ratio(1, 50))
                && Self::laser_clear_ground(terrain, origin, from_height, agent.body.position, avatar.height))
                .then_some((fraction, index))
        }).min_by_key(|(fraction, index)| (*fraction, *index))
    }

    pub fn tuning(&self, weapon: Weapon) -> &WeaponTuning {
        &self.tuning[weapon as usize]
    }

    /// Whether the scenario offers this weapon. Scenes without weapons still
    /// allow the sword, as in the original game.
    pub fn has_weapon(&self, weapon: Weapon) -> bool {
        offers_weapon(&self.weapons, weapon)
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
                } else if t.windup.is_some() {
                    // Planted for the swing; the route resumes after it lands.
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
            if let Some(windup) = t.windup.filter(|w| w.lands <= tick) {
                let mut out = Messages::default();
                out.swing_lands.push(SwingLands { threat: t.id, agent: windup.agent, hit: self.swing_reaches(t, windup.agent, terrain) });
                result.push((t.position, out));
            }
        }
        if self.player_health > 0 {
            for (i, a) in self.agents.iter().enumerate() {
                if a.avatar().is_some() && a.body.health > 0 {
                    let mut out = Messages::default();
                    self.threats_in_reach(i as AgentIx, a, tick, terrain, &mut out);
                    if !out.is_empty() {
                        result.push((a.body.position, out));
                    }
                }
            }
        }
        for (i, d) in self.defenders.iter().enumerate() {
            let mut out = Messages::default();
            self.aim_defender(i as u16, d, tick, &mut out);
            result.push((d.position, out));
        }
        for (i, a) in self.agents.iter().enumerate() {
            if a.avatar().is_some() {
                let mut out = Messages::default();
                self.read_avatar(i as AgentIx, a, tick, terrain, &mut out);
                result.push((a.body.position, out));
            }
        }
        result
    }

    /// How a threat hits players, if its character does.
    fn threat_attack(&self, t: &Threat) -> Option<AttackDef> {
        self.characters[self.waves[t.wave as usize].character as usize].attack
    }

    /// Stage 3: whether a threat's swing landing now reaches `agent`, alive,
    /// within the attack's reach of the threat and low enough
    /// (`SWING_HEIGHT`). A player who lifted off during the windup is missed.
    fn swing_reaches(&self, t: &Threat, agent: AgentIx, terrain: &Terrain) -> bool {
        let (Some(attack), Some(a)) = (self.threat_attack(t), self.agents.get(agent as usize)) else { return false };
        a.avatar().is_some() && a.body.health > 0 && distance(t.position, a.body.position) <= attack.reach && self.within_swing_height(a, terrain)
    }

    /// Whether a player's feet are low enough for a threat's swing: at most
    /// `SWING_HEIGHT` above the ground under its body. Over a drop, the
    /// ground it last stood on counts.
    fn within_swing_height(&self, a: &super::AgentState, terrain: &Terrain) -> bool {
        // Only avatars leave the ground.
        let Some(avatar) = a.avatar() else { return true };
        let radius = super::avatar::character_of(&self.characters, a).body.radius;
        let ground = terrain.footprint(a.body.position, radius).unwrap_or(avatar.flight.support);
        avatar.height - ground <= SWING_HEIGHT
    }

    /// Stage 3 for one hurtable player: every threat that could start a
    /// swing at it now. Stage 5 keeps the nearest player for each threat
    /// (`start_swings`). A player too high (`SWING_HEIGHT`) starts none.
    fn threats_in_reach(&self, agent: AgentIx, a: &super::AgentState, tick: u64, terrain: &Terrain, out: &mut Messages) {
        if !self.within_swing_height(a, terrain) {
            return;
        }
        let position = a.body.position;
        for t in &self.threats {
            let Some(attack) = self.threat_attack(t) else { continue };
            if !can_start_swing(t, tick) {
                continue;
            }
            let gap = distance(t.position, position);
            if gap <= attack.reach {
                out.start_swing.push(StartSwing { threat: t.id, agent, distance: gap });
            }
        }
    }

    /// Stage 5: threats start the swings stage 3 found. A threat named by
    /// several takes the nearest player, the lowest agent index on a tie,
    /// whatever order the messages came in. It checks again that nothing
    /// earlier in stage 5 (a strike's stagger, a kill) stopped it.
    pub(crate) fn start_swings(&mut self, tick: u64, mut messages: Vec<StartSwing>) {
        messages.sort_by_key(|m| (m.threat, m.distance, m.agent));
        messages.dedup_by_key(|m| m.threat);
        for message in messages {
            let Some(i) = self.threat_index(message.threat) else { continue };
            let Some(attack) = self.threat_attack(&self.threats[i]) else { continue };
            let Some(target) = self.agents.get(message.agent as usize).map(|a| a.body.position) else { continue };
            let t = &mut self.threats[i];
            if !can_start_swing(t, tick) {
                continue;
            }
            let lands = tick + attack.windup;
            t.windup = Some(Windup { agent: message.agent, lands });
            if target != t.position {
                t.heading = nav::bearing(t.position, target);
            }
            self.events.push(Event::ThreatWindup { threat: t.id, agent: message.agent, lands });
        }
    }

    /// Stage 5: a threat's swing lands. Its windup ends and its cooldown
    /// starts, hit or miss. A hit hurts the player through `hurt_player`
    /// (the laser's path, so a kill releases its transfer) and, if it
    /// lives, pushes it away from the threat: a velocity the avatar's own
    /// movement then brakes out (`walk`). The push ends a dig. An avatar in
    /// its own melee swing takes it as `push`, which moves it as far.
    pub(crate) fn swing_lands(&mut self, tick: u64, message: SwingLands, terrain: &mut Terrain) {
        let Some(i) = self.threat_index(message.threat) else { return };
        let Some(attack) = self.threat_attack(&self.threats[i]) else { return };
        let t = &mut self.threats[i];
        // A stagger or shock earlier in stage 5 cancelled it.
        if t.windup.is_none_or(|w| w.agent != message.agent) {
            return;
        }
        t.windup = None;
        t.ready_at = tick + 1 + attack.cooldown;
        let (threat, from, heading) = (t.id, t.position, t.heading);
        let agent = message.agent as usize;
        // A laser earlier in stage 5 may have killed the player.
        let hit = message.hit && self.player_health > 0 && self.agents[agent].body.health > 0;
        let killed = hit && self.hurt_player(agent, attack.damage, tick, terrain);
        if hit && !killed {
            let super::AgentState { body, role: super::Role::Avatar(avatar), .. } = &mut self.agents[agent] else { unreachable!("threats hit only avatars") };
            let mut away = (body.position - from).normalize_or_zero();
            if away == FxVec2::ZERO {
                away = nav::heading_vector(heading);
            }
            avatar.dig = None;
            let swinging = avatar.swing.as_ref().is_some_and(|s| s.weapon != Weapon::Laser);
            if swinging {
                body.push += away * attack.knockback;
            } else {
                body.velocity += away * attack.knockback;
            }
        }
        let damage = if hit { attack.damage } else { 0 };
        self.events.push(Event::ThreatSwing { threat, agent: message.agent, hit, damage, killed });
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
        if !d.enabled || d.ready_at > tick || self.depots[d.depot as usize].inventory.available(d.ammo) == 0 {
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

    /// What a strike from a shooter at `origin` and `height` reaches. It
    /// reads neither the shooter's own state nor where `self` has it, so
    /// client prediction aims a predicted avatar against the confirmed
    /// settlement with it (`read_avatar`).
    pub(super) fn strike_targets(&self, shooter: AgentIx, height: Fx, origin: FxVec2, forward: FxVec2, weapon: Weapon, terrain: &Terrain) -> StrikeTargets {
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
        let mut hit_player = false;
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
                let ground_block = Self::laser_ground_block(terrain, origin, forward, reach, height);
                let blocked = static_block.min(cover_block.map_or(Fx::ONE, |(fraction, _)| fraction)).min(ground_block);
                let player = self.laser_player(shooter as usize, height, origin, forward, reach, terrain)
                    .filter(|(fraction, _)| *fraction < blocked);
                let threat = targets.iter().filter_map(|&(_, id)| {
                    let position = self.threat(id)?.position;
                    segment_disc(origin, end, position, Fx::from_ratio(3, 10))
                        .filter(|fraction| *fraction < blocked && Self::laser_clear_ground(terrain, origin, height, position, terrain.ground(position).unwrap_or_default()))
                        .map(|fraction| (fraction, id, position))
                }).min_by_key(|(fraction, id, _)| (*fraction, *id));
                if let Some((_, id, position)) = threat.filter(|(fraction, _, _)| player.is_none_or(|(p, _)| *fraction <= p)) {
                    targets.retain(|&(_, target)| target == id);
                    beam = Some(position);
                } else {
                    targets.clear();
                    beam = Some(player.map_or(origin + forward * (reach * blocked), |(_, index)| self.agents[index].body.position));
                    hit_player = player.is_some();
                    if player.is_none() {
                        cover = cover_block.and_then(|(fraction, index)|
                            (fraction == blocked && fraction < static_block && fraction < ground_block).then_some(index as u16));
                    }
                }
            }
            Weapon::Sword => {}
        }
        StrikeTargets { targets: targets.into_iter().map(|(_, id)| id).collect(), beam_end: beam, cover, hit_player }
    }

    /// Applies a resolving strike. Returns how many threats it hit.
    pub(crate) fn strike(&mut self, tick: u64, message: Strike, terrain: &mut Terrain) -> u32 {
        let agent = message.agent as usize;
        let tuning = self.tuning(message.weapon).clone();
        let Some(avatar) = self.agents[agent].avatar_mut() else { return 0 };
        if avatar.attack_ready > tick {
            return 0;
        }
        avatar.attack_ready = tick + tuning.cooldown();
        self.events.extend(strike_event(&message));
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
            return 1;
        }
        if message.weapon == Weapon::Laser && message.targets.is_empty() && self.player_health > 0 {
            if let Some(to) = message.beam_end {
                let hit = self.agents.iter().enumerate().position(|(index, target)| {
                    index != agent && target.avatar().is_some() && target.body.health > 0 && target.body.position == to
                });
                if let Some(target) = hit {
                    self.hurt_player(target, tuning.damage, tick, terrain);
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
            // A stagger or shock cancels a windup.
            t.windup = None;
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
                t.windup = None;
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
        if !d.enabled || d.ready_at > tick || self.depots[depot].inventory.available(ammo) == 0 {
            return;
        }
        let (from, launch_height, damage, kind, period) = (d.position, d.muzzle_height, d.damage, d.kind, d.period);
        let arrival = tick + flight_ticks(distance(from, message.to), d.speed);
        self.depots[depot].inventory.remove(ammo, 1);
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
        self.events.push(Event::Shot { defender: message.defender });
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
                windup: None,
                ready_at: 0,
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

/// Whether a threat may start a swing on `tick`: not already winding up,
/// its cooldown over, neither staggered nor shocked, and still on its route.
fn can_start_swing(t: &Threat, tick: u64) -> bool {
    t.windup.is_none() && tick >= t.ready_at && tick >= t.stagger_until && tick >= t.shocked_until && !t.route.is_empty()
}

/// See `Settlement::has_weapon`.
pub(super) fn offers_weapon(weapons: &[Weapon], weapon: Weapon) -> bool {
    weapons.contains(&weapon) || (weapons.is_empty() && weapon == Weapon::Sword)
}
