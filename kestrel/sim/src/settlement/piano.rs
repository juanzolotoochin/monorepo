//! Shared piano performance state. A MIDI device is merely one producer of
//! note commands; the settlement validates reach and owns the held notes.

use fx::Fx;
use serde::{Deserialize, Serialize};

use super::{AgentIx, Settlement};

const REACH: Fx = Fx::from_ratio(14, 5);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PianoNote {
    pub agent: AgentIx,
    pub piano: u16,
    pub pitch: u8,
    pub velocity: u8,
    /// Changes on every press, including a repeated press of the same key.
    pub serial: u64,
}

impl Settlement {
    pub fn play_piano(&mut self, agent: usize, piano: u16, pitch: u8, velocity: u8) {
        if pitch >= 128 || velocity > 127 || usize::from(piano) >= self.pianos.len() {
            return;
        }
        let Some(player) = self.agents.get(agent).filter(|a| a.avatar().is_some()) else { return };
        let key = (agent as AgentIx, piano, pitch);
        if velocity == 0 {
            self.piano_notes.retain(|note| (note.agent, note.piano, note.pitch) != key);
            return;
        }
        if !player.body.alive()
            || (player.body.position - self.pianos[usize::from(piano)]).length_squared() >= REACH * REACH {
            return;
        }
        self.piano_serial = self.piano_serial.wrapping_add(1);
        self.piano_notes.retain(|note| (note.agent, note.piano, note.pitch) != key);
        self.piano_notes.push(PianoNote { agent: agent as AgentIx, piano, pitch, velocity, serial: self.piano_serial });
    }

    pub(crate) fn release_distant_piano_notes(&mut self) {
        self.piano_notes.retain(|note| {
            self.agents.get(note.agent as usize).is_some_and(|agent| {
                agent.avatar().is_some()
                    && agent.body.alive()
                    && self.pianos.get(usize::from(note.piano)).is_some_and(|&position|
                        (agent.body.position - position).length_squared() < REACH * REACH)
            })
        });
    }
}
