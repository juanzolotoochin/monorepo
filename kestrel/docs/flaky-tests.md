# Flaky tests

Tests marked `flaky = True` in Bazel, with what is known so far. Bazel runs a
flaky test up to three times and passes it if any attempt passes, so these
mask real regressions as well as noise. Remove the flag once a test is fixed.

## Summary

| Target | Failing test | Failure | Alone |
| --- | --- | --- | --- |
| `//kestrel/headless:bench_test` | `large_world_fits_the_tick_budget` | mean 81.48 ms per tick, budget 10 ms | passes, 1.1 s |
| `//kestrel/net:net_test` | `tests::udp_smoke` | host ran 97 ticks, needs 144 | passes, 3.1 s (54 s when failing) |
| `//kestrel/session:session_test` | `tests::two_players_walk_around_together` | host tick 178, client 177 | passes, 9.1 s |
| `//kestrel/session:session_test` | `tests::two_players_fight_ghouls_over_udp` | only the host hit ghouls: `{0: 4}` | passes, 9.1 s |

All four run against the wall clock: for a fixed stretch of real time, or
against a time budget. Their pass or fail depends on how much CPU they get, not
only on the code.

## When they failed

- Date: 2026-10-25, on the `kestrel-debug-ui` branch (PR #84), rebased onto
  `9dc779f` ("Build Kestrel for browser WebAssembly").
- Machine: Windows 11 Home (10.0.26200), 24 logical CPUs, `-c opt` builds
  (`x64_windows-opt`).
- Command: `bazel test //kestrel/...` with a cold cache for the new wasm32
  and egui crates. The run took 725 s. While these tests ran, Bazel was
  compiling `wgpu-core`, `wasm-bindgen-macro-support` and `egui` with 20 to 24
  actions in flight. Tests run alongside builds by default.
- Failing run: https://app.buildbuddy.io/invocation/7859db76-f9f3-43df-a1e0-dbed9cdf05d6
- Same tree with nothing left to compile (`--build_tests_only`), 22/22 pass:
  https://app.buildbuddy.io/invocation/8c788da7-5028-426a-88d8-5e993dbba2ca
- Earlier full runs on the same machine passed too when little was being
  rebuilt, for example
  https://app.buildbuddy.io/invocation/948b901e-18a8-4066-ac4d-3f718567cc42.
- None of the three crates depends on the change under test (`debug_ui` and
  `kestrel/app`). That change only added two small dependency bumps (`log`,
  `self_cell`) that these crates also link.

## Details

### `//kestrel/headless:bench_test`

`kestrel/headless/bench_test.rs`: about 100k movers (`Scenario::large()`), 100
ticks, `default_threads()` threads (every logical CPU, 24 here). In optimized
builds it asserts a mean tick of at most 10 ms.

```text
BenchResult { movers: 99248, threads: 24, mean_ms: 81.47986299999998, max_ms: 324.2284 }
panicked at kestrel/headless/bench_test.rs:18:9:
mean 81.48 ms per tick exceeds 10 ms
```

Likely cause: the bench uses every core while Bazel uses them for compiling.
With the worker pool oversubscribed, each tick waits for the slowest worker.

Options:

- Add `tags = ["exclusive"]` so Bazel runs it with nothing else in flight. It
  stays a real check, at the cost of a serialized test.
- Move the budget assertion to a separate `manual` or CI-only benchmark target
  on a quiet machine, and keep only the mover-count check here.

### `//kestrel/net:net_test`: `tests::udp_smoke`

`kestrel/net/src/tests.rs`, around line 273. A host and a client on localhost
UDP, updated in a loop with 2 ms sleeps for 3 s of real time. It then asserts
that the host ran at least `0.8 * 3 s * 60` = 144 ticks.

```text
panicked at kestrel/net/src\tests.rs:301:5:
host ran 97 ticks
```

The whole `net_test` binary took 54 s instead of the usual 3 s, so the other
tests in it, which use simulated time, were starved too. They still passed. A
comment in the test already notes that the ticks must stay cheap "even when
other tests load the machine". It's the scheduling, not the tick cost, that
falls behind: each `sleep(2 ms)` can last much longer on a loaded Windows
machine.

Options:

- Assert on progress rather than rate: keep updating until the client has N
  confirmed ticks, with a generous deadline, then check the histories match.
- Or measure ticks against the elapsed time the loop actually saw, not the
  nominal 3 s.

### `//kestrel/session:session_test`: `two_players_walk_around_together`

`kestrel/session/src/lib.rs`, around lines 385 to 417. Host and client walk for
4 s of real time. Then the host stops and the client catches up, one tick per
update, for at most 2 s of real time. Finally it asserts equal ticks.

```text
panicked at kestrel/session/src/lib.rs:415:9:
assertion `left == right` failed
  left: 178
 right: 177
```

The client was one tick short when the 2 s catch-up deadline ran out.

Option: let the catch-up loop run until the ticks match (bounded by an
iteration count or a much longer deadline), since the client's clock is
already simulated (`now += 2 ms`).

### `//kestrel/session:session_test`: `two_players_fight_ghouls_over_udp`

`kestrel/session/src/lib.rs`, around lines 523 to 560. The Red Clearing
(`combat_arena_supply.ron`) over UDP for 9 s of real time. The host fights
the center lane with a sword and the guest the flanks with a spear. It asserts
that both avatars hit a ghoul.

```text
panicked at kestrel/session/src/lib.rs:558:9:
both players hit ghouls: {0: 4}
```

Only the host's avatar (agent 0) landed hits. Most likely the starved client
joined late or ran too few ticks to reach a ghoul within 9 s of real time.

Option: run until both avatars have hit something or a tick count is reached,
and use real time only as a generous outer deadline.

## Reproducing

On a quiet machine these pass. To reproduce the failures, load the CPU while
they run. For example, run them during a cold build of other targets:

```sh
bazel clean   # or change a widely used crate
bazel test //kestrel/...
```

Or run them alone with many copies at once:

```sh
bazel test //kestrel/net:net_test //kestrel/session:session_test //kestrel/headless:bench_test \
  --runs_per_test=24 --cache_test_results=no
```

`--runs_per_test` reports how many attempts failed; check that the counts drop
to zero after a fix before removing `flaky = True`.
