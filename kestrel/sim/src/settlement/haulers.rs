//! Haulers: the original game's worker state machine, split along the tick's
//! stages. Movement and phase changes that only touch the hauler itself run in
//! the parallel self update; decisions that touch depots, docks or stock are
//! messages, handled in the apply phase.

use content::scenario::{self, ticks_from_60hz};
use content::{millis_to_ticks, per_tick};
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::{AgentIx, DepotIx, DepotState, ItemIx, LinkIx, LinkState};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentState {
    pub id: String,
    /// Player avatars: driven by commands, never by haul links.
    pub controlled: bool,
    /// The player whose commands drive this avatar.
    pub owner: Option<crate::PlayerId>,
    /// The character it is.
    pub character: content::character::CharacterIx,
    /// The name the owner goes by; empty until they give one.
    pub name: String,
    pub home: FxVec2,
    /// Meters per tick.
    pub speed: Fx,
    pub base_radius: Fx,
    /// Haul links in the order to try them: priority, then file order.
    pub links: Vec<LinkIx>,
    pub transfer_ticks: u64,
    pub equipment: Option<EquipmentDef>,

    pub position: FxVec2,
    pub heading: Fx,
    pub travel_heading: Fx,
    pub travelled: Fx,
    pub phase: Phase,
    pub route: Vec<FxVec2>,
    /// Units carried, per item.
    pub cargo: Vec<u32>,
    pub cargo_capacity: u32,
    pub assignment: Option<Assignment>,
    pub equipped: bool,
    pub trailer: Option<Trailer>,
    pub transfer_at: u64,
    pub retries: u64,
    pub departure_dock: Option<DepotIx>,
    pub traffic_replan_at: u64,
    /// Consecutive ticks spent giving way to another agent.
    pub yielding: u64,
    /// Units flying into / out of the cargo bed.
    pub pending_in: u32,
    pub pending_out: u32,

    /// Player avatars only.
    pub avatar: super::avatar::AvatarState,
    /// Weapons are ready again on this tick.
    pub attack_ready: u64,

    pub avoidance_steers: u64,
    pub dock_wait_ticks: u64,
    pub traffic_wait_ticks: u64,
    pub loaded_travel: Fx,
    pub empty_travel: Fx,
}

impl AgentState {
    pub(super) fn new(
        def: &scenario::Agent,
        character: content::character::CharacterIx,
        rules: &content::character::CharacterDef,
        position: FxVec2,
        all_links: &[scenario::HaulLink],
        items: usize,
        item: &impl Fn(&str) -> ItemIx,
        depot: &impl Fn(&str) -> DepotIx,
    ) -> AgentState {
        let mut links: Vec<(u32, usize)> = all_links
            .iter()
            .enumerate()
            .filter(|(_, l)| def.links.contains(&l.id))
            .map(|(i, l)| (l.priority, i))
            .collect();
        links.sort_by_key(|&(priority, i)| (std::cmp::Reverse(priority), i));
        AgentState {
            id: def.id.clone(),
            controlled: def.controlled,
            // A scenario's avatar belongs to the host.
            owner: def.controlled.then_some(crate::PlayerId(0)),
            character,
            name: String::new(),
            home: position,
            // Copied from the character, like a wave's tuning.
            speed: rules.movement.walk.div_int(content::TICKS_PER_SECOND as i64),
            base_radius: rules.body.radius,
            links: links.into_iter().map(|(_, i)| i as LinkIx).collect(),
            transfer_ticks: rules.carry.as_ref().expect("players and haulers carry").transfer,
            equipment: def.equipment.as_ref().map(|e| EquipmentDef {
                depot: depot(&e.depot),
                item: item(&e.item),
                capacity: e.capacity,
                speed_factor: e.speed_factor,
                radius: e.radius,
                hitch: e.offset[2].abs().max(Fx::from_ratio(3, 5)),
            }),
            position,
            heading: Fx::ZERO,
            travel_heading: Fx::ZERO,
            travelled: Fx::ZERO,
            phase: Phase::Idle,
            route: Vec::new(),
            cargo: vec![0; items],
            cargo_capacity: def.capacity,
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
            avatar: Default::default(),
            attack_ready: 0,
            avoidance_steers: 0,
            dock_wait_ticks: 0,
            traffic_wait_ticks: 0,
            loaded_travel: Fx::ZERO,
            empty_travel: Fx::ZERO,
        }
    }

    /// The body radius; a hitched animal stands wider.
    pub fn radius(&self) -> Fx {
        if self.equipped {
            self.base_radius.max(Fx::from_ratio(7, 20))
        } else {
            self.base_radius
        }
    }

    pub fn equipped_item(&self) -> Option<ItemIx> {
        self.equipment.as_ref().filter(|_| self.equipped).map(|e| e.item)
    }

    fn speed_now(&self) -> Fx {
        match &self.equipment {
            Some(e) if self.equipped => self.speed * e.speed_factor,
            _ => self.speed,
        }
    }

    fn carrying(&self) -> bool {
        self.cargo.iter().any(|&u| u > 0)
    }

    /// A route to `to`, turning carefully when towing a trailer.
    pub(super) fn plan(&self, terrain: &Terrain, to: FxVec2, extra: &[Disc]) -> Vec<FxVec2> {
        let (Some(t), Some(e)) = (&self.trailer, &self.equipment) else {
            return nav::path_avoiding(terrain, self.position, to, self.radius(), extra);
        };
        // A trailer already inside an obstacle can't pass the swept-path check;
        // plan for the body and let the trailer be dragged clear.
        if !terrain.clear(t.position, t.position, e.radius) {
            return nav::path_avoiding(terrain, self.position, to, self.radius(), extra);
        }
        let route = nav::articulated_path_avoiding(terrain, self.position, to, self.travel_heading, t.position, e.hitch, self.radius(), e.radius, extra);
        if route.is_empty() {
            // No turning circle fits here (a tight corner): take the direct
            // route rather than wait forever. The trailer follows its hitch.
            return nav::path_avoiding(terrain, self.position, to, self.radius(), extra);
        }
        route
    }

    /// Where to put a newly attached trailer: behind the body if there is
    /// room, otherwise in the nearest clear direction. Returns the trailer
    /// position and the body's travel heading.
    pub(super) fn trailer_spot(&self, terrain: &Terrain) -> (FxVec2, Fx) {
        let e = self.equipment.as_ref().expect("only equipped agents get trailers");
        for sixths in [0, 1, -1, 2, -2, 3, -3, 4, -4, 5, -5, 6] {
            let heading = self.travel_heading + Fx::PI.mul_int(sixths).div_int(6);
            let spot = self.position - nav::heading_vector(heading) * e.hitch;
            if terrain.clear(spot, spot, e.radius) {
                return (spot, heading);
            }
        }
        (self.position - nav::heading_vector(self.travel_heading) * e.hitch, self.travel_heading)
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

/// Everyone's body as of the end of last tick, for avoidance.
#[derive(Clone, Debug)]
pub(super) struct Body {
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
    pub bodies: &'a [Body],
    pub dock_owner: &'a [Option<AgentIx>],
    pub depots: &'a [DepotState],
    pub links: &'a [LinkState],
}

pub(super) fn body(a: &AgentState) -> Body {
    Body {
        position: a.position,
        radius: a.radius(),
        trailer: match (&a.trailer, &a.equipment) {
            (Some(t), Some(e)) => Some(Disc { center: t.position, radius: e.radius }),
            _ => None,
        },
        next: if a.controlled { None } else { a.route.first().copied() },
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
    if let Some(d) = a.departure_dock {
        let dock = ctx.depots[d as usize].position;
        let clearance = a.departure_clearance();
        if distance(a.position, dock) >= clearance && a.trailer.as_ref().is_none_or(|t| distance(t.position, dock) >= clearance) {
            a.departure_dock = None;
        }
    }
    match a.phase {
        Phase::Idle | Phase::Returning => {
            if a.phase == Phase::Idle && distance(a.position, a.home) > Fx::from_ratio(1, 10) {
                a.route = a.plan(ctx.terrain, a.home, &[]);
                a.phase = Phase::Returning;
            }
            if a.phase == Phase::Returning {
                let speed = a.speed_now();
                travel(a, me, ctx, speed);
                if a.route.is_empty() {
                    a.phase = Phase::Idle;
                }
            }
        }
        Phase::ToPickup | Phase::ToDrop | Phase::ToEquip => approach(a, me, ctx),
        Phase::Loading => {
            let job = a.assignment.as_ref().expect("loading agents have a job");
            if job.loaded == job.units && a.pending_in == 0 {
                let link = &ctx.links[job.link as usize];
                a.departure_dock = Some(link.from);
                a.route = a.plan(ctx.terrain, ctx.depots[link.to as usize].position, &[]);
                a.phase = Phase::ToDrop;
            }
        }
        Phase::Unloading => {
            let job = a.assignment.as_ref().expect("unloading agents have a job");
            if job.unloaded == job.units && a.pending_out == 0 {
                a.departure_dock = Some(ctx.links[job.link as usize].to);
                a.assignment = None;
                a.phase = Phase::Idle;
            }
        }
        Phase::Attaching => {
            if a.equipped && ctx.tick >= a.transfer_at {
                let e = a.equipment.as_ref().expect("attaching agents have equipment");
                a.departure_dock = Some(e.depot);
                a.cargo_capacity = e.capacity;
                a.phase = Phase::Idle;
            }
        }
    }
    if let (Some(trailer), Some(e)) = (&mut a.trailer, &a.equipment) {
        let d = distance(trailer.position, a.position);
        if d > e.hitch {
            let previous = trailer.position;
            let direction = (a.position - trailer.position) / d;
            trailer.position = a.position - direction * e.hitch;
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
        Some(o) if ctx.bodies[o as usize].trailer.is_some() => Fx::from_int(3),
        Some(o) => Fx::from_ratio(3, 2) + ctx.bodies[o as usize].radius,
        None => Fx::from_ratio(3, 2),
    };
    let to_goal = distance(a.position, goal);
    if a.route.is_empty() && to_goal > Fx::from_ratio(1, 10) && ctx.tick >= a.retries {
        a.route = a.plan(ctx.terrain, goal, &[]);
        a.retries = ctx.tick + APPROACH_REPLAN_TICKS;
    }
    if busy && to_goal < queue_distance + Fx::from_ratio(1, 5) {
        a.dock_wait_ticks += 1;
        // A larger carrier may take the dock while this one queues close by:
        // back out of its turning envelope.
        let d = to_goal.max(Fx::from_ratio(1, 100));
        if d < queue_distance - Fx::from_ratio(1, 10) {
            let route = std::mem::take(&mut a.route);
            let (min, max) = (ctx.terrain.min(), ctx.terrain.max());
            let margin = Fx::from_ratio(1, 10);
            let away = goal + (a.position - goal) * ((queue_distance + margin) / d);
            a.route = vec![FxVec2::new(
                away.x.clamp(min.x + margin, max.x - margin),
                away.y.clamp(min.y + margin, max.y - margin),
            )];
            let speed = a.speed;
            travel(a, me, ctx, speed);
            a.route = route;
        }
        return;
    }
    let speed = a.speed_now();
    travel(a, me, ctx, speed);
    if a.route.is_empty() && !busy && owner == Some(me) && distance(a.position, goal) <= Fx::from_ratio(1, 10) {
        a.transfer_at = ctx.tick;
        a.phase = match a.phase {
            Phase::ToPickup => Phase::Loading,
            Phase::ToDrop => Phase::Unloading,
            _ => Phase::Attaching,
        };
    }
}

/// Moves along the route and counts the distance.
fn travel(a: &mut AgentState, me: AgentIx, ctx: &Ctx, speed: Fx) {
    let before = a.position;
    move_worker(a, me, ctx, speed);
    let moved = distance(before, a.position);
    a.travelled += moved;
    if a.carrying() {
        a.loaded_travel += moved;
    } else {
        a.empty_travel += moved;
    }
}

/// Bounded local steering: keep the global route, steer around nearby bodies
/// and trailers, and wait if no collision-free step exists.
fn move_worker(a: &mut AgentState, me: AgentIx, ctx: &Ctx, step: Fx) {
    let tolerance = Fx::from_ratio(1, 10);
    while a.route.first().is_some_and(|&w| distance(a.position, w) <= tolerance) {
        a.route.remove(0);
    }
    if a.route.is_empty() {
        return;
    }
    let radius = a.radius();
    // Skip a waypoint once the one after it is in reach: grid waypoints in a
    // tight gap can be closer to a rock than the body fits, and circling them
    // wastes time. Carts keep theirs, which shape the turns the cart can make.
    if a.trailer.is_none() {
        while a.route.len() >= 2 && ctx.terrain.clear(a.position, a.route[1], radius) {
            a.route.remove(0);
        }
    }
    let near = Fx::from_int(3);
    let mut nearby = Vec::new();
    for (i, b) in ctx.bodies.iter().enumerate() {
        if i == me as usize {
            continue;
        }
        if distance(a.position, b.position) < near {
            nearby.push(Disc { center: b.position, radius: b.radius });
        }
        if let Some(t) = b.trailer {
            if distance(a.position, t.center) < near {
                nearby.push(t);
            }
        }
    }
    let dynamic = nearby.len();
    let gap = Fx::from_ratio(2, 25); // 0.08
    let epsilon = Fx::from_bits(42_950); // 0.00001
    // A destination hidden under another body can't be reached: hold nearby.
    if a.route.len() == 1 {
        let goal = a.route[0];
        if distance(a.position, goal) < Fx::from_ratio(4, 5) && nearby.iter().any(|o| distance(goal, o.center) < radius + o.radius + gap) {
            a.traffic_wait_ticks += 1;
            return;
        }
    }
    if let (Some(t), Some(e)) = (&a.trailer, &a.equipment) {
        nearby.push(Disc { center: t.position, radius: e.radius });
    }
    let mut route = a.route.clone();
    let mut direct = a.position;
    move_route(&mut direct, &mut route, step);
    let blocked_by_agent = nearby[..dynamic].iter().any(|o| {
        let before = distance(a.position, o.center);
        let after = distance(direct, o.center);
        after <= radius + o.radius + gap && after <= before + epsilon
    });
    if blocked_by_agent && ctx.tick >= a.traffic_replan_at {
        let goal = *a.route.last().expect("non-empty route");
        let detour = {
            let saved = std::mem::take(&mut a.route);
            let detour = a.plan(ctx.terrain, goal, &nearby[..dynamic]);
            a.route = saved;
            detour
        };
        if !detour.is_empty() {
            a.route = detour;
            route = a.route.clone();
            direct = a.position;
            move_route(&mut direct, &mut route, step);
        }
        a.traffic_replan_at = ctx.tick + TRAFFIC_REPLAN_TICKS;
    }
    let delta = direct - a.position;
    // Two agents meeting head-on would both step aside and back forever: the
    // later one waits while the earlier one steers around it.
    let oncoming = ctx.bodies[..me as usize].iter().any(|b| {
        let Some(next) = b.next else {
            return false;
        };
        let after = distance(direct, b.position);
        after <= radius + b.radius + gap && after <= distance(a.position, b.position) + epsilon && (next - b.position).dot(a.position - b.position) > Fx::ZERO
    });
    if oncoming && a.yielding < YIELD_TICKS {
        a.yielding += 1;
        a.traffic_wait_ticks += 1;
        return;
    }
    if !oncoming {
        a.yielding = 0;
    }
    // If going straight is blocked, keep the way it is already going before
    // trying fixed swerves: picking a new swerve each tick (each needing a turn
    // in place) makes an agent dither around an obstacle instead of passing it.
    let straight = Fx::ZERO;
    let mut angles: Vec<Fx> = [0, 55, 105, 155, -55, -105, -155].iter().map(|&h| Fx::from_ratio(h, 100)).collect();
    if delta != FxVec2::ZERO {
        let keep = angle_between(nav::bearing(FxVec2::ZERO, delta), a.travel_heading);
        if keep != straight && keep.abs() <= Fx::from_ratio(3, 2) {
            angles.insert(1, keep);
        }
    }
    for angle in angles {
        let (sin, cos) = angle.sin_cos();
        let next = FxVec2::new(a.position.x + delta.x * cos + delta.y * sin, a.position.y + delta.y * cos - delta.x * sin);
        let clear = ctx.terrain.clear(a.position, next, radius)
            && nearby.iter().all(|o| {
                let before = distance(a.position, o.center);
                let after = distance(next, o.center);
                // A player may walk into a hauler: let the hauler escape the overlap.
                after > radius + o.radius + gap || after > before + epsilon
            });
        if clear {
            let target = nav::bearing(a.position, next);
            let travel_turn = angle_between(a.travel_heading, target);
            let turn = angle_between(a.heading, target);
            a.heading += turn.clamp(-TURN_PER_TICK, TURN_PER_TICK);
            if travel_turn.abs() > TRAVEL_TURN_PER_TICK {
                a.travel_heading += travel_turn.clamp(-TRAVEL_TURN_PER_TICK, TRAVEL_TURN_PER_TICK);
                return;
            }
            a.travel_heading = target;
            a.position = next;
            if angle == straight {
                a.route = route;
            } else {
                a.avoidance_steers += 1;
            }
            return;
        }
    }
    a.traffic_wait_ticks += 1;
}

/// Stage 3 for one agent: dock claims and decisions that need the economy.
pub(super) fn read(a: &AgentState, me: AgentIx, tick: u64, settlement: &super::Settlement, terrain: &Terrain, out: &mut Messages) {
    if a.controlled {
        return;
    }
    if let Some(d) = a.departure_dock {
        let dock = settlement.depots[d as usize].position;
        let clearance = a.departure_clearance();
        if distance(a.position, dock) < clearance || a.trailer.as_ref().is_some_and(|t| distance(t.position, dock) < clearance) {
            out.claim_dock.push(ClaimDock { depot: d, agent: me, claim: DockClaim::Holding });
        }
    }
    match a.phase {
        Phase::Loading | Phase::Unloading | Phase::Attaching => {
            let dock = a.dock(&settlement.links).expect("docked agents have a dock");
            out.claim_dock.push(ClaimDock { depot: dock, agent: me, claim: DockClaim::Holding });
        }
        Phase::ToPickup | Phase::ToDrop | Phase::ToEquip => {
            let dock = a.dock(&settlement.links).expect("approaching agents have a dock");
            let distance = distance(a.position, settlement.depots[dock as usize].position);
            out.claim_dock.push(ClaimDock { depot: dock, agent: me, claim: DockClaim::Approaching(distance) });
        }
        _ => {}
    }
    match a.phase {
        Phase::Idle | Phase::Returning if tick >= a.retries => decide(a, me, settlement, terrain, out),
        Phase::Loading => {
            let job = a.assignment.as_ref().expect("loading agents have a job");
            if job.loaded < job.units && tick >= a.transfer_at {
                out.load.push(Load { agent: me });
            }
        }
        Phase::Unloading => {
            let job = a.assignment.as_ref().expect("unloading agents have a job");
            if job.unloaded < job.units && tick >= a.transfer_at {
                out.unload.push(Unload { agent: me });
            }
        }
        Phase::Attaching if !a.equipped => out.attach.push(Attach { agent: me }),
        _ => {}
    }
}

/// Picks the next job: equipment first, then haul links by priority.
fn decide(a: &AgentState, me: AgentIx, s: &super::Settlement, terrain: &Terrain, out: &mut Messages) {
    if !a.equipped {
        if let Some(e) = &a.equipment {
            if s.depots[e.depot as usize].available(e.item) > 0 {
                let route = a.plan(terrain, s.depots[e.depot as usize].position, &[]);
                if !route.is_empty() {
                    out.equip.push(Equip { agent: me, route });
                    return;
                }
            }
        }
    }
    for &l in &a.links {
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
    depots[link.from as usize].available(link.item).min(to.free()).min(demand).min(a.cargo_capacity)
}

