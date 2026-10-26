# Testing

## `headless:bench_test` fails on remote executors

- **Status:** open
- **Why:** `large_world_fits_the_tick_budget` measures 26–33 ms per tick on
  BuildBuddy's remote machines against a 10 ms budget, so it fails on
  `master` and makes `bazel test //kestrel/...` red for everyone.
- **What to do:** decide whether the budget applies to a known machine
  (then tag the test `manual` or `local`, or run it only where the hardware
  is known) or whether the sim got slower (then profile it). Check which
  commit it started failing on.
- **Where:** `kestrel/headless/bench_test.rs`; its target in
  `kestrel/headless/BUILD.bazel`.
- **Found in:** the full test run for PR #111
  (https://app.buildbuddy.io/invocation/b95f8bf3-e295-4ed7-91a4-b2f8bef0b839).

## Done
