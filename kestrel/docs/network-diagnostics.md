# Network diagnostics

In a hosted or joined game, press `/` to open the debug overlay and choose
**Diagnostics → Network**. Collect data on both the host and client while
repeating a short move-and-shoot sequence. The window is observational: it
does not change input delay, simulation pacing, prediction, or the protocol.
The view is drawn by [network telemetry](../app/src/telemetry/network.rs) using
the existing [telemetry menu provider](../app/src/telemetry.rs).

The window keeps up to ten seconds and 600 frame samples. It continues to
show the last session's values after leaving the game. A new network session
or world clears the history. Existing control-port inspection responses also
include the latest values under `telemetry.network`.
Use **Copy diagnostics and history** to copy a JSON report with the latest
values and retained timing samples. This report measures latency and pacing;
it is not a simulation replay or a desync capture.

## Reading the window

- **RTT:** the smoothed round-trip estimate used by scheduling, and the latest
  sample. It is unavailable on the host and before a client gets a ping reply.
- **Host next tick:** the tick from the last received host packet, not a live
  synchronized host clock. Both local and host tick counters name the next tick
  to simulate. A tick is approximately 16.7 ms.
- **Ready turns:** consecutive official turns available to simulate. A gap in
  the sequence blocks the client even if later turns have arrived.
- **Missing-turn waits:** updates with simulation ticks due but missing turns,
  plus the observed caller-clock time between updates while waiting. This is
  not a measurement of how long an individual network packet spent in flight.
- **Catch-up ticks:** extra ticks run beyond the paced ticks due, to reduce an
  excess turn backlog. Normal multi-tick frames are not counted as catch-up.
- **Pending application:** queued local commands not simulated locally yet.
  This includes commands the host has already acknowledged receiving.
- **Input margin:** the configured scheduling margin. A client also adds its
  estimate of one-way latency to the last observed host tick. The four-tick
  jitter target controls catch-up; it is not a mandatory fixed playback delay.
- **Host peers:** receipt acknowledgements, packet age, latest requested and
  assigned ticks, and per-world late-input counts and total ticks shifted.
  The host cannot infer a peer's current simulation tick from its receipt ack.
- **Movement preview:** horizontal distance from the confirmed avatar, explicit
  easing corrections, clamped frames, and distance-reset snaps. These are
  view observations, not deterministic state mismatches. Initial avatar setup
  does not count as a distance snap.

Graphs use local monotonic wall time, with per-frame p95 values over the retained
history. The slowest tick graph has no sample on frames without a tick.
Traffic rates use up to one second of local observations. They are unavailable
until two samples exist. Packet loss, remote assigned ticks on the client, and
per-shot animation/fire/hit timelines are not measured in this first window.

## Feeling a slower network

On Windows, [`tools/network-lab.ps1`](../tools/network-lab.ps1) opens a host
and a client window on one machine, with a localhost relay between them that
delays every datagram:

```powershell
.\kestrel\tools\network-lab.ps1 -DelayMs 75 -JitterMs 20 -Seed 42
```

Each datagram waits `max(0, DelayMs + uniform(-JitterMs, +JitterMs))` ms in
each direction, so round trips grow by about `2 × DelayMs`. It reuses the built
game; `-Build` builds it once first. Each run's parameters, relay counters and
logs land in `tools/network_lab/runs/`. The two windows share one CPU and GPU,
so read frame time next to the network graphs.

## First investigation

1. On localhost, record the configured input margin and pending command count
   during movement and firing. Observe whether visible hitches coincide with
   missing-turn waits or expensive frames/ticks.
2. Repeat over LAN and the connection with poor feel. Compare ready turns, RTT,
   packet age, catch-up activity, and movement preview offsets.
3. Inspect the host's peer section for late inputs and shifted ticks. These
   distinguish inputs missing their requested turn from commands intentionally
   scheduled ahead of time.

Use [NetSim](../net/src/netsim.rs) for repeatable outages, jitter, loss, and spikes.
The [networking tests](../net/src/tests.rs) verify receipt versus application, starvation followed by catch-up,
and that host/client hashes still agree. Per-action shot traces and replayable
desync captures are follow-up work; this window does not export replay evidence.
