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

## `movement_test` can't be built or run from Windows

- **Status:** open
- **Why:** the Steam Deck commit (`b3e66f5`) added the Linux gamepad backend
  to the `demo` crate, which links `libudev.so.1`, and tagged
  `//kestrel/app:movement_test` `local` because remote workers lack that
  library. On Windows the `local` tag also forces the test's Rust compile
  onto this machine with the Linux exec tools, which fails with
  "%1 is not a valid Win32 application". Without the tag it compiles
  remotely but can't start there (`libudev.so.1: cannot open shared
  object`). So the movement, prediction and margin tests run nowhere from
  Windows.
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

## Done
