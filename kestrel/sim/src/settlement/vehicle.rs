//! Vehicles: characters with the `Vehicle` brain, driven by the player
//! sitting in them.
//!
//! A vehicle keeps its own table (`Settlement::vehicles`), as threats do,
//! and its index is stable for the life of the world. It is lent to its
//! driver: the avatar's `AvatarState::seat` is the one link between them,
//! and while it is set the driver's movement commands steer the vehicle
//! instead of the avatar (`drive`). Only the driver writes the vehicle's
//! controls, in stage 1, so they need no message; climbing in does, since
//! two players may reach for the same seat on one tick (`Settlement::board`).
//!
//! A built building's slot is a seat too (`Seat::Building`, D3): boarding
//! it possesses the building, putting out the sprite there. One handler
//! boards both, the nearer one winning.
//!
//! Driving is an arcade model in `Fx`: the stick is a direction on the
//! ground, the vehicle turns toward it (or backs toward it, when it points
//! behind) and speeds up toward the throttle. Four wheels sample the ground
//! (in a volume, the floor each rolls on: `ground_under`); a wheel rolls up
//! a step no higher than `climb` and over any drop, and the chassis rides on
//! their mean height, rising at `lift` and falling under `gravity`. Discs,
//! rock columns and the arena edge block it like an avatar's body.
//!
//! The crawler (`kestrel/CRAWLER.md`) is a vehicle too, driven the same
//! way, but it walks on four feet instead of rolling (`crawler`): its
//! `legs` are set, and its `wheels` are its feet's floors.

use content::character::{CharacterDef, CharacterIx, Offset, VehicleDef};
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::action::Action;
use super::avatar::{AvatarState, BundleRefusal, Intent, Seat};
use super::crawler::{self, Legs, Pose, Standing};
use super::interact::{self, INTERACT_REACH};
use super::{AgentIx, AgentState, BuildingIx, Event, Occupant, Role, Settlement, Slot};
use crate::messages::Board;
use crate::nav::{angle_between, bearing, heading_vector};
use crate::terrain::Terrain;

pub type VehicleIx = u16;

/// How far beyond touching a player can be to climb in, in meters.
const BOARD_REACH: Fx = Fx::from_ratio(4, 5);
/// A stick pointing further than this from the heading backs up (120°).
pub(super) const REVERSE_ANGLE: Fx = Fx::from_ratio(20_944, 10_000);
/// The longest stretch of a move checked at once, in meters.
pub(super) const SUBSTEP: Fx = Fx::from_ratio(1, 10);
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
    /// A crawler's feet and gait (format 47); `None` for a rover.
    pub legs: Option<Legs>,
    /// A crawler's gun: its turret and ammo store (format 51, TU.2 of the
    /// turret, `gun`). `None` without a gun, or where the scenario has no
    /// item for its ammo.
    #[serde(default)]
    pub gun: Option<super::gun::GunState>,
}

/// A driver's controls: a direction on the ground (length 0 to 1) and
/// whether to boost to the run speed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Control {
    pub direction: FxVec2,
    pub boost: bool,
}

pub(super) fn tuning(def: &CharacterDef) -> &VehicleDef {
    def.vehicle.as_ref().expect("vehicle characters have vehicle tuning")
}

impl VehicleState {
    /// A parked vehicle at `position`, settled on the ground. `None` where
    /// a wheel would be over a drop or outside the arena.
    /// A crawler stands with its feet at rest (`crawler::park`).
    pub fn new(character: CharacterIx, def: &CharacterDef, position: FxVec2, heading: Fx, terrain: &Terrain) -> Option<VehicleState> {
        let (wheels, height, legs) = match &def.crawler {
            Some(c) => {
                let (legs, height) = crawler::park(Pose { position, heading }, c, terrain)?;
                (std::array::from_fn(|i| legs.feet[i].floor), height, Some(legs))
            }
            None => {
                let wheels = ground_under(terrain, wheel_points(position, heading, tuning(def)), None)?;
                (wheels, mean(wheels), None)
            }
        };
        Some(VehicleState { character, position, heading, speed: Fx::ZERO, height, fall: Fx::ZERO, wheels, control: Control::default(), legs, gun: None })
    }

    /// Where the driver sits: on the vehicle, facing its way, `seat` above
    /// the chassis base.
    pub fn seat(&self, def: &CharacterDef) -> (FxVec2, Fx) {
        (self.position, self.height + tuning(def).seat)
    }

    /// Where the driver (`None`) or someone at `spot` on a crawler's deck
    /// sits (a rider or the gunner, `crawler::deck_spot`), as high as the
    /// driver.
    pub fn seat_at(&self, def: &CharacterDef, spot: Option<Offset>) -> (FxVec2, Fx) {
        let (position, height) = self.seat(def);
        match spot {
            Some(o) => (Pose::of(self).at(o), height),
            None => (position, height),
        }
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

/// The ground under each wheel. In a volume scene, given the wheels'
/// ground before (`from`), each reads the floor it rolls on
/// (`Terrain::floor`): the highest no more than `climb` and a layer over
/// its last, so a wheel stays on its floor over a tunnel or a cave, and a
/// wall too high to climb is its top or none (both refused by
/// `rollable`). Elsewhere, and when parked, the top of the ground.
fn ground_under(terrain: &Terrain, points: [FxVec2; 4], from: Option<([Fx; 4], Fx)>) -> Option<[Fx; 4]> {
    let mut ground = [Fx::ZERO; 4];
    for (i, (g, p)) in ground.iter_mut().zip(points).enumerate() {
        if !terrain.inside(p) {
            return None;
        }
        *g = match from {
            Some((wheels, climb)) if terrain.volume.is_some() => terrain.floor(p, wheels[i] + climb)?,
            _ => terrain.ground(p)?,
        };
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

pub(super) fn approach(current: Fx, target: Fx, max_change: Fx) -> Fx {
    current + (target - current).clamp(-max_change, max_change)
}

/// Pushes a body of `radius` at `p` out of the terrain's discs and rock
/// columns, three passes, and keeps it in the arena.
pub(super) fn push_out(terrain: &Terrain, mut p: FxVec2, radius: Fx) -> FxVec2 {
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

/// Stage 2 for one vehicle with nobody standing round it: see
/// `self_update_among`.
#[cfg(test)]
pub(super) fn self_update(v: &mut VehicleState, terrain: &Terrain, def: &CharacterDef) {
    self_update_among(v, terrain, def, &[]);
}

/// Stage 2 for one vehicle: turn and speed toward its controls, then roll
/// over the ground. A crawler walks instead (`crawler::self_update`), its
/// feet clear of `bodies`. Returns which of a crawler's feet came down
/// this tick (none for a rover).
pub(super) fn self_update_among(v: &mut VehicleState, terrain: &Terrain, def: &CharacterDef, bodies: &[Standing]) -> [bool; 4] {
    if v.legs.is_some() {
        return crawler::self_update(v, terrain, def, bodies);
    }
    let vd = tuning(def);
    let radius = def.body.radius;
    // The ground under it may have been dug since.
    if let Some(wheels) = ground_under(terrain, wheel_points(v.position, v.heading, vd), Some((v.wheels, vd.climb))) {
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
        if let Some(wheels) = ground_under(terrain, wheel_points(v.position, heading, vd), Some((v.wheels, vd.climb))).filter(|&w| rollable(v.wheels, w, vd.climb)) {
            v.heading = heading;
            v.wheels = wheels;
        }
    }

    let start = v.position;
    let delta = heading_vector(v.heading) * v.speed;
    let steps = (delta.length() / SUBSTEP).ceil().floor_to_int().max(1);
    for _ in 0..steps {
        let candidate = push_out(terrain, v.position + delta.div_int(steps), radius);
        match ground_under(terrain, wheel_points(candidate, v.heading, vd), Some((v.wheels, vd.climb))).filter(|&w| rollable(v.wheels, w, vd.climb)) {
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
    [false; 4]
}

impl Settlement {
    /// Stage 2: every vehicle drives itself. Each writes only itself; a
    /// crawler's feet keep clear of `bodies`, as they stood last tick. The
    /// feet that came down are kept for stage 3 (`Settlement::landings`).
    pub(super) fn vehicles_self_update(&mut self, terrain: &Terrain, bodies: &[Standing]) {
        use rayon::prelude::*;
        let characters = &self.characters;
        let landed: Vec<[bool; 4]> = self.vehicles.par_iter_mut().map(|v| self_update_among(v, terrain, &characters[v.character as usize], bodies)).collect();
        self.landings = landed
            .iter()
            .enumerate()
            .flat_map(|(ix, feet)| (0..4u8).filter(move |&leg| feet[leg as usize]).map(move |leg| (ix as VehicleIx, leg)))
            .collect();
    }

    /// The bodies a crawler's feet keep clear of, if there is a crawler.
    pub(super) fn standing_for_crawlers(&self) -> Vec<Standing> {
        if self.vehicles.iter().any(|v| v.legs.is_some()) {
            crawler::standing(&self.agents)
        } else {
            Vec::new()
        }
    }

    /// The vehicle `agent` drives, if it drives one.
    pub fn driving(&self, agent: usize) -> Option<VehicleIx> {
        self.seat(agent)?.vehicle()
    }

    /// The seat `agent` sits in, if it sits in one: a vehicle it drives or
    /// a building it possesses.
    pub fn seat(&self, agent: usize) -> Option<Seat> {
        self.agents.get(agent)?.avatar()?.seat
    }

    /// The agent driving vehicle `vehicle`, if anyone is.
    pub fn driver_of(&self, vehicle: VehicleIx) -> Option<usize> {
        self.agents.iter().position(|a| a.avatar().is_some_and(|p| p.seat == Some(Seat::Vehicle(vehicle))))
    }

    /// The agent riding vehicle `vehicle` in `place`, if anyone is.
    pub fn rider_of(&self, vehicle: VehicleIx, place: u8) -> Option<usize> {
        self.agents.iter().position(|a| a.avatar().is_some_and(|p| p.seat == Some(Seat::Rider { vehicle, place })))
    }

    /// The agent at vehicle `vehicle`'s gun, if anyone is.
    pub fn gunner_of(&self, vehicle: VehicleIx) -> Option<usize> {
        self.agents.iter().position(|a| a.avatar().is_some_and(|p| p.seat == Some(Seat::Gunner { vehicle })))
    }

    /// The free seat of vehicle `ix` for an elder at `from`: the driver's
    /// first, then, on a crawler, the gun's if it has one (T3 of the
    /// turret), then the nearest free rider's place on the deck (the lower
    /// place on a tie). `None` when every seat is taken.
    fn free_seat(&self, ix: VehicleIx, from: FxVec2) -> Option<Seat> {
        if self.driver_of(ix).is_none() {
            return Some(Seat::Vehicle(ix));
        }
        let v = &self.vehicles[ix as usize];
        let c = self.characters[v.character as usize].crawler.as_ref()?;
        // A sprite at the gun keeps it until TU.4's crew job lets an elder
        // take it from them.
        if c.gun.is_some() && self.gunner_of(ix).is_none() && self.sprite_gunner_of(ix).is_none() {
            return Some(Seat::Gunner { vehicle: ix });
        }
        (0..crawler::RIDERS.len() as u8)
            .filter(|&place| self.rider_of(ix, place).is_none())
            .min_by_key(|&place| ((Pose::of(v).at(crawler::RIDERS[place as usize]) - from).length_squared(), place))
            .map(|place| Seat::Rider { vehicle: ix, place })
    }

    /// Stage 5: players climb into the nearest free seat in reach, in
    /// command order (players in order, `World::apply_commands`), so the
    /// first to reach for a seat gets it.
    ///
    /// - A vehicle is in reach within `BOARD_REACH` of touching it and free
    ///   while nobody drives it, as it always was. A crawler (CR.4) is in
    ///   reach within `BOARD_REACH` of touching its body's box, and free
    ///   while any of its four seats is: the cab first, then the gun (TU.1
    ///   of the turret), then the nearest free rider's place on the deck
    ///   (`free_seat`).
    /// - A building (D3) is in reach while its slot spot is within the
    ///   interact reach, so the boarding key and interact (which names the
    ///   building, `building`) reach the same. Only a built one counts; it
    ///   is free unless another elder possesses it, and boarding it is
    ///   refused while a carry job runs or the bundle holds anything
    ///   (`interact::board_refusal`).
    /// - Between a vehicle and a building, the nearer one wins: the vehicle's
    ///   center against the building's slot spot. A tie goes to the vehicle,
    ///   and among buildings to the lower index.
    ///
    /// When nothing free is in reach but a building is, the nearest one's
    /// refusal is an event the view explains (`Event::BundleRefused`); an
    /// interact on a building out of reach is refused as `OutOfReach`.
    /// Taking a building puts out a sprite in its slot or on its way to it
    /// (`possess`).
    pub(crate) fn board(&mut self, messages: Vec<Board>, terrain: &Terrain) {
        for Board { agent: me, building: wanted } in messages {
            let agent = me as usize;
            let Some(a) = self.agents.get(agent) else { continue };
            let Some(avatar) = a.avatar() else { continue };
            if !a.body.alive() || avatar.seat.is_some() || avatar.dig.is_some() {
                continue;
            }
            let (position, radius) = (a.body.position, a.radius());
            let vehicle = (0..self.vehicles.len())
                .filter(|_| wanted.is_none())
                .filter_map(|ix| Some((ix, self.free_seat(ix as VehicleIx, position)?)))
                .map(|(ix, seat)| {
                    let v = &self.vehicles[ix];
                    let def = &self.characters[v.character as usize];
                    match &def.crawler {
                        Some(c) => (crawler::box_distance_squared(Pose::of(v), c, position), radius + BOARD_REACH, (ix, seat)),
                        None => ((v.position - position).length_squared(), def.body.radius + radius + BOARD_REACH, (ix, seat)),
                    }
                })
                .filter(|&(d2, reach, _)| d2 <= reach * reach)
                .min_by_key(|&(d2, _, (ix, _))| (d2, ix))
                .map(|(d2, _, seat)| (d2, seat));
            // Built buildings in reach, nearest first, with why each can't
            // be boarded, if it can't.
            let mut buildings: Vec<(Fx, BuildingIx, Option<BundleRefusal>)> = (0..self.buildings.len() as BuildingIx)
                .filter(|&b| wanted.is_none_or(|w| w == b))
                .filter(|&b| self.buildings[b as usize].built() && !self.building_kind(b).is_lantern())
                .map(|b| {
                    let building = &self.buildings[b as usize];
                    let spot = building.slot_position(&self.building_kinds[building.kind as usize]);
                    ((spot - position).length_squared(), b, interact::board_refusal(&a.carry, building.slot, me))
                })
                .filter(|&(d2, _, _)| d2 <= INTERACT_REACH * INTERACT_REACH)
                .collect();
            buildings.sort_by_key(|&(d2, b, _)| (d2, b));
            let building = buildings.iter().find(|(_, _, refusal)| refusal.is_none()).map(|&(d2, b, _)| (d2, b));
            match (vehicle, building) {
                (Some((dv, (ix, seat))), Some((db, _))) if dv <= db => self.drive_off(agent, ix, seat),
                (Some((_, (ix, seat))), None) => self.drive_off(agent, ix, seat),
                (_, Some((_, b))) => self.possess(agent, b, terrain),
                (None, None) => {
                    let reason = match buildings.first() {
                        Some(&(_, _, refusal)) => refusal,
                        None if wanted.is_some() => Some(BundleRefusal::OutOfReach),
                        None => None,
                    };
                    if let Some(reason) = reason {
                        self.events.push(Event::BundleRefused { agent: me, reason });
                    }
                }
            }
        }
    }

    /// Seats `agent` in vehicle `ix`'s `seat`, stopping what it was doing.
    /// A new driver takes the controls at rest.
    fn drive_off(&mut self, agent: usize, ix: usize, seat: Seat) {
        let AgentState { body, role: Role::Avatar(avatar), .. } = &mut self.agents[agent] else { return };
        avatar.seat = Some(seat);
        sit_down(body, avatar);
        if seat.vehicle().is_some() {
            self.vehicles[ix].control = Control::default();
        }
    }

    /// Seats `agent` in building `b`'s slot (D3), stopping what it was
    /// doing. A sprite inside is put out beside the building
    /// (`buildings::eject_spot`); one on its way loses its claim where it
    /// is. Either stops and, finding its slot gone on its next step, plans
    /// again (`sprites`): another free building of its kind, or a wait.
    fn possess(&mut self, agent: usize, b: BuildingIx, terrain: &Terrain) {
        let sprite = match self.buildings[b as usize].slot {
            Slot::Claimed(s) => Some((s, false)),
            Slot::Occupied(Occupant::Sprite(s)) => Some((s, true)),
            Slot::Empty | Slot::Occupied(Occupant::Elder(_)) => None,
        };
        if let Some((s, inside)) = sprite {
            self.release_slot(s);
            let spot = inside.then(|| self.eject_spot(b, self.agents[s as usize].radius(), self.agents[agent].radius(), terrain)).flatten();
            let body = &mut self.agents[s as usize].body;
            if let Some(spot) = spot {
                body.position = spot;
            }
            body.route.clear();
            body.velocity = FxVec2::ZERO;
            body.push = FxVec2::ZERO;
        }
        self.buildings[b as usize].slot = Slot::Occupied(Occupant::Elder(agent as AgentIx));
        let AgentState { body, role: Role::Avatar(avatar), .. } = &mut self.agents[agent] else { return };
        avatar.seat = Some(Seat::Building(b));
        sit_down(body, avatar);
    }

    /// Housekeeping: drivers, riders and gunners sit in their seats, and a vehicle nobody
    /// drives lets go of its controls. An elder possessing a building
    /// stands at its slot spot, facing it; a building whose elder no longer
    /// sits there (it left, died or respawned, `avatar::act`) empties its
    /// slot, for a waiting sprite to take (A3).
    pub(crate) fn seat_housekeeping(&mut self, terrain: &Terrain) {
        let mut driven = vec![false; self.vehicles.len()];
        for a in &mut self.agents {
            let AgentState { body, role: Role::Avatar(avatar), .. } = a else { continue };
            match avatar.seat {
                Some(Seat::Vehicle(ix)) => {
                    let v = &self.vehicles[ix as usize];
                    seat(body, avatar, v, &self.characters[v.character as usize], None);
                    driven[ix as usize] = true;
                }
                Some(on_deck @ (Seat::Rider { vehicle: ix, .. } | Seat::Gunner { vehicle: ix })) => {
                    let v = &self.vehicles[ix as usize];
                    seat(body, avatar, v, &self.characters[v.character as usize], crawler::deck_spot(on_deck));
                }
                Some(Seat::Building(b)) => {
                    let building = &self.buildings[b as usize];
                    let spot = building.slot_position(&self.building_kinds[building.kind as usize]);
                    body.position = spot;
                    body.heading = bearing(spot, building.position);
                    body.velocity = FxVec2::ZERO;
                    if let Some(ground) = terrain.footprint_near(spot, avatar.height, body.base_radius) {
                        avatar.height = ground;
                        avatar.flight.support = ground;
                    }
                    avatar.flight.airborne = false;
                }
                None => {}
            }
        }
        for (v, driven) in self.vehicles.iter_mut().zip(driven) {
            if !driven {
                v.control = Control::default();
            }
        }
        // A sprite at a crawler's gun (TU.2 of the turret) stands at the
        // gunner's place, as an elder there does.
        for a in &mut self.agents {
            let AgentState { body, role: Role::Sprite(sprite), .. } = a else { continue };
            let Some(ix) = sprite.gun else { continue };
            let v = &self.vehicles[ix as usize];
            let (position, height) = v.seat_at(&self.characters[v.character as usize], Some(crawler::GUNNER));
            body.position = position;
            body.heading = v.heading;
            body.velocity = FxVec2::ZERO;
            body.push = FxVec2::ZERO;
            body.route.clear();
            sprite.height = height;
        }
        for (i, building) in self.buildings.iter_mut().enumerate() {
            let Slot::Occupied(Occupant::Elder(a)) = building.slot else { continue };
            let seated = self.agents.get(a as usize).and_then(AgentState::avatar).is_some_and(|avatar| avatar.seat == Some(Seat::Building(i as BuildingIx)));
            if !seated {
                building.slot = Slot::Empty;
            }
        }
    }
}

/// Stops what a player sitting down was doing: walking, flying, a swing
/// and what it queued.
fn sit_down(body: &mut super::Body, avatar: &mut AvatarState) {
    avatar.intent = Some(Intent::Stop);
    avatar.flight.requested = false;
    avatar.swing = None;
    avatar.queued = None;
    avatar.queued_at = None;
    body.route.clear();
    body.velocity = FxVec2::ZERO;
    body.push = FxVec2::ZERO;
}

/// Puts a driver (`spot` `None`) or a rider or gunner (at `spot` on the
/// deck) in their seat on the vehicle, facing its way.
pub(super) fn seat(body: &mut super::Body, avatar: &mut AvatarState, v: &VehicleState, def: &CharacterDef, spot: Option<Offset>) {
    let (position, height) = v.seat_at(def, spot);
    body.position = position;
    body.heading = v.heading;
    body.velocity = FxVec2::ZERO;
    avatar.height = height;
    avatar.flight.airborne = false;
    avatar.flight.support = v.height;
}

/// Where a driver climbing out stands: beside the vehicle (left, right,
/// behind, then in front), on ground it could walk to from the seat's
/// level. `None` when every side is blocked. A crawler's driver, rider or
/// gunner (at `spot` on the deck) climbs down beside its body's box
/// (`crawler::exit_spot`).
pub(super) fn exit_spot(v: &VehicleState, def: &CharacterDef, spot: Option<Offset>, radius: Fx, terrain: &Terrain) -> Option<(FxVec2, Fx)> {
    if let Some(c) = &def.crawler {
        return crawler::exit_spot(v, c, spot, radius, terrain);
    }
    let forward = heading_vector(v.heading);
    let right = FxVec2::new(forward.y, -forward.x);
    let gap = def.body.radius + radius + Fx::from_ratio(3, 20);
    [-right, right, -forward, forward].into_iter().find_map(|side| {
        let p = v.position + side * gap;
        let ground = terrain.footprint_near(p, v.height, radius)?;
        let level = (ground - v.height).abs() <= Fx::from_ratio(3, 5);
        (level && terrain.clear(p, p, radius)).then_some((p, ground))
    })
}
