//! Player avatars: controlled agents, moved by commands instead of haul links.
//!
//! This is the original game's player controller (which lived in the Bevy
//! app) moved into the simulation, so every peer moves every avatar the same
//! way. A command sets an intent (walk in a direction, go to a point, or
//! stop); each tick the avatar accelerates toward it and slides along
//! obstacles. Attacks wind up, resolve on a tick, and lock movement until
//! the swing (or the rebound after a hit) ends. In digging scenes an avatar
//! digs out the tile it stands on, standing still for the dig.

use content::scenario::{ticks_from_60hz, Weapon};
use content::{millis_to_ticks, per_tick, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::combat::{Event, TossEnd};
use super::haulers::AgentState;
use super::{AgentIx, DepotIx, Endpoint, ItemIx, Settlement};
use crate::messages::{ManualUnit, Messages, Strike};
use crate::nav::{self, angle_between, distance};
use crate::terrain::{Terrain, MAX_LAYERS};

/// The longest player name, in characters.
pub const NAME_CHARS: usize = 16;

/// The avatar's collision radius, as in the original controller.
pub const BODY_RADIUS: Fx = Fx::from_bits(1_374_389_535); // 0.32
const WALK_SPEED: Fx = Fx::from_bits(16_320_875_725); // 3.8 m/s
const RUN_SPEED: Fx = Fx::from_bits(24_051_816_858); // 5.6 m/s
/// Velocity change per tick, in m/s: accelerating 26 m/s², braking 32 m/s²,
/// stopping for an attack 35 m/s².
const ACCELERATION: Fx = per_tick(Fx::from_int(26));
const BRAKING: Fx = per_tick(Fx::from_int(32));
const ATTACK_BRAKING: Fx = per_tick(Fx::from_int(35));
/// Fraction of the remaining turn taken per tick (1 - e^(-20/TICKS_PER_SECOND)).
const TURN_BLEND: Fx = match TICKS_PER_SECOND {
    30 => Fx::from_bits(2_089_857_561), // 0.4866
    60 => Fx::from_bits(1_217_488_751), // 0.2835
    _ => panic!("add TURN_BLEND for this tick rate"),
};
/// Turning toward the aim during the windup (1 - e^(-28/TICKS_PER_SECOND)).
const AIM_BLEND: Fx = match TICKS_PER_SECOND {
    30 => Fx::from_bits(2_606_011_260), // 0.6068
    60 => Fx::from_bits(1_601_640_183), // 0.3729
    _ => panic!("add AIM_BLEND for this tick rate"),
};
/// Ticks from an attack command to contact (0.22 s).
pub const WINDUP_TICKS: u64 = ticks_from_60hz(13);
/// The visible windup, during which the avatar turns toward the aim (0.16 s).
const AIM_TICKS: u64 = ticks_from_60hz(10);
/// A swing that hits nothing ends at 0.58 s; one that lands rebounds for 0.42 s.
const SWING_TICKS: u64 = ticks_from_60hz(35);
pub const SWORD_WINDUP_TICKS: u64 = ticks_from_60hz(7);
const SWORD_SWING_TICKS: u64 = ticks_from_60hz(26);
const REBOUND_TICKS: u64 = ticks_from_60hz(25);
const COMBO_WINDOW_TICKS: u64 = ticks_from_60hz(75);
/// Manual transfers start within this distance of the dock.
const DOCK_REACH: Fx = Fx::from_bits(3_650_722_202); // 0.85
/// Jetpack: hover height above the ground, and climb and sink speeds per tick
/// (5.5 m/s and 3 m/s).
const HOVER: Fx = Fx::from_bits(7_730_941_133); // 1.8
const CLIMB_PER_TICK: Fx = per_tick(Fx::from_ratio(11, 2));
const SINK_PER_TICK: Fx = per_tick(Fx::from_int(3));
pub const JUMP_PREP_TICKS: u64 = ticks_from_60hz(8);
pub const JUMP_LAND_TICKS: u64 = ticks_from_60hz(10);
const JUMP_VELOCITY: Fx = per_tick(Fx::from_int(4));
const JUMP_GRAVITY: Fx = per_tick(per_tick(Fx::from_int(14)));
/// A dig takes 2 s, standing still.
pub const DIG_TICKS: u64 = millis_to_ticks(2000);
/// Dug ore flies out of the pit for 0.6 s.
const ORE_FLIGHT_TICKS: u64 = millis_to_ticks(600);
/// While digging, the avatar steps to the middle of its tile (1.5 m/s), so
/// it drops into the pit cleanly.
const DIG_CENTERING: Fx = per_tick(Fx::from_ratio(3, 2));

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intent {
    Stop,
    /// Walk this way until told otherwise. `direction` has length 0 or 1.
    Walk { direction: FxVec2, run: bool },
    /// Walk to this point along a planned route.
    GoTo { target: FxVec2, run: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Swing {
    pub weapon: Weapon,
    #[serde(default)]
    pub stroke: SwordStroke,
    /// Unit vector toward the aim.
    pub aim: FxVec2,
    pub muzzle: [Fx; 3],
    pub started: u64,
    pub resolve_at: u64,
    /// Movement resumes on this tick.
    pub until: u64,
    /// Set once contact was resolved; then `hits` holds how many it hit.
    pub resolved: bool,
    pub hits: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwordStroke {
    #[default]
    Forehand,
    Backhand,
    Overhead,
}

impl SwordStroke {
    pub fn next(self) -> Self {
        match self {
            Self::Forehand => Self::Backhand,
            Self::Backhand => Self::Overhead,
            Self::Overhead => Self::Forehand,
        }
    }
}

/// Digging out the tile under the avatar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dig {
    pub tile: (i32, i32),
    pub started: u64,
    /// The layer comes out on this tick.
    pub until: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualJob {
    pub depot: DepotIx,
    pub item: ItemIx,
    pub remaining: u32,
    pub deposit: bool,
    pub next_tick: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flight {
    /// The jetpack is held on.
    pub requested: bool,
    pub airborne: bool,
    /// The last ground height under the avatar.
    pub support: Fx,
}

/// A deterministic grounded jump. Vertical velocity is meters per tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum JumpPhase {
    #[default]
    Grounded,
    Preparing { started: u64 },
    Airborne { started: u64, velocity: Fx },
    Landing { started: u64 },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvatarState {
    #[serde(default)]
    pub health: u32,
    #[serde(default)]
    pub max_health: u32,
    #[serde(default)]
    pub respawn_at: u64,
    /// Height of the feet above the arena floor.
    pub height: Fx,
    pub flight: Flight,
    #[serde(default)]
    pub jump: JumpPhase,
    /// A pile job that failed while walking, for housekeeping to report.
    pub pile_failure: Option<String>,
    /// Meters per second.
    pub velocity: FxVec2,
    pub intent: Option<Intent>,
    pub run: bool,
    pub swing: Option<Swing>,
    #[serde(default)]
    pub combo_next: SwordStroke,
    #[serde(default)]
    pub combo_until: u64,
    /// The weapon in hand, so every peer shows it.
    pub weapon: Weapon,
    /// Upper body facing; movement heading remains on the agent.
    #[serde(default)]
    pub aim: Option<FxVec2>,
    /// A follow-up attack clicked during the end of a swing.
    pub queued: Option<(Weapon, FxVec2, [Fx; 3])>,
    pub job: Option<ManualJob>,
    #[serde(default)]
    pub dig: Option<Dig>,
}

fn approach(current: FxVec2, target: FxVec2, max_change: Fx) -> FxVec2 {
    let delta = target - current;
    let length = delta.length();
    if length <= max_change {
        target
    } else {
        current + delta * (max_change / length)
    }
}

/// Moves `position` by `delta` in small steps, pushing out of obstacles and
/// never leaving the arena, as the original controller did.
fn slide(terrain: &Terrain, mut position: FxVec2, delta: FxVec2) -> FxVec2 {
    let step = Fx::from_ratio(3, 50);
    let steps = (delta.length() / step).ceil().floor_to_int().max(1);
    let (min, max) = (terrain.min(), terrain.max());
    let clamp = |p: FxVec2| FxVec2::new(p.x.clamp(min.x, max.x), p.y.clamp(min.y, max.y));
    let extra = Fx::from_ratio(1, 50);
    for _ in 0..steps {
        let previous = position;
        position = clamp(position + delta.div_int(steps));
        for _ in 0..3 {
            for o in &terrain.obstacles {
                let separation = position - o.center;
                let clearance = o.radius + BODY_RADIUS + extra;
                if separation.length_squared() < clearance * clearance {
                    let length = separation.length();
                    let away = if length == Fx::ZERO { FxVec2::new(Fx::ONE, Fx::ZERO) } else { separation / length };
                    position += away * (clearance - length);
                }
            }
        }
        if !terrain.inside(position) {
            position = previous;
        }
    }
    position
}

/// Stage 2 for one avatar.
pub(super) fn self_update(a: &mut AgentState, me: AgentIx, tick: u64, terrain: &Terrain, piles: Option<&super::piles::PileLab>) {
    if a.avatar.max_health > 0 && a.avatar.health == 0 {
        if tick >= a.avatar.respawn_at {
            a.position = a.home;
            a.avatar.height = terrain.footprint(a.home, BODY_RADIUS).unwrap_or_default();
            a.avatar.health = a.avatar.max_health;
            a.avatar.respawn_at = 0;
            a.avatar.flight = Flight::default();
            a.avatar.jump = JumpPhase::Grounded;
            a.avatar.velocity = FxVec2::ZERO;
            a.avatar.intent = Some(Intent::Stop);
        }
        return;
    }
    if let Some(lab) = piles {
        if let Some(error) = Settlement::pile_job_self_update(lab, me, a, terrain) {
            a.avatar.pile_failure = Some(error.to_string());
        }
    }
    walk(a, tick, terrain);
    if terrain.jump_enabled {
        jump_step(a, tick, terrain);
    } else {
        fly(a, terrain);
    }
}

fn jump_step(a: &mut AgentState, tick: u64, terrain: &Terrain) {
    let avatar = &mut a.avatar;
    let ground = terrain.footprint(a.position, BODY_RADIUS);
    if let Some(y) = ground {
        avatar.flight.support = y;
    }
    match avatar.jump {
        JumpPhase::Grounded => {
            avatar.flight.airborne = false;
            if let Some(y) = ground { avatar.height = y; }
        }
        JumpPhase::Preparing { started } => {
            if tick >= started + JUMP_PREP_TICKS {
                avatar.jump = JumpPhase::Airborne { started: tick, velocity: JUMP_VELOCITY };
                avatar.flight.airborne = true;
                avatar.height += JUMP_VELOCITY;
            }
        }
        JumpPhase::Airborne { started, velocity } => {
            let next_velocity = velocity - JUMP_GRAVITY;
            avatar.height += next_velocity;
            if next_velocity <= Fx::ZERO && ground.is_some_and(|y| avatar.height <= y) {
                avatar.height = ground.unwrap();
                avatar.flight.airborne = false;
                avatar.jump = JumpPhase::Landing { started: tick };
            } else {
                avatar.jump = JumpPhase::Airborne { started, velocity: next_velocity };
            }
        }
        JumpPhase::Landing { started } => {
            if let Some(y) = ground { avatar.height = y; }
            if tick >= started + JUMP_LAND_TICKS {
                avatar.jump = JumpPhase::Grounded;
            }
        }
    }
}

fn walk(a: &mut AgentState, tick: u64, terrain: &Terrain) {
    let per_tick = Fx::from_int(content::TICKS_PER_SECOND);
    let avatar = &mut a.avatar;
    if let Some(dig) = &avatar.dig {
        avatar.velocity = approach(avatar.velocity, FxVec2::ZERO, BRAKING);
        let center = Terrain::pit_center(dig.tile);
        a.position = slide(terrain, a.position, approach(a.position, center, DIG_CENTERING) - a.position);
        return;
    }
    if let Some(swing) = &avatar.swing {
        if tick < swing.until && swing.weapon != Weapon::Laser {
            avatar.velocity = approach(avatar.velocity, FxVec2::ZERO, ATTACK_BRAKING);
            if tick < swing.started + AIM_TICKS {
                let target = nav::bearing(FxVec2::ZERO, swing.aim);
                a.heading += angle_between(a.heading, target) * AIM_BLEND;
            }
            return;
        }
        if tick >= swing.until {
            avatar.swing = None;
        }
    }
    let speed = terrain.locomotion.map_or_else(
        || if avatar.run { RUN_SPEED } else { WALK_SPEED },
        |profile| if avatar.run { profile.run } else { profile.walk },
    );
    let mut desired = FxVec2::ZERO;
    match &avatar.intent {
        Some(Intent::Walk { direction, .. }) => desired = *direction * speed,
        Some(Intent::GoTo { .. }) if !a.route.is_empty() => {
            let near = Fx::from_ratio(9, 50);
            while a.route.len() > 1 && distance(a.route[0], a.position) < near {
                a.route.remove(0);
            }
            let delta = a.route[0] - a.position;
            let remaining = delta.length();
            let braking = if a.route.len() == 1 { (Fx::from_int(56) * remaining).sqrt() } else { speed };
            desired = delta.normalize_or_zero() * speed.min(braking);
            if remaining < Fx::from_ratio(11, 200) {
                a.position = a.route[0];
                avatar.velocity = FxVec2::ZERO;
                a.route.clear();
                avatar.intent = Some(Intent::Stop);
                desired = FxVec2::ZERO;
            }
        }
        _ => {}
    }
    let change = if desired == FxVec2::ZERO { BRAKING } else { ACCELERATION };
    avatar.velocity = approach(avatar.velocity, desired, change);
    let previous = a.position;
    let mut delta = avatar.velocity / per_tick;
    if a.route.len() == 1 {
        let remaining = a.route[0] - previous;
        if delta.length() > remaining.length() {
            delta = remaining;
            avatar.velocity = FxVec2::ZERO;
        }
    }
    let candidate = slide(terrain, previous, delta);
    let airborne = a.avatar.flight.airborne;
    let allowed = if airborne {
        terrain.flyable(previous, candidate, a.avatar.height, BODY_RADIUS)
    } else {
        terrain.walkable(previous, candidate, BODY_RADIUS)
    };
    if allowed {
        if !airborne {
            if let Some(ground) = terrain.footprint(candidate, BODY_RADIUS) {
                a.avatar.height = ground;
            }
        }
        a.position = candidate;
    }
    let moved = a.position - previous;
    let length = moved.length();
    a.travelled += length;
    if length > Fx::from_ratio(1, 1000) {
        let target = nav::bearing(FxVec2::ZERO, moved);
        a.heading += angle_between(a.heading, target) * TURN_BLEND;
    }
}

/// The jetpack: hover over the highest ground ahead while held, fall back to
/// the ground when released, and keep flying over drops.
fn fly(a: &mut AgentState, terrain: &Terrain) {
    let flight = &mut a.avatar.flight;
    let ground = terrain.footprint(a.position, BODY_RADIUS);
    let landing = ground;
    if let Some(y) = ground {
        flight.support = y;
    }
    let active = flight.requested || (flight.airborne && landing.is_none());
    let direction = match a.route.first() {
        Some(&next) => (next - a.position).normalize_or_zero(),
        None => a.avatar.velocity.normalize_or_zero(),
    };
    let mut target = ground.unwrap_or(flight.support);
    if active {
        // Look far enough ahead to climb at walking and running speed.
        let r = BODY_RADIUS;
        let offsets = [(Fx::ZERO, Fx::ZERO), (r, Fx::ZERO), (-r, Fx::ZERO), (Fx::ZERO, r), (Fx::ZERO, -r)];
        for i in 0..=20 {
            let p = a.position + direction * Fx::from_ratio(i, 10);
            for &(dx, dy) in &offsets {
                if let Some(y) = terrain.ground(FxVec2::new(p.x + dx, p.y + dy)) {
                    target = target.max(y);
                }
            }
        }
        target += HOVER;
    }
    let delta = (target - a.avatar.height).clamp(-SINK_PER_TICK, CLIMB_PER_TICK);
    a.avatar.height += delta;
    flight.airborne = active || ground.is_none_or(|y| a.avatar.height > y + Fx::from_ratio(1, 100));
}

impl Settlement {
    /// The first player avatar, if there is one.
    pub fn player(&self) -> Option<super::AgentIx> {
        self.agents.iter().position(|a| a.controlled).map(|i| i as super::AgentIx)
    }

    /// Commands: names `player`'s avatar, keeping up to `NAME_CHARS`
    /// printable characters.
    pub fn rename(&mut self, player: crate::PlayerId, name: &str) {
        let Some(agent) = self.avatar_of(player) else { return };
        let name: String = name.chars().filter(|c| !c.is_control()).take(NAME_CHARS).collect();
        self.agents[agent as usize].name = name.trim().to_string();
    }

    /// Commands: puts a weapon the scenario offers in the avatar's hand.
    pub fn equip_weapon(&mut self, agent: usize, weapon: Weapon) {
        if !self.has_weapon(weapon) {
            return;
        }
        if let Some(a) = self.agents.get_mut(agent).filter(|a| a.controlled) {
            a.avatar.weapon = weapon;
        }
    }

    /// The avatar `player` controls.
    pub fn avatar_of(&self, player: crate::PlayerId) -> Option<AgentIx> {
        self.agents.iter().position(|a| a.owner == Some(player)).map(|i| i as AgentIx)
    }

    /// Commands: gives a player who joined an avatar of their own, a copy
    /// of the scene's first avatar beside the spawn point. Players who
    /// already have one keep it.
    pub fn join(&mut self, player: crate::PlayerId) {
        if self.avatar_of(player).is_some() {
            return;
        }
        let Some(template) = self.player().map(|i| self.agents[i as usize].clone()) else { return };
        // Joiners stand in a row beside the spawn, so they don't overlap.
        let slot = self.agents.iter().filter(|a| a.controlled).count() as i64;
        let position = FxVec2::new(self.spawn.x + Fx::from_ratio(slot * 9, 10), self.spawn.y);
        // Only the definition carries over; the state starts fresh.
        self.agents.push(AgentState {
            id: format!("player-{}", player.0),
            owner: Some(player),
            name: String::new(),
            home: position,
            position,
            heading: Fx::PI,
            travel_heading: Fx::ZERO,
            travelled: Fx::ZERO,
            phase: super::haulers::Phase::Idle,
            route: Vec::new(),
            cargo: vec![0; template.cargo.len()],
            assignment: None,
            equipped: false,
            trailer: None,
            transfer_at: 0,
            retries: 0,
            departure_dock: None,
            traffic_replan_at: 0,
            yielding: 0,
            pending_in: 0,
            pending_out: 0,
            avatar: AvatarState {
                height: self.spawn_height,
                health: self.player_health,
                max_health: self.player_health,
                weapon: self.weapons.first().copied().unwrap_or_default(),
                intent: Some(Intent::Stop),
                ..AvatarState::default()
            },
            attack_ready: 0,
            avoidance_steers: 0,
            dock_wait_ticks: 0,
            traffic_wait_ticks: 0,
            loaded_travel: Fx::ZERO,
            empty_travel: Fx::ZERO,
            ..template
        });
    }

    /// Commands: holds the jetpack on or off. Flying off stops a dig.
    pub fn jetpack(&mut self, agent: usize, enabled: bool) {
        if let Some(a) = self.agents.get_mut(agent).filter(|a| a.controlled) {
            a.avatar.flight.requested = enabled;
            if enabled {
                a.avatar.dig = None;
            }
        }
    }

    /// Starts a jump with a short crouch before takeoff. Midair presses do nothing.
    pub fn jump(&mut self, agent: usize, tick: u64) {
        if let Some(a) = self.agents.get_mut(agent).filter(|a| a.controlled) {
            if matches!(a.avatar.jump, JumpPhase::Grounded)
                && !a.avatar.flight.airborne
                && a.avatar.dig.is_none()
                && a.avatar.swing.is_none()
            {
                a.avatar.jump = JumpPhase::Preparing { started: tick };
            }
        }
    }

    /// Commands: starts digging out the tile under the avatar, if the scene
    /// allows digging, the avatar stands on the ground with its hands free,
    /// the pit there isn't as deep as it goes, and no pile or machine sits
    /// on the tile.
    pub fn dig(&mut self, tick: u64, agent: usize, terrain: &Terrain) {
        if !terrain.diggable {
            return;
        }
        let Some(position) = self.agents.get(agent).map(|a| a.position) else { return };
        if self.piles.as_ref().is_some_and(|lab| lab.blocks_dig(Terrain::pit_tile(position))) {
            return;
        }
        let Some(a) = self.agents.get_mut(agent).filter(|a| a.controlled) else { return };
        let busy = a.avatar.swing.as_ref().is_some_and(|s| tick < s.until) || a.avatar.job.is_some();
        let flight = &a.avatar.flight;
        if busy || a.avatar.dig.is_some() || flight.requested || flight.airborne {
            return;
        }
        let tile = Terrain::pit_tile(a.position);
        if terrain.pit(tile) >= MAX_LAYERS || !terrain.inside(Terrain::pit_center(tile)) {
            return;
        }
        a.route.clear();
        a.avatar.intent = Some(Intent::Stop);
        a.avatar.dig = Some(Dig { tile, started: tick, until: tick + DIG_TICKS });
    }

    /// Housekeeping: digs that are done take their layer out of the ground.
    /// In a mine, the layer's ore flies out onto a pile beside the pit.
    pub(crate) fn dig_housekeeping(&mut self, tick: u64, terrain: &mut Terrain) {
        let mut dug = Vec::new();
        for a in self.agents.iter_mut().filter(|a| a.controlled) {
            if let Some(dig) = a.avatar.dig.take_if(|d| tick + 1 >= d.until) {
                if terrain.deepen(dig.tile) {
                    dug.push(dig.tile);
                }
            }
        }
        for tile in dug {
            let layer = terrain.pit(tile) - 1;
            let Some(mine) = terrain.mine() else { continue };
            let Some(material) = mine.ore(tile, layer) else { continue };
            let units = mine.yield_of(tile, layer);
            let Some(lab) = self.piles.as_mut() else { continue };
            let Some((id, first)) = lab.mined(material, units, tile, terrain) else { continue };
            let position = lab.piles[&id].position;
            let from = TossEnd::Pit { position: Terrain::pit_center(tile) };
            for slot in first..first + units {
                self.events.push(Event::PileToss { material, from, to: TossEnd::Pile { id, slot, position }, ticks: ORE_FLIGHT_TICKS });
            }
        }
    }

    /// Commands: puts the avatar back at the scene's spawn point, and
    /// restarts the memory trial (closing its door again).
    pub fn respawn(&mut self, agent: usize, terrain: &mut Terrain) {
        if let Some(m) = self.memory.as_mut() {
            if m.unlocked {
                terrain.obstacles.extend_from_slice(&m.doors);
                terrain.revision += 1;
            }
            *m = super::memory::Memory::new(m.trial, m.center, std::mem::take(&mut m.doors));
        }
        let (spawn, height) = (self.spawn, self.spawn_height);
        let Some(a) = self.agents.get_mut(agent).filter(|a| a.controlled) else { return };
        a.position = spawn;
        a.heading = Fx::PI;
        a.route.clear();
        a.avatar = AvatarState { height, ..AvatarState::default() };
        a.avatar.intent = Some(Intent::Stop);
        self.cancel_pile_job(agent);
    }

    /// Stage 3 for one avatar: a strike resolving this tick, and manual
    /// transfer units that are due.
    pub(super) fn read_avatar(&self, me: AgentIx, a: &AgentState, tick: u64, terrain: &Terrain, out: &mut Messages) {
        if let Some(swing) = &a.avatar.swing {
            if !swing.resolved && swing.resolve_at == tick && a.attack_ready <= tick {
                let (targets, beam_end) = self.strike_targets(me, a.position, swing.aim, swing.weapon, terrain);
                out.strike.push(Strike {
                    agent: me,
                    weapon: swing.weapon,
                    origin: a.position,
                    forward: swing.aim,
                    muzzle: swing.muzzle,
                    targets,
                    beam_end,
                });
            }
        }
        if let Some(job) = &a.avatar.job {
            if tick >= job.next_tick && distance(a.position, self.depots[job.depot as usize].position) <= DOCK_REACH {
                out.manual_unit.push(ManualUnit { agent: me });
            }
        }
        if let Some(step) = self.read_pile_job(me, a, tick) {
            out.pile_step.push(step);
        }
    }

    /// Commands: sets an avatar's movement intent.
    pub fn move_avatar(&mut self, agent: usize, intent: Intent, terrain: &Terrain) {
        if self.agents.get(agent).is_some_and(|a| a.controlled) {
            self.cancel_pile_job(agent);
        }
        let Some(a) = self.agents.get_mut(agent).filter(|a| a.controlled && (a.avatar.max_health == 0 || a.avatar.health > 0)) else { return };
        // Walking off stops a dig.
        let walking = match intent {
            Intent::Stop => false,
            Intent::Walk { direction, .. } => direction != FxVec2::ZERO,
            Intent::GoTo { .. } => true,
        };
        if a.avatar.dig.is_some() && !walking {
            return;
        }
        a.avatar.dig = None;
        a.route.clear();
        if let Intent::GoTo { target, run } = intent {
            // On foot, routes avoid steps and drops; with the jetpack, anything goes.
            let flying = a.avatar.flight.requested || a.avatar.flight.airborne;
            let walkable = |p: FxVec2, q: FxVec2| flying || terrain.walkable(p, q, BODY_RADIUS);
            a.route = nav::path_with(terrain, a.position, target, BODY_RADIUS, &[], &walkable);
            a.avatar.run = run;
        } else if let Intent::Walk { run, .. } = intent {
            a.avatar.run = run;
        }
        a.avatar.intent = Some(intent);
    }

    /// Commands: the player stopped driving the avatar directly (pause, focus
    /// loss, a controller disconnecting). The jetpack always switches off.
    /// Manual or click travel stops, unless an accepted logistics job owns
    /// the route: a pile haul, or a depot transfer walking to its dock. Both
    /// are judged on the state this tick, after any earlier command in the
    /// same turn, so a haul accepted or rejected just before still decides.
    /// Unlike `move_avatar(Stop)` this never cancels a pile job.
    pub fn release_direct_control(&mut self, agent: usize) {
        let hauling = self.piles.as_ref().is_some_and(|lab| lab.hauler(agent as AgentIx).is_some_and(|h| h.job.is_some()));
        let Some(a) = self.agents.get_mut(agent).filter(|a| a.controlled) else { return };
        a.avatar.flight.requested = false;
        let docking = a.avatar.job.is_some() && matches!(a.avatar.intent, Some(Intent::GoTo { .. }));
        if hauling || docking {
            return;
        }
        if matches!(a.avatar.intent, Some(Intent::Walk { .. } | Intent::GoTo { .. })) {
            a.route.clear();
            a.avatar.intent = Some(Intent::Stop);
        }
    }

    /// Commands: starts an attack, or buffers one deliberate press during it.
    pub fn aim_avatar(&mut self, agent: usize, aim: FxVec2) {
        let Some(a) = self.agents.get_mut(agent).filter(|a| a.controlled) else { return };
        let aim = aim.normalize_or_zero();
        if aim != FxVec2::ZERO {
            a.avatar.aim = Some(aim);
        }
    }

    pub fn attack(&mut self, tick: u64, agent: usize, weapon: Weapon, aim: FxVec2, muzzle: [Fx; 3]) {
        if !self.has_weapon(weapon) {
            return;
        }
        if self.agents.get(agent).is_some_and(|a| a.controlled) {
            self.cancel_pile_job(agent);
        }
        let swift_sword = self.swift_sword;
        let Some(a) = self.agents.get_mut(agent).filter(|a| a.controlled && (a.avatar.max_health == 0 || a.avatar.health > 0)) else { return };
        if a.avatar.dig.is_some() {
            return;
        }
        let aim = aim.normalize_or_zero();
        if aim == FxVec2::ZERO || muzzle.iter().any(|v| v.abs() > Fx::from_int(4)) {
            return;
        }
        match &a.avatar.swing {
            Some(swing) if tick < swing.until => {
                a.avatar.queued = Some((weapon, aim, muzzle));
            }
            _ => start_swing(a, tick, weapon, aim, muzzle, swift_sword),
        }
    }

    /// Housekeeping: a buffered attack starts once the swing ends.
    pub(crate) fn avatar_housekeeping(&mut self, tick: u64) {
        let swift_sword = self.swift_sword;
        for a in self.agents.iter_mut().filter(|a| a.controlled && (a.avatar.max_health == 0 || a.avatar.health > 0)) {
            let finished = a.avatar.swing.as_ref().is_none_or(|s| tick + 1 >= s.until);
            if finished {
                if let Some((weapon, aim, muzzle)) = a.avatar.queued.take() {
                    start_swing(a, tick + 1, weapon, aim, muzzle, swift_sword);
                }
            }
        }
    }

    /// Records a resolved strike on the avatar: a hit shortens the swing to the rebound.
    pub(crate) fn swing_resolved(&mut self, agent: AgentIx, tick: u64, hits: u32) {
        let a = &mut self.agents[agent as usize];
        if let Some(swing) = &mut a.avatar.swing {
            swing.resolved = true;
            swing.hits = hits;
            if hits > 0 && matches!(swing.weapon, Weapon::Sword | Weapon::Spear) {
                swing.until = tick + REBOUND_TICKS;
            }
        }
    }

    /// The most units `agent` could move between its cargo and `depot` now.
    pub fn transfer_limit(&self, agent: usize, depot: usize, item: usize, deposit: bool) -> u32 {
        let (Some(a), Some(d)) = (self.agents.get(agent), self.depots.get(depot)) else { return 0 };
        if item >= self.items.len() {
            return 0;
        }
        if deposit {
            a.cargo[item].min(d.free())
        } else {
            let carried: u32 = a.cargo.iter().sum::<u32>() + a.pending_in;
            a.cargo_capacity.saturating_sub(carried).min(d.available(item as ItemIx))
        }
    }

    /// Commands: reserves an exact transfer; units move once the avatar is at the dock.
    pub fn transfer(&mut self, tick: u64, agent: usize, depot: usize, item: usize, units: u32, deposit: bool) {
        let Some(a) = self.agents.get(agent).filter(|a| a.controlled) else { return };
        if a.avatar.job.is_some() || a.pending_in > 0 || a.pending_out > 0 || units == 0 {
            return;
        }
        if self.transfer_limit(agent, depot, item, deposit) < units {
            return;
        }
        if deposit {
            self.depots[depot].incoming[item] += units;
        } else {
            self.depots[depot].out[item] += units;
        }
        self.agents[agent].avatar.job = Some(ManualJob {
            depot: depot as DepotIx,
            item: item as ItemIx,
            remaining: units,
            deposit,
            next_tick: tick,
        });
    }

    /// Commands: releases the unlaunched part of a transfer. Units in the air
    /// finish their flight.
    pub fn cancel_transfer(&mut self, agent: usize) {
        let Some(job) = self.agents.get_mut(agent).and_then(|a| a.avatar.job.take()) else { return };
        let depot = &mut self.depots[job.depot as usize];
        if job.deposit {
            depot.incoming[job.item as usize] -= job.remaining;
        } else {
            depot.out[job.item as usize] -= job.remaining;
        }
    }

    /// Moves one unit of a manual transfer.
    pub(crate) fn manual_unit(&mut self, tick: u64, message: ManualUnit) {
        let agent = message.agent as usize;
        let Some(job) = self.agents[agent].avatar.job.clone() else { return };
        if tick < job.next_tick {
            return;
        }
        let (d, item) = (job.depot as usize, job.item);
        let dock = self.depots[d].position;
        let at = self.agents[agent].position;
        if job.deposit {
            self.agents[agent].cargo[item as usize] -= 1;
            self.agents[agent].pending_out += 1;
            self.launch(tick, item, Endpoint::Cargo(message.agent), Endpoint::Depot(job.depot), at, dock);
        } else {
            self.depots[d].stock[item as usize] -= 1;
            self.depots[d].out[item as usize] -= 1;
            self.metrics.picked += 1;
            self.agents[agent].pending_in += 1;
            self.launch(tick, item, Endpoint::Depot(job.depot), Endpoint::Cargo(message.agent), dock, at);
        }
        let a = &mut self.agents[agent];
        let next = tick + a.transfer_ticks;
        let job = a.avatar.job.as_mut().expect("checked");
        job.remaining -= 1;
        job.next_tick = next;
        if job.remaining == 0 {
            a.avatar.job = None;
        }
    }
}

fn start_swing(a: &mut AgentState, tick: u64, weapon: Weapon, aim: FxVec2, muzzle: [Fx; 3], swift_sword: bool) {
    let stroke = if weapon == Weapon::Sword {
        if tick <= a.avatar.combo_until { a.avatar.combo_next } else { SwordStroke::Forehand }
    } else {
        SwordStroke::Forehand
    };
    a.avatar.combo_next = if weapon == Weapon::Sword { stroke.next() } else { SwordStroke::Forehand };
    a.avatar.combo_until = tick + COMBO_WINDOW_TICKS;
    a.avatar.weapon = weapon;
    if weapon == Weapon::Laser {
        a.avatar.aim = Some(aim);
    }
    if weapon != Weapon::Laser {
        a.route.clear();
        a.avatar.intent = Some(Intent::Stop);
    }
    a.avatar.swing = Some(Swing {
        weapon,
        stroke,
        aim,
        muzzle,
        started: tick,
        resolve_at: tick + if weapon == Weapon::Laser { ticks_from_60hz(2) } else if weapon == Weapon::Sword && swift_sword { SWORD_WINDUP_TICKS } else { WINDUP_TICKS },
        until: tick + if weapon == Weapon::Laser { ticks_from_60hz(8) } else if weapon == Weapon::Sword && swift_sword { SWORD_SWING_TICKS } else { SWING_TICKS },
        resolved: false,
        hits: 0,
    });
}
