# Wall-clock-sensitive tests

Three Bazel test targets use real elapsed time. None of them is marked `flaky = True`: each is written so that a busy machine only makes it slower, not red. Keep it that way, because Bazel's retries hide a test that fails for a real reason.

| Target | Timing dependency | Relevant test |
| --- | --- | --- |
| `//kestrel/headless:bench_test` | Measures the mean simulation tick time while using the machine's worker threads. It holds a budget only when `KESTREL_TICK_BUDGET_MS` is set. | `large_world_fits_the_tick_budget` |
| `//kestrel/net:net_test` | Runs a host and a client over local UDP in real time. | `tests::udp_smoke` |
| `//kestrel/session:session_test` | Runs two-player UDP scenarios in real time. | `two_players_walk_around_together`, `two_players_fight_ghouls_over_udp` |

The network tests check that the protocol makes progress and that the machines agree, not how fast they go:

- They run until the game reaches a state (the client has played 60 ticks, both players walked 2 m, both hit ghouls), with a deadline far above the usual time (30 s), rather than for a fixed time.
- They compare ticks and hashes the peers both ran. Before comparing final states, the host makes one last update a tick later. It sends once per tick of time, so its last update in the loop may have run a tick it never sent.
- Pacing (ticks per second) is tested on the simulated network in `kestrel/net/src/tests.rs`, where time is simulated too.

The benchmark's 10 ms budget only means something on a known machine with nothing else running, and remote test machines are neither. To check it:

```text
bazel test -c opt --test_env=KESTREL_TICK_BUDGET_MS=10 //kestrel/headless:bench_test
```

To check a change to one of these tests, run it many times, for example with `--runs_per_test=100`.
