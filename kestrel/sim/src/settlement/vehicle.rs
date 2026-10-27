//! Vehicles: characters with the `Vehicle` brain, driven by the player
//! sitting in them.
//!
//! A vehicle keeps its own table (`Settlement::vehicles`), as threats do,
//! and its index is stable for the life of the world. It is lent to its
//! driver: the avatar's `AvatarState::driving` is the one link between them,
//! and while it is set the driver's movement commands steer the vehicle
//! instead of the avatar (`drive`). Only the driver writes the vehicle's
//! controls, in stage 1, so they need no message; climbing in does, since
//! two players may reach for the same seat on one tick (`Settlement::board`).
//!
//! Driving is an arcade model in `Fx`: the stick is a direction on the
//! ground, the vehicle turns toward it (or backs toward it, when it points
//! behind) and speeds up toward the throttle. Four wheels sample the ground;
//! a wheel rolls up a step no higher than `climb` and over any drop, and the
//! chassis rides on their mean height, rising at `lift` and falling under
//! `gravity`. Discs, rock columns and the arena edge block it like an
//! avatar's body.

use content::character::{CharacterDef, CharacterIx, VehicleDef};
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::action::Action;
use super::avatar::Intent;
use super::{AgentState, Role, Settlement};
use crate::messages::Board;
use crate::nav::{angle_between, bearing, heading_vector};
use crate::terrain::Terrain;

pub type VehicleIx = u16;

/// How far beyond touching a player can be to climb in, in meters.
const BOARD_REACH: Fx = Fx::from_ratio(4, 5);
/// A stick pointing further than this from the heading backs up (120°).
const REVERSE_ANGLE: Fx = Fx::from_ratio(20_944, 10_000);
/// The longest stretch of a move checked at once, in meters.
const SUBSTEP: Fx = Fx::from_ratio(1, 10);
/// A rock column blocks like the disc around it (as in `Terrain::clear`).
const ROCK_RADIUS: Fx = Fx::from_ratio(71, 100);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VehicleState {
    /// The character it is.
    pub character: CharacterIx,
    pub position: FxVec2,
    pub heading: Fx,
    /// Meters per tick along the heading; negative while backing up.
    pub speed: Fx,
    /// The chassis base above the arena floor: the wheels' mean ground once
    /// it settles.
    pub height: Fx,
    /// Meters per tick it is falling.
    pub fall: Fx,
    /// The ground under each wheel: front left, front right, rear left,
    /// rear right. The view tilts the chassis and drops the wheels from it.
    pub wheels: [Fx; 4],
    /// What its driver asks for. Zero without a driver.
    pub control: Control,
}

/// A driver's controls: a direction on the ground (length 0 to 1) and
/// whether to boost to the run speed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Control {
    pub direction: FxVec2,
    pub boost: bool,
}

fn tuning(def: &CharacterDef) -> &VehicleDef {
    def.vehicle.as_ref().expect("vehicle characters have vehicle tuning")
}

impl VehicleState {
    /// A parked vehicle at `position`, settled on the ground. `None` where
    /// a wheel would be over a drop or outside the arena.
    pub fn new(character: CharacterIx, def: &CharacterDef, position: FxVec2, heading: Fx, terrain: &Terrain) -> Option<VehicleState> {
        let wheels = ground_under(terrain, wheel_points(position, heading, tuning(def)))?;
        let height = mean(wheels);
        Some(VehicleState { character, position, heading, speed: Fx::ZERO, height, fall: Fx::ZERO, wheels, control: Control::default() })
    }

    /// Where the driver sits: on the vehicle, facing its way, `seat` above
    /// the chassis base.
    pub fn seat(&self, def: &CharacterDef) -> (FxVec2, Fx) {
        (self.position, self.height + tuning(def).seat)
    }

    /// Meters per second along the ground.
    pub fn velocity(&self) -> FxVec2 {
        heading_vector(self.heading) * self.speed.mul_int(content::TICKS_PER_SECOND as i64)
    }
}

/// The four wheels' contact points, in `VehicleState::wheels` order.
pub fn wheel_points(position: FxVec2, heading: Fx, vehicle: &VehicleDef) -> [FxVec2; 4] {
    let forward = heading_vector(heading);
    let right = FxVec2::new(forward.y, -forward.x);
    let (ahead, side) = (forward * (vehicle.wheelbase / Fx::from_int(2)), right * (vehicle.track / Fx::from_int(2)));
    [position + ahead - side, position + ahead + side, position - ahead - side, position - ahead + side]
}

fn ground_under(terrain: &Terrain, points: [FxVec2; 4]) -> Option<[Fx; 4]> {
    let mut ground = [Fx::ZERO; 4];
    for (g, p) in ground.iter_mut().zip(points) {
        if !terrain.inside(p) {
            return None;
        }
        *g = terrain.ground(p)?;
    }
    Some(ground)
}

fn mean(wheels: [Fx; 4]) -> Fx {
    (wheels[0] + wheels[1] + wheels[2] + wheels[3]).div_int(4)
}

/// Whether the wheels can go from `from` to the ground `to`: no wheel
/// rises more than `climb`.
fn rollable(from: [Fx; 4], to: [Fx; 4], climb: Fx) -> bool {
    from.iter().zip(&to).all(|(&a, &b)| b - a <= climb)
}

fn approach(current: Fx, target: Fx, max_change: Fx) -> Fx {
    current + (target - current).clamp(-max_change, max_change)
}

/// Pushes a body of `radius` at `p` out of the terrain's discs and rock
/// columns, three passes, and keeps it in the arena.
fn push_out(terrain: &Terrain, mut p: FxVec2, radius: Fx) -> FxVec2 {
    let (min, max) = (terrain.min(), terrain.max());
    let extra = Fx::from_ratio(1, 50);
    let rock = |p: FxVec2| {
        let reach = (ROCK_RADIUS + radius).ceil().floor_to_int() as i32 + 1;
        let (x0, y0) = ((p.x.floor_to_int() as i32) - reach, (p.y.floor_to_int() as i32) - reach);
        let mut discs = Vec::new();
        for x in x0..=x0 + 2 * reach {
            for y in y0..=y0 + 2 * reach {
                if terrain.is_rock((x, y)) {
                    discs.push((FxVec2::new(Fx::from_int(x) + Fx::HALF, Fx::from_int(y) + Fx::HALF), ROCK_RADIUS));
                }
            }
        }
        discs
    };
    for _ in 0..3 {
        let discs = terrain.blocking_obstacles().map(|o| (o.center, o.radius)).chain(rock(p));
        for (center, r) in discs.collect::<Vec<_>>() {
            let separation = p - center;
            let clearance = r + radius + extra;
            if separation.length_squared() < clearance * clearance {
                let length = separation.length();
                let away = if length == Fx::ZERO { FxVec2::new(Fx::ONE, Fx::ZERO) } else { separation / length };
                p += away * (clearance - length);
            }
        }
        p = FxVec2::new(p.x.clamp(min.x, max.x), p.y.clamp(min.y, max.y));
    }
    p
}

/// Applies the driver's action to the vehicle they drive. Returns whether
/// it was the vehicle's alone: movement steers the vehicle instead of the
/// avatar. Letting go of direct control also lets go of the controls, and
/// still goes on to the avatar.
pub(super) fn drive(vehicle: &mut VehicleState, action: &Action) -> bool {
    match action {
        Action::Move(intent) => {
            vehicle.control = match *intent {
                Intent::Walk { direction, run } => Control { direction, boost: run },
                // Clicking a point to drive to isn't supported yet: stop.
                Intent::Stop | Intent::GoTo { .. } => Control::default(),
            };
            true
        }
        Action::Release => {
            vehicle.control = Control::default();
            false
        }
        _ => false,
    }
}

/// Stage 2 for one vehicle: turn and speed toward its controls, then roll
/// over the ground.
pub(super) fn self_update(v: &mut VehicleState, terrain: &Terrain, def: &CharacterDef) {
    let vd = tuning(def);
    let radius = def.body.radius;
    // The ground under it may have been dug since.
    if let Some(wheels) = ground_under(terrain, wheel_points(v.position, v.heading, vd)) {
        v.wheels = wheels;
    }
    let top = if v.control.boost { def.movement.run } else { def.movement.walk }.div_int(content::TICKS_PER_SECOND as i64);
    let (mut target, mut turn) = (Fx::ZERO, Fx::ZERO);
    let direction = v.control.direction;
    if direction != FxVec2::ZERO {
        let want = bearing(FxVec2::ZERO, direction);
        let throttle = direction.length().min(Fx::ONE);
        let ahead = angle_between(v.heading, want);
        if ahead.abs() > REVERSE_ANGLE {
            // Back up, swinging the rear toward the stick.
            target = -(vd.reverse * throttle);
            turn = angle_between(v.heading + Fx::PI, want);
        } else {
            target = top * throttle;
            // Ease off through a tight turn.
            if ahead.abs() > Fx::FRAC_PI_2 {
                target = target / Fx::from_int(2);
            }
            turn = ahead;
        }
    }
    let speeding_up = target != Fx::ZERO && (target.is_negative() == v.speed.is_negative() || v.speed == Fx::ZERO) && target.abs() > v.speed.abs();
    v.speed = approach(v.speed, target, if speeding_up { vd.acceleration } else { vd.braking });

    // Turning needs rolling: full rate from a third of the top speed.
    let rolling = (v.speed.abs() * Fx::from_int(3) / def.movement.walk.div_int(content::TICKS_PER_SECOND as i64)).min(Fx::ONE);
    let rate = vd.turn * rolling;
    if rate > Fx::ZERO && turn != Fx::ZERO {
        let heading = v.heading + turn.clamp(-rate, rate);
        // A turn that would swing a wheel up a cliff or over the edge doesn't happen.
        if let Some(wheels) = ground_under(terrain, wheel_points(v.position, heading, vd)).filter(|&w| rollable(v.wheels, w, vd.climb)) {
            v.heading = heading;
            v.wheels = wheels;
        }
    }

    let start = v.position;
    let delta = heading_vector(v.heading) * v.speed;
    let steps = (delta.length() / SUBSTEP).ceil().floor_to_int().max(1);
    for _ in 0..steps {
        let candidate = push_out(terrain, v.position + delta.div_int(steps), radius);
        match ground_under(terrain, wheel_points(candidate, v.heading, vd)).filter(|&w| rollable(v.wheels, w, vd.climb)) {
            Some(wheels) => {
                v.position = candidate;
                v.wheels = wheels;
            }
            None => break,
        }
    }
    // Whatever stopped or deflected it took that much of its speed.
    let made = (v.position - start).dot(heading_vector(v.heading));
    if made.abs() < v.speed.abs() {
        v.speed = made;
    }

    let rest = mean(v.wheels);
    if v.height > rest {
        v.fall += vd.gravity;
        v.height = (v.height - v.fall).max(rest);
        if v.height == rest {
            v.fall = Fx::ZERO;
        }
    } else {
        v.fall = Fx::ZERO;
        v.height = (v.height + vd.lift).min(rest);
    }
}

impl Settlement {
    /// Stage 2: every vehicle drives itself. Each writes only itself.
    pub(super) fn vehicles_self_update(&mut self, terrain: &Terrain) {
        use rayon::prelude::*;
        let characters = &self.characters;
        self.vehicles.par_iter_mut().for_each(|v| self_update(v, terrain, &characters[v.character as usize]));
    }

    /// The vehicle `agent` drives, if it drives one.
    pub fn driving(&self, agent: usize) -> Option<VehicleIx> {
        self.agents.get(agent)?.avatar()?.driving
    }

    /// The agent driving vehicle `vehicle`, if anyone is.
    pub fn driver_of(&self, vehicle: VehicleIx) -> Option<usize> {
        self.agents.iter().position(|a| a.avatar().is_some_and(|p| p.driving == Some(vehicle)))
    }

    /// Stage 5: players climb into the nearest free vehicle in reach, in
    /// command order, so the first to reach for a seat gets it.
    pub(crate) fn board(&mut self, messages: Vec<Board>) {
        for Board { agent } in messages {
            let agent = agent as usize;
            let Some(a) = self.agents.get(agent) else { continue };
            let Some(avatar) = a.avatar() else { continue };
            if !a.body.alive() || avatar.driving.is_some() || avatar.dig.is_some() {
                continue;
            }
            let (position, radius) = (a.body.position, a.radius());
            let nearest = (0..self.vehicles.len())
                .filter(|&ix| self.driver_of(ix as VehicleIx).is_none())
                .map(|ix| {
                    let v = &self.vehicles[ix];
                    let reach = self.characters[v.character as usize].body.radius + radius + BOARD_REACH;
                    ((v.position - position).length_squared(), reach, ix)
                })
                .filter(|&(d2, reach, _)| d2 <= reach * reach)
                .min_by_key(|&(d2, _, ix)| (d2, ix));
            let Some((_, _, ix)) = nearest else { continue };
            let AgentState { body, role: Role::Avatar(avatar), .. } = &mut self.agents[agent] else { continue };
            avatar.driving = Some(ix as VehicleIx);
            avatar.intent = Some(Intent::Stop);
            avatar.flight.requested = false;
            avatar.swing = None;
            avatar.queued = None;
            avatar.queued_at = None;
            body.route.clear();
            body.velocity = FxVec2::ZERO;
            body.push = FxVec2::ZERO;
            self.vehicles[ix].control = Control::default();
        }
    }

    /// Housekeeping: drivers sit in their seats, and a vehicle nobody
    /// drives lets go of its controls.
    pub(crate) fn vehicle_housekeeping(&mut self) {
        let mut driven = vec![false; self.vehicles.len()];
        for a in &mut self.agents {
            let AgentState { body, role: Role::Avatar(avatar), .. } = a else { continue };
            let Some(ix) = avatar.driving else { continue };
            let v = &self.vehicles[ix as usize];
            seat(body, avatar, v, &self.characters[v.character as usize]);
            driven[ix as usize] = true;
        }
        for (v, driven) in self.vehicles.iter_mut().zip(driven) {
            if !driven {
                v.control = Control::default();
            }
        }
    }
}

/// Puts a driver in the vehicle's seat, facing its way.
pub(super) fn seat(body: &mut super::Body, avatar: &mut super::avatar::AvatarState, v: &VehicleState, def: &CharacterDef) {
    let (position, height) = v.seat(def);
    body.position = position;
    body.heading = v.heading;
    body.velocity = FxVec2::ZERO;
    avatar.height = height;
    avatar.flight.airborne = false;
    avatar.flight.support = v.height;
}

/// Where a driver climbing out stands: beside the vehicle (left, right,
/// behind, then in front), on ground it could walk to from the seat's
/// level. `None` when every side is blocked.
pub(super) fn exit_spot(v: &VehicleState, def: &CharacterDef, radius: Fx, terrain: &Terrain) -> Option<(FxVec2, Fx)> {
    let forward = heading_vector(v.heading);
    let right = FxVec2::new(forward.y, -forward.x);
    let gap = def.body.radius + radius + Fx::from_ratio(3, 20);
    [-right, right, -forward, forward].into_iter().find_map(|side| {
        let p = v.position + side * gap;
        let ground = terrain.footprint(p, radius)?;
        let level = (ground - v.height).abs() <= Fx::from_ratio(3, 5);
        (level && terrain.clear(p, p, radius)).then_some((p, ground))
    })
}
