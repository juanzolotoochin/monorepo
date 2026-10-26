//! The per-tick budget: 10 ms at 100k entities, on a known machine.
//!
//! By default this only checks that the benchmark runs and prints the mean
//! tick time: remote test machines vary too much in speed and load to hold a
//! budget. To enforce one, run it optimized on a machine you know, with
//! nothing else running:
//!
//! ```text
//! bazel test -c opt --test_env=KESTREL_TICK_BUDGET_MS=10 //kestrel/headless:bench_test
//! ```

use content::Content;
use headless::{bench, default_threads};
use sim::scenario::Scenario;

#[test]
fn large_world_fits_the_tick_budget() {
    let optimized = !cfg!(debug_assertions);
    let ticks = if optimized { 100 } else { 3 };
    let result = bench(&Scenario::large(), 3, ticks, default_threads(), &Content::builtin());
    println!("{result:?}");
    assert!(result.movers > 90_000, "{} movers", result.movers);
    let budget = std::env::var("KESTREL_TICK_BUDGET_MS").ok().map(|ms| ms.parse::<f64>().expect("KESTREL_TICK_BUDGET_MS is a number of milliseconds"));
    if let Some(budget) = budget {
        assert!(optimized, "the tick budget only means something in an optimized build (-c opt)");
        assert!(result.mean_ms <= budget, "mean {:.2} ms per tick exceeds {budget} ms", result.mean_ms);
    }
}
