//! The day and night clock (M7.1).

use content::scenario::{DayCycle, Scenario};
use fx::Fx;

use super::clock::{Clock, Phase, TimeOfDay};

const SACRED_TREE: &str = include_str!("../../../app/assets/levels/sacred_tree_supply.ron");
const FOREST: &str = include_str!("../../../app/assets/levels/forest_supply.ron");

/// Days of 6 ticks and nights of 3, so the boundaries are easy to see.
const CLOCK: Clock = Clock { day: 6, night: 3 };

#[test]
fn phases_and_fractions_turn_at_dawn_and_dusk() {
    let phases: Vec<Phase> = (0..10).map(|t| CLOCK.phase(t)).collect();
    use Phase::{Day as D, Night as N};
    assert_eq!(phases, [D, D, D, D, D, D, N, N, N, D]);
    assert_eq!(CLOCK.fraction(0), Fx::ZERO);
    assert_eq!(CLOCK.fraction(3), Fx::HALF);
    assert_eq!(CLOCK.fraction(5), Fx::from_ratio(5, 6));
    assert_eq!(CLOCK.fraction(6), Fx::ZERO, "dusk starts the night");
    assert_eq!(CLOCK.fraction(8), Fx::from_ratio(2, 3));
    assert_eq!(CLOCK.fraction(9), Fx::ZERO, "dawn starts the next day");
    assert!((0..100).all(|t| CLOCK.fraction(t) >= Fx::ZERO && CLOCK.fraction(t) < Fx::ONE));
}

#[test]
fn nights_are_counted_from_the_first_dusk() {
    let nights: Vec<u64> = (0..20).map(|t| CLOCK.night(t)).collect();
    assert_eq!(nights, [0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2]);
    assert_eq!(CLOCK.night(9 * 100 + 6), 101);
}

#[test]
fn the_time_to_dusk_counts_down_and_wraps_at_dusk() {
    let to_dusk: Vec<u64> = (0..16).map(|t| CLOCK.ticks_to_dusk(t)).collect();
    assert_eq!(to_dusk, [6, 5, 4, 3, 2, 1, 9, 8, 7, 6, 5, 4, 3, 2, 1, 9]);
    for t in 0..50 {
        let dusk = t + CLOCK.ticks_to_dusk(t);
        assert_eq!(CLOCK.phase(dusk), Phase::Night);
        assert_eq!(CLOCK.fraction(dusk), Fx::ZERO);
        assert_eq!(CLOCK.phase(dusk - 1), Phase::Day);
    }
}

#[test]
fn the_sacred_tree_has_150_s_days_and_75_s_nights() {
    let clock = Clock::of(&Scenario::parse(SACRED_TREE).unwrap()).unwrap();
    let s = content::TICKS_PER_SECOND as u64;
    assert_eq!(clock, Clock { day: 150 * s, night: 75 * s });
    assert_eq!(clock.cycle(), 225 * s);
    assert_eq!(clock.at(0), TimeOfDay { phase: Phase::Day, fraction: Fx::ZERO, night: 0, to_dusk: 150 * s });
    let dusk = 150 * s;
    assert_eq!(clock.at(dusk - 1).phase, Phase::Day);
    assert_eq!(clock.at(dusk), TimeOfDay { phase: Phase::Night, fraction: Fx::ZERO, night: 1, to_dusk: 225 * s });
    assert_eq!(clock.fraction(dusk + 75 * s / 2), Fx::HALF);
    let second_dawn = 225 * s;
    assert_eq!(clock.at(second_dawn), TimeOfDay { phase: Phase::Day, fraction: Fx::ZERO, night: 1, to_dusk: 150 * s });
    // Four nights fall within a 15-minute playtest.
    assert_eq!(clock.night(15 * 60 * s), 4);
}

#[test]
fn no_cycle_means_no_clock() {
    assert_eq!(Clock::of(&Scenario::parse(FOREST).unwrap()), None);
    let mut tree = Scenario::parse(SACRED_TREE).unwrap();
    tree.day_cycle = None;
    assert_eq!(Clock::of(&tree), None);
    // Fractional seconds round up to whole ticks, as the loader's do.
    tree.day_cycle = Some(DayCycle { day: Fx::from_ratio(1, 100), night: Fx::from_ratio(35, 100) });
    assert_eq!(Clock::of(&tree), Some(Clock { day: 1, night: 21 }));
}
