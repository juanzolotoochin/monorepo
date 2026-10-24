//! Input logs: the turns since a save. A save plus its input log reproduces
//! every later tick exactly, which is what replays, joins and the
//! determinism tests rely on.
//!
//! Layout: `"GLOG"`, format version (u32, little endian), postcard(InputLog).

use serde::{Deserialize, Serialize};
use sim::Command;

use crate::LoadError;

pub const INPUT_FORMAT_VERSION: u32 = 1;
const MAGIC: &[u8; 4] = b"GLOG";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputLog {
    /// The tick of the first turn: the save's tick.
    pub start_tick: u64,
    /// `turns[i]` holds the commands for tick `start_tick + i`.
    pub turns: Vec<Vec<Command>>,
}

impl InputLog {
    pub fn new(start_tick: u64) -> InputLog {
        InputLog { start_tick, turns: Vec::new() }
    }

    /// The commands for `tick`, or none if the log doesn't cover it.
    pub fn turn(&self, tick: u64) -> &[Command] {
        tick.checked_sub(self.start_tick)
            .and_then(|i| self.turns.get(i as usize))
            .map_or(&[], Vec::as_slice)
    }

    /// The tick after the last recorded turn.
    pub fn end_tick(&self) -> u64 {
        self.start_tick + self.turns.len() as u64
    }

    pub fn push(&mut self, turn: Vec<Command>) {
        self.turns.push(turn);
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&INPUT_FORMAT_VERSION.to_le_bytes());
        out.extend(postcard::to_allocvec(self).expect("input logs always encode"));
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<InputLog, LoadError> {
        if bytes.len() < 8 || &bytes[..4] != MAGIC {
            return Err(LoadError::NotASave);
        }
        let found = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if found != INPUT_FORMAT_VERSION {
            return Err(LoadError::FormatVersion { found, expected: INPUT_FORMAT_VERSION });
        }
        postcard::from_bytes(&bytes[8..]).map_err(crate::corrupt)
    }
}
