# Test time log

How long tasks take from start to finish, and how much of that is spent
waiting on Bazel (the sum of `INFO: Elapsed time` over every build, test
and run). `kestrel/AGENTS.md` says when to add a row. Newest rows last.

| Date | Task | Total | Bazel | Bazel runs | Share | Notes |
|---|---|---|---|---|---|---|
| 2026-10-26 | Inventory steps 1–3 (type, depots, cargo) | 26.6 min | 18.1 min | 9 | 68% | Measured from the agent's transcript: time from each Bazel tool call to its result. Full suite per commit, plus golden regeneration. |
| 2026-10-26 | Inventory step 4 (player transfer) | 16.2 min | 8.1 min | 5 | 50% | Same method. |
| 2026-10-26 | Inventory step 5 (haulers) | 12.8 min | 5.2 min | 6 | 41% | Same method. |
| 2026-10-26 | Inventory step 6 (piles, collector) | 20.9 min | 8.3 min | 5 | 40% | Same method. |
| 2026-10-26 | Inventory steps 7–10 (pile rules, cleanup) | 46.0 min | 24.8 min | 12 calls, 19 Bazel runs | 54% | Same method. Three test-fix loops (compile error, outdated test, wrong expected total). |
| 2026-10-26 | **Inventory, all 10 steps** | **122.5 min** | **64.5 min** | **37 calls** | **53%** | Agent time only; the arbiter's checks between batches add a few minutes. A full suite run takes 1.5–2.5 min; `sim_test` alone about 40 s; golden regeneration about 20–30 s. |
