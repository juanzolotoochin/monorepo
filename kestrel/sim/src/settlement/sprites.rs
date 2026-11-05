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
//!
//! A miner (`SpriteJob::Mine`, `miners`) plans one errand at a time and
//! plans again when it is done: carry its bundle to the tree's stone pile,
//! fetch stone lying on the ground, or claim, walk to and dig a stone tile.
//! Its dig claim is its own (`SpriteState::dig_claim`): stage 5 grants it
//! (`Settlement::claim_dig_tiles`) and the sprite gives it up when it drops
//! its plan. Its bundle's carry jobs start in stage 5 too
//! (`Settlement::sprite_bundles`) and move their units as an elder's do.
//!
//! **Fleeing (M6.7):** a sprite off the tree's hill that a ghoul chases, or
//! that one comes within its detect radius of, drops its plan and runs
//! home beside the tree (`Alarm::Threatened`, the plan `flee_plan`), so a
//! miner neither walks into ghouls nor lures them away from the tree. At
//! home it waits, and stays fled while a ghoul chases it or is within
//! `CALM_DETECTS` detect radii (`Alarm::Wary`); then it plans its job
//! again. A sprite holding a slot claim or moving a bundle finishes that
//! first; inside a building it is safe.

use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::action::{Action, View};
use super::avatar::{Dig, Intent, DOCK_REACH};
use super::haulers::{self, Ctx, Walker, RETRY_TICKS};
use super::miners::{self, Targets, Town};
use super::piles::PileId;
use super::{AgentIx, AgentState, Body, BuildingIx, BuildingKindIx, Carry, DepotIx, Direction, InventoryRef, Occupant, Role, Settlement, Slot};
use crate::messages::{ActionMessages, CarryUnit, ClaimDigTile, ClaimSlot, EnterSlot, Messages, ReserveStock, SpriteBundle, StockChange};
use crate::terrain::Terrain as Ground;
use crate::nav::{self, distance};
use crate::terrain::{Disc, Terrain};

/// How close to a step's point counts as there, as a hauler's approach does.
const ARRIVED: Fx = Fx::from_ratio(1, 10);
/// The most steps the brain looks at in one tick: a plan is a few steps,
/// and each pass moves on one or plans once.
const PASSES: usize = 8;
/// How many of a ghoul's detect radii away it must be before a fled
/// sprite goes back to work.
const CALM_DETECTS: i64 = 3;

/// An enemy that chases, as a sprite's brain sees it: where it stands,
/// how far it sees, and whom it chases.
#[derive(Clone, Copy, Debug)]
pub(super) struct Danger {
    pub position: FxVec2,
    pub detect: Fx,
    pub quarry: Option<AgentIx>,
}

/// How the enemies about stand to a sprite.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Alarm {
    /// None near, none after it.
    #[default]
    Calm,
    /// One chases it, or is within `CALM_DETECTS` detect radii: a fled
    /// sprite stays home.
    Wary,
    /// Off the hill's plateau, one chases it or sees it (within its detect
    /// radius): it flees.
    Threatened,
}

/// The alarm for sprite `me` at `body`, from `danger`. Only scenes with a
/// hill (the Sacred Tree's) have somewhere to flee to; elsewhere sprites
/// stay calm, as they were.
fn alarm(me: AgentIx, body: &Body, terrain: &Terrain, danger: &[Danger]) -> Alarm {
    let Some(hill) = terrain.mine.and_then(|m| m.hill) else { return Alarm::Calm };
    let near = |d: &Danger, reach: Fx| d.quarry == Some(me) || distance(body.position, d.position) <= reach;
    if !hill.on_plateau(Terrain::pit_tile(body.position)) && danger.iter().any(|d| near(d, d.detect)) {
        Alarm::Threatened
    } else if danger.iter().any(|d| near(d, d.detect.mul_int(CALM_DETECTS))) {
        Alarm::Wary
    } else {
        Alarm::Calm
    }
}

/// A fleeing sprite's plan: run home beside the tree, and wait.
fn flee_plan(body: &Body) -> Vec<Step> {
    vec![Step::GoTo(body.home), Step::Wait]
}

/// What a player told a sprite to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpriteJob {
    /// Go home and stand there.
    #[default]
    Idle,
    /// Work in a building of this kind.
    Staff(BuildingKindIx),
    /// Dig the nearest exposed stone and carry it to the tree's stone pile
    /// (`miners`, `SACRED_TREE_MINING.md`, M9). Appended, so older saves
    /// and input logs decode unchanged.
    Mine,
    /// The test walker's (V.7, V8 of the verticality design): follow the
    /// player through the volume, keeping `follow::KEEP` away, and do
    /// nothing else (`follow`). Appended, as `Mine` was.
    Follow,
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
    /// A miner's: hold a dig claim on this tile while walking there and
    /// digging it. Stage 3 asks, stage 5 grants; done once the sprite holds
    /// it.
    ClaimTile((i32, i32)),
    /// A miner's: face `tile` and dig out the layer under `depth` (the
    /// pit's depth when planned). Done once the pit is deeper.
    Dig { tile: (i32, i32), depth: u8 },
    /// A miner's: fill its bundle from this ground pile. Done once it holds
    /// what the job moved.
    Pickup(PileId),
    /// Empty the bundle into this depot (the tree's stone pile). Done once
    /// the bundle is empty.
    Deposit(DepotIx),
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
    /// The tile a miner may dig, granted in stage 5
    /// (`Settlement::claim_dig_tiles`): no two sprites hold the same one.
    /// It gives it up when it drops its plan.
    #[serde(default)]
    pub dig_claim: Option<(i32, i32)>,
    /// The dig under way, as an elder's (`AvatarState::dig`): housekeeping
    /// takes the layer out when it is done (`Settlement::dig_housekeeping`).
    #[serde(default)]
    pub dig: Option<Dig>,
    /// A miner's route requests on its `GoTo` step that found no route; at
    /// `miners::GIVE_UP_ASKS` it gives that goal up.
    #[serde(default)]
    pub asks: u8,
    /// The goal tiles a miner gave up as unreachable, as of terrain
    /// revision `unreachable_at`: a change of the ground clears them.
    #[serde(default)]
    pub unreachable: Vec<(i32, i32)>,
    #[serde(default)]
    pub unreachable_at: u64,
    /// The stone a miner could dig, cached by terrain revision
    /// (`miners::Targets`). Not state: the ground gives it again.
    #[serde(skip)]
    pub targets: Targets,
    /// Its feet's height in a volume scene (V.7): the floor it stands on,
    /// as `AvatarState::height` is an avatar's. Zero elsewhere, where
    /// sprites stand on `Terrain::ground`.
    #[serde(default)]
    pub height: Fx,
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

    /// The tile the plan claims to dig, if it claims one.
    fn claimed_tile(&self) -> Option<(i32, i32)> {
        self.plan.iter().find_map(|step| match *step {
            Step::ClaimTile(t) => Some(t),
            _ => None,
        })
    }

    /// Where the step it is on stands: done, still going (with the action
    /// to take, if any), or broken. A miner's plan is one errand: done, it
    /// plans the next.
    fn progress(&mut self, body: &mut Body, carry: &Carry, view: &View, town: &Town) -> Progress {
        if self.plan.is_empty() {
            return Progress::Replan;
        }
        // Threatened off the hill: drop the plan (and the route it walks)
        // and flee.
        if town.alarm == Alarm::Threatened && self.may_flee(carry) && !self.fleeing(body) {
            body.route.clear();
            return Progress::Replan;
        }
        let Some(step) = self.current() else {
            return if self.job == SpriteJob::Mine { Progress::Replan } else { Progress::Stay(None) };
        };
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
                if self.holds != self.claimed().map(Hold::Claimed) || self.dig_claim != self.claimed_tile() {
                    return Progress::Replan;
                }
                // Walking to a pile, in reach of it is there: someone may
                // stand on the spot.
                if let Some(holder) = self.holder_next(view) {
                    if distance(body.position, holder) <= miners::REACH {
                        body.route.clear();
                        self.asks = 0;
                        return Progress::Next;
                    }
                }
                if !body.route.is_empty() {
                    self.asks = 0;
                    return Progress::Stay(None);
                }
                if distance(body.position, target) <= ARRIVED {
                    self.asks = 0;
                    return Progress::Next;
                }
                // Not there and no route: ask for one, now and then.
                if view.tick < self.retry_at {
                    return Progress::Stay(None);
                }
                // A miner gives up a goal it finds no route to, until the
                // ground changes.
                if self.job == SpriteJob::Mine {
                    if self.asks >= miners::GIVE_UP_ASKS {
                        self.unreachable.push(Ground::pit_tile(target));
                        self.unreachable_at = view.terrain.revision;
                        return Progress::Replan;
                    }
                    self.asks += 1;
                }
                self.retry_at = view.tick + RETRY_TICKS;
                Progress::Stay(Some(Action::Move(Intent::GoTo { target, run: false })))
            }
            Step::ClaimTile(tile) => {
                if self.dig_claim == Some(tile) {
                    return Progress::Next;
                }
                // Stage 3 asks, unless another sprite holds it.
                if self.dig_claim.is_none() && !town.claims.iter().any(|&(t, a)| t == tile && a != view.me) {
                    return Progress::Stay(None);
                }
                Progress::Replan
            }
            Step::Dig { tile, depth } => {
                // Dug: by this sprite, or someone got there first.
                if view.terrain.pit(tile) > depth {
                    self.dig = None;
                    return Progress::Next;
                }
                if self.dig_claim != Some(tile) {
                    return Progress::Replan;
                }
                if self.dig.is_some() {
                    return Progress::Stay(None);
                }
                // Face the tile and start, if the ground and what stands on
                // it still allow it.
                let Some(mine) = town.mine else { return Progress::Replan };
                body.route.clear();
                body.heading = crate::nav::bearing(body.position, Ground::pit_center(tile));
                match view.dig_in_front(body.position, body.heading, body.base_radius) {
                    Ok(t) if t == tile => {
                        self.dig = Some(Dig { tile, started: view.tick, until: view.tick + mine.dig, layer: None });
                        Progress::Stay(None)
                    }
                    _ => Progress::Replan,
                }
            }
            Step::Pickup(pile) => {
                // Under way: stage 3 moves the units.
                if carry.job.is_some() || carry.inventory.total_in() > 0 {
                    return Progress::Stay(None);
                }
                if carry.carrying() {
                    return Progress::Next;
                }
                // Stage 3 starts it, while the pile still has stone for it.
                let other = InventoryRef::Pile(pile);
                if in_reach(body, view, other) && super::avatar::bundle_job(carry, view, other, Direction::Pickup).is_ok() {
                    return Progress::Stay(None);
                }
                Progress::Replan
            }
            Step::Deposit(depot) => {
                if carry.job.is_some() || view.leaving_cargo() {
                    return Progress::Stay(None);
                }
                if !carry.carrying() {
                    return Progress::Next;
                }
                let other = InventoryRef::Depot(depot);
                if in_reach(body, view, other) && super::avatar::bundle_job(carry, view, other, Direction::Deposit).is_ok() {
                    return Progress::Stay(None);
                }
                Progress::Replan
            }
            Step::Enter(b) => match self.holds {
                // Stage 3 asks to step in; then it works there.
                Some(Hold::Claimed(held) | Hold::Inside(held)) if held == b => Progress::Stay(None),
                // It lost the slot.
                _ => Progress::Replan,
            },
        }
    }

    /// Where the holder of the step after this one stands, if that step
    /// moves a bundle (`Pickup`, `Deposit`).
    fn holder_next(&self, view: &View) -> Option<FxVec2> {
        match self.plan.get(self.step as usize + 1)? {
            Step::Pickup(pile) => view.piles?.piles.get(pile).map(|p| p.position),
            Step::Deposit(depot) => view.depots.get(*depot as usize).map(|d| d.position),
            _ => None,
        }
    }

    /// Whether it may drop its plan to flee: not on its way to a slot it
    /// claimed, nor moving a bundle's units.
    fn may_flee(&self, carry: &Carry) -> bool {
        self.holds.is_none() && carry.job.is_none()
    }

    /// Whether its plan is fleeing home (`flee_plan`).
    fn fleeing(&self, body: &Body) -> bool {
        self.plan == flee_plan(body)
    }

    /// Drops the plan, and the dig claim and dig with it, and makes a new
    /// one for the job. Whatever the job, a sprite carrying something
    /// takes it to the tree's stone pile first. Threatened, or fled with a
    /// ghoul still about, it flees home instead and waits there.
    fn replan(&mut self, body: &Body, carry: &Carry, view: &View, town: &Town) {
        self.dig_claim = None;
        self.dig = None;
        self.asks = 0;
        let flee = self.may_flee(carry) && (town.alarm == Alarm::Threatened || (town.alarm == Alarm::Wary && self.fleeing(body)));
        if flee {
            self.plan = flee_plan(body);
            self.step = 0;
            // At home already, it waits a retry before looking again.
            self.retry_at = if distance(body.position, body.home) <= ARRIVED { view.tick + RETRY_TICKS } else { view.tick };
            return;
        }
        self.plan = match self.job {
            SpriteJob::Mine => miners::plan(self, body, carry, view, town),
            job => {
                let mut steps = miners::deliver(carry, view, town);
                steps.extend(plan(job, body, view));
                steps
            }
        };
        self.step = 0;
        self.retry_at = if self.plan == [Step::Wait] { view.tick + RETRY_TICKS } else { view.tick };
    }
}

/// Whether `body` is in reach of `holder` to move units, as an elder's
/// bundle must be (`DOCK_REACH`).
fn in_reach(body: &Body, view: &View, holder: InventoryRef) -> bool {
    let at = match holder {
        InventoryRef::Pile(id) => view.piles.and_then(|lab| lab.piles.get(&id)).map(|p| p.position),
        InventoryRef::Depot(d) => view.depots.get(d as usize).map(|d| d.position),
        _ => None,
    };
    at.is_some_and(|at| distance(body.position, at) <= DOCK_REACH)
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

/// Housekeeping: ends a miner's dig that is done, returning it if its
/// layer is still in the ground (someone else may have dug it out first).
pub(super) fn finish_dig(a: &mut AgentState, tick: u64, terrain: &Ground) -> Option<Dig> {
    let sprite = a.sprite_mut()?;
    let dig = sprite.dig.take_if(|d| tick + 1 >= d.until)?;
    match sprite.current() {
        Some(Step::Dig { tile, depth }) if tile == dig.tile && terrain.pit(tile) == depth => Some(dig),
        _ => None,
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
    if a.sprite().is_some_and(|s| s.job == SpriteJob::Follow) {
        let walk = def.movement.walk.div_int(content::TICKS_PER_SECOND as i64);
        let AgentState { body, role: Role::Sprite(sprite), .. } = a else { unreachable!("a sprite") };
        super::follow::self_update(body, sprite, ctx, walk);
        return;
    }
    let view = View { me, ..ctx.view };
    let town = Town { claims: ctx.dig_claims, stone: ctx.stone, mine: def.sprite.and_then(|s| s.mine), alarm: alarm(me, &a.body, ctx.terrain, ctx.danger) };
    if let Some(action) = think(a, &view, &town) {
        // A sprite's actions only move it: nothing for the world.
        let mut out = ActionMessages::default();
        super::action::act(a, &action, &view, def, &mut out);
        debug_assert!(out.is_empty(), "sprite actions emit no messages");
    }
    let AgentState { character, body, role: Role::Sprite(sprite), .. } = a else { unreachable!("sprites walk") };
    // A digger stands still, as the elder does.
    if sprite.inside() || sprite.dig.is_some() {
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
pub(super) fn think(a: &mut AgentState, view: &View, town: &Town) -> Option<Action> {
    let AgentState { body, carry, role: Role::Sprite(sprite), .. } = a else { unreachable!("sprites think") };
    for _ in 0..PASSES {
        match sprite.progress(body, carry, view, town) {
            Progress::Stay(action) => return action,
            Progress::Next => sprite.step += 1,
            Progress::Replan => sprite.replan(body, carry, view, town),
        }
    }
    None
}

/// Stage 3 for a sprite: asks for the slot on a `Claim` step while it is
/// free, and to step in on an `Enter` step once the claim is held. A miner
/// asks for its tile on a `ClaimTile` step, starts its bundle's carry job
/// at a pile, then moves a unit whenever one is due and it is in reach, as
/// an elder's bundle does (`read_avatar`).
pub(super) fn read(a: &AgentState, me: AgentIx, tick: u64, s: &Settlement, out: &mut Messages) {
    let Some(sprite) = a.sprite().filter(|_| a.body.alive()) else { return };
    if let Some(job) = &a.carry.job {
        if tick >= job.next_tick && distance(a.body.position, s.position_of(job.other)) <= DOCK_REACH {
            out.player_carry.push(CarryUnit { agent: me });
        }
        return;
    }
    let reach = |other: InventoryRef| distance(a.body.position, s.position_of(other)) <= DOCK_REACH;
    match sprite.current() {
        Some(Step::ClaimTile(tile)) if sprite.dig_claim.is_none() => out.claim_dig_tile.push(ClaimDigTile { agent: me, tile }),
        Some(Step::Pickup(pile)) if !a.carry.carrying() && a.carry.inventory.total_in() == 0 && s.piles.as_ref().is_some_and(|lab| lab.piles.contains_key(&pile)) => {
            if reach(InventoryRef::Pile(pile)) {
                out.sprite_bundle.push(SpriteBundle { agent: me, other: InventoryRef::Pile(pile), direction: Direction::Pickup });
            }
        }
        Some(Step::Deposit(depot)) if a.carry.carrying() && (depot as usize) < s.depots.len() => {
            if reach(InventoryRef::Depot(depot)) {
                out.sprite_bundle.push(SpriteBundle { agent: me, other: InventoryRef::Depot(depot), direction: Direction::Deposit });
            }
        }
        _ => {}
    }
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
    /// The enemies that chase, for sprites to flee from (`Danger`).
    pub(super) fn danger(&self) -> Vec<Danger> {
        self.threats
            .iter()
            .filter_map(|t| {
                let chase = self.threat_def(t).chase?;
                Some(Danger { position: t.position, detect: chase.detect, quarry: t.goal.agent() })
            })
            .collect()
    }

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
        self.drop_sprite_errand(agent as usize);
        let sprite = self.agents[agent as usize].sprite_mut().expect("checked");
        sprite.job = job;
        sprite.plan.clear();
        sprite.step = 0;
    }

    /// Ends what a miner's errand holds outside its plan: its bundle's
    /// carry job (giving back what it holds on both sides; units in the air
    /// still land), its dig and its dig claim. What it carries it keeps, to
    /// take to the tree's stone pile when it plans again.
    fn drop_sprite_errand(&mut self, agent: usize) {
        let releases = self.agents[agent].carry.drop_carry(agent as AgentIx);
        self.reserve_stock(releases);
        let sprite = self.agents[agent].sprite_mut().expect("a sprite");
        sprite.dig = None;
        sprite.dig_claim = None;
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
        self.drop_sprite_errand(agent);
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
            sprite.asks = 0;
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

    /// Stage 5: grants dig claims. Of the claims a tile can take (no sprite
    /// holds it, and the miner is alive, on that `ClaimTile` step, holding
    /// none), the lowest agent index wins, whatever the message order. The
    /// others find it taken on their next step and plan again.
    pub(crate) fn claim_dig_tiles(&mut self, claims: Vec<ClaimDigTile>) {
        let mut winners: std::collections::BTreeMap<(i32, i32), AgentIx> = std::collections::BTreeMap::new();
        for c in claims {
            let held = self.agents.iter().filter_map(AgentState::sprite).any(|s| s.dig_claim == Some(c.tile));
            let wanted = self.agents.get(c.agent as usize).filter(|a| a.body.alive()).and_then(AgentState::sprite).is_some_and(|sprite| {
                sprite.job == SpriteJob::Mine && sprite.dig_claim.is_none() && sprite.current() == Some(Step::ClaimTile(c.tile))
            });
            if wanted && !held {
                let winner = winners.entry(c.tile).or_insert(c.agent);
                *winner = (*winner).min(c.agent);
            }
        }
        for (tile, agent) in winners {
            self.agents[agent as usize].sprite_mut().expect("checked").dig_claim = Some(tile);
        }
    }

    /// Stage 5: miners at a pile start their bundle's carry job there by the
    /// rule an elder's bundle follows (`avatar::bundle_job`), holding both
    /// sides now. One the rule refuses (the pile emptied, the stone pile
    /// filled) starts nothing, and its sprite plans again.
    pub(crate) fn sprite_bundles(&mut self, messages: Vec<SpriteBundle>, tick: u64, terrain: &Ground) {
        for m in messages {
            let Some(a) = self.agents.get(m.agent as usize).filter(|a| a.body.alive() && a.carry.job.is_none()) else { continue };
            let step = match m.other {
                InventoryRef::Pile(id) => Step::Pickup(id),
                InventoryRef::Depot(d) => Step::Deposit(d),
                _ => continue,
            };
            if a.sprite().and_then(SpriteState::current) != Some(step) {
                continue;
            }
            let Ok(job) = super::avatar::bundle_job(&a.carry, &self.view(m.agent, tick, terrain), m.other, m.direction) else { continue };
            let carry = &mut self.agents[m.agent as usize].carry;
            let reserved = job.reserve(&mut carry.inventory, super::Side::Own);
            debug_assert!(reserved, "`bundle_job` sized it to the cargo");
            carry.job = Some(job.clone());
            self.reserve_stock(vec![ReserveStock { agent: m.agent, job, change: StockChange::Reserve }]);
        }
    }
}
