//! Player avatars: controlled agents, moved by commands instead of haul links.
//!
//! This is the original game's player controller (which lived in the Bevy
//! app) moved into the simulation, so every peer moves every avatar the same
//! way. A command sets an intent (walk in a direction, go to a point, or
//! stop); each tick the avatar accelerates toward it and slides along
//! obstacles. Attacks wind up, resolve on a tick, and lock movement until
//! the swing (or the rebound after a hit) ends. In digging scenes an avatar
//! digs out the tile it stands on, standing still for the dig.

use content::character::{AttackTiming, AvatarDef, CharacterDef, JetpackDef, JumpDef};
use content::scenario::{ticks_from_60hz, Weapon};
use content::{millis_to_ticks, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::combat::{Event, TossEnd};
use super::{AgentState, Body, Carry, CarryJob, Direction, Role, Side};
use super::action::{Action, View};
use super::buildings::BurstRefusal;
use super::interact::{self, Interaction};
use super::{AgentIx, DepotState, InventoryRef, ItemIx, Kinds, Settlement};
use crate::messages::{ActionMessages, BundleRefused, Burst, BurstRefused, CarryUnit, DropBundle, DropHaul, ElderFire, DropNotes, Install, Messages, ReserveStock, RestartMemory, StartHaul, StockChange, Strike};
use crate::nav::{self, angle_between, distance};
use crate::terrain::{Terrain, MAX_LAYERS};

/// The longest player name, in characters.
pub const NAME_CHARS: usize = 16;

// Body radius, speeds, acceleration, swing timing and the jump, jetpack and
// dig tuning come from the avatar's character definition (`content::character`).
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
/// Manual transfers start within this distance of the dock.
pub(super) const DOCK_REACH: Fx = Fx::from_bits(3_650_722_202); // 0.85
/// Dug ore flies out of the pit for 0.6 s.
const ORE_FLIGHT_TICKS: u64 = millis_to_ticks(600);

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
    /// The tick the `Attack` was pressed on when it waited in
    /// `AvatarState::queued` first. It keys predicted events (`pressed`).
    #[serde(default)]
    pub queued_at: Option<u64>,
}

impl Swing {
    /// The tick the `Attack` that started this swing applied on: the
    /// player's command, which predicted events are matched by
    /// (`ABILITY_PREDICTION_HANDOFF.md`, D7).
    pub fn pressed(&self) -> u64 {
        self.queued_at.unwrap_or(self.started)
    }
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

    /// The stroke a new attack swings and the one queued after it. Swords
    /// cycle while the combo window is open; other weapons reset it.
    pub fn combo(weapon: Weapon, next: SwordStroke, in_window: bool) -> (SwordStroke, SwordStroke) {
        if weapon != Weapon::Sword {
            return (Self::Forehand, Self::Forehand);
        }
        let stroke = if in_window { next } else { Self::Forehand };
        (stroke, stroke.next())
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
    /// The player whose commands drive this avatar.
    pub owner: Option<crate::PlayerId>,
    /// The name the owner goes by; empty until they give one.
    pub name: String,
    /// Weapons are ready again on this tick.
    pub attack_ready: u64,
    /// A Burst is ready again on this tick (D3). It is the elder's, not the
    /// building's, so hopping between buildings doesn't reset it, and it
    /// outlasts leaving a seat and a respawn, as `attack_ready` does.
    #[serde(default)]
    pub burst_ready: u64,
    /// Dead until this tick, then back at home (`Body::health`).
    #[serde(default)]
    pub respawn_at: u64,
    /// Height of the feet above the arena floor.
    pub height: Fx,
    pub flight: Flight,
    #[serde(default)]
    pub jump: JumpPhase,
    /// A pile job that failed while walking, for housekeeping to report.
    pub pile_failure: Option<String>,
    pub intent: Option<Intent>,
    pub run: bool,
    pub swing: Option<Swing>,
    #[serde(default)]
    pub combo_next: SwordStroke,
    #[serde(default)]
    pub combo_until: u64,
    /// The weapon in hand, so every peer shows it.
    pub weapon: Weapon,
    /// Upper body facing; movement heading remains on the agent. While
    /// possessing a ballista, the reticle instead: an offset from the
    /// ballista, within its range (`BuildingKind::reticle`).
    #[serde(default)]
    pub aim: Option<FxVec2>,
    /// A follow-up attack clicked during the end of a swing.
    pub queued: Option<(Weapon, FxVec2, [Fx; 3])>,
    /// The tick `queued` was pressed on (see `Swing::queued_at`); set exactly
    /// when `queued` is.
    #[serde(default)]
    pub queued_at: Option<u64>,
    #[serde(default)]
    pub dig: Option<Dig>,
    /// Repairable cover selected while the player holds the repair control.
    #[serde(default)]
    pub repair: Option<u16>,
    /// An action this tick let go of the pile haul (`drop_haul`). Until
    /// stage 5 ends it, the haul neither steers the avatar nor moves units;
    /// stage 5 clears this, so it is never set between ticks and isn't
    /// saved or hashed. A replay clears it too, ending its own copy of the haul.
    #[serde(skip)]
    pub dropped_haul: bool,
    /// The seat this avatar sits in: a vehicle it drives (`vehicle`), whose
    /// movement commands steer the vehicle, or a building it possesses
    /// (D3), where movement does nothing. Housekeeping keeps it in the seat.
    #[serde(default)]
    pub seat: Option<Seat>,
}

/// Where a seated avatar sits (`AvatarState::seat`). Boarding enters the
/// nearest free one in reach (`Settlement::board`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Seat {
    /// Driving a vehicle.
    Vehicle(super::VehicleIx),
    /// Possessing a building, as its slot's `Occupant::Elder`.
    Building(super::BuildingIx),
}

impl Seat {
    /// The vehicle, if it is a vehicle's seat.
    pub fn vehicle(self) -> Option<super::VehicleIx> {
        match self {
            Seat::Vehicle(ix) => Some(ix),
            Seat::Building(_) => None,
        }
    }

    /// The building, if it is a building's slot.
    pub fn building(self) -> Option<super::BuildingIx> {
        match self {
            Seat::Building(b) => Some(b),
            Seat::Vehicle(_) => None,
        }
    }
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
fn slide(terrain: &Terrain, mut position: FxVec2, delta: FxVec2, radius: Fx) -> FxVec2 {
    let step = Fx::from_ratio(3, 50);
    let steps = (delta.length() / step).ceil().floor_to_int().max(1);
    let (min, max) = (terrain.min(), terrain.max());
    let clamp = |p: FxVec2| FxVec2::new(p.x.clamp(min.x, max.x), p.y.clamp(min.y, max.y));
    let extra = Fx::from_ratio(1, 50);
    for _ in 0..steps {
        let previous = position;
        position = clamp(position + delta.div_int(steps));
        for _ in 0..3 {
            for o in terrain.blocking_obstacles() {
                let separation = position - o.center;
                let clearance = o.radius + radius + extra;
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
pub(super) fn self_update(body: &mut Body, avatar: &mut AvatarState, me: AgentIx, tick: u64, terrain: &Terrain, piles: Option<&super::piles::PileLab>, def: &CharacterDef) {
    let radius = def.body.radius;
    if !body.alive() {
        if tick >= avatar.respawn_at {
            body.position = body.home;
            avatar.height = terrain.footprint(body.home, radius).unwrap_or_default();
            body.health = body.max_health;
            avatar.respawn_at = 0;
            avatar.flight = Flight::default();
            avatar.jump = JumpPhase::Grounded;
            body.velocity = FxVec2::ZERO;
            body.push = FxVec2::ZERO;
            avatar.intent = Some(Intent::Stop);
        }
        return;
    }
    // A driver rides, the vehicle moving (`vehicle::self_update`); an elder
    // in a building stays at its slot.
    if avatar.seat.is_some() {
        return;
    }
    if let Some(lab) = piles.filter(|_| !avatar.dropped_haul) {
        if let Some(error) = Settlement::pile_job_self_update(lab, me, body, avatar, terrain, radius) {
            avatar.pile_failure = Some(error.to_string());
        }
    }
    walk(body, avatar, tick, terrain, def);
    match def.jetpack() {
        Some(jetpack) => fly(body, avatar, terrain, radius, jetpack),
        None => jump_step(body, avatar, tick, terrain, radius, def.jump()),
    }
}

/// A grounded jump, or for a character with neither jump nor jetpack, just
/// keeping to the ground.
fn jump_step(body: &Body, avatar: &mut AvatarState, tick: u64, terrain: &Terrain, radius: Fx, jump: Option<&JumpDef>) {
    let ground = terrain.footprint(body.position, radius);
    if let Some(y) = ground {
        avatar.flight.support = y;
    }
    let Some(jump) = jump else {
        avatar.flight.airborne = false;
        if let Some(y) = ground { avatar.height = y; }
        return;
    };
    match avatar.jump {
        JumpPhase::Grounded => {
            avatar.flight.airborne = false;
            if let Some(y) = ground { avatar.height = y; }
        }
        JumpPhase::Preparing { started } => {
            if tick >= started + jump.prep {
                avatar.jump = JumpPhase::Airborne { started: tick, velocity: jump.velocity };
                avatar.flight.airborne = true;
                avatar.height += jump.velocity;
            }
        }
        JumpPhase::Airborne { started, velocity } => {
            let next_velocity = velocity - jump.gravity;
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
            if tick >= started + jump.land {
                avatar.jump = JumpPhase::Grounded;
            }
        }
    }
}

fn walk(body: &mut Body, avatar: &mut AvatarState, tick: u64, terrain: &Terrain, def: &CharacterDef) {
    let tuning = avatar_tuning(def);
    let radius = def.body.radius;
    let per_tick = Fx::from_int(content::TICKS_PER_SECOND);
    if let Some(dig) = &avatar.dig {
        body.velocity = approach(body.velocity, FxVec2::ZERO, tuning.braking);
        let center = Terrain::pit_center(dig.tile);
        let centering = def.dig().map_or(Fx::ZERO, |d| d.centering);
        body.position = slide(terrain, body.position, approach(body.position, center, centering) - body.position, radius);
        return;
    }
    if let Some((started, until, weapon, aim)) = avatar.swing.as_ref().map(|s| (s.started, s.until, s.weapon, s.aim)) {
        if tick < until && weapon != Weapon::Laser {
            body.velocity = approach(body.velocity, FxVec2::ZERO, tuning.attack_braking);
            if body.push != FxVec2::ZERO {
                // A threat's push moves the swinging avatar as it would an
                // idle one: braked at the walking rate, by the walking rules.
                body.push = approach(body.push, FxVec2::ZERO, tuning.braking);
                let delta = body.push / per_tick;
                step(body, avatar, terrain, delta, radius);
            }
            if tick < started + tuning.attack.aim {
                let target = nav::bearing(FxVec2::ZERO, aim);
                body.heading += angle_between(body.heading, target) * AIM_BLEND;
            }
            return;
        }
        if tick >= until {
            avatar.swing = None;
        }
    }
    // What is left of a push from the swing moves the avatar from now on.
    body.velocity += std::mem::take(&mut body.push);
    let speed = if avatar.run { def.movement.run } else { def.movement.walk };
    let mut desired = FxVec2::ZERO;
    match &avatar.intent {
        Some(Intent::Walk { direction, .. }) => desired = *direction * speed,
        Some(Intent::GoTo { .. }) if !body.route.is_empty() => {
            let near = Fx::from_ratio(9, 50);
            while body.route.len() > 1 && distance(body.route[0], body.position) < near {
                body.route.remove(0);
            }
            let delta = body.route[0] - body.position;
            let remaining = delta.length();
            let braking = if body.route.len() == 1 { (Fx::from_int(56) * remaining).sqrt() } else { speed };
            desired = delta.normalize_or_zero() * speed.min(braking);
            if remaining < Fx::from_ratio(11, 200) {
                body.position = body.route[0];
                body.velocity = FxVec2::ZERO;
                body.route.clear();
                avatar.intent = Some(Intent::Stop);
                desired = FxVec2::ZERO;
            }
        }
        _ => {}
    }
    let change = if desired == FxVec2::ZERO { tuning.braking } else { tuning.acceleration };
    body.velocity = approach(body.velocity, desired, change);
    let previous = body.position;
    let mut delta = body.velocity / per_tick;
    if body.route.len() == 1 {
        let remaining = body.route[0] - previous;
        if delta.length() > remaining.length() {
            delta = remaining;
            body.velocity = FxVec2::ZERO;
        }
    }
    step(body, avatar, terrain, delta, radius);
    let moved = body.position - previous;
    let length = moved.length();
    if length > Fx::from_ratio(1, 1000) {
        let target = nav::bearing(FxVec2::ZERO, moved);
        body.heading += angle_between(body.heading, target) * TURN_BLEND;
    }
}

/// Moves the avatar by `delta`, sliding along obstacles, if the ground
/// (or, in the air, the flight rules) allows it. On foot, it keeps to the
/// ground it reaches.
fn step(body: &mut Body, avatar: &mut AvatarState, terrain: &Terrain, delta: FxVec2, radius: Fx) {
    let previous = body.position;
    let candidate = slide(terrain, previous, delta, radius);
    let airborne = avatar.flight.airborne;
    let allowed = if airborne {
        terrain.flyable(previous, candidate, avatar.height, radius)
    } else {
        terrain.walkable(previous, candidate, radius)
    };
    if allowed {
        if !airborne {
            if let Some(ground) = terrain.footprint(candidate, radius) {
                avatar.height = ground;
            }
        }
        body.position = candidate;
    }
    body.travelled += (body.position - previous).length();
}

/// The jetpack: hover over the highest ground ahead while held, fall back to
/// the ground when released, and keep flying over drops.
fn fly(body: &Body, avatar: &mut AvatarState, terrain: &Terrain, radius: Fx, jetpack: &JetpackDef) {
    let flight = &mut avatar.flight;
    let ground = terrain.footprint(body.position, radius);
    let landing = ground;
    if let Some(y) = ground {
        flight.support = y;
    }
    let active = flight.requested || (flight.airborne && landing.is_none());
    let direction = match body.route.first() {
        Some(&next) => (next - body.position).normalize_or_zero(),
        None => body.velocity.normalize_or_zero(),
    };
    let mut target = ground.unwrap_or(flight.support);
    if active {
        // Look far enough ahead to climb at walking and running speed.
        let r = radius;
        let offsets = [(Fx::ZERO, Fx::ZERO), (r, Fx::ZERO), (-r, Fx::ZERO), (Fx::ZERO, r), (Fx::ZERO, -r)];
        for i in 0..=20 {
            let p = body.position + direction * Fx::from_ratio(i, 10);
            for &(dx, dy) in &offsets {
                if let Some(y) = terrain.ground(FxVec2::new(p.x + dx, p.y + dy)) {
                    target = target.max(y);
                }
            }
        }
        target += jetpack.hover;
    }
    let delta = (target - avatar.height).clamp(-jetpack.sink, jetpack.climb);
    avatar.height += delta;
    flight.airborne = active || ground.is_none_or(|y| avatar.height > y + Fx::from_ratio(1, 100));
}

impl Settlement {
    /// The first player avatar, if there is one.
    pub fn player(&self) -> Option<super::AgentIx> {
        self.agents.iter().position(|a| a.avatar().is_some()).map(|i| i as super::AgentIx)
    }

    /// Commands: names `player`'s avatar, keeping up to `NAME_CHARS`
    /// printable characters.
    pub fn rename(&mut self, player: crate::PlayerId, name: &str) {
        let Some(avatar) = self.avatar_of(player).and_then(|agent| self.agents[agent as usize].avatar_mut()) else { return };
        let name: String = name.chars().filter(|c| !c.is_control()).take(NAME_CHARS).collect();
        avatar.name = name.trim().to_string();
    }

    /// The avatar `player` controls.
    pub fn avatar_of(&self, player: crate::PlayerId) -> Option<AgentIx> {
        self.agents.iter().position(|a| a.avatar().is_some_and(|avatar| avatar.owner == Some(player))).map(|i| i as AgentIx)
    }

    /// Commands: applies a player's action to their avatar (`act`). What it
    /// does to the world goes to `out`, which stage 5 applies
    /// (`apply_actions`) and `AvatarReplay` never does. A pile haul's lab
    /// part (`StartHaul`) is the exception: it applies here, at once.
    pub(crate) fn act(&mut self, tick: u64, me: AgentIx, action: &Action, terrain: &Terrain, out: &mut ActionMessages) {
        // Only owned avatars get commands: commands never drive haulers.
        if !self.agents.get(me as usize).is_some_and(|a| a.avatar().is_some()) {
            return;
        }
        // A driver's movement steers the vehicle lent to them: only they write it.
        if let Some(ix) = self.driving(me as usize) {
            if super::vehicle::drive(&mut self.vehicles[ix as usize], action) {
                return;
            }
        }
        self.act_agent(tick, me, action, terrain, out);
        for StartHaul { agent, start } in std::mem::take(&mut out.haul) {
            self.piles.as_mut().expect("a haul starts only in the pile lab").start_haul(agent, start);
        }
    }

    /// Applies an action to any agent (`action::act`), by its role: a
    /// command's on an avatar, or an accepted job's route on a hauler.
    pub(super) fn act_agent(&mut self, tick: u64, me: AgentIx, action: &Action, terrain: &Terrain, out: &mut ActionMessages) {
        // The view reads no agents, so the agent can be written meanwhile.
        let mut agents = std::mem::take(&mut self.agents);
        let a = &mut agents[me as usize];
        super::action::act(a, action, &self.view(me, tick, terrain), character_of(&self.characters, a), out);
        self.agents = agents;
    }

    /// Stage 5, first in the settlement's part: what this tick's actions did
    /// to the world, by message type in a fixed order (the resources they
    /// touch are separate) and in command order within a type. They come
    /// before everything else stage 5 applies, as the commands they come
    /// from ran before everything else in the tick.
    pub(crate) fn apply_actions(&mut self, messages: ActionMessages, tick: u64, terrain: &mut Terrain) {
        for _ in messages.restart_memory {
            self.restart_memory(terrain);
        }
        for message in messages.drop_haul {
            self.cancel_pile_job(message.agent as usize, message.serial);
            if let Some(avatar) = self.agents[message.agent as usize].avatar_mut() {
                avatar.dropped_haul = false;
            }
        }
        for message in messages.drop_notes {
            self.piano_notes.retain(|note| note.agent != message.agent || note.serial > message.serial);
        }
        self.reserve_stock(messages.reserve_stock);
        self.drop_bundles(messages.drop_bundle, tick, terrain);
        self.board(messages.board, terrain);
        for BundleRefused { agent, reason } in messages.bundle_refused {
            self.events.push(Event::BundleRefused { agent, reason });
        }
        self.install(messages.install);
        self.bursts(messages.burst, tick);
        self.elder_fire(messages.elder_fire, tick);
        for BurstRefused { agent, reason } in messages.burst_refused {
            self.events.push(Event::BurstRefused { agent, reason });
        }
        self.place_blueprints(messages.place_blueprint, terrain);
    }

    /// Stage 5: a hit takes `damage` from player `agent`'s health. At 0 the
    /// player dies and respawns at home later (`self_update`). Death ends
    /// what the `Respawn` action ends: what the avatar was doing, its dig
    /// and route, its carry job (`drop_carry`, applied at once) and its pile
    /// haul. The memory trial restarts at once, as `RestartMemory` does.
    /// Returns whether it died.
    pub(super) fn hurt_player(&mut self, agent: usize, damage: u32, tick: u64, terrain: &mut Terrain) -> bool {
        let AgentState { body, carry, role: Role::Avatar(avatar), .. } = &mut self.agents[agent] else { return false };
        body.health = body.health.saturating_sub(damage);
        if body.health > 0 {
            return false;
        }
        avatar.respawn_at = tick + ticks_from_60hz(120);
        avatar.swing = None;
        avatar.queued = None;
        avatar.queued_at = None;
        body.velocity = FxVec2::ZERO;
        body.push = FxVec2::ZERO;
        avatar.dig = None;
        // The vehicle stays where it is, free for anyone; a building's slot
        // empties in housekeeping (`seat_housekeeping`).
        avatar.seat = None;
        body.route.clear();
        let releases = carry.drop_carry(agent as AgentIx);
        self.reserve_stock(releases);
        self.end_pile_job(agent, "You died. Haul canceled; cargo is retained.", "Haul canceled because the player died");
        self.restart_memory(terrain);
        true
    }

    /// Holds and gives back carry jobs' units on their other side, in
    /// command order. The agent's own side was held when the job was set.
    /// A reservation is checked again here, since another one earlier in the
    /// order may have taken what the action saw: if the other side can no
    /// longer cover it, nothing is reserved there and the agent's job rolls
    /// back, so the first transfer in the order wins. The job that rolls back
    /// is that reservation's: a release from the same agent later in the
    /// order (it dropped that job, and its own side with it) is dropped with
    /// it, and a later reservation means the agent took a new job, which
    /// stands on its own. A delivery to a site must also fit what its next
    /// part still takes, which the inventory alone doesn't know.
    pub(super) fn reserve_stock(&mut self, messages: Vec<ReserveStock>) {
        // Agents whose last reservation was refused, until their job goes.
        let mut refused: Vec<AgentIx> = Vec::new();
        for message in messages {
            let ReserveStock { agent, job, change } = message;
            let fits = match job.other {
                InventoryRef::Site(b) => self.buildings.get(b as usize).is_some_and(|site| job.remaining <= site.wanted(&self.building_kinds[site.kind as usize], job.item)),
                _ => true,
            };
            let other = self.inventory_mut(job.other);
            match change {
                StockChange::Reserve => {
                    refused.retain(|&a| a != agent);
                    if !fits || !job.reserve(other, Side::Other) {
                        refused.push(agent);
                    }
                }
                StockChange::Release => match refused.iter().position(|&a| a == agent) {
                    Some(i) => {
                        refused.swap_remove(i);
                    }
                    None => job.release(other, Side::Other),
                },
            }
        }
        // Each still has the refused job: it ends, and its own side with it.
        // The other side holds nothing, so its release goes nowhere.
        for agent in refused {
            self.agents[agent as usize].carry.drop_carry(agent);
        }
    }

    /// Housekeeping: digs that are done take their layer out of the ground.
    /// In a mine, the layer's ore flies out onto a pile beside the pit.
    pub(crate) fn dig_housekeeping(&mut self, tick: u64, terrain: &mut Terrain) {
        let mut dug = Vec::new();
        for a in &mut self.agents {
            if let Some(dig) = finish_dig(a, tick) {
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
            let item = lab.item(material);
            let Some((id, first)) = lab.mined(item, units, tile, terrain) else { continue };
            self.accounts[item as usize].generated += units as u64;
            let position = lab.piles[&id].position;
            let from = TossEnd::Pit { position: Terrain::pit_center(tile) };
            for slot in first..first + units {
                self.events.push(Event::PileToss { material, from, to: TossEnd::Pile { id, slot, position }, ticks: ORE_FLIGHT_TICKS });
            }
        }
    }

    /// Stage 3 for one avatar: a strike resolving this tick, and manual
    /// transfer units and pile steps that are due. It reads the avatar from
    /// `a`, never from `self`, so `AvatarReplay` runs it on its predicted
    /// avatar against the confirmed settlement.
    pub(super) fn read_avatar(&self, me: AgentIx, a: &AgentState, tick: u64, terrain: &Terrain, out: &mut Messages) {
        let Some(avatar) = a.avatar() else { return };
        if let Some(swing) = resolving(a, tick) {
            let aimed = self.strike_targets(me, avatar.height, a.body.position, swing.aim, swing.weapon, terrain);
            out.strike.push(Strike {
                agent: me,
                weapon: swing.weapon,
                origin: a.body.position,
                forward: swing.aim,
                muzzle: swing.muzzle,
                targets: aimed.targets,
                beam_end: aimed.beam_end,
                cover: aimed.cover,
                hit_player: aimed.hit_player,
                started: swing.pressed(),
            });
        }
        // A depot transfer's unit, or a bundle's at a pile; a haul's are
        // `read_pile_job`'s.
        if let Some(job) = a.carry.job.as_ref().filter(|j| !self.piles.as_ref().is_some_and(|lab| lab.is_haul(j))) {
            if tick >= job.next_tick && distance(a.body.position, self.position_of(job.other)) <= DOCK_REACH {
                out.player_carry.push(CarryUnit { agent: me });
            }
        }
        if let Some(unit) = self.read_pile_job(me, a, tick) {
            out.pile_carry.push(unit);
        }
    }

    /// Housekeeping: a buffered attack starts once the swing ends.
    pub(crate) fn avatar_housekeeping(&mut self, tick: u64) {
        let characters = &self.characters;
        for a in self.agents.iter_mut().filter(|a| a.body.alive()) {
            let def = character_of(characters, a);
            start_queued(a, tick, def);
        }
    }

    /// Records a resolved strike on the avatar: a hit shortens the swing to the rebound.
    pub(crate) fn swing_resolved(&mut self, agent: AgentIx, tick: u64, hits: u32) {
        let a = &mut self.agents[agent as usize];
        let def = character_of(&self.characters, a);
        swing_resolved(a, tick, hits, def);
    }

    /// The carry job `agent`'s bundle action on `other` would start now, or
    /// why it can't (`bundle_job`): for the view and scripts, which read the
    /// same rule the action applies.
    pub fn bundle_job(&self, agent: AgentIx, other: InventoryRef, direction: Direction, tick: u64, terrain: &Terrain) -> Result<CarryJob, BundleRefusal> {
        let a = self.agents.get(agent as usize).ok_or(BundleRefusal::NotAHolder)?;
        bundle_job(&a.carry, &self.view(agent, tick, terrain), other, direction)
    }

    /// The most units `agent` could move between its cargo and `depot` now.
    pub fn transfer_limit(&self, agent: usize, depot: usize, item: usize, deposit: bool) -> u32 {
        self.agents.get(agent).map_or(0, |a| transfer_limit(&a.carry, &self.depots, self.depot_items(), depot, item, deposit))
    }
}

// The avatar's own part of each action and of housekeeping. `Settlement`
// runs them on the world's agents and `AvatarReplay` on a copy, so client
// prediction moves an avatar exactly as the simulation does.

/// Applies an action to a player avatar: its own part, reading the rest of
/// the world through `view` (see `action`), and what it does to the world as
/// messages in `out`. Checks run in the order the command handlers always ran
/// them.
pub(super) fn act(a: &mut AgentState, action: &Action, view: &View, def: &CharacterDef, out: &mut ActionMessages) {
    let AgentState { body, carry, role: Role::Avatar(avatar), .. } = a else { return };
    let me = view.me;
    // A depot transfer. A pile haul is a carry job too, but its walk is the
    // lab's, and these checks treat it as they did before it was one: it
    // doesn't stop a dig, keep a released route or end on a cancel.
    let transferring = carry.job.as_ref().is_some_and(|j| !view.piles.is_some_and(|lab| lab.is_haul(j)));
    match avatar.seat {
        Some(Seat::Vehicle(ix)) => {
            // Sitting in a vehicle, a player can look around, climb out,
            // let go or respawn; movement went to the vehicle (`vehicle::drive`).
            match *action {
                Action::Board => {
                    let v = &view.vehicles[ix as usize];
                    let vehicle_def = &view.characters[v.character as usize];
                    if let Some((position, ground)) = super::vehicle::exit_spot(v, vehicle_def, def.body.radius, view.terrain) {
                        avatar.seat = None;
                        body.position = position;
                        avatar.height = ground;
                        avatar.flight.support = ground;
                        avatar.intent = Some(Intent::Stop);
                    }
                    return;
                }
                Action::Aim(_) | Action::Equip(_) | Action::Release | Action::Respawn => {}
                Action::Burst => {
                    out.burst_refused.push(BurstRefused { agent: me, reason: BurstRefusal::NotPossessing });
                    return;
                }
                _ => return,
            }
        }
        Some(Seat::Building(b)) => {
            // Possessing a building (D3), a player can look around, Burst,
            // leave (boarding again, or interacting: the seat is a seated
            // elder's one target), let go or respawn. Movement and dropping
            // do nothing. Leaving puts it beside the building; housekeeping
            // empties the slot (`seat_housekeeping`), for a waiting sprite
            // to take. In a ballista (B3) the aim is the reticle, an offset
            // from the ballista within its range, and `Attack` shoots there;
            // a seated elder never swings its own weapon.
            let kind = &view.building_kinds[view.buildings[b as usize].kind as usize];
            match *action {
                Action::Board | Action::Interact(interact::InteractTarget::Slot(_)) => {
                    if matches!(*action, Action::Interact(target) if target != interact::InteractTarget::Slot(b)) {
                        return;
                    }
                    if let Some(position) = view.leave_spot(b, def.body.radius) {
                        let ground = view.terrain.footprint(position, def.body.radius).unwrap_or(avatar.height);
                        avatar.seat = None;
                        // A ballista's reticle back to a facing.
                        avatar.aim = avatar.aim.map(FxVec2::normalize_or_zero).filter(|aim| *aim != FxVec2::ZERO);
                        body.position = position;
                        avatar.height = ground;
                        avatar.flight.support = ground;
                        avatar.intent = Some(Intent::Stop);
                    }
                    return;
                }
                // Stage 5 checks the cooldown and the output's room, and
                // adds the units (`Settlement::bursts`).
                Action::Burst => {
                    if body.alive() {
                        out.burst.push(Burst { agent: me });
                    }
                    return;
                }
                Action::Aim(offset) if kind.defence.is_some() => {
                    if let Some(reticle) = kind.reticle(offset) {
                        avatar.aim = Some(reticle);
                    }
                    return;
                }
                // Stage 5 checks the elder still sits there, the ammo and
                // the reload, and shoots (`Settlement::elder_fire`).
                Action::Attack { aim, .. } => {
                    if let Some(reticle) = kind.reticle(aim).filter(|_| body.alive()) {
                        avatar.aim = Some(reticle);
                        out.elder_fire.push(ElderFire { agent: me, building: b, to: view.buildings[b as usize].position + reticle });
                    }
                    return;
                }
                Action::Aim(_) | Action::Equip(_) | Action::Release | Action::Respawn => {}
                _ => return,
            }
        }
        None => {}
    }
    match *action {
        Action::Board => {
            if body.alive() && avatar.dig.is_none() {
                out.board.push(crate::messages::Board { agent: me, building: None });
            }
        }
        Action::Move(ref intent) => {
            // Moving lets go of a pile haul, dead or alive.
            drop_haul(avatar, view, out);
            if body.alive() {
                set_intent(body, avatar, intent.clone(), view.terrain, def);
            }
        }
        // A planned route is a hauler's; no command sends one.
        Action::Follow(_) => {}
        Action::Aim(aim) => aim_at(avatar, aim),
        Action::Attack { weapon, aim, muzzle } => {
            if view.has_weapon(weapon) {
                drop_haul(avatar, view, out);
                if body.alive() {
                    attack(body, avatar, view.tick, weapon, aim, muzzle, def);
                }
            }
        }
        Action::Equip(weapon) => {
            if view.has_weapon(weapon) {
                avatar.weapon = weapon;
            }
        }
        Action::Jetpack(enabled) => jetpack(avatar, enabled, def),
        Action::Jump => jump(avatar, view.tick, def),
        Action::DigDown => {
            if view.can_dig_at(body.position) {
                dig(body, avatar, transferring, view.tick, view.terrain, def);
            }
        }
        Action::Transfer { depot, item, units, deposit } => {
            // One carry job at a time: a pile haul's too.
            let busy = carry.job.is_some() || carry.inventory.total_in() > 0 || view.leaving_cargo();
            if !busy && units > 0 && transfer_limit(carry, view.depots, view.items, depot as usize, item as usize, deposit) >= units {
                let direction = if deposit { Direction::Deposit } else { Direction::Pickup };
                let job = CarryJob { other: InventoryRef::Depot(depot), item, direction, remaining: units, next_tick: view.tick };
                // The job holds its units in the cargo now, and at the depot
                // in stage 5.
                let reserved = job.reserve(&mut carry.inventory, Side::Own);
                debug_assert!(reserved, "`transfer_limit` checked the cargo");
                carry.job = Some(job.clone());
                out.reserve_stock.push(ReserveStock { agent: me, job, change: StockChange::Reserve });
            }
        }
        Action::Bundle { other, direction } => match bundle_job(carry, view, other, direction) {
            Ok(job) => start_bundle(carry, job, me, out),
            Err(reason) => out.bundle_refused.push(BundleRefused { agent: me, reason }),
        },
        Action::Interact(target) => {
            // The target the client highlighted, checked again: still in
            // reach and still usable. A sprite's is the app's job menu, so
            // the sim has nothing to do for it.
            let at = interact::position(view, target);
            if !body.alive() || matches!(target, interact::InteractTarget::Sprite(_)) {
            } else if !at.is_some_and(|p| distance(body.position, p) <= interact::INTERACT_REACH) {
                out.bundle_refused.push(BundleRefused { agent: me, reason: BundleRefusal::OutOfReach });
            } else {
                match interact::interaction(view, carry, target) {
                    Ok(Interaction::Bundle(job)) => {
                        let spot = interact::stand_spot(view, job.other, body.position);
                        start_bundle(carry, job, me, out);
                        // Step in to the dock, as the verbs walk the elder
                        // there; the units move once it is in reach.
                        if let (Some(at), Some(spot)) = (at, spot) {
                            if distance(body.position, at) > DOCK_REACH {
                                set_intent(body, avatar, Intent::GoTo { target: spot, run: false }, view.terrain, def);
                            }
                        }
                    }
                    Ok(Interaction::Install { building, .. }) => out.install.push(Install { agent: me, building }),
                    // Stage 5 seats the elder, putting out a sprite there.
                    Ok(Interaction::Board { building }) => out.board.push(crate::messages::Board { agent: me, building: Some(building) }),
                    // A site's needs are the view's to show; leaving is the
                    // seated elder's (above).
                    Ok(Interaction::JobMenu | Interaction::Needs { .. } | Interaction::Leave { .. }) => {}
                    Err(reason) => out.bundle_refused.push(BundleRefused { agent: me, reason }),
                }
            }
        }
        Action::Drop => {
            // Checked here for the refusal's sake, and again in stage 5,
            // which makes the pile: another drop may have come first.
            if body.alive() && view.piles.is_some() {
                match super::piles::drop_plan(carry, body, view) {
                    Ok(_) => out.drop_bundle.push(DropBundle { agent: me }),
                    Err(reason) => out.bundle_refused.push(BundleRefused { agent: me, reason }),
                }
            }
        }
        // A seated elder's alone (above).
        Action::Burst => out.burst_refused.push(BurstRefused { agent: me, reason: BurstRefusal::NotPossessing }),
        Action::CancelTransfer => {
            if transferring {
                out.reserve_stock.extend(carry.drop_carry(me));
            }
        }
        Action::RepairCover(asset) => {
            avatar.repair = asset.filter(|&asset| {
                view.terrain.destructibles.iter().any(|piece| piece.asset == asset && piece.repairable && piece.health < piece.max_health)
            });
        }
        Action::Release => {
            // Letting go releases held piano keys.
            out.drop_notes.push(DropNotes { agent: me, serial: view.piano_serial });
            release(body, avatar, transferring, view.hauling() && !avatar.dropped_haul);
        }
        Action::Haul { action, pace, approach } => {
            if let Some(start) = super::piles::haul(body, carry, avatar, view, def.body.radius, action, pace, approach) {
                out.haul.push(StartHaul { agent: me, start });
            }
        }
        Action::Respawn => {
            // Respawning restarts the memory trial, closing its door again.
            out.restart_memory.push(RestartMemory { agent: me });
            // The fresh avatar has no job, so the old one gives back its
            // stock: a depot transfer's or a pile haul's.
            out.reserve_stock.extend(carry.drop_carry(me));
            respawn(body, avatar, view.spawn, view.spawn_height);
            drop_haul(avatar, view, out);
        }
    }
}

/// Starts a bundle's carry job, as a transfer does: its cargo side now,
/// the other in stage 5.
fn start_bundle(carry: &mut Carry, job: CarryJob, me: AgentIx, out: &mut ActionMessages) {
    let reserved = job.reserve(&mut carry.inventory, Side::Own);
    debug_assert!(reserved, "`bundle_job` sized it to the cargo");
    carry.job = Some(job.clone());
    out.reserve_stock.push(ReserveStock { agent: me, job, change: StockChange::Reserve });
}

/// Lets go of the agent's pile haul, if one runs: `DropHaul` ends it in
/// stage 5, and until then the haul neither steers the avatar (stage 2)
/// nor moves units (stage 3).
fn drop_haul(avatar: &mut AvatarState, view: &View, out: &mut ActionMessages) {
    if avatar.dropped_haul {
        return;
    }
    if let Some(serial) = view.haul() {
        avatar.dropped_haul = true;
        out.drop_haul.push(DropHaul { agent: view.me, serial });
    }
}

/// See `Settlement::transfer_limit`. `items` are the depot items
/// (`Settlement::depot_items`): minerals don't go to depots. The cargo's
/// one capacity holds depot items and minerals in any mix, unless it is a
/// bundle (`OneKind`), and a tree pile (`Kinds::Only`) takes its item only.
fn transfer_limit(carry: &Carry, depots: &[DepotState], items: usize, depot: usize, item: usize, deposit: bool) -> u32 {
    let Some(d) = depots.get(depot) else { return 0 };
    let item_ix = item as ItemIx;
    if item >= items || !d.inventory.accepts(item_ix) || !carry.inventory.accepts(item_ix) {
        return 0;
    }
    if deposit {
        // Units not held by a running job.
        carry.inventory.available(item as ItemIx).min(d.inventory.free())
    } else {
        // Space not taken by units carried, flying in or held by a job.
        carry.inventory.free().min(d.inventory.available(item as ItemIx))
    }
}

/// Why a bundle action can't start (`bundle_job`), or an interact, a drop
/// or boarding a building. Stage 5 reports it as `Event::BundleRefused`;
/// `message` says it for the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BundleRefusal {
    /// A carry job is running, or units are still in the air.
    Busy,
    /// Not a depot or a building output there is.
    NotAHolder,
    /// A building output takes nothing back: it is picked up from only.
    PickupOnly,
    /// Nothing there that isn't promised to someone else.
    NothingToTake,
    /// The bundle holds another item than the one there (D6).
    OtherItem,
    /// The bundle has no room left.
    BundleFull,
    /// The bundle holds nothing to deposit.
    NothingCarried,
    /// The pile takes another item: wood goes to the wood pile (D6).
    WrongPile,
    /// The pile has no room left.
    PileFull,
    /// An interact target that is no longer in reach (`interact`).
    OutOfReach,
    /// A sprite's job is set empty-handed only (`interact`, D5).
    HandsFull,
    /// A site only takes deliveries (D4).
    DepositOnly,
    /// The site's next part takes none of what the bundle holds.
    NotNeeded,
    /// A drop with nothing in the bundle (`piles::drop_plan`).
    NothingToDrop,
    /// A drop would make a new pile, and the ground has its most.
    TooManyPiles,
    /// A drop's new pile has no clear ground at the agent's feet: an
    /// obstacle, another pile, a dock or a building's spot, or the edge.
    NoRoom,
    /// Boarding a building with a bundle in hand (D3): drop it first.
    Carrying,
    /// Boarding a building another elder possesses.
    Possessed,
}

impl BundleRefusal {
    /// The refusal in words, for the view.
    pub fn message(self) -> &'static str {
        match self {
            BundleRefusal::Busy => "Still moving the last load",
            BundleRefusal::NotAHolder => "Nothing to carry there",
            BundleRefusal::PickupOnly => "Outputs are picked up from, not filled",
            BundleRefusal::NothingToTake => "Nothing there to pick up",
            BundleRefusal::OtherItem => "Carrying something else: deposit it first",
            BundleRefusal::BundleFull => "The bundle is full",
            BundleRefusal::NothingCarried => "Carrying nothing to deposit",
            BundleRefusal::WrongPile => "That pile takes something else",
            BundleRefusal::PileFull => "That pile is full",
            BundleRefusal::OutOfReach => "Too far away",
            BundleRefusal::HandsFull => "Hands full: deposit the bundle first",
            BundleRefusal::DepositOnly => "A site only takes deliveries",
            BundleRefusal::NotNeeded => "The site doesn't need that",
            BundleRefusal::NothingToDrop => "Nothing carried",
            BundleRefusal::TooManyPiles => "Too many piles: pick one up first",
            BundleRefusal::NoRoom => "No room to drop it here",
            BundleRefusal::Carrying => "Drop the bundle first (Q)",
            BundleRefusal::Possessed => "Another elder is in there",
        }
    }
}

/// The carry job a bundle action starts on `other` (D6), or why it can't:
/// a pickup (from a building output, or a withdrawal from a depot) tops the
/// bundle up with the holder's item, as many units as fit; a deposit moves
/// every unit of the bundle's item the depot has room for, and a delivery
/// to a site every unit its next part still takes (D4). Only depots,
/// building outputs, sites and ground piles are holders here; outputs take
/// no deposits, and sites nothing else. A ground pile tops the bundle up as
/// an output does, and takes a deposit of its own item (a drop's merge,
/// `piles::drop_plan`); minerals in it are a haul's, not a bundle's. Its
/// units move once the agent is at the holder (`read_avatar`), as a
/// transfer's do. M2.2's interact acts through this too.
pub fn bundle_job(carry: &Carry, view: &View, other: InventoryRef, direction: Direction) -> Result<CarryJob, BundleRefusal> {
    if carry.job.is_some() || carry.inventory.total_in() > 0 || view.leaving_cargo() {
        return Err(BundleRefusal::Busy);
    }
    let holder = match other {
        InventoryRef::Depot(d) => view.depots.get(d as usize).map(|d| &d.inventory),
        InventoryRef::Output(b) => view.buildings.get(b as usize).map(|b| &b.output),
        InventoryRef::Site(b) => view.buildings.get(b as usize).and_then(|b| b.site()).map(|(_, materials)| materials),
        // A dispenser's output is the pile lab's alone.
        InventoryRef::Pile(id) => view.piles.and_then(|lab| lab.piles.get(&id)).filter(|p| p.source.is_none()).map(|p| &p.inventory),
        InventoryRef::Agent(_) | InventoryRef::Collector => None,
    };
    let Some(holder) = holder else { return Err(BundleRefusal::NotAHolder) };
    let cargo = &carry.inventory;
    // Depot items only: minerals don't go to depots or come out of them.
    let depot_item = |item: &ItemIx| (*item as usize) < view.items;
    let (item, units) = match direction {
        // A site is a drop-off: nothing is taken back out (D4), nor out of
        // a ballista's ammo store (B2).
        Direction::Pickup if matches!(other, InventoryRef::Site(_)) => return Err(BundleRefusal::DepositOnly),
        Direction::Pickup if matches!(other, InventoryRef::Output(b) if super::interact::ammo_store(view, b)) => return Err(BundleRefusal::DepositOnly),
        Direction::Pickup => {
            // The holder's item: a tree pile's own, what an output holds, or
            // in a mixed depot the bundle's item, else the first there.
            let item = match holder.kinds {
                Kinds::Only(item) => Some(item),
                Kinds::OneKind => holder.kind(),
                Kinds::AnyMix => cargo.kind().filter(depot_item).or_else(|| (0..view.items as ItemIx).find(|&i| holder.available(i) > 0)),
            };
            let Some(item) = item.filter(depot_item) else { return Err(BundleRefusal::NothingToTake) };
            if !cargo.accepts(item) {
                return Err(BundleRefusal::OtherItem);
            }
            if holder.available(item) == 0 {
                return Err(BundleRefusal::NothingToTake);
            }
            if cargo.free() == 0 {
                return Err(BundleRefusal::BundleFull);
            }
            (item, cargo.free().min(holder.available(item)))
        }
        Direction::Deposit => {
            // A gatherer's output is only picked up from; a ballista's ammo
            // store takes as much of its ammo as fits.
            if matches!(other, InventoryRef::Output(b) if !super::interact::ammo_store(view, b)) {
                return Err(BundleRefusal::PickupOnly);
            }
            let Some(item) = (0..view.items as ItemIx).find(|&i| cargo.available(i) > 0) else { return Err(BundleRefusal::NothingCarried) };
            if let InventoryRef::Site(b) = other {
                // Every unit the next part still takes, the rest staying in
                // the bundle.
                let site = &view.buildings[b as usize];
                let wanted = site.wanted(&view.building_kinds[site.kind as usize], item);
                if wanted == 0 {
                    return Err(BundleRefusal::NotNeeded);
                }
                return Ok(CarryJob { other, item, direction, remaining: cargo.available(item).min(wanted), next_tick: view.tick });
            }
            if !holder.accepts(item) {
                return Err(BundleRefusal::WrongPile);
            }
            if holder.free() == 0 {
                return Err(BundleRefusal::PileFull);
            }
            (item, cargo.available(item).min(holder.free()))
        }
    };
    Ok(CarryJob { other, item, direction, remaining: units, next_tick: view.tick })
}

/// Sets a live avatar's intent. Walking off stops a dig; a `GoTo` plans its
/// route.
fn set_intent(body: &mut Body, avatar: &mut AvatarState, intent: Intent, terrain: &Terrain, def: &CharacterDef) {
    // Walking off stops a dig.
    let walking = match intent {
        Intent::Stop => false,
        Intent::Walk { direction, .. } => direction != FxVec2::ZERO,
        Intent::GoTo { .. } => true,
    };
    if avatar.dig.is_some() && !walking {
        return;
    }
    avatar.dig = None;
    body.route.clear();
    if let Intent::GoTo { target, run } = intent {
        // On foot, routes avoid steps and drops; with the jetpack, anything goes.
        let flying = avatar.flight.requested || avatar.flight.airborne;
        let radius = def.body.radius;
        let walkable = |p: FxVec2, q: FxVec2| flying || terrain.walkable(p, q, radius);
        body.route = nav::path_with(terrain, body.position, target, radius, &[], &walkable);
        avatar.run = run;
    } else if let Intent::Walk { run, .. } = intent {
        avatar.run = run;
    }
    avatar.intent = Some(intent);
}

/// The player stopped driving the avatar directly (pause, focus loss, a
/// controller disconnecting). The jetpack always switches off. Manual or
/// click travel stops, unless an accepted logistics job owns the route: a
/// pile haul (`hauling`), or a depot transfer walking to its dock. Both are
/// judged on the state this tick, after any earlier command in the same
/// turn, so a haul accepted or rejected just before still decides. Unlike
/// `Action::Move(Stop)` this never cancels a pile job.
fn release(body: &mut Body, avatar: &mut AvatarState, carrying: bool, hauling: bool) {
    avatar.flight.requested = false;
    avatar.repair = None;
    let docking = carrying && matches!(avatar.intent, Some(Intent::GoTo { .. }));
    if hauling || docking {
        return;
    }
    if matches!(avatar.intent, Some(Intent::Walk { .. } | Intent::GoTo { .. })) {
        body.route.clear();
        avatar.intent = Some(Intent::Stop);
    }
}

fn aim_at(avatar: &mut AvatarState, aim: FxVec2) {
    let aim = aim.normalize_or_zero();
    if aim != FxVec2::ZERO {
        avatar.aim = Some(aim);
    }
}

/// Starts a live avatar's attack with a weapon the scene offers, or buffers
/// it during a swing.
fn attack(body: &mut Body, avatar: &mut AvatarState, tick: u64, weapon: Weapon, aim: FxVec2, muzzle: [Fx; 3], def: &CharacterDef) {
    if avatar.dig.is_some() {
        return;
    }
    let aim = aim.normalize_or_zero();
    if aim == FxVec2::ZERO || muzzle.iter().any(|v| v.abs() > Fx::from_int(4)) {
        return;
    }
    match &avatar.swing {
        Some(swing) if tick < swing.until => {
            avatar.queued = Some((weapon, aim, muzzle));
            avatar.queued_at = Some(tick);
        }
        _ => start_swing(body, avatar, tick, weapon, aim, muzzle, def.swift_sword(), avatar_tuning(def).attack),
    }
}

/// Housekeeping for a live avatar: a buffered attack starts once the swing ends.
pub(super) fn start_queued(a: &mut AgentState, tick: u64, def: &CharacterDef) {
    let AgentState { body, role: Role::Avatar(avatar), .. } = a else { return };
    let finished = avatar.swing.as_ref().is_none_or(|s| tick + 1 >= s.until);
    if finished {
        if let Some((weapon, aim, muzzle)) = avatar.queued.take() {
            let pressed = avatar.queued_at.take();
            start_swing(body, avatar, tick + 1, weapon, aim, muzzle, def.swift_sword(), avatar_tuning(def).attack);
            if let Some(swing) = &mut avatar.swing {
                swing.queued_at = pressed;
            }
        }
    }
}

/// The swing that resolves on this tick, if the weapon is ready.
pub(super) fn resolving(a: &AgentState, tick: u64) -> Option<&Swing> {
    let avatar = a.avatar()?;
    avatar.swing.as_ref().filter(|swing| !swing.resolved && swing.resolve_at == tick && avatar.attack_ready <= tick)
}

/// Records a resolved strike: a hit shortens the swing to the rebound.
pub(super) fn swing_resolved(a: &mut AgentState, tick: u64, hits: u32, def: &CharacterDef) {
    let Some(avatar) = a.avatar_mut() else { return };
    let rebound = avatar_tuning(def).attack.rebound;
    if let Some(swing) = &mut avatar.swing {
        swing.resolved = true;
        swing.hits = hits;
        if hits > 0 && matches!(swing.weapon, Weapon::Sword | Weapon::Spear) {
            swing.until = tick + rebound;
        }
    }
}

fn jetpack(avatar: &mut AvatarState, enabled: bool, def: &CharacterDef) {
    if def.jetpack().is_none() {
        return;
    }
    avatar.flight.requested = enabled;
    if enabled {
        avatar.dig = None;
    }
}

fn jump(avatar: &mut AvatarState, tick: u64, def: &CharacterDef) {
    if def.jump().is_none() {
        return;
    }
    if matches!(avatar.jump, JumpPhase::Grounded)
        && !avatar.flight.airborne
        && avatar.dig.is_none()
        && avatar.swing.is_none()
    {
        avatar.jump = JumpPhase::Preparing { started: tick };
    }
}

/// Starts digging where `View::can_dig_at` allows it, if the character
/// digs, stands on the ground with its hands free, and the pit isn't as deep
/// as it goes.
fn dig(body: &mut Body, avatar: &mut AvatarState, carrying: bool, tick: u64, terrain: &Terrain, def: &CharacterDef) {
    let Some(ticks) = def.dig().map(|d| d.ticks) else { return };
    let busy = avatar.swing.as_ref().is_some_and(|s| tick < s.until) || carrying;
    let flight = &avatar.flight;
    if busy || avatar.dig.is_some() || flight.requested || flight.airborne {
        return;
    }
    let tile = Terrain::pit_tile(body.position);
    if terrain.pit(tile) >= MAX_LAYERS || !terrain.inside(Terrain::pit_center(tile)) {
        return;
    }
    body.route.clear();
    avatar.intent = Some(Intent::Stop);
    avatar.dig = Some(Dig { tile, started: tick, until: tick + ticks });
}

/// Housekeeping: ends a dig that is done, returning it.
pub(super) fn finish_dig(a: &mut AgentState, tick: u64) -> Option<Dig> {
    a.avatar_mut()?.dig.take_if(|d| tick + 1 >= d.until)
}

/// Puts the avatar at the spawn point with a fresh state. The player's
/// owner, name and weapon and Burst cooldowns stay. So do the body's speed,
/// radius and max health. Its motion starts over and its health refills, as
/// after a death.
fn respawn(body: &mut Body, avatar: &mut AvatarState, spawn: FxVec2, height: Fx) {
    body.position = spawn;
    body.heading = Fx::PI;
    body.route.clear();
    body.velocity = FxVec2::ZERO;
    body.push = FxVec2::ZERO;
    body.health = body.max_health;
    *avatar = AvatarState {
        owner: avatar.owner,
        name: std::mem::take(&mut avatar.name),
        attack_ready: avatar.attack_ready,
        burst_ready: avatar.burst_ready,
        height,
        intent: Some(Intent::Stop),
        ..AvatarState::default()
    };
}

fn start_swing(body: &mut Body, avatar: &mut AvatarState, tick: u64, weapon: Weapon, aim: FxVec2, muzzle: [Fx; 3], swift_sword: bool, timing: AttackTiming) {
    let (stroke, next) = SwordStroke::combo(weapon, avatar.combo_next, tick <= avatar.combo_until);
    avatar.combo_next = next;
    avatar.combo_until = tick + timing.combo_window;
    avatar.weapon = weapon;
    if weapon == Weapon::Laser {
        avatar.aim = Some(aim);
    }
    if weapon != Weapon::Laser {
        body.route.clear();
        avatar.intent = Some(Intent::Stop);
    }
    avatar.swing = Some(Swing {
        weapon,
        stroke,
        aim,
        muzzle,
        started: tick,
        resolve_at: tick + if weapon == Weapon::Laser { ticks_from_60hz(2) } else if weapon == Weapon::Sword && swift_sword { timing.sword_windup } else { timing.windup },
        until: tick + if weapon == Weapon::Laser { ticks_from_60hz(8) } else if weapon == Weapon::Sword && swift_sword { timing.sword_swing } else { timing.swing },
        resolved: false,
        hits: 0,
        queued_at: None,
    });
}

/// An agent's character definition.
pub(crate) fn character_of<'a>(characters: &'a [CharacterDef], a: &AgentState) -> &'a CharacterDef {
    &characters[a.character as usize]
}

/// The tuning a player character carries; validated when the world is built.
fn avatar_tuning(def: &CharacterDef) -> &AvatarDef {
    def.avatar.as_ref().expect("player characters have avatar tuning")
}
