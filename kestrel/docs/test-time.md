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
| 2026-10-26 | Rebase of the inventory onto master (#117–#121), fitting `leave` | 21.5 min | 8.4 min | 5 | 39% | Sum of `INFO: Elapsed time` (the first full run's, 335 s, from the Bazel server log, as its output was cut off). One full suite with a test failure, `sim_test` once, golden regenerated once, a full suite and the debug_ui/play/service/tools build. |
| 2026-10-26 | Hauler actions, steps 1–4 | 18.5 min | 4.6 min | 10 | 25% | Sum of `INFO: Elapsed time` (one cached build's, about 5 s, estimated as its line was filtered out). Steps 1–2: 54.3 s over 2 runs (`sim_test` once). Steps 3–4: builds, a full suite that stopped at analysis (`//kestrel/...` includes a GPU demo that can't build remotely; rerun with `--build_tests_only`), the full suite with one test failure, and `sim_test` after the fix. |
| 2026-10-27 | Threats as characters, steps 1–4 | 15.9 min | 3.4 min | 6 | 21% | Sum of `INFO: Elapsed time`. Steps 1–2: 49 s over 3 runs (golden regenerated, `sim_test` once). Steps 3–4: golden regenerated (14.3 s), `//kestrel/sim/...` build (14.5 s), the full suite with `--build_tests_only` (124.3 s), all 24 passing first time. |
| 2026-10-27 | Vehicle prototype: a rover in the dig lab (sim, content, save, app view, demo script) | 34.5 min | 11.4 min | 17 | 33% | Sum of `INFO: Elapsed time` (one `sim_test` run, about 25 s, estimated as its output was cut off). `sim_test` 5 times (two test-fix loops, two debug runs), golden regenerated once (sim only; headless hashes unchanged), app built 3 times, the full suite twice stopped at analysis on the GPU demos (rerun with a `tests()` query minus the flicker tests), two scripted demo runs with frame capture (build time only). |
