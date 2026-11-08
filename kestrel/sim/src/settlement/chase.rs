//! Enemies chase, then return (`SACRED_TREE_BALLISTA.md`, B5). A threat
//! whose character has a `chase` turns on the nearest elder or sprite it can
//! see within its detect radius, walks straight at it and swings at it with
//! its usual attack. It gives up when its quarry dies, shelters in a
//! building, gets past the leash or out of sight, and plans a fresh route
//! to its wave's target from where it stands.
//!
//! Buildings come second (B8): with no elder or sprite about, it attacks a
//! building in its reach or in the way of its route (`spot_building`),
//! walking to its edge and swinging at it, until it breaks back into a bare
//! site (`Settlement::break_building`) or an elder or sprite comes into
//! sight.
//!
//! The shape is the sprites' (D2): a goal (`Goal`), a plan for it (the
//! route to the tree, or a straight line at the quarry), something that
//! invalidates it, and a replan (`route_home`). A real planner later
//! replaces the deciding and the planning, nothing else.
//!
//! The work splits along the tick's stages:
//! - stage 2: the threat walks its plan (`combat::follow_route`). A chaser
//!   steers at where its quarry stood at the end of last tick: threats move
//!   before agents do, so that is all they read of others.
//! - stage 3: a threat on its way to the tree looks for a quarry once every
//!   `look` ticks; a chaser checks its quarry still holds every tick
//!   (`Settlement::read_chase`). Either sends a `ThreatGoal`.
//! - stage 5: the goal changes; giving up plans the route
//!   (`Settlement::threat_goals`).
//!
//! Characters without a `chase` never send a message, so other scenes'
//! enemies behave as they did.
//!
//! **Floors (M8.2):** in a volume scene a threat has a feet height
//! (`Threat::height`) and walks on foot (`Terrain::walk_at`). It spots a
//! quarry it sees in 3D (`sees_at`: a line from eye to eye through no
//! rock) within its detect radius, and keeps it within the leash, both in
//! 3D; it needn't see it to keep it, since it doesn't walk straight at it.
//! It walks straight at its quarry while both are on one floor and the walk
//! is clear, as elsewhere; otherwise it follows a 3D path
//! (`nav::path_toward_at`, `Threat::chase`), planned again every
//! `follow::REPLAN_TICKS`. A quarry it can't reach (up a cliff, on a floor
//! over it) it walks as close to as it gets and waits there, planning
//! again after `Threat::backoff`'s wait, which doubles while it can't. Its
//! route home is a 3D path too (`route_home_at`).
//!
//! **Routes and the ground (M6.7):** a route is planned for the ground as
//! it was. A blueprint going down replans every route (`replan_routes`); a
//! dig replans the routes that pass near it (`reroute_round`). With no
//! walkable way to the tree (a moat round a ramp's foot), a threat walks
//! to the reachable point nearest it and waits there (`Threat::stranded`),
//! chasing whoever comes in sight, and tries again every few seconds
//! (`retry_stranded`).

use content::character::ChaseDef;
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::{AgentIx, AgentState, BuildingIx, Settlement, Stage, Threat};
use crate::messages::{Messages, ThreatGoal};
use crate::nav::{self, distance};
use crate::terrain::{segment_disc, Terrain};

/// The clearance a threat walks with, as its wave's route was planned.
pub(super) const WALK_CLEARANCE: Fx = Fx::from_ratio(1, 5);
/// A thin line of sight, as a sword's (`strike_targets`).
const SIGHT: Fx = Fx::from_ratio(1, 50);

/// Whom a threat chases: an elder or a sprite, or, with neither about, a
/// building that blocks its way or stands within its reach (B5, B8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quarry {
    Agent(AgentIx),
    Building(BuildingIx),
}

/// What a threat is after.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Goal {
    /// Its wave's target: it walks `Threat::route` and breaches at its end.
    #[default]
    Tree,
    /// A quarry it saw: it walks straight at it and swings when in reach.
    /// Its route waits, and is planned afresh when it gives up.
    Chase(Quarry),
}

impl Goal {
    /// The agent it chases, if it chases one.
    pub fn agent(self) -> Option<AgentIx> {
        match self {
            Goal::Chase(Quarry::Agent(a)) => Some(a),
            Goal::Chase(Quarry::Building(_)) | Goal::Tree => None,
        }
    }

    /// The building it chases, if it chases one.
    pub fn building(self) -> Option<BuildingIx> {
        match self {
            Goal::Chase(Quarry::Building(b)) => Some(b),
            Goal::Chase(Quarry::Agent(_)) | Goal::Tree => None,
        }
    }
}

/// How close a chaser walks to its quarry: inside its attack's reach, so
/// the swing starts.
pub(super) fn stop_distance(reach: Fx) -> Fx {
    reach * Fx::from_ratio(4, 5)
}

/// Where a chaser at `from` stops on its way to `to`.
fn approach(from: FxVec2, to: FxVec2, stop: Fx) -> FxVec2 {
    let gap = distance(from, to);
    if gap <= stop {
        return from;
    }
    to - (to - from).normalize_or_zero() * stop
}

/// Whether a threat at `from` sees `to` and can walk straight to striking
/// distance of it: a thin line of sight all the way, and a clear walk to
/// where it would stop. A quarry hugging a building is still seen. In
/// sloped scenes the walk keeps to walkable ground, so a quarry up a cliff
/// (the Sacred Tree's hill) is out of reach, and the threat walks on.
pub(super) fn in_sight(terrain: &Terrain, from: FxVec2, to: FxVec2, stop: Fx) -> bool {
    terrain.clear(from, to, SIGHT) && walk_clear(terrain, from, approach(from, to, stop))
}

/// How high over a body's feet its eyes are, for a threat's sight in a
/// volume (`sees_at`).
const EYE: Fx = Fx::from_ratio(3, 2);

/// Whether a threat whose feet are at `from_z` over `from` sees a body
/// whose feet are at `to_z` over `to`, in a volume scene: a thin line of
/// sight past the obstacles, and a line from eye to eye that goes through
/// no solid cell, sampled every 0.25 m. Elsewhere [`Terrain::clear`].
pub fn sees_at(terrain: &Terrain, from: FxVec2, from_z: Fx, to: FxVec2, to_z: Fx) -> bool {
    if !terrain.clear(from, to, SIGHT) {
        return false;
    }
    let Some(volume) = &terrain.volume else { return true };
    let (a, b) = (from_z + EYE, to_z + EYE);
    let steps = (distance_at(from, a, to, b) / Fx::from_ratio(1, 4)).ceil().floor_to_int().max(1);
    (1..steps).all(|i| {
        let f = Fx::from_ratio(i, steps);
        let p = from + (to - from) * f;
        !volume.is_solid(Terrain::pit_tile(p), crate::volume::layer_at(a + (b - a) * f))
    })
}

/// The distance in 3D from `a`, at height `az`, to `b`, at height `bz`.
pub fn distance_at(a: FxVec2, az: Fx, b: FxVec2, bz: Fx) -> Fx {
    let (flat, up) = (distance(a, b), bz - az);
    (flat * flat + up * up).sqrt()
}

/// Walks `position`, its feet at `height`, along `route` by up to `step`
/// meters on foot in a volume scene, waypoint by waypoint, each step a
/// straight walk (`Terrain::walk_at`) its feet follow, consuming the
/// waypoints it reaches. Returns false if the ground refused a step: the
/// route went stale (a dig), and it stopped short.
pub(super) fn walk_route_at(position: &mut FxVec2, height: &mut Fx, route: &mut Vec<FxVec2>, step: Fx, terrain: &Terrain) -> bool {
    let mut left = step;
    while left > Fx::ZERO {
        let Some(&target) = route.first() else { return true };
        let gap = distance(*position, target);
        let next = if gap <= left { target } else { *position + (target - *position) * (left / gap) };
        let Some(feet) = terrain.walk_at(*position, next, *height, WALK_CLEARANCE) else {
            // A step can clip a wall's corner the path's 0.1 m samples
            // skipped: slide along it (its x or y alone), as the walker
            // does (`follow`), and go on next tick.
            let d = next - *position;
            let slid = [FxVec2::new(d.x, Fx::ZERO), FxVec2::new(Fx::ZERO, d.y)]
                .into_iter()
                .filter(|&s| s != FxVec2::ZERO)
                .find_map(|s| terrain.walk_at(*position, *position + s, *height, WALK_CLEARANCE).map(|z| (*position + s, z)));
            let Some((p, z)) = slid else { return false };
            *position = p;
            *height = z;
            return true;
        };
        *position = next;
        *height = feet;
        if next == target {
            route.remove(0);
        }
        left -= gap.min(left);
    }
    true
}

/// Stage 2 for a chaser in a volume scene (M8.2): a step of at most `step`
/// meters toward its quarry at `to`, feet at `feet`, stopping `stop` short
/// of it on its floor. Straight at it while both are on one floor
/// (`combat::FLOOR_REACH`) and the walk there is clear, as `steer` does;
/// else along its 3D path (`Threat::chase`), planned again when its
/// backoff allows: every `follow::REPLAN_TICKS`, or, after a plan that
/// didn't reach, longer (meanwhile it waits at the path's end, as close as
/// it gets).
pub(super) fn chase_at(t: &mut Threat, to: FxVec2, feet: Fx, stop: Fx, step: Fx, tick: u64, terrain: &Terrain) {
    let same = (feet - t.height).abs() <= super::combat::FLOOR_REACH;
    let gap = distance(t.position, to);
    if same && gap <= stop {
        t.chase.clear();
        return;
    }
    let straight = || terrain.clear(t.position, to, WALK_CLEARANCE) && terrain.walk_at(t.position, approach(t.position, to, stop), t.height, WALK_CLEARANCE).is_some();
    if same && straight() {
        let next = t.position + (to - t.position) * (step.min(gap - stop) / gap);
        if let Some(z) = terrain.walk_at(t.position, next, t.height, WALK_CLEARANCE) {
            t.chase.clear();
            t.position = next;
            t.height = z;
            return;
        }
        // The step clips a corner the straight line's samples skipped:
        // take the path.
    }
    if t.backoff.due(tick) {
        let plan = nav::path_toward_at(terrain, t.position, t.height, to, Some(feet), WALK_CLEARANCE, &[]);
        t.backoff.planned(tick, plan.reached, super::follow::REPLAN_TICKS);
        t.chase = plan.route.into_iter().map(|(p, _)| p).collect();
    }
    if !walk_route_at(&mut t.position, &mut t.height, &mut t.chase, step, terrain) {
        // Stale: plan again next tick, unless a failed plan's wait runs.
        t.chase.clear();
        if t.backoff.failures == 0 {
            t.backoff.at = t.backoff.at.min(tick + 1);
        }
    }
}

/// [`route_home`] in a volume scene (M8.2): a 3D path from `from`, feet at
/// `z`, to `target` on its top floor (`nav::path_toward_at`), or as close
/// as it gets, and whether it gets there.
pub(super) fn route_home_at(terrain: &Terrain, from: FxVec2, z: Fx, target: FxVec2) -> (Vec<FxVec2>, bool) {
    let plan = nav::path_toward_at(terrain, from, z, target, Some(terrain.height(target)), WALK_CLEARANCE, &[]);
    (plan.route.into_iter().map(|(p, _)| p).collect(), plan.reached)
}

/// Where a threat set down at `position` puts its feet: in a volume scene
/// the top floor there, the surface its wave walks in on (M8.2); zero
/// elsewhere.
pub(super) fn surface_feet(terrain: &Terrain, position: FxVec2) -> Fx {
    if terrain.volume.is_none() {
        return Fx::ZERO;
    }
    let top = terrain.height(position);
    terrain.footprint_near(position, top, WALK_CLEARANCE).unwrap_or(top)
}

/// Whether a threat can walk straight from `a` to `b`: clear of the
/// terrain and, in sloped scenes, on walkable ground.
pub(super) fn walk_clear(terrain: &Terrain, a: FxVec2, b: FxVec2) -> bool {
    terrain.clear(a, b, WALK_CLEARANCE) && (!terrain.sloped || terrain.walkable(a, b, WALK_CLEARANCE))
}

/// Stage 2 for a chaser: a step of at most `step` meters straight at
/// `target`, stopping `stop` short of it, if the ground there is clear.
pub(super) fn steer(position: &mut FxVec2, target: FxVec2, stop: Fx, step: Fx, terrain: &Terrain) {
    let gap = distance(*position, target);
    if gap <= stop {
        return;
    }
    let next = *position + (target - *position) * (step.min(gap - stop) / gap);
    if walk_clear(terrain, *position, next) {
        *position = next;
    }
}

/// The plan for `Goal::Tree`: a route from `from` to `target`, as a wave
/// plans one (`combat_housekeeping`), and whether it gets there. With no
/// walkable way to it, the route ends at the reachable point nearest it
/// (`nav::path_toward`), where the threat waits (`Threat::stranded`).
pub(super) fn route_home(terrain: &Terrain, from: FxVec2, target: FxVec2) -> (Vec<FxVec2>, bool) {
    nav::path_toward(terrain, from, target, WALK_CLEARANCE)
}

/// How near a dug tile's middle a route must pass for the dig to change
/// it: a dig reshapes the corners of the 3 x 3 tiles round it (M1), and a
/// walker's footprint reaches a little past the tile it stands on.
const DUG_REACH: Fx = Fx::from_int(2);

/// How often a stranded threat plans again, ground changed or not: every
/// 5 s, spread over threats by id (`Settlement::retry_stranded`).
const STRANDED_RETRY: u64 = 5 * content::TICKS_PER_SECOND as u64;

/// Whether a walk from `from` along `route` passes within `DUG_REACH` of
/// any of the `dug` tiles.
fn crosses(from: FxVec2, route: &[FxVec2], dug: &[(i32, i32)]) -> bool {
    let mut a = from;
    route.iter().any(|&b| {
        let near = dug.iter().any(|&tile| {
            let center = Terrain::pit_center(tile);
            distance(a, center) <= DUG_REACH || segment_disc(a, b, center, DUG_REACH).is_some()
        });
        a = b;
        near
    })
}

impl Settlement {
    /// Whether agent `a` can be chased: a live player not seated (an elder
    /// on foot), or a sprite outside a building (B6: a building shelters
    /// its occupant). A dead one is no quarry (`Body::alive`). Where player
    /// combat is off (no `player_health`) a player counts as alive and its
    /// chaser's swings do no harm.
    fn chaseable(&self, a: &AgentState) -> bool {
        if let Some(avatar) = a.avatar() {
            return a.body.alive() && avatar.seat.is_none();
        }
        a.sprite().is_some_and(|sprite| a.body.alive() && !sprite.inside())
    }

    /// Whether threat `t` still chases agent `agent`: chaseable, within the
    /// leash and in sight.
    fn keeps(&self, t: &Threat, agent: AgentIx, def: &ChaseDef, stop: Fx, terrain: &Terrain) -> bool {
        let Some(a) = self.agents.get(agent as usize) else { return false };
        if terrain.volume.is_some() {
            // It follows a path, so it needn't see it (M8.2).
            return self.chaseable(a) && distance_at(t.position, t.height, a.body.position, a.height()) <= def.leash;
        }
        self.chaseable(a) && distance(t.position, a.body.position) <= def.leash && in_sight(terrain, t.position, a.body.position, stop)
    }

    /// The quarry threat `t` turns on, if any: the nearest chaseable agent
    /// within the detect radius that it sees, the lowest index on a tie.
    fn spot(&self, t: &Threat, def: &ChaseDef, stop: Fx, terrain: &Terrain) -> Option<AgentIx> {
        self.agents
            .iter()
            .enumerate()
            .filter(|(_, a)| self.chaseable(a))
            .filter_map(|(i, a)| {
                let (at, z) = (a.body.position, a.height());
                if terrain.volume.is_some() {
                    // Seen in 3D, on any floor (M8.2): it paths there.
                    let gap = distance_at(t.position, t.height, at, z);
                    return (gap <= def.detect && sees_at(terrain, t.position, t.height, at, z)).then_some((gap, i));
                }
                let gap = distance(t.position, at);
                (gap <= def.detect && in_sight(terrain, t.position, at, stop)).then_some((gap, i))
            })
            .min_by_key(|&(gap, i)| (gap, i))
            .map(|(_, i)| i as AgentIx)
    }

    /// Whether building `b` has something an enemy's swing can take (B8):
    /// built, or a site with a part installed or materials delivered. A
    /// bare site, as a broken building is, has nothing left to lose, so
    /// nobody attacks it and its chaser goes back to the tree.
    pub(super) fn breakable(&self, b: BuildingIx) -> bool {
        // A lantern burns forever (L2 of the depths design): ghouls leave it.
        self.buildings.get(b as usize).filter(|building| !self.building_kinds[building.kind as usize].is_lantern()).is_some_and(|building| match &building.stage {
            Stage::Built => true,
            Stage::Site { part, materials } => *part > 0 || materials.holds_any(),
        })
    }

    /// Whether threat `t` still attacks building `b`: something to lose,
    /// and its edge within the leash.
    fn keeps_building(&self, t: &Threat, b: BuildingIx, def: &ChaseDef, terrain: &Terrain) -> bool {
        self.breakable(b) && self.building_gap(t.position, b) <= def.leash && self.on_building_floor(t, b, terrain)
    }

    /// Whether threat `t` is on the floor building `b` stands on
    /// (`BuildingState::height`, M8.6) in a volume scene; always elsewhere.
    pub(super) fn on_building_floor(&self, t: &Threat, b: BuildingIx, terrain: &Terrain) -> bool {
        terrain.volume.is_none() || (self.buildings[b as usize].height - t.height).abs() <= super::combat::FLOOR_REACH
    }

    /// The building threat `t` turns on when no elder or sprite is about
    /// (B5, B8), if any: one with something to lose whose edge is within
    /// its attack's `reach`, or within the detect radius and in the way of
    /// its route's next leg (a building put down on a wave's route: threats
    /// walk their routes as planned). The nearest edge, the lowest index on
    /// a tie.
    fn spot_building(&self, t: &Threat, def: &ChaseDef, reach: Fx, terrain: &Terrain) -> Option<BuildingIx> {
        let radius = self.threat_radius(t);
        let next = t.route.first().copied().unwrap_or(t.position);
        (0..self.buildings.len() as BuildingIx)
            .filter(|&b| self.breakable(b) && self.on_building_floor(t, b, terrain))
            .map(|b| (self.building_gap(t.position, b), b))
            .filter(|&(gap, b)| {
                let building = &self.buildings[b as usize];
                let blocks = || segment_disc(t.position, next, building.position, self.building_kinds[building.kind as usize].radius + radius).is_some();
                gap <= reach || (gap <= def.detect && blocks())
            })
            .min()
            .map(|(_, b)| b)
    }

    /// Stage 3: every threat that chases decides. One on its way to the
    /// tree looks for a quarry on its look ticks (spread over threats by
    /// id): an elder or sprite, else a building (`spot_building`). One
    /// attacking a building looks for an elder or sprite on its look ticks
    /// too, and turns on one it sees (buildings come second). A chaser
    /// gives up the moment its quarry no longer holds. A threat at its
    /// route's end breaches instead.
    pub(super) fn read_chase(&self, tick: u64, terrain: &Terrain, result: &mut Vec<(FxVec2, Messages)>) {
        for t in &self.threats {
            let def = self.threat_def(t);
            let (Some(chase), Some(attack)) = (def.chase, def.attack) else { continue };
            if t.route.is_empty() {
                continue;
            }
            let stop = stop_distance(attack.reach);
            let look = (tick + t.id) % chase.look == 0;
            let goal = match t.goal {
                Goal::Chase(Quarry::Agent(agent)) if !self.keeps(t, agent, &chase, stop, terrain) => Goal::Tree,
                Goal::Chase(Quarry::Building(b)) if !self.keeps_building(t, b, &chase, terrain) => Goal::Tree,
                Goal::Chase(Quarry::Building(_)) if look => match self.spot(t, &chase, stop, terrain) {
                    Some(agent) => Goal::Chase(Quarry::Agent(agent)),
                    None => continue,
                },
                Goal::Tree if look => match self.spot(t, &chase, stop, terrain) {
                    Some(agent) => Goal::Chase(Quarry::Agent(agent)),
                    None => match self.spot_building(t, &chase, attack.reach, terrain) {
                        Some(b) => Goal::Chase(Quarry::Building(b)),
                        None => continue,
                    },
                },
                _ => continue,
            };
            let mut out = Messages::default();
            out.threat_goal.push(ThreatGoal { threat: t.id, goal });
            result.push((t.position, out));
        }
    }

    /// Checks every chaser: its character chases, its quarry is an agent
    /// that exists, and it still has a route to come back to.
    pub(super) fn validate_chases(&self) -> Result<(), String> {
        for t in self.threats.iter().filter(|t| t.goal != Goal::Tree) {
            if self.threat_def(t).chase.is_none() {
                return Err(format!("threat {} chases but its character doesn't", t.id));
            }
            if t.goal.agent().is_some_and(|a| a as usize >= self.agents.len()) {
                return Err(format!("threat {} chases an agent that doesn't exist", t.id));
            }
            if t.goal.building().is_some_and(|b| b as usize >= self.buildings.len()) {
                return Err(format!("threat {} attacks a building that doesn't exist", t.id));
            }
            if let Some(w) = t.windup {
                let exists = match w.target {
                    super::combat::SwingTarget::Agent(a) => (a as usize) < self.agents.len(),
                    super::combat::SwingTarget::Building(b) => (b as usize) < self.buildings.len(),
                };
                if !exists {
                    return Err(format!("threat {} winds up at a target that doesn't exist", t.id));
                }
            }
            if t.route.is_empty() {
                return Err(format!("threat {} chases with no route back", t.id));
            }
        }
        if let Some(t) = self.threats.iter().find(|t| t.stranded && t.route.is_empty()) {
            return Err(format!("threat {} is stranded with nowhere to wait", t.id));
        }
        Ok(())
    }

    /// Stage 5: threats take their new goals. A chaser that gave up plans a
    /// route to its wave's target from where it stands; with no way from
    /// here, it keeps the route it had. A threat killed earlier in stage 5
    /// is gone.
    pub(crate) fn threat_goals(&mut self, messages: Vec<ThreatGoal>, terrain: &Terrain) {
        for message in messages {
            let Some(i) = self.threats.binary_search_by_key(&message.threat, |t| t.id).ok() else { continue };
            let t = &self.threats[i];
            if t.goal == message.goal {
                continue;
            }
            // A new goal plans afresh (M8.2, in a volume scene: elsewhere
            // these stay empty).
            self.threats[i].chase.clear();
            self.threats[i].backoff = super::Backoff::default();
            if message.goal == Goal::Tree {
                self.give_up(i, terrain);
            } else {
                self.threats[i].goal = message.goal;
            }
        }
    }

    /// Threat `i` (by table index) goes back to the tree: it plans a route
    /// from where it stands to its wave's target (the route's last point
    /// without a wave), keeping the old route if there is no way.
    pub(super) fn give_up(&mut self, i: usize, terrain: &Terrain) {
        self.threats[i].goal = Goal::Tree;
        self.replan_route(i, terrain);
    }

    /// Threat `i` plans its way to its wave's target afresh, from where it
    /// stands: there, or, with no walkable way, as close as it gets
    /// (stranded). It keeps the old route if it can't move at all.
    fn replan_route(&mut self, i: usize, terrain: &Terrain) {
        let t = &self.threats[i];
        let Some(target) = self.waves.get(t.wave as usize).map(|w| w.target).or(t.route.last().copied()) else { return };
        let (route, reaches) = if terrain.volume.is_some() { route_home_at(terrain, t.position, t.height, target) } else { route_home(terrain, t.position, target) };
        if !route.is_empty() {
            self.threats[i].route = route;
            self.threats[i].stranded = !reaches;
        }
    }

    /// Stage 5, after the ground changed under the town (a blueprint went
    /// down): every threat on its way to the tree plans its route afresh
    /// from where it stands, and each wave still sending enemies plans its
    /// own again at its next spawn, so they walk round a new site instead
    /// of through it. Routes are planned once otherwise (M5.6).
    pub(super) fn replan_routes(&mut self, terrain: &Terrain) {
        for w in self.waves.iter_mut().filter(|w| w.sending()) {
            w.route = None;
        }
        for i in 0..self.threats.len() {
            if self.threats[i].goal == Goal::Tree {
                self.replan_route(i, terrain);
            }
        }
    }

    /// Housekeeping, after digs took layers out of the `dug` tiles (M6.7):
    /// a wave still sending whose route passes near one plans it again at
    /// its next spawn, and so does one that was cut off; every threat on its
    /// way to the tree whose remaining route passes near one plans afresh
    /// from where it stands, so ghouls walk round a pit dug in their way.
    /// Routes far from the digs stay as they are: a dig only reshapes the
    /// ground round it.
    pub(super) fn reroute_round(&mut self, dug: &[(i32, i32)], terrain: &Terrain) {
        for w in self.waves.iter_mut().filter(|w| w.sending()) {
            let cut = w.route.as_ref().is_some_and(|r| r.last() != Some(&w.target));
            if cut || w.route.as_ref().is_some_and(|r| crosses(w.position, r, dug)) {
                w.route = None;
            }
        }
        for i in 0..self.threats.len() {
            let t = &self.threats[i];
            if t.goal == Goal::Tree && crosses(t.position, &t.route, dug) {
                self.replan_route(i, terrain);
            }
        }
    }

    /// Housekeeping: a stranded threat on its way to the tree tries again
    /// every `STRANDED_RETRY` ticks (spread by id), so a lane that opens
    /// again (a pit dug into a stair, a site gone) takes it to the tree. A
    /// failed search costs the whole window, so it isn't run on every dig.
    pub(super) fn retry_stranded(&mut self, tick: u64, terrain: &Terrain) {
        for i in 0..self.threats.len() {
            let t = &self.threats[i];
            if t.goal == Goal::Tree && t.stranded && (tick + t.id) % STRANDED_RETRY == 0 {
                self.replan_route(i, terrain);
            }
        }
    }
}
