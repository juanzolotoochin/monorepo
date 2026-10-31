//! Input logs: the turns since a save. A save plus its input log reproduces
//! every later tick exactly, which is what replays, joins and the
//! determinism tests rely on.
//!
//! Layout: `"GLOG"`, format version (u32, little endian), postcard(InputLog).
//!
//! Versions:
//! - 1: the command vocabulary up to `CommandKind::DigDown`.
//! - 2: adds `CommandKind::ReleaseDirectControl`, appended after every
//!   version-1 variant. Version 1 logs still load: postcard names variants
//!   by position, so their bytes decode the same way, and a version-1 log
//!   that claims a newer command is refused as corrupt.
//! - 3: adds `PileAction::DropMineral` (a drop that names its mineral),
//!   appended after every older pile action, so older logs decode the same
//!   way; one that claims it is refused as corrupt.
//! - 4: adds `CommandKind::SetSpriteJob`, appended after every older
//!   command; an older log that claims it is refused as corrupt.
//! - 5: adds `CommandKind::Bundle`, appended likewise.
//! - 6: adds `CommandKind::Interact`, appended likewise.
//! - 7: adds `CommandKind::PlaceBlueprint`, appended likewise, and
//!   `InteractTarget::Site`, appended after the older targets; an older log
//!   that claims either is refused as corrupt.
//! - 8: adds `CommandKind::Drop`, appended likewise, and
//!   `InteractTarget::Pile`, appended after the older targets; an older log
//!   that claims either is refused as corrupt.

use serde::{Deserialize, Serialize};
use sim::settlement::piles::PileAction;
use sim::{Command, CommandKind};

use crate::LoadError;

pub const INPUT_FORMAT_VERSION: u32 = 8;
/// The oldest input log this build reads.
pub const OLDEST_INPUT_FORMAT_VERSION: u32 = 1;
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
        if !(OLDEST_INPUT_FORMAT_VERSION..=INPUT_FORMAT_VERSION).contains(&found) {
            return Err(LoadError::FormatVersion { found, expected: INPUT_FORMAT_VERSION });
        }
        let log: InputLog = postcard::from_bytes(&bytes[8..]).map_err(crate::corrupt)?;
        if found < 2 && log.turns.iter().flatten().any(|c| matches!(c.kind, CommandKind::ReleaseDirectControl { .. })) {
            return Err(crate::corrupt("a version 1 input log holds a command added in version 2"));
        }
        if found < 3 && log.turns.iter().flatten().any(|c| matches!(c.kind, CommandKind::Pile { action: PileAction::DropMineral(..), .. })) {
            return Err(crate::corrupt(format!("a version {found} input log holds a pile action added in version 3")));
        }
        if found < 4 && log.turns.iter().flatten().any(|c| matches!(c.kind, CommandKind::SetSpriteJob { .. })) {
            return Err(crate::corrupt(format!("a version {found} input log holds a command added in version 4")));
        }
        if found < 5 && log.turns.iter().flatten().any(|c| matches!(c.kind, CommandKind::Bundle { .. })) {
            return Err(crate::corrupt(format!("a version {found} input log holds a command added in version 5")));
        }
        if found < 6 && log.turns.iter().flatten().any(|c| matches!(c.kind, CommandKind::Interact { .. })) {
            return Err(crate::corrupt(format!("a version {found} input log holds a command added in version 6")));
        }
        let added_in_7 = |kind: &CommandKind| {
            matches!(kind, CommandKind::PlaceBlueprint { .. } | CommandKind::Interact { target: sim::settlement::InteractTarget::Site(_), .. })
        };
        if found < 7 && log.turns.iter().flatten().any(|c| added_in_7(&c.kind)) {
            return Err(crate::corrupt(format!("a version {found} input log holds a command added in version 7")));
        }
        let added_in_8 = |kind: &CommandKind| {
            matches!(kind, CommandKind::Drop { .. } | CommandKind::Interact { target: sim::settlement::InteractTarget::Pile(_), .. })
        };
        if found < 8 && log.turns.iter().flatten().any(|c| added_in_8(&c.kind)) {
            return Err(crate::corrupt(format!("a version {found} input log holds a command added in version 8")));
        }
        Ok(log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx::{Fx, FxVec2};
    use sim::settlement::avatar::Intent;
    use sim::PlayerId;

    fn log(kinds: Vec<CommandKind>) -> InputLog {
        let mut log = InputLog::new(3);
        log.push(kinds.into_iter().map(|kind| Command { player: PlayerId(0), kind }).collect());
        log
    }

    fn with_version(mut bytes: Vec<u8>, version: u32) -> Vec<u8> {
        bytes[4..8].copy_from_slice(&version.to_le_bytes());
        bytes
    }

    #[test]
    fn release_round_trips_in_the_current_version() {
        let written = log(vec![CommandKind::ReleaseDirectControl { agent: 2 }]);
        let bytes = written.to_bytes();
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), INPUT_FORMAT_VERSION);
        assert_eq!(InputLog::from_bytes(&bytes).unwrap(), written);
    }

    fn pile(action: PileAction) -> CommandKind {
        CommandKind::Pile { agent: 0, action, pace: Fx::ZERO, approach: None }
    }

    #[test]
    fn a_named_drop_round_trips_in_the_current_version() {
        let written = log(vec![pile(PileAction::DropMineral(FxVec2::new(Fx::ONE, Fx::ZERO), 2))]);
        assert_eq!(InputLog::from_bytes(&written.to_bytes()).unwrap(), written);
    }

    /// A version-2 log, with the drop and delivery it knew, loads unchanged;
    /// one that claims a named drop is refused.
    #[test]
    fn version_two_logs_still_load_without_named_drops() {
        let at = FxVec2::new(Fx::ONE, Fx::ZERO);
        let written = log(vec![pile(PileAction::Drop(at)), pile(PileAction::Deliver), CommandKind::ReleaseDirectControl { agent: 1 }]);
        assert_eq!(InputLog::from_bytes(&with_version(written.to_bytes(), 2)).unwrap(), written);
        let named = with_version(log(vec![pile(PileAction::DropMineral(at, 0))]).to_bytes(), 2);
        assert!(matches!(InputLog::from_bytes(&named), Err(LoadError::Corrupt(_))));
    }

    /// A version-1 log is the same bytes under an older header: it loads
    /// unchanged.
    #[test]
    fn version_one_logs_still_load() {
        let walk = Intent::Walk { direction: FxVec2::new(Fx::ONE, Fx::ZERO), run: true };
        let written = log(vec![
            CommandKind::MoveAvatar { agent: 0, intent: walk },
            CommandKind::Jetpack { agent: 0, enabled: false },
            CommandKind::DigDown { agent: 0 },
        ]);
        let bytes = with_version(written.to_bytes(), 1);
        assert_eq!(InputLog::from_bytes(&bytes).unwrap(), written);
    }

    /// Bytes a version-1 build wrote, frozen: start tick 3, one turn with
    /// player 0's `Jetpack { agent: 0, enabled: true }` (variant 12) and
    /// `DigDown { agent: 0 }` (variant 23). Reordering the vocabulary breaks
    /// this.
    #[test]
    fn frozen_version_one_bytes_decode_to_the_same_commands() {
        let bytes = [b'G', b'L', b'O', b'G', 1, 0, 0, 0, 3, 1, 2, 0, 12, 0, 1, 0, 23, 0];
        let expected = log(vec![CommandKind::Jetpack { agent: 0, enabled: true }, CommandKind::DigDown { agent: 0 }]);
        assert_eq!(InputLog::from_bytes(&bytes).unwrap(), expected);
    }

    #[test]
    fn a_version_one_log_with_a_newer_command_is_refused() {
        let bytes = with_version(log(vec![CommandKind::ReleaseDirectControl { agent: 0 }]).to_bytes(), 1);
        assert!(matches!(InputLog::from_bytes(&bytes), Err(LoadError::Corrupt(_))));
    }

    #[test]
    fn a_version_three_log_with_a_sprite_job_is_refused() {
        let kind = CommandKind::SetSpriteJob { agent: 1, job: sim::settlement::SpriteJob::Staff(0) };
        let written = log(vec![kind]);
        assert_eq!(InputLog::from_bytes(&written.to_bytes()).unwrap(), written);
        let bytes = with_version(written.to_bytes(), 3);
        assert!(matches!(InputLog::from_bytes(&bytes), Err(LoadError::Corrupt(_))));
    }

    #[test]
    fn a_version_seven_log_with_a_drop_or_a_ground_pile_is_refused() {
        let drop = CommandKind::Drop { agent: 0 };
        let pile = CommandKind::Interact { agent: 0, target: sim::settlement::InteractTarget::Pile(3) };
        for kind in [drop, pile] {
            let written = log(vec![kind]);
            assert_eq!(InputLog::from_bytes(&written.to_bytes()).unwrap(), written);
            let bytes = with_version(written.to_bytes(), 7);
            assert!(matches!(InputLog::from_bytes(&bytes), Err(LoadError::Corrupt(_))));
        }
        // An older target still loads from a version 7 log.
        let site = log(vec![CommandKind::Interact { agent: 0, target: sim::settlement::InteractTarget::Site(1) }]);
        assert_eq!(InputLog::from_bytes(&with_version(site.to_bytes(), 7)).unwrap(), site);
    }

    #[test]
    fn a_version_six_log_with_a_blueprint_or_a_site_is_refused() {
        let place = CommandKind::PlaceBlueprint { kind: 1, position: FxVec2::new(Fx::from_int(5), Fx::ONE) };
        let site = CommandKind::Interact { agent: 0, target: sim::settlement::InteractTarget::Site(1) };
        for kind in [place, site] {
            let written = log(vec![kind]);
            assert_eq!(InputLog::from_bytes(&written.to_bytes()).unwrap(), written);
            let bytes = with_version(written.to_bytes(), 6);
            assert!(matches!(InputLog::from_bytes(&bytes), Err(LoadError::Corrupt(_))));
        }
        // An older target still loads from a version 6 log.
        let output = log(vec![CommandKind::Interact { agent: 0, target: sim::settlement::InteractTarget::Output(0) }]);
        assert_eq!(InputLog::from_bytes(&with_version(output.to_bytes(), 6)).unwrap(), output);
    }

    #[test]
    fn a_version_five_log_with_an_interact_is_refused() {
        let kind = CommandKind::Interact { agent: 0, target: sim::settlement::InteractTarget::Output(0) };
        let written = log(vec![kind]);
        assert_eq!(InputLog::from_bytes(&written.to_bytes()).unwrap(), written);
        let bytes = with_version(written.to_bytes(), 5);
        assert!(matches!(InputLog::from_bytes(&bytes), Err(LoadError::Corrupt(_))));
    }

    #[test]
    fn a_version_four_log_with_a_bundle_is_refused() {
        let kind = CommandKind::Bundle { agent: 0, other: sim::settlement::InventoryRef::Output(0), direction: sim::settlement::Direction::Pickup };
        let written = log(vec![kind]);
        assert_eq!(InputLog::from_bytes(&written.to_bytes()).unwrap(), written);
        let bytes = with_version(written.to_bytes(), 4);
        assert!(matches!(InputLog::from_bytes(&bytes), Err(LoadError::Corrupt(_))));
    }

    #[test]
    fn newer_and_unknown_versions_are_refused() {
        let bytes = log(vec![]).to_bytes();
        for version in [0, INPUT_FORMAT_VERSION + 1] {
            assert_eq!(
                InputLog::from_bytes(&with_version(bytes.clone(), version)).unwrap_err(),
                LoadError::FormatVersion { found: version, expected: INPUT_FORMAT_VERSION }
            );
        }
    }
}
