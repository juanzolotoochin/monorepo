//! Haulers: the original game's worker state machine, split along the tick's
//! stages. Movement and phase changes that only touch the hauler itself run in
//! the parallel self update; decisions that touch depots, docks or stock are
//! messages, handled in the apply phase.

use content::scenario::{self, ticks_from_60hz};
use content::{millis_to_ticks, per_tick};
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::{AgentIx, AgentState, DepotIx, DepotState, ItemIx, LinkIx, LinkState};
use crate::messages::{Attach, ClaimDock, DockClaim, Equip, Load, Reserve, Retry, Unload};
use crate::messages::Messages;
use crate::nav::{self, angle_between, distance};
use crate::terrain::{Disc, Terrain};

/// Seconds-based constants of the original game.
pub(super) const RETRY_TICKS: u64 = ticks_from_60hz(16);
const APPROACH_REPLAN_TICKS: u64 = millis_to_ticks(1000);
const TRAFFIC_REPLAN_TICKS: u64 = millis_to_ticks(4000);
/// The longest an agent gives way before steering on its own.
const YIELD_TICKS: u64 = millis_to_ticks(1500);
pub(super) const ATTACH_TICKS: u64 = millis_to_ticks(1000);
/// Facing turns at up to 3 rad/s.
const TURN_PER_TICK: Fx = per_tick(Fx::from_int(3));
/// A larger change of travel direction turns in place, at up to 15 rad/s.
const TRAVEL_TURN_PER_TICK: Fx = per_tick(Fx::from_int(15));

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    #[default]
    Idle,
    Returning,
    ToPickup,
    Loading,
    ToDrop,
    Unloading,
    ToEquip,
    Attaching,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub link: LinkIx,
    pub units: u32,
    pub loaded: u32,
    pub unloaded: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trailer {
    pub position: FxVec2,
    pub heading: Fx,
    pub travelled: Fx,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentDef {
    pub depot: DepotIx,
    pub item: ItemIx,
    pub capacity: u32,
    pub speed_factor: Fx,
    pub radius: Fx,
    /// Distance from the body to the trailer.
    pub hitch: Fx,
}

/// A hauler's role: its haul links, the job it is on and the equipment it
/// can fetch. Its body and cargo are the agent's (`AgentState`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HaulerState {
    /// Haul links in the order to try them: priority, then file order.
    pub links: Vec<LinkIx>,
    pub equipment: Option<EquipmentDef>,
    pub phase: Phase,
    pub assignment: Option<Assignment>,
    pub equipped: bool,
    pub trailer: Option<Trailer>,
    pub transfer_at: u64,
    pub retries: u64,
    pub departure_dock: Option<DepotIx>,
    pub traffic_replan_at: u64,
    /// Consecutive ticks spent giving way to another agent.
    pub yielding: u64,

    pub avoidance_steers: u64,
    pub dock_wait_ticks: u64,
    pub traffic_wait_ticks: u64,
    pub loaded_travel: Fx,
    pub empty_travel: Fx,
}

impl HaulerState {
    pub(super) fn new(
        def: &scenario::Agent,
        all_links: &[scenario::HaulLink],
        item: &impl Fn(&str) -> ItemIx,
        depot: &impl Fn(&str) -> DepotIx,
    ) -> HaulerState {
        let mut links: Vec<(u32, usize)> = all_links
            .iter()
            .enumerate()
            .filter(|(_, l)| def.links.contains(&l.id))
            .map(|(i, l)| (l.priority, i))
            .collect();
        links.sort_by_key(|&(priority, i)| (std::cmp::Reverse(priority), i));
        HaulerState {
            links: links.into_iter().map(|(_, i)| i as LinkIx).collect(),
            equipment: def.equipment.as_ref().map(|e| EquipmentDef {
                depot: depot(&e.depot),
                item: item(&e.item),
                capacity: e.capacity,
                speed_factor: e.speed_factor,
                radius: e.radius,
                hitch: e.offset[2].abs().max(Fx::from_ratio(3, 5)),
            }),
            ..HaulerState::default()
        }
    }
}

impl AgentState {
    fn speed_now(&self) -> Fx {
        match &self.hauler.equipment {
            Some(e) if self.hauler.equipped => self.body.speed * e.speed_factor,
            _ => self.body.speed,
        }
    }

    /// A route to `to`, turning carefully when towing a trailer.
    pub(super) fn plan(&self, terrain: &Terrain, to: FxVec2, extra: &[Disc]) -> Vec<FxVec2> {
        let (Some(t), Some(e)) = (&self.hauler.trailer, &self.hauler.equipment) else {
            return nav::path_avoiding(terrain, self.body.position, to, self.radius(), extra);
        };
        // A trailer already inside an obstacle can't pass the swept-path check;
        // plan for the body and let the trailer be dragged clear.
        if !terrain.clear(t.position, t.position, e.radius) {
            return nav::path_avoiding(terrain, self.body.position, to, self.radius(), extra);
        }
        let route = nav::articulated_path_avoiding(terrain, self.body.position, to, self.body.travel_heading, t.position, e.hitch, self.radius(), e.radius, extra);
        if route.is_empty() {
            // No turning circle fits here (a tight corner): take the direct
            // route rather than wait forever. The trailer follows its hitch.
            return nav::path_avoiding(terrain, self.body.position, to, self.radius(), extra);
        }
        route
    }

    /// Where to put a newly attached trailer: behind the body if there is
    /// room, otherwise in the nearest clear direction. Returns the trailer
    /// position and the body's travel heading.
    pub(super) fn trailer_spot(&self, terrain: &Terrain) -> (FxVec2, Fx) {
        let e = self.hauler.equipment.as_ref().expect("only equipped agents get trailers");
        for sixths in [0, 1, -1, 2, -2, 3, -3, 4, -4, 5, -5, 6] {
            let heading = self.body.travel_heading + Fx::PI.mul_int(sixths).div_int(6);
            let spot = self.body.position - nav::heading_vector(heading) * e.hitch;
            if terrain.clear(spot, spot, e.radius) {
                return (spot, heading);
            }
        }
        (self.body.position - nav::heading_vector(self.body.travel_heading) * e.hitch, self.body.travel_heading)
    }

    fn dock(&self, links: &[LinkState]) -> Option<DepotIx> {
        match self.hauler.phase {
            Phase::ToPickup | Phase::Loading => self.hauler.assignment.as_ref().map(|j| links[j.link as usize].from),
            Phase::ToDrop | Phase::Unloading => self.hauler.assignment.as_ref().map(|j| links[j.link as usize].to),
            Phase::ToEquip | Phase::Attaching => self.hauler.equipment.as_ref().map(|e| e.depot),
            _ => None,
        }
    }

    fn departure_clearance(&self) -> Fx {
        if self.hauler.trailer.is_some() {
            Fx::from_ratio(11, 5)
        } else {
            Fx::from_ratio(9, 10)
        }
    }
}

/// Everyone's footprint as of the end of last tick, for avoidance.
#[derive(Clone, Debug)]
pub(super) struct Footprint {
    pub position: FxVec2,
    pub radius: Fx,
    pub trailer: Option<Disc>,
    /// Where it is walking to next, if it is walking.
    pub next: Option<FxVec2>,
}

pub(super) struct Ctx<'a> {
    pub tick: u64,
    pub characters: &'a [content::character::CharacterDef],
    pub terrain: &'a Terrain,
    pub piles: Option<&'a super::piles::PileLab>,
    pub footprints: &'a [Footprint],
    pub dock_owner: &'a [Option<AgentIx>],
    pub depots: &'a [DepotState],
    pub links: &'a [LinkState],
}

pub(super) fn footprint(a: &AgentState) -> Footprint {
    Footprint {
        position: a.body.position,
        radius: a.radius(),
        trailer: match (&a.hauler.trailer, &a.hauler.equipment) {
            (Some(t), Some(e)) => Some(Disc { center: t.position, radius: e.radius }),
            _ => None,
        },
        next: if a.controlled { None } else { a.body.route.first().copied() },
    }
}

/// Moves `position` along `route` by up to `distance`, consuming reached waypoints.
pub(crate) fn move_route(position: &mut FxVec2, route: &mut Vec<FxVec2>, mut distance: Fx) {
    while let Some(&goal) = route.first() {
        let remaining = nav::distance(*position, goal);
        if remaining <= distance + Fx::from_bits(42_950) {
            *position = goal;
            route.remove(0);
            distance = (distance - remaining).max(Fx::ZERO);
        } else {
            *position = *position + (goal - *position) * (distance / remaining);
            break;
        }
        if distance <= Fx::ZERO {
            break;
        }
    }
}

/// Stage 2 for one non-controlled agent.
pub(super) fn self_update(a: &mut AgentState, me: AgentIx, ctx: &Ctx) {
    if a.controlled {
        let def = super::avatar::character_of(ctx.characters, a);
        return super::avatar::self_update(a, me, ctx.tick, ctx.terrain, ctx.piles, def);
    }
    if let Some(d) = a.hauler.departure_dock {
        let dock = ctx.depots[d as usize].position;
        let clearance = a.departure_clearance();
        if distance(a.body.position, dock) >= clearance && a.hauler.trailer.as_ref().is_none_or(|t| distance(t.position, dock) >= clearance) {
            a.hauler.departure_dock = None;
        }
    }
    match a.hauler.phase {
        Phase::Idle | Phase::Returning => {
            if a.hauler.phase == Phase::Idle && distance(a.body.position, a.body.home) > Fx::from_ratio(1, 10) {
                a.body.route = a.plan(ctx.terrain, a.body.home, &[]);
                a.hauler.phase = Phase::Returning;
            }
            if a.hauler.phase == Phase::Returning {
                let speed = a.speed_now();
                travel(a, me, ctx, speed);
                if a.body.route.is_empty() {
                    a.hauler.phase = Phase::Idle;
                }
            }
        }
        Phase::ToPickup | Phase::ToDrop | Phase::ToEquip => approach(a, me, ctx),
        Phase::Loading => {
            let job = a.hauler.assignment.as_ref().expect("loading agents have a job");
            if job.loaded == job.units && a.carry.pending_in == 0 {
                let link = &ctx.links[job.link as usize];
                a.hauler.departure_dock = Some(link.from);
                a.body.route = a.plan(ctx.terrain, ctx.depots[link.to as usize].position, &[]);
                a.hauler.phase = Phase::ToDrop;
            }
        }
        Phase::Unloading => {
            let job = a.hauler.assignment.as_ref().expect("unloading agents have a job");
            if job.unloaded == job.units && a.carry.pending_out == 0 {
                a.hauler.departure_dock = Some(ctx.links[job.link as usize].to);
                a.hauler.assignment = None;
                a.hauler.phase = Phase::Idle;
            }
        }
        Phase::Attaching => {
            if a.hauler.equipped && ctx.tick >= a.hauler.transfer_at {
                let e = a.hauler.equipment.as_ref().expect("attaching agents have equipment");
                a.hauler.departure_dock = Some(e.depot);
                a.carry.capacity = e.capacity;
                a.hauler.phase = Phase::Idle;
            }
        }
    }
    if let (Some(trailer), Some(e)) = (&mut a.hauler.trailer, &a.hauler.equipment) {
        let d = distance(trailer.position, a.body.position);
        if d > e.hitch {
            let previous = trailer.position;
            let direction = (a.body.position - trailer.position) / d;
            trailer.position = a.body.position - direction * e.hitch;
            trailer.heading = Fx::atan2(direction.x, direction.y);
            trailer.travelled += distance(previous, trailer.position);
        }
    }
}

fn approach(a: &mut AgentState, me: AgentIx, ctx: &Ctx) {
    let dock = a.dock(ctx.links).expect("approaching agents have a dock");
    let goal = ctx.depots[dock as usize].position;
    let owner = ctx.dock_owner[dock as usize];
    let busy = owner.is_some_and(|o| o != me);
    // Queue far enough out for the owner to get past: wider for a big
    // animal, and clear of a cart's turning envelope.
    let queue_distance = match owner {
        Some(o) if ctx.footprints[o as usize].trailer.is_some() => Fx::from_int(3),
        Some(o) => Fx::from_ratio(3, 2) + ctx.footprints[o as usize].radius,
        None => Fx::from_ratio(3, 2),
    };
    let to_goal = distance(a.body.position, goal);
    if a.body.route.is_empty() && to_goal > Fx::from_ratio(1, 10) && ctx.tick >= a.hauler.retries {
        a.body.route = a.plan(ctx.terrain, goal, &[]);
        a.hauler.retries = ctx.tick + APPROACH_REPLAN_TICKS;
    }
    if busy && to_goal < queue_distance + Fx::from_ratio(1, 5) {
        a.hauler.dock_wait_ticks += 1;
        // A larger carrier may take the dock while this one queues close by:
        // back out of its turning envelope.
        let d = to_goal.max(Fx::from_ratio(1, 100));
        if d < queue_distance - Fx::from_ratio(1, 10) {
            let route = std::mem::take(&mut a.body.route);
            let (min, max) = (ctx.terrain.min(), ctx.terrain.max());
            let margin = Fx::from_ratio(1, 10);
            let away = goal + (a.body.position - goal) * ((queue_distance + margin) / d);
            a.body.route = vec![FxVec2::new(
                away.x.clamp(min.x + margin, max.x - margin),
                away.y.clamp(min.y + margin, max.y - margin),
            )];
            let speed = a.body.speed;
            travel(a, me, ctx, speed);
            a.body.route = route;
        }
        return;
    }
    let speed = a.speed_now();
    travel(a, me, ctx, speed);
    if a.body.route.is_empty() && !busy && owner == Some(me) && distance(a.body.position, goal) <= Fx::from_ratio(1, 10) {
        a.hauler.transfer_at = ctx.tick;
        a.hauler.phase = match a.hauler.phase {
            Phase::ToPickup => Phase::Loading,
            Phase::ToDrop => Phase::Unloading,
            _ => Phase::Attaching,
        };
    }
}

/// Moves along the route and counts the distance.
fn travel(a: &mut AgentState, me: AgentIx, ctx: &Ctx, speed: Fx) {
    let before = a.body.position;
    move_worker(a, me, ctx, speed);
    let moved = distance(before, a.body.position);
    a.body.travelled += moved;
    if a.carry.carrying() {
        a.hauler.loaded_travel += moved;
    } else {
        a.hauler.empty_travel += moved;
    }
}

/// Bounded local steering: keep the global route, steer around nearby bodies
/// and trailers, and wait if no collision-free step exists.
fn move_worker(a: &mut AgentState, me: AgentIx, ctx: &Ctx, step: Fx) {
    let tolerance = Fx::from_ratio(1, 10);
    while a.body.route.first().is_some_and(|&w| distance(a.body.position, w) <= tolerance) {
        a.body.route.remove(0);
    }
    if a.body.route.is_empty() {
        return;
    }
    let radius = a.radius();
    // Skip a waypoint once the one after it is in reach: grid waypoints in a
    // tight gap can be closer to a rock than the body fits, and circling them
    // wastes time. Carts keep theirs, which shape the turns the cart can make.
    if a.hauler.trailer.is_none() {
        while a.body.route.len() >= 2 && ctx.terrain.clear(a.body.position, a.body.route[1], radius) {
            a.body.route.remove(0);
        }
    }
    let near = Fx::from_int(3);
    let mut nearby = Vec::new();
    for (i, b) in ctx.footprints.iter().enumerate() {
        if i == me as usize {
            continue;
        }
        if distance(a.body.position, b.position) < near {
            nearby.push(Disc { center: b.position, radius: b.radius });
        }
        if let Some(t) = b.trailer {
            if distance(a.body.position, t.center) < near {
                nearby.push(t);
            }
        }
    }
    let dynamic = nearby.len();
    let gap = Fx::from_ratio(2, 25); // 0.08
    let epsilon = Fx::from_bits(42_950); // 0.00001
    // A destination hidden under another body can't be reached: hold nearby.
    if a.body.route.len() == 1 {
        let goal = a.body.route[0];
        if distance(a.body.position, goal) < Fx::from_ratio(4, 5) && nearby.iter().any(|o| distance(goal, o.center) < radius + o.radius + gap) {
            a.hauler.traffic_wait_ticks += 1;
            return;
        }
    }
    if let (Some(t), Some(e)) = (&a.hauler.trailer, &a.hauler.equipment) {
        nearby.push(Disc { center: t.position, radius: e.radius });
    }
    let mut route = a.body.route.clone();
    let mut direct = a.body.position;
    move_route(&mut direct, &mut route, step);
    let blocked_by_agent = nearby[..dynamic].iter().any(|o| {
        let before = distance(a.body.position, o.center);
        let after = distance(direct, o.center);
        after <= radius + o.radius + gap && after <= before + epsilon
    });
    if blocked_by_agent && ctx.tick >= a.hauler.traffic_replan_at {
        let goal = *a.body.route.last().expect("non-empty route");
        let detour = {
            let saved = std::mem::take(&mut a.body.route);
            let detour = a.plan(ctx.terrain, goal, &nearby[..dynamic]);
            a.body.route = saved;
            detour
        };
        if !detour.is_empty() {
            a.body.route = detour;
            route = a.body.route.clone();
            direct = a.body.position;
            move_route(&mut direct, &mut route, step);
        }
        a.hauler.traffic_replan_at = ctx.tick + TRAFFIC_REPLAN_TICKS;
    }
    let delta = direct - a.body.position;
    // Two agents meeting head-on would both step aside and back forever: the
    // later one waits while the earlier one steers around it.
    let oncoming = ctx.footprints[..me as usize].iter().any(|b| {
        let Some(next) = b.next else {
            return false;
        };
        let after = distance(direct, b.position);
        after <= radius + b.radius + gap && after <= distance(a.body.position, b.position) + epsilon && (next - b.position).dot(a.body.position - b.position) > Fx::ZERO
    });
    if oncoming && a.hauler.yielding < YIELD_TICKS {
        a.hauler.yielding += 1;
        a.hauler.traffic_wait_ticks += 1;
        return;
    }
    if !oncoming {
        a.hauler.yielding = 0;
    }
    // If going straight is blocked, keep the way it is already going before
    // trying fixed swerves: picking a new swerve each tick (each needing a turn
    // in place) makes an agent dither around an obstacle instead of passing it.
    let straight = Fx::ZERO;
    let mut angles: Vec<Fx> = [0, 55, 105, 155, -55, -105, -155].iter().map(|&h| Fx::from_ratio(h, 100)).collect();
    if delta != FxVec2::ZERO {
        let keep = angle_between(nav::bearing(FxVec2::ZERO, delta), a.body.travel_heading);
        if keep != straight && keep.abs() <= Fx::from_ratio(3, 2) {
            angles.insert(1, keep);
        }
    }
    for angle in angles {
        let (sin, cos) = angle.sin_cos();
        let next = FxVec2::new(a.body.position.x + delta.x * cos + delta.y * sin, a.body.position.y + delta.y * cos - delta.x * sin);
        let clear = ctx.terrain.clear(a.body.position, next, radius)
            && nearby.iter().all(|o| {
                let before = distance(a.body.position, o.center);
                let after = distance(next, o.center);
                // A player may walk into a hauler: let the hauler escape the overlap.
                after > radius + o.radius + gap || after > before + epsilon
            });
        if clear {
            let target = nav::bearing(a.body.position, next);
            let travel_turn = angle_between(a.body.travel_heading, target);
            let turn = angle_between(a.body.heading, target);
            a.body.heading += turn.clamp(-TURN_PER_TICK, TURN_PER_TICK);
            if travel_turn.abs() > TRAVEL_TURN_PER_TICK {
                a.body.travel_heading += travel_turn.clamp(-TRAVEL_TURN_PER_TICK, TRAVEL_TURN_PER_TICK);
                return;
            }
            a.body.travel_heading = target;
            a.body.position = next;
            if angle == straight {
                a.body.route = route;
            } else {
                a.hauler.avoidance_steers += 1;
            }
            return;
        }
    }
    a.hauler.traffic_wait_ticks += 1;
}

/// Stage 3 for one agent: dock claims and decisions that need the economy.
pub(super) fn read(a: &AgentState, me: AgentIx, tick: u64, settlement: &super::Settlement, terrain: &Terrain, out: &mut Messages) {
    if a.controlled {
        return;
    }
    if let Some(d) = a.hauler.departure_dock {
        let dock = settlement.depots[d as usize].position;
        let clearance = a.departure_clearance();
        if distance(a.body.position, dock) < clearance || a.hauler.trailer.as_ref().is_some_and(|t| distance(t.position, dock) < clearance) {
            out.claim_dock.push(ClaimDock { depot: d, agent: me, claim: DockClaim::Holding });
        }
    }
    match a.hauler.phase {
        Phase::Loading | Phase::Unloading | Phase::Attaching => {
            let dock = a.dock(&settlement.links).expect("docked agents have a dock");
            out.claim_dock.push(ClaimDock { depot: dock, agent: me, claim: DockClaim::Holding });
        }
        Phase::ToPickup | Phase::ToDrop | Phase::ToEquip => {
            let dock = a.dock(&settlement.links).expect("approaching agents have a dock");
            let distance = distance(a.body.position, settlement.depots[dock as usize].position);
            out.claim_dock.push(ClaimDock { depot: dock, agent: me, claim: DockClaim::Approaching(distance) });
        }
        _ => {}
    }
    match a.hauler.phase {
        Phase::Idle | Phase::Returning if tick >= a.hauler.retries => decide(a, me, settlement, terrain, out),
        Phase::Loading => {
            let job = a.hauler.assignment.as_ref().expect("loading agents have a job");
            if job.loaded < job.units && tick >= a.hauler.transfer_at {
                out.load.push(Load { agent: me });
            }
        }
        Phase::Unloading => {
            let job = a.hauler.assignment.as_ref().expect("unloading agents have a job");
            if job.unloaded < job.units && tick >= a.hauler.transfer_at {
                out.unload.push(Unload { agent: me });
            }
        }
        Phase::Attaching if !a.hauler.equipped => out.attach.push(Attach { agent: me }),
        _ => {}
    }
}

/// Picks the next job: equipment first, then haul links by priority.
fn decide(a: &AgentState, me: AgentIx, s: &super::Settlement, terrain: &Terrain, out: &mut Messages) {
    if !a.hauler.equipped {
        if let Some(e) = &a.hauler.equipment {
            if s.depots[e.depot as usize].available(e.item) > 0 {
                let route = a.plan(terrain, s.depots[e.depot as usize].position, &[]);
                if !route.is_empty() {
                    out.equip.push(Equip { agent: me, route });
                    return;
                }
            }
        }
    }
    for &l in &a.hauler.links {
        let link = &s.links[l as usize];
        if !link.enabled {
            continue;
        }
        let units = job_size(a, link, &s.depots);
        if units == 0 {
            continue;
        }
        let from = s.depots[link.from as usize].position;
        let to = s.depots[link.to as usize].position;
        let route = a.plan(terrain, from, &[]);
        if route.is_empty() || nav::path(terrain, from, to, a.radius()).is_empty() {
            continue;
        }
        out.reserve.push(Reserve { agent: me, link: l, units, route });
        return;
    }
    out.retry.push(Retry { agent: me });
}

/// How many units a link needs moved that this agent can take now.
pub(super) fn job_size(a: &AgentState, link: &LinkState, depots: &[DepotState]) -> u32 {
    let to = &depots[link.to as usize];
    let demand = link.target.saturating_sub(to.stock[link.item as usize] + to.incoming[link.item as usize]);
    depots[link.from as usize].available(link.item).min(to.free()).min(demand).min(a.carry.capacity)
}

