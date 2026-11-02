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
/// where it would stop. A quarry hugging a building is still seen.
pub(super) fn in_sight(terrain: &Terrain, from: FxVec2, to: FxVec2, stop: Fx) -> bool {
    terrain.clear(from, to, SIGHT) && terrain.clear(from, approach(from, to, stop), WALK_CLEARANCE)
}

/// Stage 2 for a chaser: a step of at most `step` meters straight at
/// `target`, stopping `stop` short of it, if the ground there is clear.
pub(super) fn steer(position: &mut FxVec2, target: FxVec2, stop: Fx, step: Fx, terrain: &Terrain) {
    let gap = distance(*position, target);
    if gap <= stop {
        return;
    }
    let next = *position + (target - *position) * (step.min(gap - stop) / gap);
    if terrain.clear(*position, next, WALK_CLEARANCE) {
        *position = next;
    }
}

/// The plan for `Goal::Tree`: a route from `from` to `target`, as a wave
/// plans one (`combat_housekeeping`).
pub(super) fn route_home(terrain: &Terrain, from: FxVec2, target: FxVec2) -> Vec<FxVec2> {
    nav::path(terrain, from, target, WALK_CLEARANCE)
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
        self.chaseable(a) && distance(t.position, a.body.position) <= def.leash && in_sight(terrain, t.position, a.body.position, stop)
    }

    /// The quarry threat `t` turns on, if any: the nearest chaseable agent
    /// within the detect radius that it sees, the lowest index on a tie.
    fn spot(&self, t: &Threat, def: &ChaseDef, stop: Fx, terrain: &Terrain) -> Option<AgentIx> {
        self.agents
            .iter()
            .enumerate()
            .filter(|(_, a)| self.chaseable(a))
            .map(|(i, a)| (distance(t.position, a.body.position), i, a.body.position))
            .filter(|&(gap, _, at)| gap <= def.detect && in_sight(terrain, t.position, at, stop))
            .min_by_key(|&(gap, i, _)| (gap, i))
            .map(|(_, i, _)| i as AgentIx)
    }

    /// Whether building `b` has something an enemy's swing can take (B8):
    /// built, or a site with a part installed or materials delivered. A
    /// bare site, as a broken building is, has nothing left to lose, so
    /// nobody attacks it and its chaser goes back to the tree.
    pub(super) fn breakable(&self, b: BuildingIx) -> bool {
        self.buildings.get(b as usize).is_some_and(|building| match &building.stage {
            Stage::Built => true,
            Stage::Site { part, materials } => *part > 0 || materials.holds_any(),
        })
    }

    /// Whether threat `t` still attacks building `b`: something to lose,
    /// and its edge within the leash.
    fn keeps_building(&self, t: &Threat, b: BuildingIx, def: &ChaseDef) -> bool {
        self.breakable(b) && self.building_gap(t.position, b) <= def.leash
    }

    /// The building threat `t` turns on when no elder or sprite is about
    /// (B5, B8), if any: one with something to lose whose edge is within
    /// its attack's `reach`, or within the detect radius and in the way of
    /// its route's next leg (a building put down on a wave's route: threats
    /// walk their routes as planned). The nearest edge, the lowest index on
    /// a tie.
    fn spot_building(&self, t: &Threat, def: &ChaseDef, reach: Fx) -> Option<BuildingIx> {
        let radius = self.threat_radius(t);
        let next = t.route.first().copied().unwrap_or(t.position);
        (0..self.buildings.len() as BuildingIx)
            .filter(|&b| self.breakable(b))
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
                Goal::Chase(Quarry::Building(b)) if !self.keeps_building(t, b, &chase) => Goal::Tree,
                Goal::Chase(Quarry::Building(_)) if look => match self.spot(t, &chase, stop, terrain) {
                    Some(agent) => Goal::Chase(Quarry::Agent(agent)),
                    None => continue,
                },
                Goal::Tree if look => match self.spot(t, &chase, stop, terrain) {
                    Some(agent) => Goal::Chase(Quarry::Agent(agent)),
                    None => match self.spot_building(t, &chase, attack.reach) {
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
            if message.goal == Goal::Tree {
                let target = self.waves.get(t.wave as usize).map(|w| w.target).or(t.route.last().copied());
                if let Some(target) = target {
                    let route = route_home(terrain, t.position, target);
                    if !route.is_empty() {
                        self.threats[i].route = route;
                    }
                }
            }
            self.threats[i].goal = message.goal;
        }
    }
}
