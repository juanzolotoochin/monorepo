//! The time of day: a pure function of the tick and the scenario's day
//! cycle ([`content::scenario::DayCycle`]). The cycle starts at dawn at tick
//! 0, so tick `t` is `t mod (day + night)` ticks into its cycle. Nothing is
//! stored, so there is nothing to save or hash and every peer agrees; the
//! view reads the same functions.

use content::scenario::{self, Scenario};
use fx::Fx;

/// Day or night.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    Day,
    Night,
}

/// A scenario's day and night lengths, in ticks (each at least one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Clock {
    pub day: u64,
    pub night: u64,
}

/// Everything the clock says about one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeOfDay {
    pub phase: Phase,
    /// How far through the phase, from zero at its first tick to just under
    /// one at its last.
    pub fraction: Fx,
    /// The night this is, or the last one before it: [`Clock::night`].
    pub night: u64,
    /// Ticks until the next dusk: [`Clock::ticks_to_dusk`].
    pub to_dusk: u64,
}

impl Clock {
    pub fn new(day: u64, night: u64) -> Clock {
        Clock { day: day.max(1), night: night.max(1) }
    }

    /// The scenario's clock, or none if its days never end.
    pub fn of(scenario: &Scenario) -> Option<Clock> {
        scenario.day_cycle.map(|c| Clock::new(scenario::ticks(c.day), scenario::ticks(c.night)))
    }

    /// Ticks in a whole day and night.
    pub fn cycle(&self) -> u64 {
        self.day + self.night
    }

    /// Ticks into the current cycle: below `day` it is day, from `day` on
    /// it is night.
    fn into_cycle(&self, tick: u64) -> u64 {
        tick % self.cycle()
    }

    pub fn phase(&self, tick: u64) -> Phase {
        if self.into_cycle(tick) < self.day {
            Phase::Day
        } else {
            Phase::Night
        }
    }

    /// How far through its phase `tick` is: zero at dawn (or dusk), just
    /// under one on the phase's last tick.
    pub fn fraction(&self, tick: u64) -> Fx {
        let t = self.into_cycle(tick);
        if t < self.day {
            Fx::from_ratio(t as i64, self.day as i64)
        } else {
            Fx::from_ratio((t - self.day) as i64, self.night as i64)
        }
    }

    /// The nights begun by `tick`: 0 during the first day, `n` during the
    /// `n`th night and the day after it.
    pub fn night(&self, tick: u64) -> u64 {
        tick / self.cycle() + u64::from(self.phase(tick) == Phase::Night)
    }

    /// Ticks from `tick` to the next dusk, the first tick of a night: `day`
    /// at dawn, one on the day's last tick, and a whole cycle at dusk itself.
    pub fn ticks_to_dusk(&self, tick: u64) -> u64 {
        let t = self.into_cycle(tick);
        if t < self.day {
            self.day - t
        } else {
            self.cycle() - t + self.day
        }
    }

    pub fn at(&self, tick: u64) -> TimeOfDay {
        TimeOfDay { phase: self.phase(tick), fraction: self.fraction(tick), night: self.night(tick), to_dusk: self.ticks_to_dusk(tick) }
    }
}
