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

use content::character::{AttackDef, CharacterDef};
use content::scenario::{ticks_from_60hz, ProjectileKind, Weapon, WeaponTuning};
use content::TICKS_PER_SECOND;
use fx::{Fx, FxVec2};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::{AgentIx, BuildingIx, DepotIx, InventoryRef, ItemIx, Settlement};
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
/// A lane's ghouls set off this far apart on a night (5 s), a column, or
/// closer when that would run past the night's first third
/// ([`lane_starts`]).
pub const COLUMN_GAP: u64 = content::millis_to_ticks(5000);
/// At sunrise a ghoul burns this long (2 s), and up to as long again, by
/// its id, before it falls (`Settlement::dawn`).
pub const DAWN_BURN: u64 = content::millis_to_ticks(2000);
/// A threat's swing reaches a player whose feet are at most this high
/// above the ground under it, in meters. A jump (apex about 0.57 m) doesn't
/// dodge it; the jetpack's hover (1.8 m) does. Every attacking character is
/// a ghoul of one height, so it is a rule, not a character field.
pub const SWING_HEIGHT: Fx = Fx::ONE;
/// How far apart two bodies' feet may be and still stand on one floor, in
/// a volume scene (M8.2): a swing, a strike, a shot, a fireball or a fire
/// reaches a threat (or a threat a body) only within it. Floors stacked in
/// one column are at least a body's headroom and a layer apart (2.5 m), so
/// a ghoul on the ground can't hit a sprite in the tunnel under it; a ramp
/// or a stair a layer or two off still counts as the same floor.
pub const FLOOR_REACH: Fx = Fx::from_ratio(3, 2);

/// Whether feet at `a` and `b` stand on one floor (`FLOOR_REACH`) in a
/// volume scene; always elsewhere, where every body walks on the ground.
pub(super) fn same_floor(terrain: &Terrain, a: Fx, b: Fx) -> bool {
    terrain.volume.is_none() || (a - b).abs() <= FLOOR_REACH
}

/// A stable counter-based roll, as in the original game: outcomes never
/// depend on how many unrelated hits happened elsewhere.
pub fn combat_roll(tick: u64, entity: u64, stream: u64) -> u64 {
    let mut x = tick ^ entity.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ stream;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// The `FollowRoute` brain, a threat's stage 2: a shock freezes it, a
/// stagger slides it back, a windup plants it, and otherwise it walks its
/// plan at `speed` meters per tick, slower once below `full_hp`: its route,
/// or, chasing (`chase`), straight at `quarry`, the point it chases, to
/// `stop` short of it. Returns the meters knockback moved it.
fn follow_route(t: &mut Threat, speed: Fx, full_hp: u32, tick: u64, terrain: &Terrain, quarry: Option<FxVec2>, stop: Fx) -> Fx {
    let previous = t.position;
    let recoiling = tick < t.stagger_until;
    let mut knocked = Fx::ZERO;
    if t.burn.is_some_and(|b| b.dawn) {
        // Burning in the sunrise, it stands until it falls.
    } else if tick < t.shocked_until {
        // The coil freezes the whole body, then the route resumes.
    } else if recoiling {
        let next = t.position + t.knockback.div_int(content::TICKS_PER_SECOND as i64);
        if super::chase::walk_clear(terrain, t.position, next) {
            t.position = next;
            knocked = distance(previous, next);
        }
        t.knockback = t.knockback * KNOCKBACK_DECAY;
    } else if t.windup.is_some() {
        // Planted for the swing; the route resumes after it lands.
    } else {
        let wounded = if t.hp < full_hp { Fx::from_ratio(11, 20) } else { Fx::ONE };
        match quarry {
            Some(to) => super::chase::steer(&mut t.position, to, stop, speed * wounded, terrain),
            None => {
                super::haulers::move_route(&mut t.position, &mut t.route, speed * wounded);
                // Stranded, it waits at its route's end instead of breaching.
                if t.stranded && t.route.is_empty() {
                    t.route.push(t.position);
                }
            }
        }
    }
    let moved = distance(previous, t.position);
    t.travelled += moved;
    if moved > Fx::ZERO && !recoiling {
        t.heading = nav::bearing(previous, t.position);
    }
    knocked
}

/// [`follow_route`] in a volume scene (M8.2): the same brain, on foot with
/// a feet height (`Threat::height`, `Terrain::walk_at`). A chaser walks to
/// `quarry` (its point and feet) by `chase::chase_at`; otherwise it walks
/// its route, and if the ground refuses a step (a dig made it stale) it
/// plans its way to its wave's `target` again from where it stands, when
/// its backoff allows.
#[allow(clippy::too_many_arguments)]
fn follow_route_at(t: &mut Threat, speed: Fx, full_hp: u32, tick: u64, terrain: &Terrain, quarry: Option<(FxVec2, Fx)>, stop: Fx, target: Option<FxVec2>) -> Fx {
    let previous = t.position;
    let recoiling = tick < t.stagger_until;
    let mut knocked = Fx::ZERO;
    if t.burn.is_some_and(|b| b.dawn) || tick < t.shocked_until || (!recoiling && t.windup.is_some()) {
        // Burning in the sunrise, shocked, or planted for a swing.
    } else if recoiling {
        let next = t.position + t.knockback.div_int(content::TICKS_PER_SECOND as i64);
        if terrain.clear(t.position, next, super::chase::WALK_CLEARANCE) {
            if let Some(feet) = terrain.walk_at(t.position, next, t.height, super::chase::WALK_CLEARANCE) {
                t.position = next;
                t.height = feet;
                knocked = distance(previous, next);
            }
        }
        t.knockback = t.knockback * KNOCKBACK_DECAY;
    } else {
        let wounded = if t.hp < full_hp { Fx::from_ratio(11, 20) } else { Fx::ONE };
        match quarry {
            Some((to, feet)) => super::chase::chase_at(t, to, feet, stop, speed * wounded, tick, terrain),
            None => {
                let walked = super::chase::walk_route_at(&mut t.position, &mut t.height, &mut t.route, speed * wounded, terrain);
                if let Some(target) = target.filter(|_| !walked && t.backoff.due(tick)) {
                    let (route, reaches) = super::chase::route_home_at(terrain, t.position, t.height, target);
                    t.backoff.planned(tick, reaches, 0);
                    if !route.is_empty() {
                        t.route = route;
                        t.stranded = !reaches;
                    }
                }
                // Stranded, it waits at its route's end instead of breaching.
                if t.stranded && t.route.is_empty() {
                    t.route.push(t.position);
                }
            }
        }
    }
    let moved = distance(previous, t.position);
    t.travelled += moved;
    if moved > Fx::ZERO && !recoiling {
        t.heading = nav::bearing(previous, t.position);
    }
    knocked
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveState {
    pub id: String,
    /// The enemy it sends.
    pub character: content::character::CharacterIx,
    pub position: FxVec2,
    pub target: FxVec2,
    /// A timed wave: its first enemy on this tick, then one every `every`
    /// ticks, `count` in all. Zero for a lane.
    pub first: u64,
    pub every: u64,
    pub count: u32,
    /// Enemies sent so far: of `count`, or by a lane over all its nights.
    pub spawned: u32,
    /// Every enemy of a wave follows the same planned route.
    pub route: Option<Vec<FxVec2>>,
    /// A lane (`content::scenario::Wave::nights`): the enemies it sends on
    /// each night, the last repeating. Empty for a timed wave.
    #[serde(default)]
    pub nights: Vec<u32>,
}

impl WaveState {
    /// Whether it sends by night rather than on a timer.
    pub fn lane(&self) -> bool {
        !self.nights.is_empty()
    }

    /// Whether it may send more enemies: a timed wave until its `count`, a
    /// lane every night.
    pub fn sending(&self) -> bool {
        self.lane() || self.spawned < self.count
    }

    /// How many a lane sends on night `night` (1 the first): the last
    /// listed night's count after the list runs out. Zero for a timed wave
    /// and before the first night.
    pub fn on_night(&self, night: u64) -> u32 {
        let listed = self.nights.len() as u64;
        if night == 0 || listed == 0 {
            return 0;
        }
        self.nights[(night.min(listed) - 1) as usize]
    }
}

/// When a lane's `count` ghouls set off on a night of `night` ticks, in
/// ticks after dusk: [`COLUMN_GAP`] apart from dusk, or spread evenly over
/// the night's first third if that would run past it.
pub fn lane_starts(count: u32, night: u64) -> impl Iterator<Item = u64> {
    let gap = if count == 0 { 0 } else { COLUMN_GAP.min(night / 3 / count as u64) };
    (0..count as u64).map(move |i| i * gap)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Threat {
    pub id: u64,
    /// The wave that sent it: where it came from, not what it is.
    pub wave: u16,
    /// What it is: its radius, attack, speed, health and breach damage.
    pub character: content::character::CharacterIx,
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
    /// Burning, from standing in a fire (B4, `fire`).
    #[serde(default)]
    pub burn: Option<super::fire::Burn>,
    /// What it is after: the tree, along `route`, or a quarry it chases
    /// (B5, `chase`). Always `Tree` for a character without a `chase`.
    #[serde(default)]
    pub goal: super::chase::Goal,
    /// Its route ends short of its wave's target: no walkable way reaches
    /// the tree (a moat cut its lane, M6.7). It walks to the route's end,
    /// the reachable point nearest the tree, and waits there, still chasing
    /// whoever comes near, and plans again now and then
    /// (`Settlement::retry_stranded`) and when the ground changes.
    #[serde(default)]
    pub stranded: bool,
    /// Its feet's height in a volume scene (M8.2): the floor it walks on,
    /// as `SpriteState::height` is a sprite's. Zero elsewhere, where
    /// threats walk on `Terrain::ground`.
    #[serde(default)]
    pub height: Fx,
    /// A chaser's route to its quarry in a volume scene (M8.2): a 3D path
    /// (`nav::path_toward_at`), planned again when `backoff` says, as far
    /// as it gets when the quarry is out of reach. Empty elsewhere, where a
    /// chaser walks straight at its quarry, and while it walks `route`.
    #[serde(default)]
    pub chase: Vec<FxVec2>,
    /// When it may plan again in a volume scene: a chase, or its route
    /// home after the ground refused a step (M8.2).
    #[serde(default)]
    pub backoff: super::Backoff,
}

/// What a threat swings at: an agent (a player in reach, or the elder or
/// sprite it chases) or the building it chases (B8). Agents sort first, so
/// a threat with both in reach at one distance swings at the agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SwingTarget {
    Agent(AgentIx),
    Building(BuildingIx),
}

impl SwingTarget {
    /// The agent it swings at, if it swings at one.
    pub fn agent(self) -> Option<AgentIx> {
        match self {
            SwingTarget::Agent(a) => Some(a),
            SwingTarget::Building(_) => None,
        }
    }
}

/// A threat's swing on its way (`Threat::windup`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Windup {
    /// The player it swings at, or the sprite or building it chases.
    pub target: SwingTarget,
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

/// What a ballista shoots from and with (`Settlement::launch_shot`).
pub(super) struct Launcher {
    pub from: FxVec2,
    /// Above the ground at `from`.
    pub height: Fx,
    pub damage: u32,
    /// Meters per second.
    pub speed: Fx,
    pub kind: ProjectileKind,
    /// The defender that shoots, if a `DefenderState` does.
    pub defender: Option<u16>,
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
    /// A player's arrow in a volume scene: the floor it flies over, its
    /// shooter's feet (M8.2), so it hits only threats on that floor. Zero
    /// elsewhere and for a ballista's shot, which flies over the top floor
    /// at its launcher.
    #[serde(default)]
    pub floor: Fx,
}

impl Projectile {
    /// The floor it flies over in a volume scene (M8.2): an arrow's
    /// shooter's, a ballista's shot the top floor at its launcher.
    fn floor_at(&self, terrain: &Terrain) -> Fx {
        if self.item.is_some() {
            terrain.height(self.from)
        } else {
            self.floor
        }
    }
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
    /// A fireball's landing or a burn (`fire`).
    Fire,
}

/// What happened this tick, for sounds and effects. Not part of the state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A defender fired.
    Shot { defender: u16 },
    /// An avatar loosed an arrow. `started` is the tick its `Attack`
    /// applied on (`Swing::pressed`), as on the other strike events. Its
    /// arrow leaves `from`, `height` above the ground, and comes down at
    /// `to` `flight` ticks after this one, as the projectile it adds flies
    /// unless it hits something first: a client draws its own shot's arrow
    /// from this before the projectile is confirmed.
    BowFired { agent: AgentIx, from: FxVec2, to: FxVec2, height: Fx, flight: u64, started: u64 },
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
    /// Threat `threat` burned away in the sunrise at `position`: dispelled,
    /// not killed (no kill counts; `Settlement::dispelled` does).
    Dispelled { threat: u64, position: FxVec2 },
    /// A building put a unit in its output.
    BuildingProduced { building: super::BuildingIx },
    /// Player `agent`'s bundle action didn't start, for `reason`
    /// (`BundleRefusal::message` says it in words).
    BundleRefused { agent: AgentIx, reason: super::avatar::BundleRefusal },
    /// A blueprint became a site, the last building in the table.
    BlueprintPlaced { building: super::BuildingIx },
    /// A blueprint of `kind` couldn't go down, for `reason`.
    BlueprintRefused { kind: super::BuildingKindIx, reason: super::buildings::PlacementRefusal },
    /// A site's part `part` went in; `built` when it was the last.
    PartInstalled { building: super::BuildingIx, part: u8, built: bool },
    /// An elder's Burst put `units` in the output of the building it
    /// possesses, at once.
    BuildingBurst { building: super::BuildingIx, units: u32 },
    /// Player `agent`'s Burst didn't happen, for `reason`
    /// (`BurstRefusal::message` says it in words).
    BurstRefused { agent: AgentIx, reason: super::buildings::BurstRefusal },
    /// A ballista building fired, as a defender's `Shot`.
    BuildingShot { building: super::BuildingIx },
    /// Elder `agent`, possessing ballista `building`, threw a fireball; it
    /// lands at `to` on tick `arrival` (B4).
    FireballThrown { agent: AgentIx, building: super::BuildingIx, to: FxVec2, arrival: u64 },
    /// A fireball came down at `position` and set the ground alight.
    FireballLanded { position: FxVec2 },
    /// A threat started winding up a swing at building `building` (B8). It
    /// lands on tick `lands` unless a stagger or shock cancels it first.
    BuildingWindup { threat: u64, building: super::BuildingIx, lands: u64 },
    /// A threat's swing at building `building` landed: a hit took `damage`
    /// of its health (`broken` if that was the last, `break_building`); a
    /// miss means the threat got out of reach or the building had nothing
    /// left to lose.
    BuildingStruck { threat: u64, building: super::BuildingIx, hit: bool, damage: u32, broken: bool },
    /// Building `building` broke (B8): a site again, back to its first part,
    /// its occupant put out and what it held spilled onto ground piles.
    BuildingBroken { building: super::BuildingIx },
    /// Sprite `agent` came back beside the tree after dying (B6).
    SpriteReturned { agent: AgentIx },
    /// The Sacred Tree's health reached 0: the run is over (B7).
    TreeFell,
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
            Event::BowFired { agent, started, .. } | Event::ArcFired { agent, started, .. } | Event::LaserFired { agent, started, .. } => {
                Prediction::Predict { agent, started, on_miss: OnMiss::Keep }
            }
            _ => Prediction::Confirmed,
        }
    }
}

/// The event a resolving strike shows, from the message alone: stage 5
/// pushes it as it applies the strike, and `AvatarReplay` predicts it.
/// Sword and spear strikes show nothing until stage 5 says what they hit
/// (`Hit`). `tuning` is the strike's weapon's.
pub(super) fn strike_event(message: &Strike, tuning: &WeaponTuning) -> Option<Event> {
    let (agent, started, from) = (message.agent, message.started, message.origin);
    match message.weapon {
        Weapon::Bow => {
            let ArrowFlight { from, to, height, flight } = arrow_flight(message, tuning);
            Some(Event::BowFired { agent, from, to, height, flight, started })
        }
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

/// Where a bow strike's arrow flies: from the bow's muzzle, `height` above
/// the ground, `reach` along the shot to `to`, over `flight` ticks.
struct ArrowFlight {
    from: FxVec2,
    to: FxVec2,
    height: Fx,
    flight: u64,
}

fn arrow_flight(message: &Strike, tuning: &WeaponTuning) -> ArrowFlight {
    let (origin, forward) = (message.origin, message.forward);
    let right = FxVec2::new(forward.y, -forward.x);
    let from = origin + right * message.muzzle[0] + forward * message.muzzle[2];
    let to = from + forward * tuning.reach;
    ArrowFlight { from, to, height: message.muzzle[1], flight: flight_ticks(tuning.reach, tuning.projectile_speed) }
}

/// One end of a pile-lab toss: a unit slot in a pile, or in the carrier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TossEnd {
    Pile { id: u32, slot: u32, position: FxVec2 },
    /// A slot in an avatar's backpack.
    Cargo { agent: AgentIx, slot: u32 },
    /// The floor of a pit, where dug ore comes from, and its height: the
    /// floor the dig cleared down to in a volume scene, the pit's floor
    /// elsewhere (M8.2).
    Pit { position: FxVec2, height: Fx },
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
        let def = self.threat_def(t);
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

    /// A threat's character definition.
    pub(super) fn threat_def(&self, t: &Threat) -> &CharacterDef {
        &self.characters[t.character as usize]
    }

    /// Stage 2: every threat moves itself. Every threat's brain is
    /// `FollowRoute`, so each runs `follow_route`. A chaser reads where its
    /// quarry stood at the end of last tick: agents move after threats in
    /// this stage, so nothing it reads changes meanwhile.
    pub(crate) fn threats_self_update(&mut self, tick: u64, terrain: &Terrain) {
        // Speed in meters per tick, full hp and a chaser's stop, by
        // character: one division per character rather than per threat. The
        // division is the one waves used, so speeds stay bit-equal.
        let per_second = Fx::from_int(content::TICKS_PER_SECOND);
        let tuning: Vec<(Fx, u32, Fx)> = self
            .characters
            .iter()
            .map(|def| {
                let stop = def.attack.map_or(Fx::ZERO, |a| super::chase::stop_distance(a.reach));
                (def.movement.walk / per_second, def.health.as_ref().map_or(0, |h| h.hp), stop)
            })
            .collect();
        let (agents, buildings, kinds, waves) = (&self.agents, &self.buildings, &self.building_kinds, &self.waves);
        let volume = terrain.volume.is_some();
        let knockback_moved: Fx = self
            .threats
            .par_iter_mut()
            .map(|t| {
                let (speed, full_hp, stop) = tuning[t.character as usize];
                // A building quarry is a disc: the chaser stops short of its
                // edge rather than its center.
                let quarry = match t.goal {
                    super::chase::Goal::Chase(super::chase::Quarry::Agent(a)) => agents.get(a as usize).map(|a| (a.body.position, a.height(), stop)),
                    super::chase::Goal::Chase(super::chase::Quarry::Building(b)) => {
                        buildings.get(b as usize).map(|b| (b.position, terrain.height(b.position), stop + kinds[b.kind as usize].radius))
                    }
                    super::chase::Goal::Tree => None,
                };
                if volume {
                    let (quarry, stop) = quarry.map_or((None, stop), |(at, feet, stop)| (Some((at, feet)), stop));
                    let target = waves.get(t.wave as usize).map(|w| w.target).or(t.route.last().copied());
                    return follow_route_at(t, speed, full_hp, tick, terrain, quarry, stop, target);
                }
                let (quarry, stop) = quarry.map_or((None, stop), |(at, _, stop)| (Some(at), stop));
                follow_route(t, speed, full_hp, tick, terrain, quarry, stop)
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
            if t.route.is_empty() && !t.burn.is_some_and(|b| b.dawn) {
                let mut out = Messages::default();
                out.breach.push(Breach { threat: t.id });
                result.push((t.position, out));
            }
            if let Some(windup) = t.windup.filter(|w| w.lands <= tick) {
                let mut out = Messages::default();
                out.swing_lands.push(SwingLands { threat: t.id, target: windup.target, hit: self.swing_reaches(t, windup.target, terrain) });
                result.push((t.position, out));
            }
            if let Some(swing) = self.swing_at_quarry(t, tick, terrain) {
                let mut out = Messages::default();
                out.start_swing.push(swing);
                result.push((t.position, out));
            }
        }
        self.read_chase(tick, terrain, &mut result);
        if self.player_health > 0 {
            for (i, a) in self.agents.iter().enumerate() {
                // A building shelters the elder possessing it (B6).
                if a.avatar().is_some_and(|p| p.seat.and_then(super::Seat::building).is_none()) && a.body.health > 0 {
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
            self.aim_defender(i as u16, d, tick, terrain, &mut out);
            result.push((d.position, out));
        }
        self.aim_buildings(tick, terrain, &mut result);
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
        self.threat_def(t).attack
    }

    /// Stage 3: whether a threat's swing landing now reaches `agent`, alive,
    /// within the attack's reach of the threat and low enough
    /// (`SWING_HEIGHT`). A player who lifted off during the windup is missed.
    /// A sprite (the quarry of a chaser, B5) is reached outside a building;
    /// a building (B8) while it still has something to lose, its edge within
    /// reach.
    fn swing_reaches(&self, t: &Threat, target: SwingTarget, terrain: &Terrain) -> bool {
        let Some(attack) = self.threat_attack(t) else { return false };
        match target {
            SwingTarget::Agent(agent) => self.agents.get(agent as usize).is_some_and(|a| {
                self.swingable(a, terrain) && distance(t.position, a.body.position) <= attack.reach && same_floor(terrain, t.height, self.feet_floor(a, terrain))
            }),
            SwingTarget::Building(b) => self.breakable(b) && self.building_gap(t.position, b) <= attack.reach && self.on_building_floor(t, b, terrain),
        }
    }

    /// Whether a threat's swing can reach agent `a` at all: a live player
    /// low enough (`SWING_HEIGHT`) and not possessing a building, or a
    /// sprite outside a building: a building shelters its occupant (B6).
    fn swingable(&self, a: &super::AgentState, terrain: &Terrain) -> bool {
        match a.sprite() {
            Some(sprite) => a.body.alive() && !sprite.inside(),
            None => a.avatar().is_some_and(|p| p.seat.and_then(super::Seat::building).is_none()) && a.body.alive() && self.within_swing_height(a, terrain),
        }
    }

    /// Meters from `at` to building `b`'s edge (negative inside it).
    pub(super) fn building_gap(&self, at: FxVec2, b: BuildingIx) -> Fx {
        let building = &self.buildings[b as usize];
        distance(at, building.position) - self.building_kinds[building.kind as usize].radius
    }

    /// Stage 3 for a chaser (B5): the swing it could start at its quarry
    /// now. Players in reach draw swings from every threat where player
    /// combat is on (`threats_in_reach`; the same swing twice is one); a
    /// sprite, a player where it is off, or a building (B8), only draws its
    /// chaser's.
    fn swing_at_quarry(&self, t: &Threat, tick: u64, terrain: &Terrain) -> Option<StartSwing> {
        let attack = self.threat_attack(t)?;
        let (target, gap, reachable) = match t.goal {
            super::chase::Goal::Chase(super::chase::Quarry::Agent(agent)) => {
                let a = self.agents.get(agent as usize)?;
                let floor = same_floor(terrain, t.height, self.feet_floor(a, terrain));
                (SwingTarget::Agent(agent), distance(t.position, a.body.position), self.swingable(a, terrain) && floor)
            }
            super::chase::Goal::Chase(super::chase::Quarry::Building(b)) => {
                self.buildings.get(b as usize)?;
                (SwingTarget::Building(b), self.building_gap(t.position, b), self.breakable(b) && self.on_building_floor(t, b, terrain))
            }
            super::chase::Goal::Tree => return None,
        };
        (reachable && can_start_swing(t, tick) && gap <= attack.reach).then_some(StartSwing { threat: t.id, target, distance: gap })
    }

    /// Whether a player's feet are low enough for a threat's swing: at most
    /// `SWING_HEIGHT` above the ground under its body. Over a drop, the
    /// ground it last stood on counts.
    fn within_swing_height(&self, a: &super::AgentState, terrain: &Terrain) -> bool {
        // Only avatars leave the ground.
        let Some(avatar) = a.avatar() else { return true };
        let radius = super::avatar::character_of(&self.characters, a).body.radius;
        let ground = terrain.footprint_at(a.body.position, avatar.height, radius).unwrap_or(avatar.flight.support);
        avatar.height - ground <= SWING_HEIGHT
    }

    /// The floor agent `a` stands on, for a swing at it in a volume scene
    /// (M8.2): its feet, or for an avatar in the air the floor under it
    /// (`within_swing_height` checks how high over it it is). Zero
    /// elsewhere, where it isn't read.
    fn feet_floor(&self, a: &super::AgentState, terrain: &Terrain) -> Fx {
        let Some(avatar) = a.avatar().filter(|_| terrain.volume.is_some()) else { return a.height() };
        let radius = super::avatar::character_of(&self.characters, a).body.radius;
        terrain.footprint_at(a.body.position, avatar.height, radius).unwrap_or(avatar.flight.support)
    }

    /// Stage 3 for one hurtable player: every threat that could start a
    /// swing at it now. Stage 5 keeps the nearest player for each threat
    /// (`start_swings`). A player too high (`SWING_HEIGHT`) starts none.
    fn threats_in_reach(&self, agent: AgentIx, a: &super::AgentState, tick: u64, terrain: &Terrain, out: &mut Messages) {
        if !self.within_swing_height(a, terrain) {
            return;
        }
        let position = a.body.position;
        let floor = self.feet_floor(a, terrain);
        for t in &self.threats {
            let Some(attack) = self.threat_attack(t) else { continue };
            if !can_start_swing(t, tick) || !same_floor(terrain, t.height, floor) {
                continue;
            }
            let gap = distance(t.position, position);
            if gap <= attack.reach {
                out.start_swing.push(StartSwing { threat: t.id, target: SwingTarget::Agent(agent), distance: gap });
            }
        }
    }

    /// Stage 5: threats start the swings stage 3 found. A threat named by
    /// several takes the nearest player, the lowest agent index on a tie,
    /// whatever order the messages came in; a building only when no agent
    /// is named (B5: buildings come second). It checks again that nothing
    /// earlier in stage 5 (a strike's stagger, a kill) stopped it.
    pub(crate) fn start_swings(&mut self, tick: u64, mut messages: Vec<StartSwing>) {
        messages.sort_by_key(|m| (m.threat, m.target.agent().is_none(), m.distance, m.target));
        messages.dedup_by_key(|m| m.threat);
        for message in messages {
            let Some(i) = self.threat_index(message.threat) else { continue };
            let Some(attack) = self.threat_attack(&self.threats[i]) else { continue };
            let target = match message.target {
                SwingTarget::Agent(a) => self.agents.get(a as usize).map(|a| a.body.position),
                SwingTarget::Building(b) => self.buildings.get(b as usize).map(|b| b.position),
            };
            let Some(target) = target else { continue };
            let t = &mut self.threats[i];
            if !can_start_swing(t, tick) {
                continue;
            }
            let lands = tick + attack.windup;
            t.windup = Some(Windup { target: message.target, lands });
            if target != t.position {
                t.heading = nav::bearing(t.position, target);
            }
            self.events.push(match message.target {
                SwingTarget::Agent(agent) => Event::ThreatWindup { threat: t.id, agent, lands },
                SwingTarget::Building(building) => Event::BuildingWindup { threat: t.id, building, lands },
            });
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
        if t.windup.is_none_or(|w| w.target != message.target) {
            return;
        }
        t.windup = None;
        t.ready_at = tick + 1 + attack.cooldown;
        let (threat, from, heading) = (t.id, t.position, t.heading);
        let agent = match message.target {
            SwingTarget::Agent(agent) => agent as usize,
            SwingTarget::Building(building) => {
                // A break earlier in stage 5 may have left it nothing to lose.
                let hit = message.hit && self.breakable(building);
                let broken = hit && self.hurt_building(building, attack.damage, terrain);
                let damage = if hit { attack.damage } else { 0 };
                self.events.push(Event::BuildingStruck { threat, building, hit, damage, broken });
                return;
            }
        };
        if let Some(sprite_def) = self.agents[agent].sprite().and_then(|_| self.characters[self.agents[agent].character as usize].sprite) {
            // A sprite with health (B6): a hit hurts it, and at 0 it dies.
            // It isn't pushed: it walks its route, with no velocity to give.
            let hit = message.hit && self.agents[agent].body.alive();
            let killed = hit && self.hurt_sprite(agent, attack.damage, tick, sprite_def.respawn);
            let damage = if hit { attack.damage } else { 0 };
            self.events.push(Event::ThreatSwing { threat, agent: agent as AgentIx, hit, damage, killed });
            return;
        }
        if self.agents[agent].sprite().is_some() || self.player_health == 0 {
            // A chaser's swing at its quarry where nothing it hits has
            // health: a sprite with no `sprite` section, or a player where
            // player combat is off. It reaches or misses, and nothing more.
            self.events.push(Event::ThreatSwing { threat, agent: agent as AgentIx, hit: message.hit, damage: 0, killed: false });
            return;
        }
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
        self.events.push(Event::ThreatSwing { threat, agent: agent as AgentIx, hit, damage, killed });
    }

    fn fly(&self, p: &Projectile, tick: u64, terrain: &Terrain, out: &mut Messages) {
        let (start, end) = p.segment(tick);
        // A ballista's shot (a stock one: a defender's or a ballista
        // building's) leaves from the center of its own footprint, which
        // doesn't stop it. A player's arrow has no footprint to leave.
        let own = p.item.is_some().then_some(p.from);
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
        let floor = p.floor_at(terrain);
        let threat = self
            .threats
            .iter()
            .filter(|t| same_floor(terrain, t.height, floor))
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

    fn aim_defender(&self, index: u16, d: &DefenderState, tick: u64, terrain: &Terrain, out: &mut Messages) {
        if !d.enabled || d.ready_at > tick || self.depots[d.depot as usize].inventory.available(d.ammo) == 0 {
            return;
        }
        if let Some(t) = self.pick_target(d.position, d.range, terrain) {
            out.fire.push(Fire { defender: index, target: t.id, to: t.position });
        }
    }

    /// What a ballista aims at (the Woodland Ballista's `DefenderState` and
    /// a ballista building's both): the nearest threat within `range` of
    /// `from` that the projectiles already in flight to it won't kill,
    /// the lowest id on a tie. In a volume scene, only threats on the top
    /// floor at `from`, which a ballista stands on (M8.2).
    pub(super) fn pick_target(&self, from: FxVec2, range: Fx, terrain: &Terrain) -> Option<&Threat> {
        let floor = terrain.volume.as_ref().map(|_| terrain.height(from));
        self.threats
            .iter()
            .filter(|t| {
                let incoming: u32 = self.projectiles.iter().filter(|p| p.target == Some(t.id)).map(|p| p.damage).sum();
                distance(from, t.position) <= range && incoming < t.hp && floor.is_none_or(|floor| same_floor(terrain, t.height, floor))
            })
            .min_by_key(|t| (distance(from, t.position), t.id))
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
                // In a volume, only threats on the striker's floor (M8.2).
                (d <= reach && in_cone && visible && same_floor(terrain, t.height, height)).then_some((d, t.id))
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
                        .filter(|t| t.id != first && distance(t.position, anchor) <= Fx::from_ratio(11, 5) && same_floor(terrain, t.height, height))
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
        self.events.extend(strike_event(&message, &tuning));
        let origin = message.origin;
        if message.weapon == Weapon::Bow {
            let ArrowFlight { from, to, height, flight } = arrow_flight(&message, &tuning);
            let id = self.id();
            self.projectiles.push(Projectile {
                id,
                defender: None,
                target: None,
                item: None,
                from,
                launch_height: height,
                to,
                born: tick,
                arrival: tick + flight,
                damage: tuning.damage,
                kind: ProjectileKind::Arrow,
                floor: if terrain.volume.is_some() { self.agents[agent].height() } else { Fx::ZERO },
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

    /// Whether the Sacred Tree has fallen (B7): a town (a scene with
    /// building kinds) whose base health, the tree's, is 0. The run is
    /// over; the app says so and offers a restart. Other scenes carry on at
    /// 0, as they always have.
    pub fn tree_fallen(&self) -> bool {
        !self.building_kinds.is_empty() && self.health == 0
    }

    pub(crate) fn breach(&mut self, message: Breach) {
        let Some(i) = self.threat_index(message.threat) else { return };
        let t = self.threats.remove(i);
        let damage = self.threat_def(&t).health.as_ref().map_or(0, |h| h.breach_damage);
        let standing = self.health > 0;
        self.health = self.health.saturating_sub(damage);
        self.combat.breaches += 1;
        self.events.push(Event::Breach { position: t.position });
        if standing && self.tree_fallen() {
            self.events.push(Event::TreeFell);
        }
    }

    pub(crate) fn fire(&mut self, tick: u64, message: Fire) {
        let d = &self.defenders[message.defender as usize];
        let (depot, ammo) = (d.depot as usize, d.ammo);
        if !d.enabled || d.ready_at > tick || self.depots[depot].inventory.available(ammo) == 0 {
            return;
        }
        let launcher = Launcher { from: d.position, height: d.muzzle_height, damage: d.damage, speed: d.speed, kind: d.kind, defender: Some(message.defender) };
        let period = d.period;
        let shot = Event::Shot { defender: message.defender };
        self.launch_shot(tick, &launcher, InventoryRef::Depot(depot as DepotIx), ammo, Some(message.target), message.to, shot);
        let d = &mut self.defenders[message.defender as usize];
        d.aim = Some(message.to);
        d.ready_at = tick + period;
        d.fired_at = tick;
    }

    /// Launches a shot from `launcher` at threat `target`, standing at `to`,
    /// or with no `target` at the point `to` (a possessing elder's shot at
    /// its reticle). Every ballista's shots go through here: one unit of `ammo` leaves
    /// `holder` and counts as spent, the shot counts, the projectile flies
    /// and `event` tells the view (its sound). The caller has checked the
    /// ammo is there and the launcher is ready.
    pub(super) fn launch_shot(&mut self, tick: u64, launcher: &Launcher, holder: InventoryRef, ammo: ItemIx, target: Option<u64>, to: FxVec2, event: Event) {
        let arrival = tick + flight_ticks(distance(launcher.from, to), launcher.speed);
        self.inventory_mut(holder).remove(ammo, 1);
        self.accounts[ammo as usize].spent += 1;
        self.combat.shots += 1;
        let id = self.id();
        self.projectiles.push(Projectile {
            id,
            defender: launcher.defender,
            target,
            item: Some(ammo),
            from: launcher.from,
            launch_height: launcher.height,
            to,
            born: tick,
            arrival,
            damage: launcher.damage,
            kind: launcher.kind,
            floor: Fx::ZERO,
        });
        self.events.push(event);
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
        self.retry_stranded(tick, terrain);
        if self.clock.is_some_and(|c| c.dawn(tick)) {
            self.dawn(tick);
        }
        // A fallen tree ends the run (B7): no more enemies come.
        let waves = if self.tree_fallen() { 0 } else { self.waves.len() };
        for w in 0..waves {
            for _ in 0..self.due(w, tick) {
                self.spawn(w, terrain);
            }
        }
    }

    /// How many enemies wave `w` sends on `tick`: a timed wave one on its
    /// timer, a lane its night's column (`lane_starts`) by night.
    fn due(&self, w: usize, tick: u64) -> u32 {
        let wave = &self.waves[w];
        if !wave.lane() {
            return u32::from(tick >= wave.first && wave.spawned < wave.count && (tick - wave.first) % wave.every == 0);
        }
        let Some(clock) = self.clock else { return 0 };
        let Some(since) = clock.since_dusk(tick) else { return 0 };
        lane_starts(wave.on_night(clock.night(tick)), clock.night).filter(|&start| start == since).count() as u32
    }

    /// The next tick from `tick` on that a wave sends an enemy, if any
    /// will (within the next eight nights, for lanes): a HUD's "next wave".
    pub fn next_wave(&self, tick: u64) -> Option<u64> {
        let timed = self.waves.iter().filter(|w| !w.lane() && w.spawned < w.count).map(|w| tick.max(w.first + u64::from(w.spawned) * w.every)).min();
        let lanes = self.clock.filter(|_| self.waves.iter().any(WaveState::lane)).and_then(|clock| {
            // This night's starts still to come, then the next nights'.
            let dusk = |n: u64| (n - 1) * clock.cycle() + clock.day;
            let night = clock.night(tick).max(1);
            (night..night + 8).find_map(|n| {
                let starts = self.waves.iter().flat_map(|w| lane_starts(w.on_night(n), clock.night)).map(|start| dusk(n) + start);
                starts.filter(|&at| at >= tick).min()
            })
        });
        timed.into_iter().chain(lanes).min()
    }

    /// The sunrises by `tick`: the nights the town has seen through, while
    /// the tree stands (`tree_fallen`). None without days and nights.
    pub fn nights_survived(&self, tick: u64) -> Option<u64> {
        self.clock.map(|c| tick / c.cycle())
    }

    /// Wave `w` sends an enemy along its route, planning it first if it
    /// has none.
    fn spawn(&mut self, w: usize, terrain: &Terrain) {
        // With no way to the tree, the wave sends its enemies as close
        // as they can walk (`Threat::stranded`).
        // In a volume (M8.2) its enemies walk in on the surface, by a 3D
        // path from there.
        let feet = super::chase::surface_feet(terrain, self.waves[w].position);
        if self.waves[w].route.is_none() {
            let wave = &self.waves[w];
            let (route, _) = if terrain.volume.is_some() {
                super::chase::route_home_at(terrain, wave.position, feet, wave.target)
            } else {
                nav::path_toward(terrain, wave.position, wave.target, super::chase::WALK_CLEARANCE)
            };
            self.waves[w].route = Some(route);
        }
        let wave = &self.waves[w];
        let route = wave.route.clone().expect("planned");
        if route.is_empty() {
            return;
        }
        let (character, position, stranded) = (wave.character, wave.position, route.last() != Some(&wave.target));
        let health = self.characters[character as usize].health.as_ref().expect("a wave sends a FollowRoute character");
        let (hp, armor) = (health.hp, health.armor.as_ref().map_or(0, |a| a.points));
        let id = self.id();
        self.threats.push(Threat {
            id,
            wave: w as u16,
            character,
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
            burn: None,
            goal: super::chase::Goal::Tree,
            stranded,
            height: feet,
            chase: Vec::new(),
            backoff: super::Backoff::default(),
        });
        self.waves[w].spawned += 1;
    }

    /// Sunrise (N3): every threat still out catches fire and stands
    /// burning, dropping any swing, for `DAWN_BURN` and up to as long again
    /// (by its id, so they don't all fall at once); then it falls,
    /// dispelled (`Settlement::burn`). The burn does no damage: it isn't a
    /// kill.
    fn dawn(&mut self, tick: u64) {
        for t in &mut self.threats {
            let spread = combat_roll(tick, t.id, 0x6461_776e) % DAWN_BURN;
            t.windup = None;
            t.burn = Some(super::fire::Burn { until: tick + DAWN_BURN + spread, next: u64::MAX, damage: 0, period: DAWN_BURN, dawn: true });
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
    t.windup.is_none() && !t.burn.is_some_and(|b| b.dawn) && tick >= t.ready_at && tick >= t.stagger_until && tick >= t.shocked_until && !t.route.is_empty()
}

/// See `Settlement::has_weapon`.
pub(super) fn offers_weapon(weapons: &[Weapon], weapon: Weapon) -> bool {
    weapons.contains(&weapon) || (weapons.is_empty() && weapon == Weapon::Sword)
}
