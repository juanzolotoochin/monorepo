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
use sim::settlement::{AgentState, ReplayedStrike, Settlement};
use sim::terrain::Terrain;
use sim::CommandKind;

/// Further ahead than this from the confirmed tick, the confirmed avatar is drawn.
pub const MAX_LEAD: u64 = 60;
/// The visual offset left by a misprediction halves this often (seconds).
const OFFSET_HALF_LIFE: f32 = 0.05;
/// An offset larger than this (units) snaps instead.
const SNAP_DISTANCE: f32 = 1.5;
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
    /// How far a change of prediction moved the drawn avatar this frame.
    pub mispredicted: f32,
    /// The avatar was too far off and was put where it is predicted.
    pub snapped: bool,
    /// The strikes the predicted avatar resolved from the confirmed tick
    /// through the drawn tick, oldest first: the ticks the drawn time
    /// reached that the confirmed world hasn't run yet.
    pub strikes: Vec<ReplayedStrike>,
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
            let p = (c.to_view)(a.position);
            Vec3::new(p.x, a.avatar.height.to_f32(), p.z)
        };
        let current = &c.settlement.agents[c.agent as usize];
        let predicted = (base >= c.tick && base < c.tick + MAX_LEAD).then(|| replay(c, base + 1)).flatten();
        let mut strikes = Vec::new();
        let (from, to, fraction, positions) = match predicted {
            Some((states, struck)) => {
                strikes = struck;
                let positions: Vec<Vec3> = states.iter().map(view).collect();
                let at = |tick: u64| states[(tick - c.tick) as usize].clone();
                (at(base), at(base + 1), fraction, positions)
            }
            None => {
                // A teleport snaps instead of sliding.
                let alpha = if view(c.previous).distance(view(current)) > SNAP_DISTANCE { 1.0 } else { c.alpha };
                (c.previous.clone(), current.clone(), alpha, Vec::new())
            }
        };
        let is_predicted = !positions.is_empty();
        let position = view(&from).lerp(view(&to), fraction);

        self.offset *= 0.5f32.powf(dt.max(0.0) / OFFSET_HALF_LIFE);
        let mut mispredicted = 0.0;
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
            self.offset += error;
            mispredicted = error.length();
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
        self.first = c.tick;
        self.positions = positions;
        self.last = Some((position, is_predicted));
        Drawn {
            position: Vec3::new(position.x + self.offset.x, position.y, position.z + self.offset.z),
            target: position,
            from,
            to,
            fraction,
            mispredicted,
            snapped,
            strikes,
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

/// The avatar's states from the confirmed tick through tick `end`, each
/// pending command applied on its tick, and the strikes it resolved on the
/// way. One the host already passed was moved later; the next tick is the
/// best guess. `None` if the avatar can't be predicted (a pending pile job).
/// Walks to a point (the dock walk, scripted walks) are predicted.
fn replay(c: &Confirmed, end: u64) -> Option<(Vec<AgentState>, Vec<ReplayedStrike>)> {
    let mut replay = c.settlement.replay_avatar(c.agent, c.tick, c.terrain)?;
    let mut states = vec![replay.state().clone()];
    let mut strikes = Vec::new();
    for tick in c.tick..end {
        let due = c.pending.iter().filter(|(asked, _)| (*asked).max(c.tick) == tick).map(|(_, kind)| *kind);
        if !replay.step_with(due) {
            return None;
        }
        states.push(replay.state().clone());
        strikes.extend(replay.struck().cloned());
    }
    Some((states, strikes))
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
                pending: &pending, horizon, to_view: &to_view,
            };
            let drawn = prediction.update(&confirmed, dt);
            let time = prediction.drawn_time().unwrap();
            if let (Some(asked), Some(last)) = (last_horizon, last_time) {
                assert_eq!(drawn.mispredicted, 0.0, "frame {frame}");
                assert!(time < asked as f64, "frame {frame}: drawn at {time}, a command now asks for {asked}");
                assert!((0.75..=1.25).contains(&(time - last)), "frame {frame}: drawn time moved {}", time - last);
            }
            moved = moved.max((drawn.position.x - to_view(a.position).x).abs());
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
                previous: &settlement.agents[agent as usize], alpha: 1.0, pending, horizon, to_view: &to_view,
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
        let shown = late.position.x - (to_view(late.from.position).x).lerp(to_view(late.to.position).x, late.fraction);
        assert!(shown > 0.01, "the old prediction is still drawn: offset {shown}");
        let next = draw(&world, &[], 41);
        let decayed = next.position.x - (to_view(next.from.position).x).lerp(to_view(next.to.position).x, next.fraction);
        assert_eq!(next.mispredicted, 0.0);
        assert!((decayed - shown * 0.5f32.powf(dt / OFFSET_HALF_LIFE)).abs() < 1e-4, "{decayed} after {shown}");
    }
}
