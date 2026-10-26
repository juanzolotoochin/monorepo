//! Client-side prediction of the joined player's own avatar.
//!
//! A client's confirmed world runs an input lead behind its input: a command
//! applies on the tick it asked for, `input_horizon`, roughly the input
//! margin plus a round trip later. So each frame the avatar is replayed from
//! the confirmed state through the player's commands that haven't applied
//! yet (`sim::settlement::AvatarReplay`, the simulation's own code) and drawn
//! at the horizon. When the prediction for a drawn tick changes, the
//! difference becomes a visual offset that decays. Remote players stay
//! interpolated, so this avatar is drawn about an input lead ahead of them.

use std::collections::VecDeque;

use bevy::prelude::*;
use fx::FxVec2;
use sim::settlement::{AgentState, Event, Settlement};
use sim::terrain::Terrain;
use sim::CommandKind;

/// Further ahead than this from the confirmed tick, the confirmed avatar is drawn.
pub const MAX_LEAD: u64 = 60;
/// The visual offset left by a misprediction halves this often (seconds).
const OFFSET_HALF_LIFE: f32 = 0.05;
/// An offset larger than this (units) snaps instead.
const SNAP_DISTANCE: f32 = 1.5;
/// The visual offset left by a knockback halves this often (seconds). A
/// knockback is a change of prediction a stage 5 touch explains (a hit, a
/// death, a push), which the replay never runs: an expected correction, not
/// a misprediction, so it slides longer (D6 in `ABILITY_PREDICTION_HANDOFF.md`).
const KNOCKBACK_HALF_LIFE: f32 = 0.1;
/// A knockback offset larger than this (units) snaps instead.
const KNOCKBACK_SNAP_DISTANCE: f32 = 3.0;
/// How far (ticks) the drawn time stays before the lowest recent horizon,
/// besides the frame a command waits to be sent. A command first changes
/// the state after the tick it asks for, so drawing before that means new
/// input never changes what was already drawn.
const HORIZON_MARGIN: f64 = 0.5;
/// How long (ticks) a horizon counts toward the lowest recent one. Packets
/// arriving unevenly make the horizon dip below its trend.
const HORIZON_WINDOW: f64 = 60.0;
/// How quickly the drawn time follows the horizon (seconds) …
const FOLLOW_SECONDS: f64 = 0.2;
/// … running at most this much faster or slower than the clock while it does.
const MAX_RATE_CHANGE: f64 = 0.25;
/// Further than this from its aim (ticks), the drawn time jumps.
const MAX_DRIFT: f64 = 3.0;

/// What the confirmed world says this frame.
pub struct Confirmed<'a> {
    /// The world (by session number) and the avatar's index in it.
    pub world: u32,
    pub agent: u16,
    /// The confirmed settlement and terrain, and the tick it runs next.
    pub settlement: &'a Settlement,
    pub terrain: &'a Terrain,
    pub tick: u64,
    /// The avatar before the last confirmed tick, for drawing it confirmed.
    pub previous: &'a AgentState,
    /// How far render time is past the last confirmed tick.
    pub alpha: f32,
    /// The player's commands that haven't applied, each with its tick.
    pub pending: &'a [(u64, &'a CommandKind)],
    /// The tick a command sent now asks for.
    pub horizon: u64,
    pub to_view: &'a dyn Fn(FxVec2) -> Vec3,
    /// Recent confirmed ticks on which something the replay never runs
    /// changed the avatar (`StageFive`), oldest first.
    pub touched: &'a [u64],
}

/// Where to draw the avatar this frame.
pub struct Drawn {
    /// Horizontal position, offset included; `y` is the avatar's height.
    pub position: Vec3,
    /// The same without the offset: where the avatar is predicted.
    pub target: Vec3,
    /// The states either side of the drawn time and how far between them:
    /// predicted, or confirmed when the avatar can't be predicted.
    pub from: AgentState,
    pub to: AgentState,
    pub fraction: f32,
    /// The drawn time in ticks: `from` is the state before tick
    /// `floor(time)`. Confirmed, it is the views' render time.
    pub time: f32,
    /// How far a change of prediction moved the drawn avatar this frame,
    /// unless a stage 5 touch explains it (`knockback`).
    pub mispredicted: f32,
    /// The avatar was too far off and was put where it is predicted.
    pub snapped: bool,
    /// How far a change of prediction that a stage 5 touch explains moved
    /// the drawn avatar this frame: an expected correction.
    pub knockback: f32,
    /// A knockback left the avatar too far off, so it was put where it is
    /// predicted.
    pub knockback_snapped: bool,
    /// The events the predicted avatar showed from the confirmed tick
    /// through the drawn tick, oldest first: the ticks the drawn time
    /// reached that the confirmed world hasn't run yet.
    pub events: Vec<PredictedEvent>,
}

/// An event the replayed avatar showed (`AvatarReplay::events`).
#[derive(Clone, Debug, PartialEq)]
pub struct PredictedEvent {
    /// The tick it is predicted on.
    pub tick: u64,
    pub event: Event,
}

/// The local player's avatar as it is drawn this frame (`follow_avatar`):
/// predicted on a joined client, confirmed otherwise. Anything of the
/// avatar's that lasts (jetpack flame and sound, jump crouch, dig pose and
/// dust, the swing) is drawn from this, so a prediction that is withdrawn
/// or corrected ends or changes it on the next frame with no code of its
/// own (D5 in `ABILITY_PREDICTION_HANDOFF.md`). Remote players are drawn
/// from the confirmed world.
#[derive(Resource, Default)]
pub struct DrawnAvatar(pub Option<DrawnState>);

#[derive(Clone, Debug)]
pub struct DrawnState {
    pub agent: u16,
    /// The states either side of the drawn time.
    pub from: AgentState,
    pub to: AgentState,
    /// The drawn time in ticks, as `Drawn::time` (the render time for a
    /// confirmed avatar), to time phases such as a dig's or a jump's.
    pub time: f32,
}

impl DrawnState {
    /// The drawn avatar's state: the newer one.
    pub fn avatar(&self) -> &sim::settlement::avatar::AvatarState {
        self.to.avatar().expect("only avatars are drawn as the local avatar")
    }

    /// How long (seconds) the drawn avatar has been digging, if it is.
    pub fn dig_age(&self) -> Option<f32> {
        self.avatar().dig.as_ref().map(|dig| crate::character::dig_age(self.time, dig))
    }
}

#[derive(Default)]
pub struct AvatarPrediction {
    /// The (world, agent) predicted; anything else starts over.
    key: Option<(u32, u16)>,
    /// Drawn time in ticks: between the states before ticks `floor` and
    /// `floor + 1`. It follows the horizon on the frame clock, so a horizon
    /// that jumps as packets arrive unevenly doesn't move the avatar.
    drawn: Option<f64>,
    /// Frame time so far, in ticks.
    clock: f64,
    /// Recent horizons as `horizon - clock` at the clock they were seen,
    /// increasing, for the lowest one in the window projected to now.
    horizons: VecDeque<(f64, f64)>,
    /// Last frame's predicted positions, from the tick `first`, to tell a
    /// misprediction from movement: compared for the same tick.
    first: u64,
    positions: Vec<Vec3>,
    /// Last frame's drawn position before the offset, and whether it was
    /// predicted, to keep switching to or from the confirmed avatar smooth.
    last: Option<(Vec3, bool)>,
    offset: Vec3,
    /// The offset knockbacks left, decaying and snapping on their own terms.
    knockback: Vec3,
}

impl AvatarPrediction {
    pub fn reset(&mut self) {
        *self = AvatarPrediction::default();
    }

    /// The drawn time, in ticks, while predicting.
    #[cfg(test)]
    pub fn drawn_time(&self) -> Option<f64> {
        self.drawn
    }

    /// Advances the drawn time by `dt` seconds and predicts the avatar there.
    pub fn update(&mut self, c: &Confirmed, dt: f32) -> Drawn {
        if self.key != Some((c.world, c.agent)) {
            self.reset();
            self.key = Some((c.world, c.agent));
        }
        let (drawn, jumped) = self.follow_horizon(c, dt);
        let base = drawn.floor() as u64;
        let fraction = (drawn - base as f64) as f32;
        let view = |a: &AgentState| {
            let p = (c.to_view)(a.body.position);
            Vec3::new(p.x, a.avatar().map_or(0.0, |avatar| avatar.height.to_f32()), p.z)
        };
        let current = &c.settlement.agents[c.agent as usize];
        let predicted = (base >= c.tick && base < c.tick + MAX_LEAD).then(|| replay(c, base + 1)).flatten();
        let mut events = Vec::new();
        let (from, to, fraction, time, positions) = match predicted {
            Some((states, shown)) => {
                events = shown;
                let positions: Vec<Vec3> = states.iter().map(view).collect();
                let at = |tick: u64| states[(tick - c.tick) as usize].clone();
                (at(base), at(base + 1), fraction, drawn as f32, positions)
            }
            None => {
                // A teleport snaps instead of sliding.
                let alpha = if view(c.previous).distance(view(current)) > SNAP_DISTANCE { 1.0 } else { c.alpha };
                (c.previous.clone(), current.clone(), alpha, c.tick as f32 - 1.0 + c.alpha, Vec::new())
            }
        };
        let is_predicted = !positions.is_empty();
        let position = view(&from).lerp(view(&to), fraction);

        self.offset *= 0.5f32.powf(dt.max(0.0) / OFFSET_HALF_LIFE);
        self.knockback *= 0.5f32.powf(dt.max(0.0) / KNOCKBACK_HALF_LIFE);
        let mut mispredicted = 0.0;
        let mut knockback = 0.0;
        if is_predicted && !jumped {
            // The same ticks as predicted last frame, now.
            let old = |tick: u64| tick.checked_sub(self.first).and_then(|i| self.positions.get(i as usize)).copied();
            let new = |tick: u64| positions[(tick - c.tick) as usize];
            let error = match (old(base), old(base + 1)) {
                (Some(a), Some(b)) => a.lerp(b, fraction) - position,
                (Some(a), None) => a - new(base),
                _ => Vec3::ZERO,
            };
            let error = Vec3::new(error.x, 0.0, error.z);
            // Last frame's replay started from the confirmed tick `first`.
            // If stage 5 touched the avatar on a tick confirmed since, that
            // touch is what the replay couldn't foresee: a knockback. (A late
            // input confirmed on the same frame is counted with it.)
            if c.touched.iter().any(|&tick| tick >= self.first && tick < c.tick) {
                self.knockback += error;
                knockback = error.length();
            } else {
                self.offset += error;
                mispredicted = error.length();
            }
        }
        if let Some((last, was_predicted)) = self.last {
            let switched = was_predicted != is_predicted;
            if switched || jumped {
                // Switching to or from the confirmed avatar is felt like a
                // misprediction; a jump of the drawn time is not one.
                let pull = Vec3::new(last.x - position.x, 0.0, last.z - position.z);
                self.offset += pull;
                if switched {
                    mispredicted += pull.length();
                }
            }
        }
        let snapped = self.offset.length() > SNAP_DISTANCE;
        if snapped {
            self.offset = Vec3::ZERO;
            mispredicted = 0.0;
        }
        let knockback_snapped = self.knockback.length() > KNOCKBACK_SNAP_DISTANCE;
        if knockback_snapped {
            self.knockback = Vec3::ZERO;
            knockback = 0.0;
        }
        self.first = c.tick;
        self.positions = positions;
        self.last = Some((position, is_predicted));
        let offset = self.offset + self.knockback;
        Drawn {
            position: Vec3::new(position.x + offset.x, position.y, position.z + offset.z),
            target: position,
            from,
            to,
            fraction,
            time,
            mispredicted,
            snapped,
            knockback,
            knockback_snapped,
            events,
        }
    }

    /// Moves the drawn time on by `dt` toward just before the lowest recent
    /// horizon. Says whether it jumped there instead.
    fn follow_horizon(&mut self, c: &Confirmed, dt: f32) -> (f64, bool) {
        let ticks = dt as f64 / net::TICK.as_secs_f64();
        self.clock += ticks;
        let seen = c.horizon as f64 - self.clock;
        while self.horizons.front().is_some_and(|&(at, _)| self.clock - at > HORIZON_WINDOW) {
            self.horizons.pop_front();
        }
        while self.horizons.back().is_some_and(|&(_, value)| value >= seen) {
            self.horizons.pop_back();
        }
        self.horizons.push_back((self.clock, seen));
        let lowest = self.horizons.front().map_or(seen, |&(_, value)| value) + self.clock;
        // A command queued this frame asked for last frame's horizon.
        let aim = lowest - ticks - HORIZON_MARGIN;
        let (drawn, jumped) = match self.drawn {
            Some(drawn) if (aim - (drawn + ticks)).abs() <= MAX_DRIFT => {
                let drawn = drawn + ticks;
                let limit = MAX_RATE_CHANGE * ticks;
                (drawn + ((aim - drawn) * (dt as f64 / FOLLOW_SECONDS).min(1.0)).clamp(-limit, limit), false)
            }
            _ => (aim, true),
        };
        // Never before the last confirmed tick.
        let drawn = drawn.max(c.tick as f64 - 1.0);
        self.drawn = Some(drawn);
        (drawn, jumped)
    }
}

/// How long (ticks) `StageFive` remembers a touch: longer than any replay.
const TOUCH_HISTORY: u64 = 2 * MAX_LEAD;

/// The confirmed ticks on which something the avatar's replay never runs
/// changed the local avatar: stage 5 (a hit, a death, a push, a unit moved
/// at a dock). A change of prediction on the frame such a tick is confirmed
/// is a knockback, not a misprediction (D6 in
/// `ABILITY_PREDICTION_HANDOFF.md`).
///
/// No event says so for every case (a laser hit on a player shows none), so
/// each confirmed tick is checked against the replay itself (`touched`):
/// whatever stage 5 comes to do to an avatar, such as a ghoul's push, is
/// caught with no code of its own (see the lockstep test
/// `joined_player_pushed_by_a_ghoul_is_knocked_back_not_mispredicted`). It
/// reads the confirmed ticks the session ran and never changes the world.
#[derive(Default)]
pub struct StageFive {
    /// The (world, agent) watched; anything else starts over.
    key: Option<(u32, u16)>,
    /// The newest confirmed settlement seen, with the tick it runs next and
    /// the terrain's revision then.
    last: Option<(u64, u64, Settlement)>,
    /// Touched ticks, oldest first, for the last `TOUCH_HISTORY` ticks.
    touched: Vec<u64>,
}

impl StageFive {
    /// Recent ticks on which `agent` of `world` was touched, oldest first.
    pub fn touched(&self, world: u32, agent: u16) -> &[u64] {
        if self.key == Some((world, agent)) { &self.touched } else { &[] }
    }

    /// Checks the confirmed ticks a frame ran: `ticks` are the settlements
    /// after each, with the tick each runs next and the terrain's revision
    /// then, oldest first; `terrain` is the newest. A tick is checked only
    /// if the terrain hasn't changed since it ran (the replay needs the
    /// terrain it ran on). `applied` gives the local player's commands that
    /// applied on a tick, in order, or `None` if one of them isn't known;
    /// that tick is not checked either.
    pub fn observe<'s, 'c>(
        &mut self,
        world: u32,
        agent: u16,
        ticks: impl IntoIterator<Item = (u64, u64, &'s Settlement)>,
        terrain: &Terrain,
        applied: impl Fn(u64) -> Option<Vec<&'c CommandKind>>,
    ) {
        if self.key != Some((world, agent)) {
            *self = StageFive { key: Some((world, agent)), ..StageFive::default() };
        }
        let last = self.last.take();
        let mut before = last.as_ref().map(|(next, revision, s)| (*next, *revision, s));
        let mut newest = None;
        for (next, revision, after) in ticks {
            if let Some((tick, _, settlement)) = before.filter(|&(n, r, _)| n + 1 == next && r == terrain.revision) {
                if applied(tick).is_some_and(|commands| touched(settlement, after, agent, tick, terrain, &commands)) {
                    self.touched.push(tick);
                }
            }
            before = Some((next, revision, after));
            newest = Some((next, revision, after));
        }
        self.last = match newest {
            Some((next, revision, settlement)) => Some((next, revision, settlement.clone())),
            None => last,
        };
        let keep = self.last.as_ref().map_or(0, |(next, _, _)| next.saturating_sub(TOUCH_HISTORY));
        self.touched.retain(|&tick| tick >= keep);
    }
}

/// Whether something the replay never runs changed `agent` on `tick`: the
/// confirmed avatar after it isn't what replaying that tick from the
/// confirmed settlement before it, on the `terrain` it ran on, with the
/// `commands` that applied on it, gives. A swing's hit count is left out:
/// the replay assumes a swing hits nothing, and what the avatar hit is
/// nothing done to it. An avatar on a pile job, which walks from pile lab
/// state the replay doesn't run, is never touched.
pub fn touched(before: &Settlement, after: &Settlement, agent: u16, tick: u64, terrain: &Terrain, commands: &[&CommandKind]) -> bool {
    if before.piles.as_ref().and_then(|lab| lab.hauler(agent)).is_some_and(|hauler| hauler.job.is_some()) {
        return false;
    }
    let (Some(mut replay), Some(confirmed)) = (before.replay_avatar(agent, tick, terrain), after.agents.get(agent as usize)) else {
        return false;
    };
    if !replay.step_with(commands.iter().copied()) {
        return false;
    }
    let unhit = |a: &AgentState| {
        let mut a = a.clone();
        if let Some(swing) = a.avatar_mut().and_then(|avatar| avatar.swing.as_mut()) {
            swing.hits = 0;
        }
        a
    };
    unhit(replay.state()) != unhit(confirmed)
}

/// The avatar's states from the confirmed tick through tick `end`, each
/// pending command applied on its tick, and the events it showed on the
/// way. One the host already passed was moved later; the next tick is the
/// best guess. `None` if the avatar can't be predicted (a pending pile job).
/// Walks to a point (the dock walk, scripted walks) are predicted.
fn replay(c: &Confirmed, end: u64) -> Option<(Vec<AgentState>, Vec<PredictedEvent>)> {
    let mut replay = c.settlement.replay_avatar(c.agent, c.tick, c.terrain)?;
    let mut states = vec![replay.state().clone()];
    let mut events = Vec::new();
    for tick in c.tick..end {
        let due = c.pending.iter().filter(|(asked, _)| (*asked).max(c.tick) == tick).map(|(_, kind)| *kind);
        if !replay.step_with(due) {
            return None;
        }
        events.extend(replay.events().iter().map(|event| PredictedEvent { tick, event: event.clone() }));
        states.push(replay.state().clone());
    }
    Some((states, events))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx::Fx;
    use sim::settlement::avatar::Intent;
    use sim::settlement::SceneSetup;

    fn world() -> sim::World {
        let setup = SceneSetup { player: "exosuit".into(), extent: Fx::from_int(8), ..SceneSetup::default() };
        sim::World::new_scene(1, crate::lockstep::builtin_content(), &setup).unwrap()
    }

    fn to_view(p: FxVec2) -> Vec3 {
        Vec3::new(p.x.to_f32(), 0.0, p.y.to_f32())
    }

    #[test]
    fn drawn_time_follows_the_horizon_smoothly_and_new_input_is_not_a_misprediction() {
        let world = world();
        let settlement = world.settlement().unwrap();
        let agent = settlement.player().unwrap();
        let a = &settlement.agents[agent as usize];
        let walk = |x: i32| {
            let intent = if x == 0 { Intent::Stop } else { Intent::Walk { direction: FxVec2::new(Fx::from_int(x), Fx::ZERO), run: true } };
            CommandKind::MoveAvatar { agent, intent }
        };
        let commands = [walk(1), walk(-1), walk(0)];
        let mut prediction = AvatarPrediction::default();
        let dt = 1.0 / 60.0;
        let mut pending = Vec::new();
        let (mut last_horizon, mut last_time, mut moved) = (None, None::<f64>, 0.0f32);
        // The confirmed world stays put, so stay within `MAX_LEAD` of it.
        for frame in 0..36u64 {
            // Packets arrive unevenly: the horizon steps by 0 to 3 ticks.
            let horizon = 20 + frame + [0, 1, 0, 2, 1, 0, 0, 2][frame as usize % 8];
            // The player presses something every few frames. The command
            // asks for the horizon as of last frame's packets.
            if let Some(asked) = last_horizon.filter(|_| frame % 7 == 0) {
                pending.push((asked, &commands[(frame / 7) as usize % 3]));
            }
            let confirmed = Confirmed {
                world: 1, agent, settlement, terrain: world.terrain(), tick: world.tick(), previous: a, alpha: 1.0,
                pending: &pending, horizon, to_view: &to_view, touched: &[],
            };
            let drawn = prediction.update(&confirmed, dt);
            let time = prediction.drawn_time().unwrap();
            if let (Some(asked), Some(last)) = (last_horizon, last_time) {
                assert_eq!(drawn.mispredicted, 0.0, "frame {frame}");
                assert!(time < asked as f64, "frame {frame}: drawn at {time}, a command now asks for {asked}");
                assert!((0.75..=1.25).contains(&(time - last)), "frame {frame}: drawn time moved {}", time - last);
            }
            moved = moved.max((drawn.position.x - to_view(a.body.position).x).abs());
            last_horizon = Some(horizon);
            last_time = Some(time);
        }
        assert!(moved > 0.05, "the walks were predicted: {moved}");
    }

    #[test]
    fn a_changed_prediction_is_drawn_as_a_decaying_offset() {
        let mut world = world();
        let agent = world.settlement().unwrap().player().unwrap();
        let walk = CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ONE, Fx::ZERO), run: true } };
        let pending = [(25, &walk)];
        let mut prediction = AvatarPrediction::default();
        let dt = 1.0 / 60.0;
        let mut draw = |world: &sim::World, pending: &[(u64, &CommandKind)], horizon: u64| {
            let settlement = world.settlement().unwrap();
            let confirmed = Confirmed {
                world: 1, agent, settlement, terrain: world.terrain(), tick: world.tick(),
                previous: &settlement.agents[agent as usize], alpha: 1.0, pending, horizon, to_view: &to_view, touched: &[],
            };
            prediction.update(&confirmed, dt)
        };
        for horizon in 30..40 {
            draw(&world, &pending, horizon);
        }
        // The host applied the walk 3 ticks late: the confirmed avatar is behind.
        for _ in 0..28 {
            world.step(&[]);
        }
        world.step(&[sim::Command { player: sim::PlayerId(0), kind: walk.clone() }]);
        let late = draw(&world, &[], 40);
        assert!(late.mispredicted > 0.0 && !late.snapped);
        let shown = late.position.x - (to_view(late.from.body.position).x).lerp(to_view(late.to.body.position).x, late.fraction);
        assert!(shown > 0.01, "the old prediction is still drawn: offset {shown}");
        let next = draw(&world, &[], 41);
        let decayed = next.position.x - (to_view(next.from.body.position).x).lerp(to_view(next.to.body.position).x, next.fraction);
        assert_eq!(next.mispredicted, 0.0);
        assert!((decayed - shown * 0.5f32.powf(dt / OFFSET_HALF_LIFE)).abs() < 1e-4, "{decayed} after {shown}");
    }

    /// Draws a standing avatar for a few frames, then runs a confirmed
    /// tick on which the avatar is pushed `push` units along x, as stage 5
    /// would, and draws the frame that confirms it and the one after.
    /// `seen` says whether `StageFive` saw the push.
    fn pushed(push: i32, seen: bool) -> (Drawn, Drawn) {
        let mut world = world();
        let agent = world.settlement().unwrap().player().unwrap();
        let mut prediction = AvatarPrediction::default();
        let dt = 1.0 / 60.0;
        let mut draw = |settlement: &Settlement, terrain: &Terrain, tick: u64, touched: &[u64], horizon: u64| {
            let confirmed = Confirmed {
                world: 1, agent, settlement, terrain, tick, previous: &settlement.agents[agent as usize], alpha: 1.0,
                pending: &[], horizon, to_view: &to_view, touched,
            };
            prediction.update(&confirmed, dt)
        };
        for horizon in 30..36 {
            draw(world.settlement().unwrap(), world.terrain(), world.tick(), &[], horizon);
        }
        world.step(&[]);
        let mut settlement = world.settlement().unwrap().clone();
        settlement.agents[agent as usize].body.position.x += Fx::from_int(push);
        let touched = if seen { vec![world.tick() - 1] } else { Vec::new() };
        let hit = draw(&settlement, world.terrain(), world.tick(), &touched, 36);
        let next = draw(&settlement, world.terrain(), world.tick(), &touched, 37);
        (hit, next)
    }

    /// How far the drawn avatar is from where it is predicted, along x.
    fn offset(drawn: &Drawn) -> f32 {
        drawn.position.x - drawn.target.x
    }

    #[test]
    fn a_push_from_stage_5_is_a_smoothed_knockback_not_a_misprediction() {
        let dt = 1.0 / 60.0;
        // Past a misprediction's snap distance, but not a knockback's.
        let (hit, next) = pushed(2, true);
        assert_eq!((hit.mispredicted, hit.snapped, hit.knockback_snapped), (0.0, false, false));
        assert!((hit.knockback - 2.0).abs() < 1e-3, "knocked back {}", hit.knockback);
        assert!((offset(&hit) + 2.0).abs() < 1e-3, "drawn where it was: {}", offset(&hit));
        assert_eq!((next.knockback, next.mispredicted), (0.0, 0.0), "counted once");
        let decayed = offset(&next);
        assert!((decayed - offset(&hit) * 0.5f32.powf(dt / KNOCKBACK_HALF_LIFE)).abs() < 1e-4, "{decayed} after {}", offset(&hit));
        assert!(0.5f32.powf(dt / KNOCKBACK_HALF_LIFE) > 0.5f32.powf(dt / OFFSET_HALF_LIFE), "slower than a misprediction");

        // The same change with no stage 5 touch is a misprediction, and snaps.
        let (unseen, _) = pushed(2, false);
        assert_eq!((unseen.knockback, unseen.knockback_snapped, unseen.snapped), (0.0, false, true));
        let (small, _) = pushed(1, false);
        assert!((small.mispredicted - 1.0).abs() < 1e-3 && small.knockback == 0.0);

        // A knockback past its own snap distance snaps.
        let (far, _) = pushed(4, true);
        assert_eq!((far.knockback, far.knockback_snapped, far.snapped, far.mispredicted), (0.0, true, false, 0.0));
        assert!(offset(&far).abs() < 1e-4);
    }

    #[test]
    fn stage_five_sees_what_the_replay_never_runs_and_not_late_inputs() {
        let mut world = world();
        let agent = world.settlement().unwrap().player().unwrap();
        let walk = CommandKind::MoveAvatar { agent, intent: Intent::Walk { direction: FxVec2::new(Fx::ONE, Fx::ZERO), run: true } };
        let mut stage_five = StageFive::default();
        let observe = |stage_five: &mut StageFive, world: &sim::World, settlement: &Settlement, applied: Option<&CommandKind>| {
            let ticks = [(world.tick(), world.terrain().revision, settlement)];
            stage_five.observe(1, agent, ticks, world.terrain(), |_| Some(applied.into_iter().collect()));
            stage_five.touched(1, agent).to_vec()
        };
        observe(&mut stage_five, &world, &world.settlement().unwrap().clone(), None);
        for _ in 0..5 {
            world.step(&[]);
            assert!(observe(&mut stage_five, &world, &world.settlement().unwrap().clone(), None).is_empty());
        }
        // A walk the host moved later: on the tick it really applied on.
        world.step(&[sim::Command { player: sim::PlayerId(0), kind: walk.clone() }]);
        assert!(observe(&mut stage_five, &world, &world.settlement().unwrap().clone(), Some(&walk)).is_empty(), "a command is no touch");
        world.step(&[]);
        assert!(observe(&mut stage_five, &world, &world.settlement().unwrap().clone(), None).is_empty(), "walking is no touch");
        // A hit, as stage 5 applies one.
        world.step(&[]);
        let mut hit = world.settlement().unwrap().clone();
        hit.agents[agent as usize].body.health += 1;
        assert_eq!(observe(&mut stage_five, &world, &hit, None), [world.tick() - 1]);
        assert!(stage_five.touched(2, agent).is_empty(), "another world's");
        // The next tick starts from the hit settlement.
        world.step(&[]);
        let mut walked = world.settlement().unwrap().clone();
        walked.agents[agent as usize].body.health += 1;
        assert_eq!(observe(&mut stage_five, &world, &walked, None), [world.tick() - 2], "only the hit tick");

        // A tick run on terrain that changed since isn't checked: the replay
        // would run it on other ground.
        let mut stage_five = StageFive::default();
        let before = world.settlement().unwrap().clone();
        stage_five.observe(1, agent, [(world.tick(), world.terrain().revision, &before)], world.terrain(), |_| Some(Vec::new()));
        world.step(&[]);
        let mut moved = world.settlement().unwrap().clone();
        moved.agents[agent as usize].body.position.y += Fx::ONE;
        let mut terrain = world.terrain().clone();
        terrain.revision += 1;
        stage_five.observe(1, agent, [(world.tick(), terrain.revision, &moved)], &terrain, |_| Some(Vec::new()));
        assert!(stage_five.touched(1, agent).is_empty());
        // Nor is one whose commands aren't all known.
        world.step(&[]);
        let moved = { let mut s = world.settlement().unwrap().clone(); s.agents[agent as usize].body.position.y += Fx::from_int(3); s };
        stage_five.observe(1, agent, [(world.tick(), terrain.revision, &moved)], &terrain, |_| None);
        assert!(stage_five.touched(1, agent).is_empty());
    }

    /// A death is a stage 5 touch, but the respawn that follows it is the
    /// avatar's own step: the replay draws it on the tick it happens, with
    /// no correction of any kind.
    #[test]
    fn the_respawn_after_a_death_is_predicted() {
        let world = world();
        let agent = world.settlement().unwrap().player().unwrap();
        let mut dead = world.settlement().unwrap().clone();
        let a = &mut dead.agents[agent as usize];
        let home = a.body.home;
        a.body.position = home + FxVec2::new(Fx::from_int(4), Fx::ZERO);
        (a.body.max_health, a.body.health) = (6, 0);
        a.avatar_mut().unwrap().respawn_at = world.tick() + 12;
        let mut prediction = AvatarPrediction::default();
        let mut respawned = None;
        for horizon in 6..30u64 {
            let confirmed = Confirmed {
                world: 1, agent, settlement: &dead, terrain: world.terrain(), tick: world.tick(),
                previous: &dead.agents[agent as usize], alpha: 1.0, pending: &[], horizon, to_view: &to_view, touched: &[],
            };
            let drawn = prediction.update(&confirmed, 1.0 / 60.0);
            assert_eq!((drawn.mispredicted, drawn.snapped, drawn.knockback, drawn.knockback_snapped), (0.0, false, 0.0, false), "horizon {horizon}");
            if drawn.to.body.position == home && respawned.is_none() {
                respawned = Some(horizon);
            }
        }
        assert!(respawned.is_some(), "the replay respawned it");
    }
}
