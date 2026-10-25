# Wall-clock-sensitive tests

Three Bazel test targets use real elapsed time. Their assertions can fail when a build or other workload competes for CPU. They are marked `flaky = True` in their `BUILD.bazel` files, so a passing retry does not prove the timing issue is fixed.

| Target | Timing dependency | Relevant test |
| --- | --- | --- |
| `//kestrel/headless:bench_test` | Compares mean simulation tick time with a 10 ms budget while using the machine's worker threads. | `large_world_fits_the_tick_budget` |
| `//kestrel/net:net_test` | Counts host ticks during a fixed interval of real time over local UDP. | `tests::udp_smoke` |
| `//kestrel/session:session_test` | Runs two-player UDP scenarios and catch-up within real-time deadlines. | `two_players_walk_around_together`, `two_players_fight_ghouls_over_udp` |

The headless benchmark measures performance, while the network tests primarily need to establish protocol progress and agreement. When investigating a failure, inspect the elapsed-time assumptions and run the target alone as well as under CPU load. A durable fix should make protocol assertions depend on simulated progress or confirmed ticks where possible, and measure performance on a controlled machine. Remove `flaky = True` only after the revised test passes reliably under the workloads it is expected to tolerate.
