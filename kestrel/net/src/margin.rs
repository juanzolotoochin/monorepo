//! A client's input margin: how many ticks past the host's present (as the
//! client estimates it) an input asks to apply, to pay for its trip to the
//! host. Too small and the host moves inputs later, which a predicting client
//! sees as a misprediction; too large and every input waits longer than it
//! needs to.
//!
//! The margin is the one-way time plus a safety: a tick for the input to be
//! sent and a multiple of the measured jitter. It grows at once when an
//! input comes in late and shrinks by a tick only after a quiet period.
//! Nothing here reads a clock: callers pass `now`.

use std::time::Duration;

use crate::TICK;

/// The safety starts with this much: an input waits up to a tick to be sent,
/// which the round trip doesn't measure …
pub(crate) const SEND_WAIT: Duration = TICK;
/// … plus this many late-side deviations of the round trip (how much longer
/// than the smoothed round trip a sample takes, on average, shorter ones
/// counting as zero). For jitter as likely early as late that is twice the
/// mean deviation. Under ±20 ms of jitter each way, half of this let
/// occasional inputs in late …
pub(crate) const SAFETY_DEVIATIONS: u32 = 4;
/// … up to this much. A change in the round trip reads as deviation until
/// the smoothed round trip catches up: a 250 ms lag spike would ask for over
/// half a second. Inputs that come in late grow the margin by what they
/// needed instead.
pub(crate) const MAX_JITTER_SAFETY: Duration = Duration::from_millis(100);
/// Each round-trip sample moves the deviation this fraction of the way
/// (1/16; TCP uses 1/4 per round trip, but samples come every tick). The
/// margin keeps its highest recent value, so a noisier estimate would only
/// keep it higher.
pub(crate) const DEVIATION_GAIN: u32 = 16;
/// After this long without a late input or a change, the margin shrinks a tick.
pub(crate) const QUIET: Duration = Duration::from_secs(3);
/// Round-trip samples to take (about one a tick) before the starting margin
/// gives way to the measured one, so the deviation has settled.
pub(crate) const SAMPLES_TO_ADAPT: u32 = 32;

#[derive(Clone, Debug)]
pub(crate) struct InputMargin {
    /// The margin in use, in ticks.
    ticks: u64,
    /// A margin pinned by the player (`NetConfig::input_margin`).
    pinned: Option<u64>,
    /// The margin the measured network asks for, once measured. The margin
    /// never shrinks below it.
    measured: Option<u64>,
    /// When the margin last changed or an input came in late.
    since: Duration,
}

impl InputMargin {
    /// Starts at `start` ticks until the round trip is measured, unless pinned.
    pub(crate) fn new(start: u64, pinned: Option<u64>, now: Duration) -> InputMargin {
        InputMargin { ticks: pinned.unwrap_or(start), pinned, measured: None, since: now }
    }

    pub(crate) fn ticks(&self) -> u64 {
        self.ticks
    }

    pub(crate) fn pinned(&self) -> bool {
        self.pinned.is_some()
    }

    /// The host's starting margin, kept only until the round trip is measured.
    pub(crate) fn welcome(&mut self, start: u64, now: Duration) {
        if self.pinned.is_none() && self.measured.is_none() {
            self.ticks = start;
            self.since = now;
        }
    }

    /// The ticks the network asks for: one way plus the safety, rounded up.
    /// At least one way and a tick.
    pub(crate) fn needed(rtt: Duration, deviation: Duration) -> u64 {
        let safety = SEND_WAIT + (deviation * SAFETY_DEVIATIONS).min(MAX_JITTER_SAFETY);
        (rtt / 2 + safety).as_nanos().div_ceil(TICK.as_nanos()) as u64
    }

    /// A new round-trip estimate (smoothed, and its late-side deviation). The
    /// first moves the margin to what it asks for; after that, it only
    /// grows the margin at once. Shrinking waits for `update`.
    pub(crate) fn measure(&mut self, rtt: Duration, deviation: Duration, now: Duration) {
        let needed = InputMargin::needed(rtt, deviation);
        let first = self.measured.is_none();
        self.measured = Some(needed);
        if self.pinned.is_some() {
            return;
        }
        if first || needed > self.ticks {
            if needed != self.ticks {
                self.since = now;
            }
            self.ticks = needed;
        }
    }

    /// An input asked for a tick `headroom` ticks past the host's present
    /// and applied `late` ticks after it: grow so the same trip would be on
    /// time.
    pub(crate) fn late(&mut self, headroom: u64, late: u64, now: Duration) {
        self.since = now;
        if self.pinned.is_none() {
            self.ticks = self.ticks.max(headroom + late);
        }
    }

    /// Shrinks a tick toward the measured margin after a quiet period.
    pub(crate) fn update(&mut self, now: Duration) {
        let Some(measured) = self.measured else { return };
        if self.pinned.is_none() && self.ticks > measured && now >= self.since + QUIET {
            self.ticks -= 1;
            self.since = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// A margin measured on a 200 ms round trip that varies little, at time 0.
    fn measured() -> (InputMargin, u64) {
        let mut margin = InputMargin::new(8, None, Duration::ZERO);
        margin.measure(200 * MS, 5 * MS, Duration::ZERO);
        (margin, InputMargin::needed(200 * MS, 5 * MS))
    }

    #[test]
    fn the_measured_network_replaces_the_starting_margin() {
        let mut margin = InputMargin::new(8, None, Duration::ZERO);
        assert_eq!(margin.ticks(), 8);
        margin.welcome(9, Duration::ZERO);
        assert_eq!(margin.ticks(), 9, "the host's value until measured");
        // On a LAN, one way and a tick.
        margin.measure(17 * MS, 2 * MS, Duration::ZERO);
        assert_eq!(margin.ticks(), 2);
        margin.welcome(9, Duration::ZERO);
        assert_eq!(margin.ticks(), 2, "a new world doesn't forget the network");
    }

    #[test]
    fn the_margin_grows_at_once_on_a_late_input() {
        let (mut margin, needed) = measured();
        // An input sent with the measured margin applied 3 ticks late.
        margin.late(needed, 3, 10 * MS);
        assert_eq!(margin.ticks(), needed + 3);
        // Inputs sent before the growth come in late too; they ask for no more.
        margin.late(needed, 2, 20 * MS);
        margin.late(needed + 1, 1, 30 * MS);
        assert_eq!(margin.ticks(), needed + 3);
        // A worse network grows it at once too.
        margin.measure(400 * MS, 5 * MS, 40 * MS);
        assert!(InputMargin::needed(400 * MS, 5 * MS) > needed + 3);
        assert_eq!(margin.ticks(), InputMargin::needed(400 * MS, 5 * MS));
    }

    #[test]
    fn the_margin_shrinks_a_tick_only_after_the_quiet_period() {
        let (mut margin, needed) = measured();
        let late_at = Duration::from_secs(1);
        margin.late(needed, 3, late_at);
        // Better measurements don't shrink it.
        margin.measure(200 * MS, 5 * MS, late_at + MS);
        margin.update(late_at + QUIET - MS);
        assert_eq!(margin.ticks(), needed + 3);
        margin.update(late_at + QUIET);
        assert_eq!(margin.ticks(), needed + 2);
        margin.update(late_at + QUIET * 3 / 2);
        assert_eq!(margin.ticks(), needed + 2, "one tick per quiet period");
        // A late input restarts the quiet period without growing it.
        margin.late(needed, 1, late_at + QUIET * 3 / 2);
        margin.update(late_at + QUIET * 2);
        assert_eq!(margin.ticks(), needed + 2);
        margin.update(late_at + QUIET * 5 / 2);
        assert_eq!(margin.ticks(), needed + 1);
    }

    #[test]
    fn the_margin_never_shrinks_below_one_way_and_a_tick() {
        for rtt_ms in [0, 1, 17, 33, 34, 100, 167, 200, 241, 336] {
            let rtt = rtt_ms * MS;
            // Measured on a slow link first, then the link gets faster and
            // steady: no deviation at all.
            let mut margin = InputMargin::new(8, None, Duration::ZERO);
            margin.measure(Duration::from_secs(1), Duration::ZERO, Duration::ZERO);
            margin.late(1, 1, Duration::ZERO);
            for second in 1..200 {
                margin.measure(rtt, Duration::ZERO, Duration::from_secs(second));
                margin.update(Duration::from_secs(second));
            }
            let one_way = (rtt / 2).as_nanos().div_ceil(TICK.as_nanos()) as u64;
            assert_eq!(margin.ticks(), one_way + 1, "round trip {rtt:?}");
        }
        // The safety follows the jitter above that.
        assert!(InputMargin::needed(200 * MS, 20 * MS) > InputMargin::needed(200 * MS, Duration::ZERO));
        // Up to a point: a change of round trip reads as deviation for a while.
        assert_eq!(InputMargin::needed(200 * MS, Duration::from_secs(1)), InputMargin::needed(200 * MS, MAX_JITTER_SAFETY / SAFETY_DEVIATIONS));
    }

    #[test]
    fn a_pinned_margin_ignores_the_network() {
        let mut margin = InputMargin::new(8, Some(5), Duration::ZERO);
        assert!(margin.pinned());
        margin.welcome(9, Duration::ZERO);
        margin.measure(17 * MS, 2 * MS, Duration::ZERO);
        margin.measure(400 * MS, 50 * MS, MS);
        margin.late(5, 4, 2 * MS);
        margin.update(Duration::from_secs(60));
        assert_eq!(margin.ticks(), 5);
    }
}
