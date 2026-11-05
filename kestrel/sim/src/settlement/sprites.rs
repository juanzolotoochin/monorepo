//! Sprites: the town's workers, who staff buildings (`SACRED_TREE_HANDOFF.md`,
//! D2). A player gives a sprite a job (`SetSpriteJob`): idle, or a building
//! kind to staff. The sprite makes a plan for it, a short list of steps
//! (`plan`), and works through it.
//!
//! The steps split along the tick's stages as a hauler's work does
//! (`HAULER_ACTIONS_HANDOFF.md`):
//! - stage 2: the brain (`think`) checks the step it is on, moves on to the
//!   next when it is done, and walks (`Move(GoTo)`, through `action::act`)
//!   with the hauler's steering (`haulers::move_worker`);
//! - stage 3: a sprite on a `Claim` step asks for the slot, and one at its
//!   building's spot on an `Enter` step asks to step in (`read`);
//! - stage 5: claims are granted, the lowest agent index winning a
//!   building, and entries made (`Settlement::claim_slots`, `enter_slots`).
//!   Only stage 5 and a job change write a building's slot and the sprite's
//!   `holds` together, so the two always agree.
//!
//! **Replanning:** anything that breaks a step drops the whole plan, and the
//! sprite plans again: a refused claim (the slot went to someone else), a
//! new job (which drops the plan and gives up the slot), or the sprite no
//! longer holding the slot its plan needs (`Settlement::release_slot`, which
//! an elder possessing the building will use, D3). Nothing else edits a plan.
//!
//! Inside a building the sprite stands at its slot spot
//! (`BuildingState::slot_position`) and leaves the steering footprints:
//! others walk past it.

use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::action::{Action, View};
use super::avatar::Intent;
use super::haulers::{self, Ctx, Walker, RETRY_TICKS};
use super::{AgentIx, AgentState, Body, BuildingIx, BuildingKindIx, Occupant, Role, Settlement, Slot};
use crate::messages::{ActionMessages, ClaimSlot, EnterSlot, Messages};
use crate::nav::{self, distance};
use crate::terrain::{Disc, Terrain};

/// How close to a step's point counts as there, as a hauler's approach does.
const ARRIVED: Fx = Fx::from_ratio(1, 10);
/// The most steps the brain looks at in one tick: a plan is a few steps,
/// and each pass moves on one or plans once.
const PASSES: usize = 8;

/// What a player told a sprite to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpriteJob {
    /// Go home and stand there.
    #[default]
    Idle,
    /// Work in a building of this kind.
    Staff(BuildingKindIx),
}

/// One step of a sprite's plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    /// Hold the building's slot while walking there. Stage 3 asks, stage 5
    /// grants; done once the sprite holds the claim.
    Claim(BuildingIx),
    /// Walk to this point; done on arrival.
    GoTo(FxVec2),
    /// Step into the building whose slot the sprite claimed. The sprite
    /// stays on this step while it works there.
    Enter(BuildingIx),
    /// No building of the job's kind is free: stand, and plan again after
    /// `RETRY_TICKS`.
    Wait,
}

/// The part of a building's slot a sprite holds: the slot names it back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hold {
    /// Claimed, on its way in: the slot is `Slot::Claimed`.
    Claimed(BuildingIx),
    /// Inside, working: the slot is `Slot::Occupied(Occupant::Sprite)`.
    Inside(BuildingIx),
}

impl Hold {
    pub fn building(self) -> BuildingIx {
        match self {
            Hold::Claimed(b) | Hold::Inside(b) => b,
        }
    }
}

/// How a sprite steers, as a hauler does (`haulers::Walker`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Steering {
    pub travel_heading: Fx,
    pub traffic_replan_at: u64,
    pub yielding: u64,
    pub avoidance_steers: u64,
    pub traffic_wait_ticks: u64,
}

/// A sprite's role: its job, the plan for it and the step it is on, and the
/// slot it holds. Its body is the agent's (`AgentState`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpriteState {
    pub job: SpriteJob,
    /// Empty until the brain plans for the job (`plan`).
    pub plan: Vec<Step>,
    /// The step it is on, an index into `plan`: past the end once the plan
    /// is done.
    pub step: u16,
    /// The slot it holds. Written only with the slot itself: in stage 5,
    /// and when a job change gives it up (`Settlement::release_slot`).
    pub holds: Option<Hold>,
    /// When a `Wait` plans again, or a `GoTo` that lost its route asks for
    /// one again.
    pub retry_at: u64,
    pub steering: Steering,
    /// Dead until this tick (B6, `Body::health` at 0), then back at home
    /// beside the tree with its job (`Settlement::sprite_housekeeping`).
    #[serde(default)]
    pub respawn_at: u64,
}

impl SpriteState {
    /// The step it is on, if its plan isn't done.
    pub fn current(&self) -> Option<Step> {
        self.plan.get(self.step as usize).copied()
    }

    /// Inside a building, working.
    pub fn inside(&self) -> bool {
        matches!(self.holds, Some(Hold::Inside(_)))
    }

    /// The building the plan claims, if it claims one.
    fn claimed(&self) -> Option<BuildingIx> {
        self.plan.iter().find_map(|step| match *step {
            Step::Claim(b) => Some(b),
            _ => None,
        })
    }

    /// Where the step it is on stands: done, still going (with the action
    /// to take, if any), or broken.
    fn progress(&mut self, body: &Body, view: &View) -> Progress {
        if self.plan.is_empty() {
            return Progress::Replan;
        }
        let Some(step) = self.current() else { return Progress::Stay(None) };
        match step {
            Step::Wait if view.tick >= self.retry_at => Progress::Replan,
            Step::Wait => Progress::Stay(None),
            Step::Claim(b) => match self.holds {
                Some(Hold::Claimed(held)) if held == b => Progress::Next,
                // Stage 3 asks for it.
                None if claimable(view, b, self.job) => Progress::Stay(None),
                // Refused: the slot went to someone else.
                _ => Progress::Replan,
            },
            Step::GoTo(target) => {
                // The claim must hold all the way there.
                if self.holds != self.claimed().map(Hold::Claimed) {
                    return Progress::Replan;
                }
                if !body.route.is_empty() {
                    return Progress::Stay(None);
                }
                if distance(body.position, target) <= ARRIVED {
                    return Progress::Next;
                }
                // Not there and no route: ask for one, now and then.
                if view.tick < self.retry_at {
                    return Progress::Stay(None);
                }
                self.retry_at = view.tick + RETRY_TICKS;
                Progress::Stay(Some(Action::Move(Intent::GoTo { target, run: false })))
            }
            Step::Enter(b) => match self.holds {
                // Stage 3 asks to step in; then it works there.
                Some(Hold::Claimed(held) | Hold::Inside(held)) if held == b => Progress::Stay(None),
                // It lost the slot.
                _ => Progress::Replan,
            },
        }
    }

    /// Drops the plan and makes a new one for the job.
    fn replan(&mut self, body: &Body, view: &View) {
        self.plan = plan(self.job, body, view);
        self.step = 0;
        self.retry_at = if self.plan == [Step::Wait] { view.tick + RETRY_TICKS } else { view.tick };
    }
}

enum Progress {
    /// On this step still; take the action, if any.
    Stay(Option<Action>),
    /// The step is done: on to the next.
    Next,
    /// The step broke: plan again.
    Replan,
}

/// Whether a sprite with `job` may claim building `b` now: built, of the
/// job's kind, with nobody in or on the way to its slot.
fn claimable(view: &View, b: BuildingIx, job: SpriteJob) -> bool {
    view.buildings.get(b as usize).is_some_and(|building| {
        building.built() && building.slot == Slot::Empty && job == SpriteJob::Staff(building.kind)
    })
}

/// The plan for `job`, from where `body` stands. Idle: go home. Staffing:
/// the nearest building of the kind that can be claimed (the lowest index
/// on a tie): claim it, walk to its slot spot, step in. With none free,
/// wait.
pub fn plan(job: SpriteJob, body: &Body, view: &View) -> Vec<Step> {
    let SpriteJob::Staff(_) = job else { return vec![Step::GoTo(body.home)] };
    let nearest = (0..view.buildings.len() as BuildingIx)
        .filter(|&b| claimable(view, b, job))
        .min_by_key(|&b| (distance(body.position, view.buildings[b as usize].position), b));
    match nearest {
        Some(b) => {
            let building = &view.buildings[b as usize];
            let spot = building.slot_position(&view.building_kinds[building.kind as usize]);
            vec![Step::Claim(b), Step::GoTo(spot), Step::Enter(b)]
        }
        None => vec![Step::Wait],
    }
}

impl Walker for SpriteState {
    fn radius(&self, body: &Body) -> Fx {
        body.base_radius
    }

    fn towing(&self) -> bool {
        false
    }

    fn trailer_disc(&self) -> Option<Disc> {
        None
    }

    fn route_to(&self, body: &Body, terrain: &Terrain, to: FxVec2, extra: &[Disc]) -> Vec<FxVec2> {
        nav::path_avoiding(terrain, body.position, to, body.base_radius, extra)
    }

    fn travel_heading(&mut self) -> &mut Fx {
        &mut self.steering.travel_heading
    }

    fn traffic_replan_at(&mut self) -> &mut u64 {
        &mut self.steering.traffic_replan_at
    }

    fn yielding(&mut self) -> &mut u64 {
        &mut self.steering.yielding
    }

    fn avoidance_steers(&mut self) -> &mut u64 {
        &mut self.steering.avoidance_steers
    }

    fn traffic_wait_ticks(&mut self) -> &mut u64 {
        &mut self.steering.traffic_wait_ticks
    }
}

/// Applies an action to a sprite (see `action::act`): `GoTo` plans a route.
/// Only the sprite brain drives sprites.
pub(super) fn act(body: &mut Body, sprite: &SpriteState, action: &Action, view: &View) {
    match action {
        Action::Move(Intent::GoTo { target, .. }) => body.route = sprite.route_to(body, view.terrain, *target, &[]),
        _ => debug_assert!(false, "sprites only go to points: {action:?}"),
    }
}

/// Stage 2 for a sprite: the brain checks its step (`think`), the action
/// applies as any agent's does (`action::act`), and the sprite walks, unless
/// it is inside a building.
pub(super) fn self_update(a: &mut AgentState, me: AgentIx, ctx: &Ctx) {
    // A dead sprite waits to come back (`Settlement::sprite_housekeeping`).
    if !a.body.alive() {
        return;
    }
    let def = super::avatar::character_of(ctx.characters, a);
    let view = View { me, ..ctx.view };
    if let Some(action) = think(a, &view) {
        // A sprite's actions only move it: nothing for the world.
        let mut out = ActionMessages::default();
        super::action::act(a, &action, &view, def, &mut out);
        debug_assert!(out.is_empty(), "sprite actions emit no messages");
    }
    let AgentState { character, body, role: Role::Sprite(sprite), .. } = a else { unreachable!("sprites walk") };
    if sprite.inside() {
        return;
    }
    let walk = ctx.characters[*character as usize].movement.walk.div_int(content::TICKS_PER_SECOND as i64);
    let before = body.position;
    haulers::move_worker(body, sprite, me, ctx, walk);
    body.travelled += distance(before, body.position);
}

/// The sprite brain: moves through the plan, planning again when a step
/// breaks, and returns the action to take. It writes the sprite's own plan,
/// never its route or what it holds.
pub(super) fn think(a: &mut AgentState, view: &View) -> Option<Action> {
    let AgentState { body, role: Role::Sprite(sprite), .. } = a else { unreachable!("sprites think") };
    for _ in 0..PASSES {
        match sprite.progress(body, view) {
            Progress::Stay(action) => return action,
            Progress::Next => sprite.step += 1,
            Progress::Replan => sprite.replan(body, view),
        }
    }
    None
}

/// Stage 3 for a sprite: asks for the slot on a `Claim` step while it is
/// free, and to step in on an `Enter` step once the claim is held.
pub(super) fn read(a: &AgentState, me: AgentIx, s: &Settlement, out: &mut Messages) {
    let Some(sprite) = a.sprite().filter(|_| a.body.alive()) else { return };
    match (sprite.current(), sprite.holds) {
        (Some(Step::Claim(building)), None) if s.buildings.get(building as usize).is_some_and(|b| b.slot == Slot::Empty) => {
            out.claim_slot.push(ClaimSlot { agent: me, building });
        }
        (Some(Step::Enter(building)), Some(Hold::Claimed(held))) if held == building => {
            out.enter_slot.push(EnterSlot { agent: me, building });
        }
        _ => {}
    }
}

impl Settlement {
    /// Commands: gives sprite `agent` a new job. It gives up the slot it
    /// holds and drops its plan; the brain plans for the new job on its next
    /// step. The job it already has, an agent that isn't a sprite, or a
    /// kind the scenario hasn't, changes nothing.
    pub fn set_sprite_job(&mut self, agent: AgentIx, job: SpriteJob) {
        if let SpriteJob::Staff(kind) = job {
            if kind as usize >= self.building_kinds.len() {
                return;
            }
        }
        let Some(sprite) = self.agents.get(agent as usize).and_then(AgentState::sprite) else { return };
        if sprite.job == job {
            return;
        }
        self.release_slot(agent);
        let sprite = self.agents[agent as usize].sprite_mut().expect("checked");
        sprite.job = job;
        sprite.plan.clear();
        sprite.step = 0;
    }

    /// Takes the slot sprite `agent` holds, claimed or inside, away from it,
    /// leaving the slot empty: the one way a sprite loses a slot. A job
    /// change does it, and so will an elder possessing the building (D3),
    /// which then takes the slot. The sprite's brain finds on its next step
    /// that its plan lost what it needed, and plans again.
    pub(super) fn release_slot(&mut self, agent: AgentIx) {
        let Some(sprite) = self.agents.get_mut(agent as usize).and_then(AgentState::sprite_mut) else { return };
        let Some(hold) = sprite.holds.take() else { return };
        let building = &mut self.buildings[hold.building() as usize];
        debug_assert!(matches!(building.slot, Slot::Claimed(a) | Slot::Occupied(Occupant::Sprite(a)) if a == agent), "the slot names its sprite");
        building.slot = Slot::Empty;
    }

    /// Stage 5: grants slot claims. Of the claims a building can still take
    /// (built, empty, and the sprite still on that `Claim` step for a job of
    /// its kind), the lowest agent index wins, whatever the message order.
    /// The others find the slot taken on their next step and plan again.
    pub(crate) fn claim_slots(&mut self, claims: Vec<ClaimSlot>) {
        let mut winners: std::collections::BTreeMap<BuildingIx, AgentIx> = std::collections::BTreeMap::new();
        for c in claims {
            let Some(building) = self.buildings.get(c.building as usize) else { continue };
            let wanted = self.agents.get(c.agent as usize).and_then(AgentState::sprite).is_some_and(|sprite| {
                sprite.holds.is_none() && sprite.current() == Some(Step::Claim(c.building)) && sprite.job == SpriteJob::Staff(building.kind)
            });
            if wanted && building.built() && building.slot == Slot::Empty {
                let winner = winners.entry(c.building).or_insert(c.agent);
                *winner = (*winner).min(c.agent);
            }
        }
        for (building, agent) in winners {
            self.buildings[building as usize].slot = Slot::Claimed(agent);
            self.agents[agent as usize].sprite_mut().expect("checked").holds = Some(Hold::Claimed(building));
        }
    }

    /// Stage 5: an enemy's swing takes `damage` from sprite `agent`'s
    /// health (B6). At 0 it dies: it gives up the slot it holds (a claim:
    /// one inside a building is never hit) and its plan, stops, and comes
    /// back at home beside the tree `respawn` ticks later, keeping its job
    /// (`sprite_housekeeping`). Returns whether it died.
    pub(super) fn hurt_sprite(&mut self, agent: usize, damage: u32, tick: u64, respawn: u64) -> bool {
        let body = &mut self.agents[agent].body;
        body.health = body.health.saturating_sub(damage);
        if body.health > 0 {
            return false;
        }
        self.release_slot(agent as AgentIx);
        let AgentState { body, role: Role::Sprite(sprite), .. } = &mut self.agents[agent] else { unreachable!("only sprites are hurt here") };
        body.route.clear();
        body.velocity = FxVec2::ZERO;
        body.push = FxVec2::ZERO;
        sprite.plan.clear();
        sprite.step = 0;
        sprite.respawn_at = tick + respawn;
        true
    }

    /// Housekeeping: dead sprites whose time is up come back at home, the
    /// spot beside the tree they started on, with full health and their
    /// job. Their plan is empty, so the brain plans for the job on their
    /// next step (D2).
    pub(crate) fn sprite_housekeeping(&mut self, tick: u64) {
        for i in 0..self.agents.len() {
            let AgentState { body, role: Role::Sprite(sprite), .. } = &mut self.agents[i] else { continue };
            if body.alive() || tick < sprite.respawn_at {
                continue;
            }
            body.position = body.home;
            body.health = body.max_health;
            body.route.clear();
            sprite.respawn_at = 0;
            sprite.plan.clear();
            sprite.step = 0;
            sprite.retry_at = 0;
            sprite.steering = Steering::default();
            self.events.push(super::Event::SpriteReturned { agent: i as AgentIx });
        }
    }

    /// Stage 5: sprites at their claimed building's spot step in, and the
    /// building starts working on the next economy step.
    pub(crate) fn enter_slots(&mut self, entries: Vec<EnterSlot>) {
        for e in entries {
            let Some(building) = self.buildings.get_mut(e.building as usize) else { continue };
            let Some(sprite) = self.agents.get_mut(e.agent as usize).and_then(AgentState::sprite_mut) else { continue };
            if building.slot != Slot::Claimed(e.agent) || sprite.holds != Some(Hold::Claimed(e.building)) {
                continue;
            }
            building.slot = Slot::Occupied(Occupant::Sprite(e.agent));
            sprite.holds = Some(Hold::Inside(e.building));
        }
    }
}
