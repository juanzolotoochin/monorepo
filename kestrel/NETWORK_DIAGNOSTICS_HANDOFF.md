# Kestrel networking diagnostics and launcher handoff

## Current instruction and scope

**Update:** the launcher and relay below are now implemented:
`kestrel/tools/network-lab.ps1` and `kestrel/tools/network_lab/relay.mjs`.
A live 75 ms ± 20 ms run joined laser-pit through the relay and cleaned up on
exit. On Windows, Node timers wake in ~15.6 ms steps, so the relay polls for the
last ~17 ms before each delivery (lateness p99 < 0.1 ms, about one CPU core
while traffic flows). `-CoarseTimers` turns polling off and adds 0–15 ms.
`-Build` runs `bazel run --script_path=...` so it builds the native exe without
starting it.

The reported problem is poor client feel, particularly shooting latency and
movement responsiveness. The user clarified that they have **not** experienced
actual desyncs. Prioritize quick interactive evaluation over broad verification.
Do not change deterministic simulation, scheduling, prediction, or the protocol
as part of these tools.

## Completed diagnostics code

Press `/`, then select **Diagnostics → Network** during a networked game.

- `app/src/telemetry/network.rs` implements the read-only egui window. It keeps
  up to ten seconds / 600 frames, graphs RTT, turn availability, tick lag,
  frame time, session update cost, and the slowest simulation tick in each frame.
- The window distinguishes receipt acknowledgements from local command
  application, displays pending inputs, starvation and catch-up counters, and
  exposes per-peer host input lateness and requested/assigned ticks.
- Movement preview observations show horizontal offset, explicit reconciliation,
  clamped frames, and distance-reset snaps. Instrumentation is in
  `app/src/lockstep.rs`; it does not change movement calculations.
- **Copy diagnostics and history** copies a JSON report. Latest metrics also
  appear in existing control-port inspection under `telemetry.network`.
- The App-layer telemetry resource retains the last history after session exit.
  A new network session or world clears it.
- `net/src/lib.rs` adds `NetDiagnostics` and `PeerDiagnostics`. Host/client code
  records observational counters outside deterministic state. `Session::diagnostics`
  exposes them without changing the wire format.
- `docs/network-diagnostics.md` explains the metrics and their limits. The docs
  index and site navigation link to it.

Not implemented: per-shot animation/fire/hit timelines, oldest pending-command
age, packet-loss measurement, full command tracing, or replayable desync capture.
The client cannot know its host-assigned command tick from the current protocol.
The window explicitly labels this unknown instead of inferring a sequence match.

## Verification already completed

All three Bazel test targets passed:

```text
bazel test --config=remote --shell_executable=/bin/bash \
  //kestrel/net:net_test \
  //kestrel/session:session_test \
  //kestrel/app:movement_test
```

Results: https://app.buildbuddy.io/invocation/a641dd40-9748-4d61-a3ff-79aa31ef7ceb

Coverage added for receipt versus application, a simulated downlink outage and
subsequent catch-up with matching simulation hashes, per-peer lateness, bounded
history/reset behavior, and egui drawing with absent/retained client/host data.
The egui check draws the window contents; a manual host/client visual evaluation
has not been completed.

The native Windows game was built successfully with:

```text
bazel run --config=remote --shell_executable=/bin/bash //kestrel/app:demo
```

Build: https://app.buildbuddy.io/invocation/60d9e6f5-ccfe-417a-a6e3-5c6de1533343

The executable existed at `D:/projects/juanique/monorepo2/bazel-bin/kestrel/app/demo.exe`.
It was launched directly with PowerShell `Start-Process` after the Bazel run
returned. Process 15456 was running at the last check; it may since have exited.
Leave existing user game windows alone. Native startup logs are outside the repo:

```text
C:/Users/crist/.codex/visualizations/2026/10/26/01a1759e-975f-7810-98a2-9d95180d35ff/kestrel-launch.log
C:/Users/crist/.codex/visualizations/2026/10/26/01a1759e-975f-7810-98a2-9d95180d35ff/kestrel-launch-error.log
```

The user disliked the verification delay. Most time went to cold Bevy rendering
dependencies for Linux tests; actual tests took 5–9 seconds. Two implementation
iterations also required retries. Do not repeat the full game suite just to
launch or adjust relay conditions.

## Approved next task: quick network-feel launcher

Implement the following agreed interface, with these files proposed:

- `kestrel/tools/network-lab.ps1`: Windows launcher.
- `kestrel/tools/network_lab/relay.mjs`: localhost UDP relay using Node built-ins.

Example invocation from the repository root:

```powershell
.\kestrel\tools\network-lab.ps1 -DelayMs 75 -JitterMs 20 -Seed 42
```

Defaults:

| Parameter | Default / meaning |
| --- | --- |
| `-Scene` | `laser-pit` (this is the current pit scene; there is no `pit-lab` scene) |
| `-DelayMs` | 75 ms one-way base delay |
| `-JitterMs` | Independent uniform variation in ±20 ms per datagram |
| `-Seed` | 42; seeded samples per direction, not a deterministic game replay |
| `-HostPort` | 7777 |
| `-ProxyPort` | 7778 |
| `-Build` | Explicit opt-in to building the native game once |

Delay each datagram by `max(0, DelayMs + uniform(-JitterMs, +JitterMs))`.
Condition both directions, including hello/join and save transfer. Allow natural
reordering. Describe added RTT as approximately twice DelayMs before jitter and
polling; the existing game input margin still applies.

Topology:

```text
Client window <-> relay 127.0.0.1:7778 <-> host 127.0.0.1:7777

Host:   demo.exe --host 7777 --scene laser-pit --name Host
Client: demo.exe --join 127.0.0.1:7778 --name Client
```

Relay forwarding must preserve the expected endpoint identities. Forward client
packets to the host from the relay's socket so host replies return to the relay.
Forward replies to the actual client address from the relay address the client
expects. Bind the relay to loopback and constrain it to this host and one client;
do not turn it into a general forwarding service.

### Requirements already agreed with the user

1. Reuse the built native executable by default. Print its resolved path and
   modification time. Fail clearly if missing, with instructions for `-Build`.
   Consider an explicit binary-path override for builds outside the default path.
2. `-Build` builds the native executable once with the normal remote Bazel
   configuration and BuildBuddy integration. No tests in the interactive launch
   path. Be careful: plain remote `bazel build` defaults to Linux, whereas this
   repository's `bazel run` selects the native client platform. Resolve the build-only
   native launch flow without silently substituting a Linux executable.
3. Use Node's built-in UDP/timing APIs. No new packages, admin requirements,
   game protocol or simulation changes, or changes to OS network settings.
4. Validate numeric arguments and port conflicts. Use an explicit relay-ready
   signal; detect startup failures and clean up partial launches.
5. Bound queued packet count and bytes; report queue-cap drops. Do not allow an
   overloaded relay to grow memory indefinitely.
6. Save run parameters, seed, process IDs, relay counters, and separate logs in
   a run directory. Keep generated runs out of version control.
7. Keep the launcher attached. Ctrl+C cleans up only processes it created.
   Leave unrelated Kestrel windows running. Show both game windows; keep helper
   windows hidden.
8. Print `/ → Diagnostics → Network` instructions. Two rendering windows share
   CPU/GPU, so use frame timing alongside network metrics when evaluating feel.
9. Keep this milestone focused on delay and jitter. Do not add extra builds or
   tests to each trial. Any focused relay/script validation belongs to development,
   not every launcher invocation.

Useful trials:

```powershell
.\kestrel\tools\network-lab.ps1 -DelayMs 0 -JitterMs 0
.\kestrel\tools\network-lab.ps1 -DelayMs 75 -JitterMs 20 -Seed 42
.\kestrel\tools\network-lab.ps1 -DelayMs 100 -JitterMs 60 -Seed 42
```

The current `net/src/netsim.rs` is a seeded **virtual-clock test harness**.
It is not a real-time desktop relay. Keep the new relay separate.

## Repository and environment notes

- Read `C:/Users/crist/.claude/CLAUDE.md` first. Architecture approval for the
  launcher above has already been given; do not repeat the approval loop.
- Follow `kestrel/AGENTS.md`: remote Bazel configuration, preserve defaults,
  never use `--config=full_local`, and report BuildBuddy links immediately.
- Remote Linux genrules failed when Windows Bazel selected MSYS `bash.exe`.
  `--shell_executable=/bin/bash` fixed that specific failure without changing
  `.bazelrc` or disabling remote execution/caching.
- Inside the restricted execution sandbox, Bazel failed to start with Windows
  "Illegal System DLL Relocation." Running outside the sandbox worked. The
  underlying executable is `C:/ProgramData/chocolatey/lib/bazelisk/bazel.exe`;
  `C:/ProgramData/chocolatey/bin/bazel.exe` is a Chocolatey shim.
- Plain Git commands may print a pre-existing fsmonitor IPC error. Read-only
  commands with `git -c core.fsmonitor=false ...` avoid it without changing config.
- Node and PowerShell are installed. No launcher/relay files or new dependencies
  have been added by this agent.

Local design pages (not needed to understand this handoff):

```text
C:/Users/crist/.codex/visualizations/2026/10/26/01a1759e-975f-7810-98a2-9d95180d35ff/network-diagnostics-design.html
C:/Users/crist/.codex/visualizations/2026/10/26/01a1759e-975f-7810-98a2-9d95180d35ff/network-lab-proposal.html
```
