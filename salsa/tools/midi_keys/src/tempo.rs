//! Shared count-in policy and beat timeline for every exercise family.
use keyboard::Event;

#[derive(Clone, Copy, serde::Serialize)]
pub struct CountIn {
    pub beats: usize,
    pub bpm: u32,
}
impl CountIn {
    pub fn events(self) -> Vec<(u64, Event)> {
        events(self.beats, self.bpm)
    }
    pub fn duration_ms(self) -> u64 {
        duration_ms(self.beats, self.bpm)
    }
}

/// Words fit naturally through 75 BPM (the longest sample is under 800 ms).
/// Faster count-ins use clicks until a shorter spoken sample bank is available.
/// Speech is never cut off or pitched up merely to fit a beat.
pub fn events(beats: usize, bpm: u32) -> Vec<(u64, Event)> {
    assert!(bpm > 0, "A count-in needs a positive tempo");
    (0..beats)
        .map(|i| {
            let at = i as u64 * 60_000 / u64::from(bpm);
            let event = if beats == 4 && bpm <= 75 {
                Event::CountIn((beats - i) as u8)
            } else {
                Event::MetronomeClick
            };
            (at, event)
        })
        .collect()
}
pub fn duration_ms(beats: usize, bpm: u32) -> u64 {
    beats as u64 * 60_000 / u64::from(bpm)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn count_in_uses_absolute_beats_and_keeps_speech_within_its_supported_tempo() {
        assert_eq!(
            events(4, 60),
            vec![
                (0, Event::CountIn(4)),
                (1000, Event::CountIn(3)),
                (2000, Event::CountIn(2)),
                (3000, Event::CountIn(1))
            ]
        );
        assert_eq!(duration_ms(4, 60), 4000);
        let fast = events(4, 90);
        assert_eq!(
            fast.iter().map(|e| e.0).collect::<Vec<_>>(),
            [0, 666, 1333, 2000]
        );
        assert!(fast.iter().all(|e| e.1 == Event::MetronomeClick));
        assert_eq!(duration_ms(4, 90), 2666);
        assert!(events(4, 75)
            .iter()
            .all(|e| matches!(e.1, Event::CountIn(_))));
    }
}

/// Metronome deadlines are derived from one origin, never from repeatedly
/// adding a rounded millisecond beat. A late UI tick emits one current beat,
/// skipping missed beats instead of producing a catch-up burst.
#[derive(Clone, Copy)]
pub struct BeatClock {
    origin: std::time::Instant,
    bpm: u32,
    next: u64,
}
impl BeatClock {
    pub fn new(origin: std::time::Instant, bpm: u32) -> Self {
        assert!(bpm > 0);
        Self {
            origin,
            bpm,
            next: 0,
        }
    }
    fn deadline(&self) -> std::time::Instant {
        self.origin
            + std::time::Duration::from_nanos(
                (u128::from(self.next) * 60_000_000_000 / u128::from(self.bpm)) as u64,
            )
    }
    pub fn poll(&mut self, now: std::time::Instant) -> Option<u64> {
        if now < self.deadline() {
            return None;
        }
        let beat = ((now.duration_since(self.origin).as_nanos() * u128::from(self.bpm)
            / 60_000_000_000) as u64)
            .max(self.next);
        self.next = beat + 1;
        Some(beat)
    }
}
#[cfg(test)]
mod clock_tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn fractional_tempos_do_not_accumulate_drift_or_burst_after_a_stall() {
        let origin = Instant::now();
        let mut clock = BeatClock::new(origin, 90);
        for beat in 0..9000 {
            let at = origin + Duration::from_nanos(beat * 60_000_000_000 / 90);
            assert_eq!(clock.poll(at), Some(beat));
            assert_eq!(clock.poll(at), None);
        }
        assert_eq!(clock.deadline(), origin + Duration::from_secs(6000));
        let late = origin + Duration::from_secs(7000);
        assert_eq!(clock.poll(late), Some(10500));
        assert_eq!(clock.poll(late), None);
        assert_eq!(clock.poll(origin), None);
    }
}
