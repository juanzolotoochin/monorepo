//! Haulers: the original game's worker state machine, split along the tick's
//! stages. Movement and phase changes that only touch the hauler itself run in
//! the parallel self update; decisions that touch depots, docks or stock are
//! messages, handled in the apply phase.

use std::collections::BTreeMap;

use content::scenario::{self, ticks_from_60hz};
use content::{millis_to_ticks, per_tick};
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::action::{self, Action, View};
use super::avatar::Intent;
use super::{AgentIx, AgentState, Body, Carry, DepotIx, DepotState, Direction, ItemIx, LinkIx, LinkState, Role, Transfer};
use crate::messages::{Attach, CarryUnit, ClaimDock, DockClaim, Equip, ReserveHaul, Retry};
use crate::messages::{ActionMessages, Messages};
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

/// The haul a hauler is on: the link whose docks it uses. The units are in
/// its carry jobs: the load is `Carry::job` until it is done, and the unload
/// waits in `Carry::next` until the loaded units have landed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub link: LinkIx,
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
    /// While attaching, when the equipment is on. Loading and unloading
    /// time their units with the carry job (`CarryJob::next_tick`).
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

    pub fn equipped_item(&self) -> Option<ItemIx> {
        self.equipment.as_ref().filter(|_| self.equipped).map(|e| e.item)
    }

    /// The body radius; a hitched animal stands wider.
    pub fn radius(&self, body: &Body) -> Fx {
        if self.equipped {
            body.base_radius.max(Fx::from_ratio(7, 20))
        } else {
            body.base_radius
        }
    }

    fn speed_now(&self, body: &Body) -> Fx {
        match &self.equipment {
            Some(e) if self.equipped => body.speed * e.speed_factor,
            _ => body.speed,
        }
    }

    /// A route to `to`, turning carefully when towing a trailer.
    pub(super) fn plan(&self, body: &Body, terrain: &Terrain, to: FxVec2, extra: &[Disc]) -> Vec<FxVec2> {
        let (Some(t), Some(e)) = (&self.trailer, &self.equipment) else {
            return nav::path_avoiding(terrain, body.position, to, self.radius(body), extra);
        };
        // A trailer already inside an obstacle can't pass the swept-path check;
        // plan for the body and let the trailer be dragged clear.
        if !terrain.clear(t.position, t.position, e.radius) {
            return nav::path_avoiding(terrain, body.position, to, self.radius(body), extra);
        }
        let route = nav::articulated_path_avoiding(terrain, body.position, to, body.travel_heading, t.position, e.hitch, self.radius(body), e.radius, extra);
        if route.is_empty() {
            // No turning circle fits here (a tight corner): take the direct
            // route rather than wait forever. The trailer follows its hitch.
            return nav::path_avoiding(terrain, body.position, to, self.radius(body), extra);
        }
        route
    }

    /// Where to put a newly attached trailer: behind the body if there is
    /// room, otherwise in the nearest clear direction. Returns the trailer
    /// position and the body's travel heading.
    pub(super) fn trailer_spot(&self, body: &Body, terrain: &Terrain) -> (FxVec2, Fx) {
        let e = self.equipment.as_ref().expect("only equipped agents get trailers");
        for sixths in [0, 1, -1, 2, -2, 3, -3, 4, -4, 5, -5, 6] {
            let heading = body.travel_heading + Fx::PI.mul_int(sixths).div_int(6);
            let spot = body.position - nav::heading_vector(heading) * e.hitch;
            if terrain.clear(spot, spot, e.radius) {
                return (spot, heading);
            }
        }
        (body.position - nav::heading_vector(body.travel_heading) * e.hitch, body.travel_heading)
    }

    fn dock(&self, links: &[LinkState]) -> Option<DepotIx> {
        match self.phase {
            Phase::ToPickup | Phase::Loading => self.assignment.as_ref().map(|j| links[j.link as usize].from),
            Phase::ToDrop | Phase::Unloading => self.assignment.as_ref().map(|j| links[j.link as usize].to),
            Phase::ToEquip | Phase::Attaching => self.equipment.as_ref().map(|e| e.depot),
            _ => None,
        }
    }

    fn departure_clearance(&self) -> Fx {
        if self.trailer.is_some() {
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
    pub transfers: &'a BTreeMap<u64, Transfer>,
    /// What an agent's actions read this tick; each agent puts itself in
    /// `me`.
    pub view: View<'a>,
}

pub(super) fn footprint(a: &AgentState) -> Footprint {
    Footprint {
        position: a.body.position,
        radius: a.radius(),
        trailer: match a.hauler() {
            Some(HaulerState { trailer: Some(t), equipment: Some(e), .. }) => Some(Disc { center: t.position, radius: e.radius }),
            _ => None,
        },
        next: a.hauler().and(a.body.route.first().copied()),
    }
}

/// Applies an action to a hauler (see `action::act`): `GoTo` plans a route
/// (the trailer's path when towing), `Follow` takes one already planned.
/// Only the hauler brain and stage 5 drive haulers, with nothing else.
pub(super) fn act(body: &mut Body, hauler: &HaulerState, action: &Action, view: &View) {
    match action {
        Action::Move(Intent::GoTo { target, .. }) => body.route = hauler.plan(body, view.terrain, *target, &[]),
        Action::Follow(route) => body.route = route.clone(),
        _ => debug_assert!(false, "haulers only go to points and follow routes: {action:?}"),
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

/// Stage 2 for one agent. A hauler's brain picks what to do (`think`), the
/// action applies as any agent's does (`action::act`), and the hauler walks
/// (`walk`), as the phase it started the tick in says.
pub(super) fn self_update(a: &mut AgentState, me: AgentIx, ctx: &Ctx) {
    let def = super::avatar::character_of(ctx.characters, a);
    let start = match &mut a.role {
        Role::Hauler(hauler) => hauler.phase,
        Role::Avatar(avatar) => return super::avatar::self_update(&mut a.body, avatar, me, ctx.tick, ctx.terrain, ctx.piles, def),
    };
    if let Some(action) = think(a, me, ctx) {
        // A hauler's actions only move it: nothing for the world.
        let mut out = ActionMessages::default();
        action::act(a, &action, &View { me, ..ctx.view }, def, &mut out);
        debug_assert!(out.is_empty(), "hauler actions emit no messages");
    }
    walk(a, start, me, ctx);
}

/// The hauler brain: the phase changes that come before walking, and the
/// action to take. It writes the hauler's own state, never its route.
fn think(a: &mut AgentState, me: AgentIx, ctx: &Ctx) -> Option<Action> {
    let AgentState { body, carry, role: Role::Hauler(hauler), .. } = a else { unreachable!("haulers think") };
    if let Some(d) = hauler.departure_dock {
        let dock = ctx.depots[d as usize].position;
        let clearance = hauler.departure_clearance();
        if distance(body.position, dock) >= clearance && hauler.trailer.as_ref().is_none_or(|t| distance(t.position, dock) >= clearance) {
            hauler.departure_dock = None;
        }
    }
    let go_to = |target| Some(Action::Move(Intent::GoTo { target, run: false }));
    match hauler.phase {
        Phase::Idle if distance(body.position, body.home) > Fx::from_ratio(1, 10) => {
            hauler.phase = Phase::Returning;
            go_to(body.home)
        }
        Phase::Idle | Phase::Returning => None,
        Phase::ToPickup | Phase::ToDrop | Phase::ToEquip => {
            // An approach that lost its route plans again, now and then.
            let dock = hauler.dock(ctx.links).expect("approaching agents have a dock");
            let goal = ctx.depots[dock as usize].position;
            if body.route.is_empty() && distance(body.position, goal) > Fx::from_ratio(1, 10) && ctx.tick >= hauler.retries {
                hauler.retries = ctx.tick + APPROACH_REPLAN_TICKS;
                return go_to(goal);
            }
            None
        }
        Phase::Loading => {
            let job = hauler.assignment.as_ref().expect("loading agents have a job");
            // Loaded once the load job is done and its units have landed:
            // the unload starts, holding them in the cargo.
            if carry.job.is_none() && carry.inventory.total_in() == 0 {
                carry.start_next(ctx.tick);
                let link = &ctx.links[job.link as usize];
                hauler.departure_dock = Some(link.from);
                hauler.phase = Phase::ToDrop;
                return go_to(ctx.depots[link.to as usize].position);
            }
            None
        }
        Phase::Unloading => {
            let job = hauler.assignment.as_ref().expect("unloading agents have a job");
            if carry.job.is_none() && !super::leaving_cargo(ctx.transfers, me) {
                hauler.departure_dock = Some(ctx.links[job.link as usize].to);
                hauler.assignment = None;
                hauler.phase = Phase::Idle;
            }
            None
        }
        Phase::Attaching => {
            if hauler.equipped && ctx.tick >= hauler.transfer_at {
                let e = hauler.equipment.as_ref().expect("attaching agents have equipment");
                hauler.departure_dock = Some(e.depot);
                carry.inventory.capacity = e.capacity;
                hauler.phase = Phase::Idle;
            }
            None
        }
    }
}

/// The hauler's step, keyed on the phase the tick started in (`start`): a
/// hauler that just loaded waits a tick to set off, one that just turned
/// for home sets off at once. The trailer follows last.
fn walk(a: &mut AgentState, start: Phase, me: AgentIx, ctx: &Ctx) {
    let AgentState { body, carry, role: Role::Hauler(hauler), .. } = a else { unreachable!("haulers walk") };
    match start {
        Phase::Idle | Phase::Returning => {
            if hauler.phase == Phase::Returning {
                let speed = hauler.speed_now(body);
                travel(body, hauler, carry, me, ctx, speed);
                if body.route.is_empty() {
                    hauler.phase = Phase::Idle;
                }
            }
        }
        Phase::ToPickup | Phase::ToDrop | Phase::ToEquip => approach(body, hauler, carry, me, ctx),
        Phase::Loading | Phase::Unloading | Phase::Attaching => {}
    }
    if let (Some(trailer), Some(e)) = (&mut hauler.trailer, &hauler.equipment) {
        let d = distance(trailer.position, body.position);
        if d > e.hitch {
            let previous = trailer.position;
            let direction = (body.position - trailer.position) / d;
            trailer.position = body.position - direction * e.hitch;
            trailer.heading = Fx::atan2(direction.x, direction.y);
            trailer.travelled += distance(previous, trailer.position);
        }
    }
}

/// Walks to the dock: queues while another agent owns it, backing out of a
/// larger carrier's way, and docks on arrival. `think` replans a lost route.
fn approach(body: &mut Body, hauler: &mut HaulerState, carry: &Carry, me: AgentIx, ctx: &Ctx) {
    let dock = hauler.dock(ctx.links).expect("approaching agents have a dock");
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
    let to_goal = distance(body.position, goal);
    if busy && to_goal < queue_distance + Fx::from_ratio(1, 5) {
        hauler.dock_wait_ticks += 1;
        // A larger carrier may take the dock while this one queues close by:
        // back out of its turning envelope.
        let d = to_goal.max(Fx::from_ratio(1, 100));
        if d < queue_distance - Fx::from_ratio(1, 10) {
            let route = std::mem::take(&mut body.route);
            let (min, max) = (ctx.terrain.min(), ctx.terrain.max());
            let margin = Fx::from_ratio(1, 10);
            let away = goal + (body.position - goal) * ((queue_distance + margin) / d);
            body.route = vec![FxVec2::new(
                away.x.clamp(min.x + margin, max.x - margin),
                away.y.clamp(min.y + margin, max.y - margin),
            )];
            let speed = body.speed;
            travel(body, hauler, carry, me, ctx, speed);
            body.route = route;
        }
        return;
    }
    let speed = hauler.speed_now(body);
    travel(body, hauler, carry, me, ctx, speed);
    if body.route.is_empty() && !busy && owner == Some(me) && distance(body.position, goal) <= Fx::from_ratio(1, 10) {
        hauler.phase = match hauler.phase {
            Phase::ToPickup => Phase::Loading,
            Phase::ToDrop => Phase::Unloading,
            _ => Phase::Attaching,
        };
    }
}

/// Moves along the route and counts the distance.
fn travel(body: &mut Body, hauler: &mut HaulerState, carry: &Carry, me: AgentIx, ctx: &Ctx, speed: Fx) {
    let before = body.position;
    move_worker(body, hauler, me, ctx, speed);
    let moved = distance(before, body.position);
    body.travelled += moved;
    if carry.carrying() {
        hauler.loaded_travel += moved;
    } else {
        hauler.empty_travel += moved;
    }
}

/// Bounded local steering: keep the global route, steer around nearby bodies
/// and trailers, and wait if no collision-free step exists.
fn move_worker(body: &mut Body, hauler: &mut HaulerState, me: AgentIx, ctx: &Ctx, step: Fx) {
    let tolerance = Fx::from_ratio(1, 10);
    while body.route.first().is_some_and(|&w| distance(body.position, w) <= tolerance) {
        body.route.remove(0);
    }
    if body.route.is_empty() {
        return;
    }
    let radius = hauler.radius(body);
    // Skip a waypoint once the one after it is in reach: grid waypoints in a
    // tight gap can be closer to a rock than the body fits, and circling them
    // wastes time. Carts keep theirs, which shape the turns the cart can make.
    if hauler.trailer.is_none() {
        while body.route.len() >= 2 && ctx.terrain.clear(body.position, body.route[1], radius) {
            body.route.remove(0);
        }
    }
    let near = Fx::from_int(3);
    let mut nearby = Vec::new();
    for (i, b) in ctx.footprints.iter().enumerate() {
        if i == me as usize {
            continue;
        }
        if distance(body.position, b.position) < near {
            nearby.push(Disc { center: b.position, radius: b.radius });
        }
        if let Some(t) = b.trailer {
            if distance(body.position, t.center) < near {
                nearby.push(t);
            }
        }
    }
    let dynamic = nearby.len();
    let gap = Fx::from_ratio(2, 25); // 0.08
    let epsilon = Fx::from_bits(42_950); // 0.00001
    // A destination hidden under another body can't be reached: hold nearby.
    if body.route.len() == 1 {
        let goal = body.route[0];
        if distance(body.position, goal) < Fx::from_ratio(4, 5) && nearby.iter().any(|o| distance(goal, o.center) < radius + o.radius + gap) {
            hauler.traffic_wait_ticks += 1;
            return;
        }
    }
    if let (Some(t), Some(e)) = (&hauler.trailer, &hauler.equipment) {
        nearby.push(Disc { center: t.position, radius: e.radius });
    }
    let mut route = body.route.clone();
    let mut direct = body.position;
    move_route(&mut direct, &mut route, step);
    let blocked_by_agent = nearby[..dynamic].iter().any(|o| {
        let before = distance(body.position, o.center);
        let after = distance(direct, o.center);
        after <= radius + o.radius + gap && after <= before + epsilon
    });
    if blocked_by_agent && ctx.tick >= hauler.traffic_replan_at {
        let goal = *body.route.last().expect("non-empty route");
        let detour = {
            let saved = std::mem::take(&mut body.route);
            let detour = hauler.plan(body, ctx.terrain, goal, &nearby[..dynamic]);
            body.route = saved;
            detour
        };
        if !detour.is_empty() {
            body.route = detour;
            route = body.route.clone();
            direct = body.position;
            move_route(&mut direct, &mut route, step);
        }
        hauler.traffic_replan_at = ctx.tick + TRAFFIC_REPLAN_TICKS;
    }
    let delta = direct - body.position;
    // Two agents meeting head-on would both step aside and back forever: the
    // later one waits while the earlier one steers around it.
    let oncoming = ctx.footprints[..me as usize].iter().any(|b| {
        let Some(next) = b.next else {
            return false;
        };
        let after = distance(direct, b.position);
        after <= radius + b.radius + gap && after <= distance(body.position, b.position) + epsilon && (next - b.position).dot(body.position - b.position) > Fx::ZERO
    });
    if oncoming && hauler.yielding < YIELD_TICKS {
        hauler.yielding += 1;
        hauler.traffic_wait_ticks += 1;
        return;
    }
    if !oncoming {
        hauler.yielding = 0;
    }
    // If going straight is blocked, keep the way it is already going before
    // trying fixed swerves: picking a new swerve each tick (each needing a turn
    // in place) makes an agent dither around an obstacle instead of passing it.
    let straight = Fx::ZERO;
    let mut angles: Vec<Fx> = [0, 55, 105, 155, -55, -105, -155].iter().map(|&h| Fx::from_ratio(h, 100)).collect();
    if delta != FxVec2::ZERO {
        let keep = angle_between(nav::bearing(FxVec2::ZERO, delta), body.travel_heading);
        if keep != straight && keep.abs() <= Fx::from_ratio(3, 2) {
            angles.insert(1, keep);
        }
    }
    for angle in angles {
        let (sin, cos) = angle.sin_cos();
        let next = FxVec2::new(body.position.x + delta.x * cos + delta.y * sin, body.position.y + delta.y * cos - delta.x * sin);
        let clear = ctx.terrain.clear(body.position, next, radius)
            && nearby.iter().all(|o| {
                let before = distance(body.position, o.center);
                let after = distance(next, o.center);
                // A player may walk into a hauler: let the hauler escape the overlap.
                after > radius + o.radius + gap || after > before + epsilon
            });
        if clear {
            let target = nav::bearing(body.position, next);
            let travel_turn = angle_between(body.travel_heading, target);
            let turn = angle_between(body.heading, target);
            body.heading += turn.clamp(-TURN_PER_TICK, TURN_PER_TICK);
            if travel_turn.abs() > TRAVEL_TURN_PER_TICK {
                body.travel_heading += travel_turn.clamp(-TRAVEL_TURN_PER_TICK, TRAVEL_TURN_PER_TICK);
                return;
            }
            body.travel_heading = target;
            body.position = next;
            if angle == straight {
                body.route = route;
            } else {
                hauler.avoidance_steers += 1;
            }
            return;
        }
    }
    hauler.traffic_wait_ticks += 1;
}

/// Stage 3 for one agent: dock claims and decisions that need the economy.
pub(super) fn read(a: &AgentState, me: AgentIx, tick: u64, settlement: &super::Settlement, terrain: &Terrain, out: &mut Messages) {
    let Some(hauler) = a.hauler() else { return };
    let body = &a.body;
    if let Some(d) = hauler.departure_dock {
        let dock = settlement.depots[d as usize].position;
        let clearance = hauler.departure_clearance();
        if distance(body.position, dock) < clearance || hauler.trailer.as_ref().is_some_and(|t| distance(t.position, dock) < clearance) {
            out.claim_dock.push(ClaimDock { depot: d, agent: me, claim: DockClaim::Holding });
        }
    }
    match hauler.phase {
        Phase::Loading | Phase::Unloading | Phase::Attaching => {
            let dock = hauler.dock(&settlement.links).expect("docked agents have a dock");
            out.claim_dock.push(ClaimDock { depot: dock, agent: me, claim: DockClaim::Holding });
        }
        Phase::ToPickup | Phase::ToDrop | Phase::ToEquip => {
            let dock = hauler.dock(&settlement.links).expect("approaching agents have a dock");
            let distance = distance(body.position, settlement.depots[dock as usize].position);
            out.claim_dock.push(ClaimDock { depot: dock, agent: me, claim: DockClaim::Approaching(distance) });
        }
        _ => {}
    }
    match hauler.phase {
        Phase::Idle | Phase::Returning if tick >= hauler.retries => decide(a, hauler, me, settlement, terrain, out),
        Phase::Loading | Phase::Unloading => {
            // Docked: the job's next unit, when it is due.
            if let Some(job) = a.carry.job.as_ref().filter(|job| tick >= job.next_tick) {
                let queue = match job.direction {
                    Direction::Pickup => &mut out.hauler_load,
                    Direction::Deposit => &mut out.hauler_unload,
                };
                queue.push(CarryUnit { agent: me });
            }
        }
        Phase::Attaching if !hauler.equipped => out.attach.push(Attach { agent: me }),
        _ => {}
    }
}

/// Picks the next job: equipment first, then haul links by priority.
fn decide(a: &AgentState, hauler: &HaulerState, me: AgentIx, s: &super::Settlement, terrain: &Terrain, out: &mut Messages) {
    let body = &a.body;
    if !hauler.equipped {
        if let Some(e) = &hauler.equipment {
            if s.depots[e.depot as usize].inventory.available(e.item) > 0 {
                let route = hauler.plan(body, terrain, s.depots[e.depot as usize].position, &[]);
                if !route.is_empty() {
                    out.equip.push(Equip { agent: me, route });
                    return;
                }
            }
        }
    }
    for &l in &hauler.links {
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
        let route = hauler.plan(body, terrain, from, &[]);
        if route.is_empty() || nav::path(terrain, from, to, hauler.radius(body)).is_empty() {
            continue;
        }
        out.reserve_haul.push(ReserveHaul { agent: me, link: l, units, route });
        return;
    }
    out.retry.push(Retry { agent: me });
}

/// How many units a link needs moved that this agent can take now.
pub(super) fn job_size(a: &AgentState, link: &LinkState, depots: &[DepotState]) -> u32 {
    let to = &depots[link.to as usize];
    let demand = link.target.saturating_sub(to.inventory.count(link.item) + to.inventory.reserved_in[link.item as usize]);
    depots[link.from as usize].inventory.available(link.item).min(to.inventory.free()).min(demand).min(a.carry.inventory.capacity)
}

