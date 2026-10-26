//! The memory trial: record a few seconds of movement, then a ghost replays
//! it while the player does something else. Two pressure pads must be held at
//! once to charge a door; behind it is a core to collect. Ported from the
//! original Bevy app (`palimpsest/`).

use content::{millis_to_ticks, TICKS_PER_SECOND};
use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use super::avatar::Intent;
use super::Settlement;
use crate::nav;
use crate::terrain::{Disc, Terrain};

/// Recordings stop at 8 seconds.
pub const LIMIT_TICKS: u64 = millis_to_ticks(8000);
/// A recording must be at least 0.15 s long to deploy.
const MIN_TICKS: u64 = millis_to_ticks(150);
/// Pads charge the door over 1.2 s and drain twice as fast.
const CHARGE_PER_TICK: Fx = Fx::from_ratio(5, 6 * TICKS_PER_SECOND as i64); // 1 / 1.2 s
const DRAIN_PER_TICK: Fx = Fx::from_ratio(5, 3 * TICKS_PER_SECOND as i64); // 1 / 0.6 s

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sample {
    pub position: FxVec2,
    pub height: Fx,
    pub heading: Fx,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memory {
    /// Whether this scene has the pads, door and core.
    pub trial: bool,
    /// The arena center the pads and core are placed around.
    pub center: FxVec2,
    pub pads: Vec<FxVec2>,
    pub core: FxVec2,
    /// Obstacles that open with the door.
    pub doors: Vec<Disc>,
    pub samples: Vec<Sample>,
    pub recording: bool,
    /// Ticks recorded, and ticks of the recording replayed so far.
    pub elapsed: u64,
    pub replay: u64,
    pub ghost: Option<Sample>,
    pub pressed: Vec<bool>,
    pub charge: Fx,
    pub unlocked: bool,
    pub collected: bool,
}

impl Memory {
    pub fn new(trial: bool, center: FxVec2, doors: Vec<Disc>) -> Memory {
        let at = |x: i32, y: i32| FxVec2::new(center.x + Fx::from_int(x), center.y + Fx::from_int(y));
        Memory {
            trial,
            center,
            pads: vec![at(-4, -2), at(-1, 3)],
            core: at(5, 0),
            doors,
            samples: Vec::new(),
            recording: false,
            elapsed: 0,
            replay: 0,
            ghost: None,
            pressed: vec![false; 2],
            charge: Fx::ZERO,
            unlocked: false,
            collected: false,
        }
    }
}

impl Settlement {
    /// A player respawned (`RestartMemory`): the trial starts over, closing
    /// its door again if it was open.
    pub(super) fn restart_memory(&mut self, terrain: &mut Terrain) {
        if let Some(m) = self.memory.as_mut() {
            if m.unlocked {
                terrain.obstacles.extend_from_slice(&m.doors);
                terrain.revision += 1;
            }
            *m = Memory::new(m.trial, m.center, std::mem::take(&mut m.doors));
        }
    }

    fn avatar_sample(&self) -> Option<Sample> {
        let a = &self.agents[self.player()? as usize];
        Some(Sample { position: a.position, height: a.avatar.height, heading: a.heading })
    }

    /// Commands: starts recording the player's movement.
    pub fn memory_record(&mut self) {
        let Some(sample) = self.avatar_sample() else { return };
        let Some(m) = self.memory.as_mut() else { return };
        if m.recording {
            return;
        }
        m.samples = vec![sample];
        m.recording = true;
        m.elapsed = 0;
        m.replay = 0;
        m.ghost = None;
    }

    /// Commands: sends the player back to where the recording began; the
    /// ghost replays it from there.
    pub fn memory_deploy(&mut self) {
        let Some(player) = self.player() else { return };
        let Some(m) = self.memory.as_mut() else { return };
        if !m.recording || m.elapsed < MIN_TICKS {
            return;
        }
        let anchor = m.samples[0];
        m.recording = false;
        m.replay = 0;
        m.ghost = Some(anchor);
        let a = &mut self.agents[player as usize];
        a.position = anchor.position;
        a.avatar.height = anchor.height;
        a.heading = anchor.heading;
        a.route.clear();
        a.avatar.velocity = FxVec2::ZERO;
        a.avatar.intent = Some(Intent::Stop);
        a.avatar.swing = None;
        a.avatar.queued = None;
        a.avatar.queued_at = None;
        if let Some(hauler) = self.piles.as_mut().and_then(|lab| lab.haulers.get_mut(&player)) {
            hauler.job = None;
        }
    }

    /// Commands: forgets the recording and the ghost.
    pub fn memory_clear(&mut self) {
        if let Some(m) = self.memory.as_mut() {
            m.recording = false;
            m.samples.clear();
            m.ghost = None;
            m.elapsed = 0;
            m.replay = 0;
        }
    }

    /// Housekeeping: record or replay, then pads, door and core.
    pub(crate) fn memory_housekeeping(&mut self, terrain: &mut Terrain) {
        let Some(sample) = self.avatar_sample() else { return };
        let Some(m) = self.memory.as_mut() else { return };
        if m.recording {
            m.elapsed = (m.elapsed + 1).min(LIMIT_TICKS);
            m.samples.push(sample);
        } else if m.ghost.is_some() {
            m.replay = (m.replay + 1).min(m.elapsed);
            m.ghost = Some(m.samples[m.replay as usize]);
        }
        if m.trial {
            let on = |s: &Sample, pad: FxVec2| nav::distance(s.position, pad) < Fx::from_ratio(31, 50) && s.height.abs() < Fx::from_ratio(1, 4);
            for i in 0..m.pads.len() {
                m.pressed[i] = on(&sample, m.pads[i]) || m.ghost.as_ref().is_some_and(|g| on(g, m.pads[i]));
            }
            if !m.unlocked {
                m.charge = if m.pressed.iter().all(|&p| p) {
                    (m.charge + CHARGE_PER_TICK).min(Fx::ONE)
                } else {
                    (m.charge - DRAIN_PER_TICK).max(Fx::ZERO)
                };
                if m.charge >= Fx::ONE {
                    m.unlocked = true;
                    terrain.obstacles.retain(|o| !m.doors.contains(o));
                    terrain.revision += 1;
                }
            }
            if m.unlocked && !m.collected && nav::distance(sample.position, m.core) < Fx::from_ratio(4, 5) && sample.height < Fx::ONE {
                m.collected = true;
            }
        }
        if m.recording && m.elapsed >= LIMIT_TICKS {
            self.memory_deploy();
        }
    }
}
