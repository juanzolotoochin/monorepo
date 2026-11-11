//! The crawler's legs (`kestrel/CRAWLER.md`, C5–C7, CR.2): a vehicle that
//! walks on four feet the simulation places, in `Fx`.
//!
//! The driver steers it as a rover (`vehicle::drive`); what moves it is its
//! gait. A clock runs through the gait's cycle and each leg has its slice of
//! it: a wave walk lifts one leg at a time (rear left, front left, rear
//! right, front right), so three are always down, and a trot on boost lifts
//! the diagonal pairs. A foot lifts at the start of its slice (the end,
//! backing up, so the wave runs the other way) and swings for the gait's
//! `swing` ticks, which fit in the slice, so no two slices' feet are ever up
//! together. The gait and the direction change only while every foot is
//! down. The clock stops once the crawler stands still with its feet at
//! rest.
//!
//! A lifting foot aims for its resting spot (`CrawlerDef::stance`) as the
//! body will stand half a stance after it lands: ahead by half its stride.
//! It lands on the floor nearest that spot (`foothold`) which is in reach
//! of its hip when it lands, at most `step` up or down from where it
//! lifted, with no more than `gap` meters of the way over no floor within
//! `step` (a trench, a pit, a moat) and no wall over its swing, and never
//! on an elder, a sprite or a hauler (C10). With none, the foot stays down.
//!
//! The body moves only as far as its planted feet allow: each in reach of
//! its hip and within `WORKSPACE` of its resting spot, and no ground under
//! the body higher than its belly. A foot that can't step holds it back, so
//! the crawler stops at a pit too wide or a cliff too high, and backs off
//! when the driver backs. Turning in place is the same gait with the body
//! turning: the feet step sideways round its middle.
//!
//! The body stays level: no pitch or roll. Its height (`VehicleState::height`,
//! its feet's level, the deck `deck` over it) eases toward the mean of the
//! feet's floors, sinking further when a foot far below needs it to stay in
//! reach (a 1.5 m step down is out of reach from a body at the upper level).

use std::sync::OnceLock;

use content::character::{CharacterDef, CrawlerDef, GaitDef, Offset};
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::vehicle::{self, VehicleState, REVERSE_ANGLE, SUBSTEP};
use super::{AgentIx, AgentState, Event, Impact, Role, Seat, Settlement};
use crate::messages::{FootLands, Messages};
use crate::nav::{angle_between, bearing, heading_vector};
use crate::terrain::{Disc, Terrain};
use crate::volume::{self, ROUNDING};

/// How far a planted foot strays from its resting spot before the body
/// waits for it to step, in meters.
pub const WORKSPACE: Fx = Fx::from_int(2);
/// A foot this near where it would step to stays down.
const SETTLE: Fx = Fx::from_ratio(1, 4);
/// A hip stays at least this high over a planted foot, in meters.
pub const MIN_RISE: Fx = Fx::ONE;
/// The body hangs this far under the deck: ground under it higher than its
/// belly stops it.
pub const BELLY: Fx = Fx::from_ratio(6, 5);
/// A foot's footprint, for the bodies it never lands on, the ones it
/// shoves off and stomps (CR.5), and the paths that go round it.
pub const FOOT_RADIUS: Fx = Fx::from_ratio(2, 5);
/// Footholds are looked for on a grid this fine round the ideal spot.
const GRID: Fx = Fx::from_ratio(1, 4);
/// A stick pointing further than this from the heading turns it in place
/// (60°), up to `REVERSE_ANGLE`, past which it backs up.
const TURN_IN_PLACE: Fx = Fx::from_ratio(10_472, 10_000);
/// Stopped at a gap, a stick further than this from the heading (about 7°)
/// moves it slowly toward the feet held there as it turns (`over_a_gap`).
const TURN_NEAR: Fx = Fx::from_ratio(1, 8);

/// The legs' state: four feet, in `VehicleState::wheels` order (front left,
/// front right, rear left, rear right), and the gait's clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Legs {
    pub feet: [Foot; 4],
    pub gait: Gait,
    /// Whether the gait runs backward: the clock counts down.
    pub backing: bool,
    /// Ticks into the gait's cycle.
    pub clock: u64,
}

/// A foot: where it stands, or, while it swings, where it lifted from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Foot {
    pub spot: FxVec2,
    /// The height of the floor it stands on (a tile's level floor).
    pub floor: Fx,
    pub swing: Option<Swing>,
}

/// A foot in the air: where it lands, and how many ticks it has swung. One
/// that has swung its gait's `swing` and isn't down yet waits for the body
/// to bring the spot into reach, holding the clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Swing {
    pub to: FxVec2,
    pub floor: Fx,
    pub progress: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gait {
    /// One leg at a time, three always down.
    Walk,
    /// Diagonal pairs, on boost.
    Trot,
}

impl Gait {
    pub fn def(self, c: &CrawlerDef) -> &GaitDef {
        match self {
            Gait::Walk => &c.walk,
            Gait::Trot => &c.trot,
        }
    }

    /// The most feet it has in the air at once.
    pub fn most_up(self) -> usize {
        match self {
            Gait::Walk => 1,
            Gait::Trot => 2,
        }
    }

    /// The tick of its cycle `leg`'s slice starts on.
    fn start(self, leg: usize, cycle: u64) -> u64 {
        let (slot, slots) = match self {
            Gait::Walk => ([1, 3, 0, 2][leg], 4),
            Gait::Trot => ([0, 1, 1, 0][leg], 2),
        };
        slot * cycle / slots
    }
}

impl Legs {
    pub fn up(&self) -> impl Iterator<Item = usize> + '_ {
        self.feet.iter().enumerate().filter(|(_, f)| f.swing.is_some()).map(|(i, _)| i)
    }

    /// Where each foot will stand: planted, or where it lands.
    fn landing(&self, leg: usize) -> (FxVec2, Fx) {
        let f = &self.feet[leg];
        f.swing.as_ref().map_or((f.spot, f.floor), |s| (s.to, s.floor))
    }
}

/// Where the body is on the ground and which way it faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pose {
    pub position: FxVec2,
    pub heading: Fx,
}

impl Pose {
    pub fn of(v: &VehicleState) -> Pose {
        Pose { position: v.position, heading: v.heading }
    }

    /// `o` on leg `leg`'s side: the front legs ahead, the left ones left.
    pub fn point(self, o: Offset, leg: usize) -> FxVec2 {
        let forward = heading_vector(self.heading);
        let right = FxVec2::new(forward.y, -forward.x);
        let along = if leg < 2 { o.along } else { -o.along };
        let across = if leg % 2 == 1 { o.across } else { -o.across };
        self.position + forward * along + right * across
    }

    /// Where it will be after `ticks` at `speed` along its heading, turning
    /// `turn` a tick.
    fn ahead(self, speed: Fx, turn: Fx, ticks: u64) -> Pose {
        let n = ticks as i64;
        Pose { position: self.position + heading_vector(self.heading) * speed.mul_int(n), heading: self.heading + turn.mul_int(n) }
    }
}

/// A body a foot never lands on: an elder, sprite or hauler standing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Standing {
    pub position: FxVec2,
    pub radius: Fx,
    pub height: Fx,
}

/// The bodies standing on the ground: every agent alive, but drivers and
/// riders, and sprites inside a building.
pub fn standing(agents: &[AgentState]) -> Vec<Standing> {
    agents
        .iter()
        .filter(|a| a.body.alive() && a.avatar().is_none_or(|p| p.seat.is_none()) && a.sprite().is_none_or(|s| !s.sheltered()))
        .map(|a| Standing { position: a.body.position, radius: a.radius(), height: a.height() })
        .collect()
}

/// The body heights (`VehicleState::height`) at which each foot, given as
/// its hip, spot and floor, is in reach of its hip and at least `MIN_RISE`
/// under it: the lowest and the highest. `None` if there are none.
fn heights(c: &CrawlerDef, feet: impl IntoIterator<Item = (FxVec2, FxVec2, Fx)>) -> Option<(Fx, Fx)> {
    let (mut lo, mut hi) = (Fx::MIN, Fx::MAX);
    let reach = c.reach * c.reach;
    for (hip, spot, floor) in feet {
        let across = (spot - hip).length_squared();
        if across > reach {
            return None;
        }
        lo = lo.max(floor + MIN_RISE - c.deck);
        hi = hi.min(floor + (reach - across).sqrt() - c.deck);
    }
    (lo <= hi).then_some((lo, hi))
}

/// The planted feet at `pose`, as `heights` takes them.
fn planted<'a>(legs: &'a Legs, c: &CrawlerDef, pose: Pose) -> impl Iterator<Item = (FxVec2, FxVec2, Fx)> + 'a {
    let hip = c.hip;
    legs.feet.iter().enumerate().filter(|(_, f)| f.swing.is_none()).map(move |(i, f)| (pose.point(hip, i), f.spot, f.floor))
}

/// The level top floor over `p`'s tile (the ground elsewhere).
fn top(terrain: &Terrain, p: FxVec2) -> Option<Fx> {
    match &terrain.volume {
        Some(v) => v.top(Terrain::pit_tile(p)).map(volume::height),
        None => terrain.ground(p),
    }
}

/// Whether `p`'s tile has a floor within `step` of `from`.
fn floor_within(terrain: &Terrain, p: FxVec2, from: Fx, step: Fx) -> bool {
    match &terrain.volume {
        Some(v) => v.floors(Terrain::pit_tile(p)).any(|f| (volume::height(f) - from).abs() <= step + ROUNDING),
        None => terrain.ground(p).is_some_and(|h| (h - from).abs() <= step + ROUNDING),
    }
}

/// The floor a foot at `p` lands on, lifted from a floor at `from`: the one
/// of its tile's floors within `step` of it nearest it (the higher on a
/// tie) with no ceiling under `clear`, the hip's height.
fn landing_floor(terrain: &Terrain, p: FxVec2, from: Fx, step: Fx, clear: Fx) -> Option<Fx> {
    let near = |h: Fx| (h - from).abs() <= step + ROUNDING;
    match &terrain.volume {
        Some(v) => {
            let tile = Terrain::pit_tile(p);
            v.floors(tile)
                .filter(|&f| v.ceiling(tile, f).is_none_or(|c| volume::height(c) >= clear))
                .map(volume::height)
                .filter(|&h| near(h))
                .min_by_key(|&h| ((h - from).abs(), -h))
        }
        None => terrain.ground(p).filter(|&h| near(h)),
    }
}

/// Whether a foot at `spot` stands on a floor of the height `floor`.
fn on_floor(terrain: &Terrain, spot: FxVec2, floor: Fx) -> bool {
    match &terrain.volume {
        Some(v) => v.floors(Terrain::pit_tile(spot)).any(|f| volume::height(f) == floor),
        None => terrain.ground(spot) == Some(floor),
    }
}

/// How many meters of a foot's swing from `a` to `b` pass over no floor
/// within `step` of `from`, tile by tile. `None` if ground higher than
/// `over` (the swing's top) stands in the way.
fn gap_along(terrain: &Terrain, a: FxVec2, b: FxVec2, from: Fx, step: Fx, over: Fx) -> Option<Fx> {
    let d = b - a;
    let length = d.length();
    // Where the way crosses from one tile to the next (at k + 1/2).
    let mut cuts = vec![Fx::ZERO, Fx::ONE];
    for (start, delta) in [(a.x, d.x), (a.y, d.y)] {
        if delta == Fx::ZERO {
            continue;
        }
        let (lo, hi) = if delta > Fx::ZERO { (start, start + delta) } else { (start + delta, start) };
        let mut k = (lo - Fx::HALF).floor() + Fx::HALF;
        if k <= lo {
            k = k + Fx::ONE;
        }
        while k < hi {
            cuts.push((k - start) / delta);
            k = k + Fx::ONE;
        }
    }
    cuts.sort();
    let mut gap = Fx::ZERO;
    for w in cuts.windows(2) {
        if w[1] <= w[0] {
            continue;
        }
        let q = a + d * ((w[0] + w[1]).div_int(2));
        if top(terrain, q).is_some_and(|t| t > over + ROUNDING) {
            return None;
        }
        if !floor_within(terrain, q, from, step) {
            gap = gap + (w[1] - w[0]) * length;
        }
    }
    Some(gap)
}

/// The highest ground under the body's box at `pose`.
fn under_body(terrain: &Terrain, pose: Pose, c: &CrawlerDef) -> Option<Fx> {
    let forward = heading_vector(pose.heading);
    let right = FxVec2::new(forward.y, -forward.x);
    // Every half meter along and across, edges included.
    let (along, across) = ((c.length * Fx::from_int(2)).ceil().floor_to_int().max(1), (c.width * Fx::from_int(2)).ceil().floor_to_int().max(1));
    let mut highest: Option<Fx> = None;
    for i in 0..=along {
        for j in 0..=across {
            let a = c.length * Fx::from_ratio(i, along) - c.length.div_int(2);
            let b = c.width * Fx::from_ratio(j, across) - c.width.div_int(2);
            if let Some(t) = top(terrain, pose.position + forward * a + right * b) {
                highest = Some(highest.map_or(t, |h| h.max(t)));
            }
        }
    }
    highest
}

/// The grid offsets round an ideal spot a foothold is looked for at, nearest
/// first, within `WORKSPACE`.
fn search() -> &'static [(i64, i64)] {
    static GRID_OFFSETS: OnceLock<Vec<(i64, i64)>> = OnceLock::new();
    GRID_OFFSETS.get_or_init(|| {
        let n = (WORKSPACE / GRID).floor_to_int();
        let mut offsets: Vec<(Fx, i64, i64)> = (-n..=n)
            .flat_map(|i| (-n..=n).map(move |j| (i, j)))
            .map(|(i, j)| (FxVec2::new(GRID.mul_int(i), GRID.mul_int(j)).length_squared(), i, j))
            .filter(|&(d, _, _)| d <= WORKSPACE * WORKSPACE)
            .collect();
        offsets.sort();
        offsets.into_iter().map(|(_, i, j)| (i, j)).collect()
    })
}

/// Where leg `leg` lands, aiming for `ideal`, with the body at `now` and
/// at `landing` when it lands, its feet's level at `height`: the nearest
/// grid spot to `ideal` that is in the arena, within `WORKSPACE` of its
/// resting spot now and then (so a body held back doesn't strand it) and
/// in reach of its hip then (at a body height the other
/// feet, where they stand now, allow), on a floor within `step` of the one
/// it lifts from, across no more than `gap` of no floor and under no wall,
/// and clear of standing bodies.
#[allow(clippy::too_many_arguments)]
fn foothold(legs: &Legs, leg: usize, now: Pose, landing: Pose, ideal: FxVec2, height: Fx, terrain: &Terrain, c: &CrawlerDef, gait: &GaitDef, bodies: &[Standing]) -> Option<(FxVec2, Fx)> {
    let foot = &legs.feet[leg];
    let (hip, rest, rest_now) = (landing.point(c.hip, leg), landing.point(c.stance, leg), now.point(c.stance, leg));
    let others: Vec<_> = (0..4)
        .filter(|&i| i != leg)
        .map(|i| {
            let (spot, floor) = legs.landing(i);
            (now.point(c.hip, i), spot, floor)
        })
        .collect();
    let fits_at = |p: FxVec2| {
        let near = |r: FxVec2| (p - r).length_squared() <= WORKSPACE * WORKSPACE;
        if !terrain.inside(p) || !near(rest) || !near(rest_now) || (p - hip).length_squared() > c.reach * c.reach {
            return None;
        }
        let floor = landing_floor(terrain, p, foot.floor, c.step, height + c.deck)?;
        let on_body = |b: &Standing| (b.height - floor).abs() <= Fx::ONE && (b.position - p).length_squared() < (b.radius + FOOT_RADIUS) * (b.radius + FOOT_RADIUS);
        if bodies.iter().any(on_body) {
            return None;
        }
        heights(c, others.iter().copied().chain([(hip, p, floor)]))?;
        let gap = gap_along(terrain, foot.spot, p, foot.floor, c.step, foot.floor.max(floor) + gait.lift)?;
        (gap <= c.gap + ROUNDING).then_some((p, floor))
    };
    let around = |centre: FxVec2| search().iter().map(move |&(i, j)| centre + FxVec2::new(GRID.mul_int(i), GRID.mul_int(j)));
    // Round the ideal spot; with none there (over a pit, turning beside
    // it), round its resting spot then and now, nearer the body: a spot
    // there only if it brings the foot nearer its rest, so a foot at a
    // pit's edge doesn't shuffle along it.
    let standing = (foot.spot - rest).length();
    around(ideal).find_map(fits_at).or_else(|| around(rest).chain(around(rest_now)).filter(|&p| (p - rest).length() + SETTLE < standing).find_map(fits_at))
}

/// A parked crawler's legs at `pose`, each foot at its resting spot on the
/// top floor there, and its body's height: the feet's mean, in reach.
/// `None` where a foot is outside the arena or over nothing, or can't be
/// reached.
pub(super) fn park(pose: Pose, c: &CrawlerDef, terrain: &Terrain) -> Option<(Legs, Fx)> {
    let mut feet = Vec::with_capacity(4);
    for leg in 0..4 {
        let spot = pose.point(c.stance, leg);
        if !terrain.inside(spot) {
            return None;
        }
        feet.push(Foot { spot, floor: top(terrain, spot)?, swing: None });
    }
    // The clock a tick before the walk's first lift, so it steps at once.
    let legs = Legs { feet: feet.try_into().expect("four feet"), gait: Gait::Walk, backing: false, clock: c.walk.cycle - 1 };
    let (lo, hi) = heights(c, planted(&legs, c, pose))?;
    let mean = legs.feet.iter().fold(Fx::ZERO, |s, f| s + f.floor).div_int(4);
    Some((legs, mean.clamp(lo, hi)))
}

/// Whether the body can move from `from` to `pose`, its feet's level at
/// `height`: every planted foot in reach of its hip and within `WORKSPACE`
/// of its resting spot (or, one that came down further, no further than
/// at `from`), and no ground under the body over its belly.
fn fits(legs: &Legs, from: Pose, pose: Pose, height: Fx, terrain: &Terrain, c: &CrawlerDef) -> bool {
    let reach = heights(c, planted(legs, c, pose)).is_some_and(|(lo, hi)| lo <= height && height <= hi);
    let near = legs.feet.iter().enumerate().filter(|(_, f)| f.swing.is_none()).all(|(i, f)| {
        let d = (f.spot - pose.point(c.stance, i)).length_squared();
        d <= WORKSPACE * WORKSPACE || d <= (f.spot - from.point(c.stance, i)).length_squared()
    });
    let belly = height + c.deck - BELLY;
    reach && near && under_body(terrain, pose, c).is_none_or(|t| t <= belly + ROUNDING)
}

/// The planted feet whose resting spots at `pose` have no floor within
/// `step` of the one each stands on (the body is out over a pit, a trench
/// or a moat): how far ahead of their rests they stand, summed along the
/// heading. Negative when the body has gone on past them. `None` if no
/// foot is.
fn over_a_gap(legs: &Legs, pose: Pose, terrain: &Terrain, c: &CrawlerDef) -> Option<Fx> {
    let forward = heading_vector(pose.heading);
    legs.feet
        .iter()
        .enumerate()
        .filter(|(i, f)| f.swing.is_none() && !floor_within(terrain, pose.point(c.stance, *i), f.floor, c.step))
        .map(|(i, f)| (f.spot - pose.point(c.stance, i)).dot(forward))
        .reduce(|a, b| a + b)
}

/// Stage 2 for a crawler: steer, put down the feet that have swung, lift
/// the feet the gait lifts, move the body as far as its feet allow and
/// settle its height. Returns which feet came down this tick (CR.5: stage
/// 3 looks under them, `Settlement::read_footfalls`).
pub(super) fn self_update(v: &mut VehicleState, terrain: &Terrain, def: &CharacterDef, bodies: &[Standing]) -> [bool; 4] {
    let mut landed = [false; 4];
    let vd = vehicle::tuning(def);
    let c = def.crawler.as_ref().expect("crawlers have crawler tuning");
    let Some(mut legs) = v.legs.take() else { return landed };

    // The stick: walk toward it, turn in place toward it, or back up.
    let top_speed = if v.control.boost { def.movement.run } else { def.movement.walk }.div_int(content::TICKS_PER_SECOND as i64);
    let (mut target, mut turn) = (Fx::ZERO, Fx::ZERO);
    let direction = v.control.direction;
    if direction != FxVec2::ZERO {
        let want = bearing(FxVec2::ZERO, direction);
        let throttle = direction.length().min(Fx::ONE);
        let ahead = angle_between(v.heading, want);
        if ahead.abs() > REVERSE_ANGLE {
            target = -(vd.reverse * throttle);
            turn = angle_between(v.heading + Fx::PI, want);
        } else {
            // Stopped with a foot's resting spot over a gap (at a pit's edge)
            // and steered away, it moves slowly toward the feet held there
            // as it turns: back off a pit ahead, or on from a moat behind,
            // until its feet have floor to step round on.
            let held = over_a_gap(&legs, Pose::of(v), terrain, c);
            let creep = (vd.reverse * throttle).div_int(2);
            if held.is_some_and(Fx::is_negative) && ahead.abs() > TURN_NEAR && v.speed <= Fx::ZERO {
                target = -creep;
            } else if ahead.abs() <= TURN_IN_PLACE {
                target = top_speed * throttle;
            } else if held.is_some_and(|d| d > Fx::ZERO) && v.speed >= Fx::ZERO {
                target = creep;
            }
            turn = ahead;
        }
    }
    let speeding_up = target != Fx::ZERO && (target.is_negative() == v.speed.is_negative() || v.speed == Fx::ZERO) && target.abs() > v.speed.abs();
    v.speed = vehicle::approach(v.speed, target, if speeding_up { vd.acceleration } else { vd.braking });
    let turn = turn.clamp(-vd.turn, vd.turn);
    let pose = Pose::of(v);

    // Feet that have swung come down, once in reach.
    let swing = legs.gait.def(c).swing;
    for leg in 0..4 {
        let Some(s) = legs.feet[leg].swing.as_mut() else { continue };
        s.progress = (s.progress + 1).min(swing);
        if s.progress < swing {
            continue;
        }
        let (to, floor) = (s.to, s.floor);
        let hip = pose.point(c.hip, leg);
        if heights(c, [(hip, to, floor)]).is_some_and(|(lo, hi)| lo <= v.height && v.height <= hi) {
            legs.feet[leg] = Foot { spot: to, floor, swing: None };
            landed[leg] = true;
        } else if heights(c, planted(&legs, c, pose).chain([(hip, to, floor)])).is_none() {
            // The body can't bring it into reach: aim again from here.
            if let Some((to, floor)) = foothold(&legs, leg, pose, pose, to, v.height, terrain, c, legs.gait.def(c), bodies) {
                let s = legs.feet[leg].swing.as_mut().expect("swinging");
                (s.to, s.floor) = (to, floor);
            }
        }
    }

    // The gait and its direction change only with every foot down.
    if legs.up().next().is_none() {
        let gait = if v.control.boost && !target.is_negative() { Gait::Trot } else { Gait::Walk };
        if gait != legs.gait {
            legs.clock = legs.clock * gait.def(c).cycle / legs.gait.def(c).cycle;
            legs.gait = gait;
        }
        legs.backing = target.is_negative() || (target == Fx::ZERO && v.speed.is_negative());
    }

    // The clock runs while it moves or a foot is out of place, and holds
    // while a foot waits to come down. A foot lifts as its slice starts.
    let gait = *legs.gait.def(c);
    let waiting = legs.feet.iter().any(|f| f.swing.as_ref().is_some_and(|s| s.progress >= gait.swing));
    let restless = legs.feet.iter().enumerate().any(|(i, f)| f.swing.is_some() || (f.spot - pose.point(c.stance, i)).length() > SETTLE);
    if !waiting && (direction != FxVec2::ZERO || v.speed != Fx::ZERO || restless) {
        legs.clock = if legs.backing { (legs.clock + gait.cycle - 1) % gait.cycle } else { (legs.clock + 1) % gait.cycle };
        let landing = pose.ahead(v.speed, turn, gait.swing);
        // Aim half a stance past landing, at the speed it's making for: the
        // stance then centres on rest.
        let aim = landing.ahead(target, turn, (gait.cycle - gait.swing) / 2);
        for leg in 0..4 {
            let start = legs.gait.start(leg, gait.cycle);
            let lifts = if legs.backing { start + gait.swing - 1 } else { start };
            let ideal = aim.point(c.stance, leg);
            if legs.clock != lifts || legs.feet[leg].swing.is_some() || (legs.feet[leg].spot - ideal).length() <= SETTLE {
                continue;
            }
            if let Some((to, floor)) = foothold(&legs, leg, pose, landing, ideal, v.height, terrain, c, &gait, bodies) {
                legs.feet[leg].swing = Some(Swing { to, floor, progress: 0 });
            }
        }
    }

    // The body moves and turns as far as its planted feet allow.
    let start = v.position;
    let delta = heading_vector(v.heading) * v.speed;
    let steps = (delta.length() / SUBSTEP).ceil().floor_to_int().max(1);
    let radius = def.body.radius;
    let mut at = pose;
    for _ in 0..steps {
        let moved = vehicle::push_out(terrain, at.position + delta.div_int(steps), radius);
        let turned = at.heading + turn.div_int(steps);
        let tries = [Pose { position: moved, heading: turned }, Pose { position: moved, heading: at.heading }, Pose { position: at.position, heading: turned }];
        match tries.into_iter().find(|&p| p != at && fits(&legs, at, p, v.height, terrain, c)) {
            Some(p) => at = p,
            None => break,
        }
    }
    (v.position, v.heading) = (at.position, at.heading);
    let made = (v.position - start).dot(heading_vector(pose.heading));
    if made.abs() < v.speed.abs() {
        v.speed = made;
    }

    // The body's height: toward its feet's mean, low enough for the feet
    // coming down to reach, never with its belly in the ground.
    let pose = Pose::of(v);
    let mean = (0..4).fold(Fx::ZERO, |s, i| s + legs.landing(i).1).div_int(4);
    let coming: Vec<_> = legs
        .feet
        .iter()
        .enumerate()
        .filter_map(|(i, f)| f.swing.as_ref().map(|s| (pose.ahead(v.speed, turn, gait.swing - s.progress).point(c.hip, i), s.to, s.floor)))
        .collect();
    let mut wanted = mean;
    if let Some((lo, hi)) = heights(c, planted(&legs, c, pose).chain(coming)) {
        wanted = wanted.clamp(lo, hi);
    }
    if let Some(ground) = under_body(terrain, pose, c) {
        wanted = wanted.max(ground + BELLY - c.deck);
    }
    if let Some((lo, hi)) = heights(c, planted(&legs, c, pose)) {
        wanted = wanted.clamp(lo, hi);
    }
    v.height = vehicle::approach(v.height, wanted, vd.lift);
    v.fall = Fx::ZERO;
    v.wheels = std::array::from_fn(|i| legs.feet[i].floor);
    v.legs = Some(legs);
    landed
}

// Riders (CR.4, C9) and the gunner (TU.1 of the turret, T2): the pilot
// stands at the helm on the deck's middle (the vehicle's one seat), up to
// two riders sit on the cargo deck behind it, at `RIDERS`, and a crawler
// with a gun has a gunner's place behind the crates, at `GUNNER`. Boarding
// reaches the body's box, not a radius, and climbing down puts an elder
// beside the box on a floor level with the highest its feet stand on.

/// Where the riders sit, from the deck's middle (the pilot's place): in
/// front of the crates, left and right. The rig (`app/actors/crawler.ron`)
/// keeps these spots clear.
pub const RIDERS: [Offset; 2] = [
    Offset { along: Fx::from_ratio(-3, 4), across: Fx::from_ratio(-7, 10) },
    Offset { along: Fx::from_ratio(-3, 4), across: Fx::from_ratio(7, 10) },
];

/// Where the gunner stands, from the deck's middle: behind the crates, on
/// the middle (the third rider's place before the turret). The gun itself
/// is on the cab's roof (`GunDef::mount`), so the gunner stands apart
/// from it.
pub const GUNNER: Offset = Offset { along: Fx::from_ratio(-12, 5), across: Fx::ZERO };

/// Where `seat` stands on a crawler's deck, from its middle: a rider's
/// place or the gunner's. `None` for the pilot, at the middle, and for a
/// building's slot.
pub fn deck_spot(seat: Seat) -> Option<Offset> {
    match seat {
        Seat::Rider { place, .. } => Some(RIDERS[place as usize]),
        Seat::Gunner { .. } => Some(GUNNER),
        Seat::Vehicle(_) | Seat::Building(_) => None,
    }
}

/// How far past the body's box an elder climbing down stands, beyond its
/// own radius, in meters.
const EXIT_GAP: Fx = Fx::from_ratio(3, 20);
/// How far from a foot's spot (beyond both radii) an elder climbing down
/// stands, in meters.
const EXIT_FOOT_GAP: Fx = Fx::from_ratio(1, 5);
/// The most a spot to climb down to lies above or below the highest floor
/// its feet stand on, in meters.
const EXIT_LEVEL: Fx = Fx::from_ratio(3, 5);

impl Pose {
    /// A point `along` the heading and `across` to the right of the middle.
    pub fn at(self, o: Offset) -> FxVec2 {
        let forward = heading_vector(self.heading);
        let right = FxVec2::new(forward.y, -forward.x);
        self.position + forward * o.along + right * o.across
    }

    /// `p` in the body's frame: along the heading and across to the right.
    pub fn local(self, p: FxVec2) -> Offset {
        let forward = heading_vector(self.heading);
        let right = FxVec2::new(forward.y, -forward.x);
        let d = p - self.position;
        Offset { along: d.dot(forward), across: d.dot(right) }
    }
}

/// How far `p` is from the body's box (its length and width, seen from
/// above), squared; zero inside it.
pub fn box_distance_squared(pose: Pose, c: &CrawlerDef, p: FxVec2) -> Fx {
    let o = pose.local(p);
    let (half_length, half_width) = (c.length / Fx::from_int(2), c.width / Fx::from_int(2));
    let along = (o.along.abs() - half_length).max(Fx::ZERO);
    let across = (o.across.abs() - half_width).max(Fx::ZERO);
    along * along + across * across
}

/// The point of the body's box (seen from above) nearest `p`: `p` itself
/// inside it.
pub fn box_nearest(pose: Pose, c: &CrawlerDef, p: FxVec2) -> FxVec2 {
    let o = pose.local(p);
    let (half_length, half_width) = (c.length / Fx::from_int(2), c.width / Fx::from_int(2));
    pose.at(Offset { along: o.along.clamp(-half_length, half_length), across: o.across.clamp(-half_width, half_width) })
}

/// Whether a crawler is walking (T6 of the turret): a foot swinging, the
/// body moving, or its pilot's stick held (turning in place moves the body
/// with no speed). A walking crawler takes no deliveries. A rover, or a
/// crawler with no legs, never stands still for one.
pub fn walking(v: &VehicleState) -> bool {
    let Some(legs) = v.legs.as_ref() else { return true };
    v.speed != Fx::ZERO || v.fall != Fx::ZERO || v.control.direction != FxVec2::ZERO || legs.up().next().is_some()
}

/// Where an elder of `radius` climbing down from `spot` (`None`: the
/// pilot's, else a rider's or the gunner's, `deck_spot`) stands: its
/// seat's own spot first (the pilot's beside the box's middle on the
/// left, a rider's beside the back on their side, the gunner's behind
/// the box), then beside
/// the box on the seat's side (the left for the middle seats), level with
/// its middle, toward the back and the front; then the other side; then
/// behind the box and in front of it. The spot
/// is in the arena and clear of the terrain's discs and rock, its floor
/// within `EXIT_LEVEL` of the highest floor its feet stand on (never down
/// a pit or a trench, even with a foot down in it), and clear of every
/// foot's spot and the spot each swinging foot lands on. `None` when
/// every spot is blocked.
pub fn exit_spot(v: &VehicleState, c: &CrawlerDef, spot: Option<Offset>, radius: Fx, terrain: &Terrain) -> Option<(FxVec2, Fx)> {
    let legs = v.legs.as_ref()?;
    let pose = Pose::of(v);
    let side = Fx::from_int(if spot.is_some_and(|o| o.across > Fx::ZERO) { 1 } else { -1 });
    let (half_length, half_width) = (c.length / Fx::from_int(2), c.width / Fx::from_int(2));
    let (beside, end) = (half_width + radius + EXIT_GAP, half_length + radius + EXIT_GAP);
    let along = Fx::from_ratio(3, 2);
    let offsets = [
        Offset { along: Fx::ZERO, across: side * beside },
        Offset { along: -along, across: side * beside },
        Offset { along, across: side * beside },
        Offset { along: Fx::ZERO, across: -side * beside },
        Offset { along: -along, across: -side * beside },
        Offset { along, across: -side * beside },
        Offset { along: -end, across: Fx::ZERO },
        Offset { along: end, across: Fx::ZERO },
    ];
    // The highest floor it stands on: a foot down in a trench doesn't
    // lead its elders down there.
    let level = (0..4).map(|i| legs.landing(i).1).max()?;
    // Each seat tries its own spot first, so four climbing down together
    // land apart: the pilot's beside the middle, the riders' beside the
    // back, the gunner's behind.
    let first = match spot {
        None => 0,
        Some(o) if o.across == Fx::ZERO => 6,
        Some(_) => 1,
    };
    let offsets = std::iter::once(offsets[first]).chain(offsets.into_iter().enumerate().filter(|&(i, _)| i != first).map(|(_, o)| o));
    let spots: Vec<FxVec2> = legs.feet.iter().map(|f| f.spot).chain(legs.feet.iter().filter_map(|f| f.swing.as_ref().map(|s| s.to))).collect();
    let clearance = FOOT_RADIUS + radius + EXIT_FOOT_GAP;
    offsets.into_iter().find_map(|o: Offset| {
        let p = pose.at(o);
        if !terrain.inside(p) || !terrain.clear(p, p, radius) {
            return None;
        }
        if spots.iter().any(|&s| (s - p).length_squared() < clearance * clearance) {
            return None;
        }
        let ground = terrain.footprint_near(p, level, radius)?;
        ((ground - level).abs() <= EXIT_LEVEL).then_some((p, ground))
    })
}

// Feet and bodies (CR.5, C10): a foot coming down shoves the bodies under
// it off its spot and stomps the threats there. Stage 2 notes the feet that
// came down (`Settlement::landings`), stage 3 lists who stands under each
// (`read_footfalls`, a `FootLands` message) and stage 5 applies it
// (`foot_lands`), as a threat's swing and its knockback are. Seated elders
// (the pilot and riders) are on the deck, out of reach. A foot never comes
// down on an elder or sprite it saw when it lifted (`foothold`); one that
// walked under it since is shoved. Bodies path round the feet
// (`feet_in_the_way`), not the body, which is overhead.

/// How far past touching a foot a body it shoves stands, in meters.
const SHOVE_GAP: Fx = Fx::from_ratio(1, 10);
/// What a foot coming down on a threat takes from its health: more than a
/// ghoul has (2 or 3), most of a brute's 5. Armor doesn't stop it.
pub const STOMP_DAMAGE: u32 = 4;
/// A stomped threat that lives is thrown off its spot at this, in meters
/// per second, and staggered `STOMP_STAGGER`.
const STOMP_KNOCKBACK: Fx = Fx::from_int(4);
const STOMP_STAGGER: u64 = content::millis_to_ticks(400);
/// A body whose feet are within this of a foot's floor stands on it, as
/// `foothold` counts it; a body in a tunnel under the floor is out of it.
const ON_FLOOR: Fx = Fx::ONE;

/// Whether feet at `height` and a foot's `floor` are on one floor: always
/// outside a volume, where every body walks on the ground.
fn level_with(terrain: &Terrain, height: Fx, floor: Fx) -> bool {
    terrain.volume.is_none() || (height - floor).abs() <= ON_FLOOR
}

/// Whether a body of `radius` at `position`, its feet at `height`, is
/// under a foot at `spot` on `floor`.
fn under_foot(terrain: &Terrain, spot: FxVec2, floor: Fx, position: FxVec2, radius: Fx, height: Fx) -> bool {
    let reach = radius + FOOT_RADIUS;
    (position - spot).length_squared() < reach * reach && level_with(terrain, height, floor)
}

/// Every crawler's feet as bodies see them: where each stands, or where it
/// swings to, and its floor.
pub fn footings(vehicles: &[VehicleState]) -> Vec<(FxVec2, Fx)> {
    vehicles.iter().filter_map(|v| v.legs.as_ref()).flat_map(|legs| (0..4).map(move |i| legs.landing(i))).collect()
}

/// The feet a body of `radius` at `from`, its feet at `z`, paths round:
/// a disc of `FOOT_RADIUS` at each of `footings` level with it, but those
/// it already touches (it walks out from under them).
pub fn feet_in_the_way(footings: &[(FxVec2, Fx)], terrain: &Terrain, from: FxVec2, z: Fx, radius: Fx) -> Vec<Disc> {
    footings
        .iter()
        .filter(|&&(spot, floor)| !under_foot(terrain, spot, floor, from, radius, z) && level_with(terrain, z, floor))
        .map(|&(center, _)| Disc { center, radius: FOOT_RADIUS })
        .collect()
}

/// Where a body of `radius` under a foot at `spot` is shoved to from
/// `from`, its feet at `z`: just clear of the foot, away from it (from the
/// crawler's `middle` if it stands on the spot itself), or else turned by
/// eighths of a turn, nearest first. The spot is in the arena, clear of
/// discs, rock and every foot (`others`), and a walk from where it stands
/// reaches it; with its feet's height there. `None` when nowhere is.
fn shove_spot(terrain: &Terrain, spot: FxVec2, middle: FxVec2, from: FxVec2, z: Fx, radius: Fx, others: &[(FxVec2, Fx)]) -> Option<(FxVec2, Fx)> {
    let away = if from != spot {
        from - spot
    } else if spot != middle {
        spot - middle
    } else {
        FxVec2::new(Fx::ZERO, Fx::ONE)
    };
    let (base, reach) = (bearing(FxVec2::ZERO, away), FOOT_RADIUS + radius + SHOVE_GAP);
    let touching = (FOOT_RADIUS + radius) * (FOOT_RADIUS + radius);
    [0, 1, -1, 2, -2, 3, -3, 4].into_iter().find_map(|eighths: i64| {
        let p = spot + heading_vector(base + Fx::PI.mul_int(eighths).div_int(4)) * reach;
        if !terrain.inside(p) || !terrain.clear(p, p, radius) || others.iter().any(|&(f, _)| (f - p).length_squared() < touching) {
            return None;
        }
        Some((p, terrain.walk_at(from, p, z, radius)?))
    })
}

impl Settlement {
    /// Stage 3: for each foot that came down this tick, the agents standing
    /// under it (alive, not seated, not inside a building) and the threats,
    /// filed at the foot.
    pub(super) fn read_footfalls(&self, terrain: &Terrain) -> Vec<(FxVec2, Messages)> {
        self.landings
            .iter()
            .filter_map(|&(vehicle, leg)| {
                let foot = &self.vehicles.get(vehicle as usize)?.legs.as_ref()?.feet[leg as usize];
                let (spot, floor) = (foot.spot, foot.floor);
                let agents = self
                    .agents
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| a.body.alive() && a.avatar().is_none_or(|p| p.seat.is_none()) && a.sprite().is_none_or(|s| !s.sheltered()))
                    .filter(|(_, a)| under_foot(terrain, spot, floor, a.body.position, a.radius(), a.height()))
                    .map(|(i, _)| i as AgentIx)
                    .collect();
                let threats = self.threats.iter().filter(|t| under_foot(terrain, spot, floor, t.position, self.threat_def(t).body.radius, t.height)).map(|t| t.id).collect();
                let mut out = Messages::default();
                out.foot_lands.push(FootLands { vehicle, leg, spot, floor, agents, threats });
                Some((spot, out))
            })
            .collect()
    }

    /// Stage 5: a foot came down. Each agent still under it is shoved just
    /// clear of it (`shove_spot`; one with nowhere to go stays), and each
    /// threat still under it takes `STOMP_DAMAGE`, dying at 0 as a strike's
    /// kill (`Event::Hit`, `Impact::Stomp`), or is thrown off and staggered.
    /// Then `Event::Footfall`, for the view's dust.
    pub(crate) fn foot_lands(&mut self, tick: u64, message: FootLands, terrain: &Terrain) {
        let FootLands { vehicle, leg, spot, floor, agents, threats } = message;
        let Some(middle) = self.vehicles.get(vehicle as usize).map(|v| v.position) else { return };
        let others = footings(&self.vehicles);
        let mut shoved = 0;
        for a in agents {
            let Some(agent) = self.agents.get(a as usize) else { continue };
            let seated = agent.avatar().is_some_and(|p| p.seat.is_some()) || agent.sprite().is_some_and(|s| s.sheltered());
            let (position, radius, height) = (agent.body.position, agent.radius(), agent.height());
            if !agent.body.alive() || seated || !under_foot(terrain, spot, floor, position, radius, height) {
                continue;
            }
            let Some((to, feet)) = shove_spot(terrain, spot, middle, position, height, radius, &others) else { continue };
            let agent = &mut self.agents[a as usize];
            agent.body.position = to;
            agent.body.velocity = FxVec2::ZERO;
            agent.body.push = FxVec2::ZERO;
            match &mut agent.role {
                Role::Avatar(avatar) => {
                    // Shoved off its spot, it can't dig there.
                    avatar.dig = None;
                    if !avatar.flight.airborne {
                        avatar.height = feet;
                        avatar.flight.support = feet;
                    }
                }
                Role::Sprite(sprite) => sprite.height = feet,
                Role::Hauler(hauler) => hauler.height = feet,
            }
            shoved += 1;
        }
        let mut stomped = 0;
        for id in threats {
            let Some(i) = self.threat_index(id) else { continue };
            let t = &self.threats[i];
            if !under_foot(terrain, spot, floor, t.position, self.threat_def(t).body.radius, t.height) {
                continue;
            }
            let t = &mut self.threats[i];
            t.hp = t.hp.saturating_sub(STOMP_DAMAGE);
            let mut away = (t.position - spot).normalize_or_zero();
            if away == FxVec2::ZERO {
                away = (spot - middle).normalize_or_zero();
            }
            t.knockback = away * STOMP_KNOCKBACK;
            t.stagger_until = tick + STOMP_STAGGER;
            // A stagger cancels a windup.
            t.windup = None;
            let (killed, position) = (t.hp == 0, t.position);
            self.events.push(Event::Hit { target: id, agent: None, projectile: None, position, killed, impact: Impact::Stomp });
            if killed {
                self.threats.remove(i);
                self.combat.kills += 1;
            }
            stomped += 1;
        }
        self.events.push(Event::Footfall { vehicle, leg, position: spot, floor, shoved, stomped });
    }
}

/// Every planted foot in reach of its hip and under it, and no more feet
/// in the air than its gait lifts at once: in a trot, a diagonal pair.
pub fn check_reach(v: &VehicleState, c: &CrawlerDef) -> Result<(), String> {
    let legs = v.legs.as_ref().ok_or("a crawler without legs")?;
    let (pose, hip_height) = (Pose::of(v), v.height + c.deck);
    for (i, f) in legs.feet.iter().enumerate().filter(|(_, f)| f.swing.is_none()) {
        let rise = hip_height - f.floor;
        let distance = ((f.spot - pose.point(c.hip, i)).length_squared() + rise * rise).sqrt();
        if distance > c.reach + ROUNDING || rise < MIN_RISE - ROUNDING {
            return Err(format!("crawler foot {i} out of reach: {} m from its hip, {} m under it", distance.to_f64(), rise.to_f64()));
        }
    }
    let up: Vec<usize> = legs.up().collect();
    if up.len() > legs.gait.most_up() || (up.len() == 2 && up != [0, 3] && up != [1, 2]) {
        return Err(format!("crawler feet {up:?} up together at a {:?}", legs.gait));
    }
    Ok(())
}

/// Every planted foot in the arena on a floor of its height.
pub fn check_floors(v: &VehicleState, terrain: &Terrain) -> Result<(), String> {
    let legs = v.legs.as_ref().ok_or("a crawler without legs")?;
    for (i, f) in legs.feet.iter().enumerate().filter(|(_, f)| f.swing.is_none()) {
        if !terrain.inside(f.spot) || !on_floor(terrain, f.spot, f.floor) {
            return Err(format!("crawler foot {i} at {:?} isn't on a floor at {} m", f.spot, f.floor.to_f64()));
        }
    }
    Ok(())
}
