//! The per-tick budget: 10 ms at 100k entities. Enforced only in optimized
//! builds (`bazel test -c opt //game/headless:bench_test`).

use content::Content;
use headless::{bench, default_threads};
use sim::scenario::Scenario;

const BUDGET_MS: f64 = 10.0;

#[test]
fn large_world_fits_the_tick_budget() {
    let optimized = !cfg!(debug_assertions);
    let ticks = if optimized { 100 } else { 3 };
    let result = bench(&Scenario::large(), 3, ticks, default_threads(), &Content::builtin());
    println!("{result:?}");
    assert!(result.movers > 90_000, "{} movers", result.movers);
    if optimized {
        assert!(result.mean_ms <= BUDGET_MS, "mean {:.2} ms per tick exceeds {BUDGET_MS} ms", result.mean_ms);
    }
}
