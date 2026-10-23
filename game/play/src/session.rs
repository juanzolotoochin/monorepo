//! A single-player session: the simulation plus the turn path. Inputs are
//! scheduled `input_delay` ticks ahead exactly as in a networked game, so
//! single-player exercises the same path; the network session replaces the
//! local turn buffer with turns from the host.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use bevy::prelude::Resource;
use content::Content;
use save::LoadError;
use sim::{Command, CommandKind, PlayerId, Simulation, World, WorldConfig};
use view::Extract;

/// The simulation rate.
pub const TICK: Duration = Duration::from_nanos(1_000_000_000 / content::TICKS_PER_SECOND as u64);
/// Steps per frame at most; after a long stall the simulation slows down
/// instead of freezing the frame to catch up.
const MAX_STEPS_PER_FRAME: u32 = 4;
const LOCAL_PLAYER: PlayerId = PlayerId(0);

#[derive(Resource)]
pub struct Session {
    sim: Simulation,
    threads: usize,
    input_delay: u64,
    /// Commands by the tick they are scheduled for.
    scheduled: BTreeMap<u64, Vec<Command>>,
    accumulator: Duration,
    last_step_ms: f64,
}

impl Session {
    /// A new game is a save at tick 0, loaded through the normal load path.
    pub fn new_game(config: WorldConfig, content: &Content, threads: usize, input_delay: u64) -> Session {
        let fresh = Simulation::new(config, content.rules.clone(), 1);
        let bytes = save::write(fresh.world(), content);
        let world = save::read(&bytes, content).expect("a fresh save loads");
        Session {
            sim: Simulation::from_world(world, threads),
            threads,
            input_delay,
            scheduled: BTreeMap::new(),
            accumulator: Duration::ZERO,
            last_step_ms: 0.0,
        }
    }

    pub fn world(&self) -> &World {
        self.sim.world()
    }

    pub fn input_delay(&self) -> u64 {
        self.input_delay
    }

    pub fn last_step_ms(&self) -> f64 {
        self.last_step_ms
    }

    /// Schedules a local command for `input_delay` ticks from now.
    pub fn queue(&mut self, kind: CommandKind) {
        let tick = self.sim.world().tick() + self.input_delay;
        self.scheduled.entry(tick).or_default().push(Command { player: LOCAL_PLAYER, kind });
    }

    /// Advances real time by `delta`, running every tick that is due, and
    /// returns the extract of each tick run.
    pub fn advance(&mut self, delta: Duration) -> Vec<Extract> {
        self.accumulator += delta;
        let mut extracts = Vec::new();
        let mut steps = 0;
        while self.accumulator >= TICK {
            if steps == MAX_STEPS_PER_FRAME {
                self.accumulator = Duration::ZERO;
                break;
            }
            self.accumulator -= TICK;
            steps += 1;
            let tick = self.sim.world().tick();
            let turn = self.scheduled.remove(&tick).unwrap_or_default();
            let start = Instant::now();
            self.sim.step(&turn);
            self.last_step_ms = start.elapsed().as_secs_f64() * 1000.0;
            extracts.push(Extract::from_world(self.sim.world()));
        }
        extracts
    }

    /// How far render time is between the last two ticks.
    pub fn alpha(&self) -> f32 {
        self.accumulator.as_secs_f32() / TICK.as_secs_f32()
    }

    pub fn save(&self, content: &Content) -> Vec<u8> {
        save::write(self.sim.world(), content)
    }

    /// Replaces the world with a save. Scheduled commands are dropped.
    pub fn load(&mut self, bytes: &[u8], content: &Content) -> Result<Extract, LoadError> {
        let world = save::read(bytes, content)?;
        self.sim = Simulation::from_world(world, self.threads);
        self.scheduled.clear();
        self.accumulator = Duration::ZERO;
        Ok(Extract::from_world(self.sim.world()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx::{Fx, FxVec2};

    fn session() -> Session {
        Session::new_game(sim::scenario::Scenario::small().config, &Content::builtin(), 2, 3)
    }

    #[test]
    fn commands_apply_after_the_input_delay() {
        let mut session = session();
        session.queue(CommandKind::SpawnMovers { center: FxVec2::new(Fx::from_int(40), Fx::from_int(40)), count: 5 });
        // Ticks 0, 1 and 2 run before the command's tick.
        session.advance(TICK * 3);
        assert_eq!(session.world().mover_count(), 0);
        // Spawns are inserted in housekeeping of the tick they apply on.
        session.advance(TICK);
        assert_eq!(session.world().mover_count(), 5);
    }

    #[test]
    fn frames_run_whole_ticks_and_report_alpha() {
        let mut session = session();
        assert_eq!(session.advance(TICK / 2).len(), 0);
        assert!((session.alpha() - 0.5).abs() < 0.01);
        assert_eq!(session.advance(TICK).len(), 1);
        assert_eq!(session.advance(TICK * 100).len(), MAX_STEPS_PER_FRAME as usize);
        assert_eq!(session.alpha(), 0.0);
    }

    #[test]
    fn quickload_restores_the_saved_tick() {
        let content = Content::builtin();
        let mut session = session();
        session.advance(TICK * 4);
        let bytes = session.save(&content);
        session.advance(TICK * 4);
        session.load(&bytes, &content).unwrap();
        assert_eq!(session.world().tick(), 4);
    }
}
