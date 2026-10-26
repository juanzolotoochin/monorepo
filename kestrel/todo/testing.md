# Testing

## Check the tick budget on a known machine

- **Status:** open
- **Why:** remote machines measured 26–33 ms per tick for the large world
  against the 10 ms budget. The bench test no longer enforces the budget
  there, so a slowdown in the sim wouldn't show up anywhere.
- **What to do:** run
  `bazel test -c opt --test_env=KESTREL_TICK_BUDGET_MS=10 //kestrel/headless:bench_test`
  on a known machine. If it misses, find the commit it started on and
  profile it.
- **Where:** `kestrel/headless/bench_test.rs`.
- **Found in:** the full test run for PR #111
  (https://app.buildbuddy.io/invocation/b95f8bf3-e295-4ed7-91a4-b2f8bef0b839).

## App builds from Windows fail on the libxcb1 genrule

- **Status:** open
- **Why:** with `--config=remote`, building any `//kestrel/app` target
  from Windows runs the `rules_distroless` genrule that unpacks Debian 13's
  `libxcb1` on a Linux worker, but with this machine's shell path, so it
  fails with "`/buildbuddy-execroot/C:/Program Files/Git/usr/bin/bash.exe`
  not found". It worked while the action was in the remote cache. The action
  doesn't depend on Kestrel code, so `master` is affected the same way.
  Running it locally fails too (the Linux `zstd`).
  Adding `--shell_executable=/bin/bash` makes it build and the tests pass
  (https://app.buildbuddy.io/invocation/1ebd6404-c4fb-494c-ad36-a7d614489f4f),
  but it changes every shell action's key. `tools/network-lab.ps1 -Build`
  already passes it for this reason.
- **What to do:** put `build:remote --shell_executable=/bin/bash` in
  `.bazelrc` (remote workers are always Linux), check local Windows
  genrules aren't affected (`build:remote` only), and drop the flag from
  ad hoc commands.
- **Where:** `.bazelrc`; `third_party/x11/BUILD.bazel`; `MODULE.bazel`
  (`debian13` packages).
- **Found in:** eight-player stage 1
  (https://app.buildbuddy.io/invocation/271ae3f0-9a83-4152-a6ab-52d188bb1a6f).

## Clean up after the Debian 12 libudev switch

- **Status:** open
- **Why:** the app tests now link and run against Debian 12's `libudev1`
  (pinned `http_archive`s in `MODULE.bazel`), but `libudev1` is still in
  the `debian13` apt package list, unused. Real gamepad input on Linux and
  the Steam Deck wasn't checked after the change; it should still use the
  system libudev, since the deploy ships the binary without runfiles.
- **What to do:** remove `libudev1` from the `debian13` list and regenerate
  the apt lock facts in `MODULE.bazel.lock`. Check a controller on the Deck.
- **Where:** `MODULE.bazel`, `MODULE.bazel.lock`, `third_party/udev/BUILD.bazel`.
- **Found in:** commit `be66db0` on branch `kestrel-ability-prediction`.

## Done

### `two_players_walk_around_together` sometimes ends a tick apart

- **Status:** done (branch `kestrel-flaky-tests`). The host makes one last
  update a tick after its loop, which always sends. The network tests
  (`udp_smoke` and both session UDP tests) now run until they reach a state,
  not for a fixed time. `bench_test` holds a budget only when
  `KESTREL_TICK_BUDGET_MS` is set. None of the three targets is `flaky`
  any more. The session test passed 100 of 100 runs
  (https://app.buildbuddy.io/invocation/0ed4ca1e-c657-4b2b-b074-ee3778d4efbb),
  and net passed 30 of 30
  (https://app.buildbuddy.io/invocation/e021e5a4-2e4c-442d-ac29-e0d77c94934d).
- **Why:** the session test fails about 1 run in 30, on `master` too:
  after the host stops, the client is still a tick behind 2 s later
  (`left: 240, right: 239` at `session/src/lib.rs:455`). Bazel retries it,
  so it shows as flaky rather than red. The test runs on wall-clock time
  over real UDP, so the last turn the host closed before stopping may
  never have been sent, or may have been lost with nothing left to resend
  it.
- **What to do:** have the host make one last `update` after its loop (or
  compare against the client's last confirmed tick instead of the host's),
  and check with `--runs_per_test=30` that it no longer fails.
- **Where:** `two_players_walk_around_together` in `kestrel/session/src/lib.rs`.
- **Found in:** rebasing the eight-player branch onto `4e240d3` (2 of 32
  runs failed there, 1 of 31 on `master`).

### `movement_test` can't be built or run from Windows

- **Status:** done (branch `kestrel-ability-prediction`). The runfiles
  had only `libudev.so` (the link name), but the loader looks up the SONAME
  `libudev.so.1`, and the Debian 13 build needs glibc 2.38 while the remote
  image (Debian 12) has 2.36. `third_party/udev` now imports Debian 12's
  libudev as `libudev.so` to link and `libudev.so.1` to run, so tests find
  it in the runfiles; the `local` tag is gone
  (https://app.buildbuddy.io/invocation/e876e2d3-a82d-4ede-bd61-ca08fb8d7bfa).
- **Why:** the Steam Deck commit (`b3e66f5`) added the Linux gamepad backend
  to the `demo` crate, which links `libudev.so.1`, and tagged
  `//kestrel/app:movement_test` `local` because remote workers lack that
  library. On Windows the `local` tag also forces the test's Rust compile
  onto this machine with the Linux exec tools, which fails with
  "%1 is not a valid Win32 application". Without the tag it compiles
  remotely but can't start there (`libudev.so.1: cannot open shared
  object`). So the movement, prediction and margin tests run nowhere from
  Windows. The same missing library breaks every app test that runs the
  `demo` binary remotely: `control_protocol_test`, `forest_motion_test`,
  `combat_arena_test`, `combat_weapons_test` and `combat_replay_test` all
  fail with it (https://app.buildbuddy.io/invocation/12fae88b-1671-4d4a-a474-656eeb4ad1f8,
  https://app.buildbuddy.io/invocation/02f89587-4116-4f62-84cd-9cac5ec7fc9d).
- **What to do:** add `libudev` to the remote test image, or keep the
  gamepad backend out of the test binary (a feature or a separate test
  crate), or load libudev lazily so a missing library only disables
  gamepads. Then drop the `local` tag.
- **Where:** `movement_test` and the `demo` deps in
  `kestrel/app/BUILD.bazel`; `kestrel/app/src/input/native.rs`;
  `third_party/udev/BUILD.bazel`.
- **Found in:** rebasing PR #111 onto master
  (https://app.buildbuddy.io/invocation/ee33f5f6-ab6c-4fcd-abbd-7b9aa1fcbf05 on
  master, https://app.buildbuddy.io/invocation/f344f7cf-8476-457c-a616-c2be2f35721f without the tag).
