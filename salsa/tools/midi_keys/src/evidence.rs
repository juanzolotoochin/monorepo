//! Recorded MIDI facts shared by grading and notation. Offsets are relative to
//! capture start, which may be a key release, not necessarily the first attack.
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Clone, Serialize, Deserialize)]
pub struct MidiEvidence {
    pub offset_ms: u64,
    pub channel: usize,
    pub note: usize,
    pub velocity: u8,
}

pub fn next_key_event(events: &[MidiEvidence], attack: usize) -> Option<&MidiEvidence> {
    let on = events.get(attack).filter(|e| e.velocity > 0)?;
    events[attack + 1..]
        .iter()
        .find(|e| e.channel == on.channel && e.note == on.note)
}

/// A retrigger without an intervening release is not a valid held note.
pub fn held_ms(events: &[MidiEvidence], attack: usize) -> Option<u64> {
    let off = next_key_event(events, attack).filter(|e| e.velocity == 0)?;
    off.offset_ms.checked_sub(events[attack].offset_ms)
}

pub fn signed_ms(at: Instant, origin: Instant) -> i64 {
    if at >= origin {
        at.duration_since(origin).as_millis() as i64
    } else {
        -(origin.duration_since(at).as_millis() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn releases_are_matched_by_channel_and_key_without_crossing_retriggers() {
        let e = |offset_ms, channel, velocity| MidiEvidence {
            offset_ms,
            channel,
            note: 60,
            velocity,
        };
        let events = [e(0, 0, 0), e(100, 0, 90), e(200, 1, 0), e(300, 0, 0)];
        assert_eq!(held_ms(&events, 0), None);
        assert_eq!(held_ms(&events, 1), Some(200));
        assert_eq!(held_ms(&[e(0, 0, 80), e(50, 0, 90), e(150, 0, 0)], 0), None);
        assert_eq!(held_ms(&[e(100, 0, 80)], 0), None);
        assert_eq!(held_ms(&[e(100, 0, 80), e(50, 0, 0)], 0), None);
    }
}
