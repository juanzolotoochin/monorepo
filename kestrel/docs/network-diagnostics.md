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
- **Input margin:** how far past the host's present a client's inputs ask to
  apply, `adaptive` or `pinned` (see [the input margin](#the-input-margin)). A
  client also adds its estimate of one-way latency to the last observed host
  tick. The four-tick jitter target controls catch-up; it is not a mandatory
  fixed playback delay. The F3 overlay's `NET` line shows it too, as
  `margin 117 ms`. On the host the row shows the margin clients start with.
- **Late inputs (detected locally):** a client's own inputs that applied later
  than they asked for, seen in the host's turns, and the ticks they moved in
  all. They match the host's counts for that peer. The copied report has them
  as `late_inputs_detected_locally` and
  `late_input_shifted_ticks_detected_locally`, and the margin over time as
  `input_margin_ticks` in the history.
- **Host peers:** receipt acknowledgements, packet age, latest requested and
  assigned ticks, and per-world late-input counts and total ticks shifted.
  The host cannot infer a peer's current simulation tick from its receipt ack.
- **Movement preview:** a client predicts its own avatar (see below). The
  section shows the horizontal distance from the confirmed avatar (about one
  input lead while moving), the last frame's misprediction, and counts of
  mispredicted frames and snaps. These are view observations, not
  deterministic state mismatches. A line counts the client's predicted laser
  shots: drawn, confirmed, mispredicted, and how long before its confirmation
  the last one was drawn (about one input lead).
- **Preview corrections:** a misprediction moves the drawn avatar; consecutive
  mispredicted frames form one `mispredict` correction, whose distance is the
  sum of each frame's starting error. A `snap` is a correction over 1.5 units.
  Corrections under 0.05 units are dropped, except snaps, but their count and
  total distance are shown. **Show corrections in the world** draws, for two
  seconds, a box where the avatar was drawn when the correction started and a
  line to where the new prediction pulls it. The copied report lists them
  under `preview_corrections`, with duration, distance, positions, local tick,
  pending inputs and RTT at the start.
- **Laser mispredictions:** a `laser` correction is one predicted laser shot
  (see below) that the confirmed shot didn't match: it ended elsewhere, by any
  distance, or never fired. It is always kept. In the world it is a red ball
  where the drawn beam ended and a line to where the confirmed shot ended. In
  the copied report `from` is the drawn end, `to` the confirmed end (the same
  point when `never_fired` is true) and `distance` the gap between them. The
  report's top level also has `laser_shots_drawn`, `laser_shots_confirmed`,
  `laser_mispredictions` and `laser_last_lead_ms`.

## The input margin

A client asks for each input to apply on `host tick + one way + margin`: the
last host tick it heard of, brought to the host's present with half the round
trip, plus a margin that pays for the input's own trip to the host. An input
that arrives after its tick is moved to the host's next open tick, and a
predicting client then mispredicts. The margin
([`net/src/margin.rs`](../net/src/margin.rs)) is sized from the network the
client measures:

- It starts at the host's margin (133 ms) until about 32 round trips have been
  sampled, then becomes one way, plus a tick (an input waits up to a tick to
  be sent), plus four times the round trip's late-side deviation (about twice
  its mean deviation), at most 100 ms of that, rounded up to ticks. It is
  never less than one way and a tick: 2 ticks (33 ms) on a LAN.
- It grows at once when the measured network asks for more, or when one of
  the client's inputs comes in late (to what that input would have needed).
- It shrinks by one tick after 3 s without a late input or a change, down to
  what the network asks for.
- An input never asks for an earlier tick than the one before it, so a
  shrinking margin can't reorder inputs.

`--input-margin-ms <ms>` on a joining client pins the margin instead. The host
and its own inputs are unaffected.

## Movement prediction

A client's confirmed world runs about an input lead behind its input: the
input margin, the round trip, and the jitter buffer, roughly 250 ms at 160 ms
RTT. So the client draws its own avatar predicted. Every frame it replays the
avatar from the confirmed world through the commands it sent that haven't
applied yet, each on the tick it asked for, using the simulation's own
per-avatar step ([`AvatarReplay`](../sim/src/settlement/prediction.rs)). The
avatar is drawn at the input horizon, the tick a command sent now would apply
on, so a key press shows within a frame or two. The drawn time follows the
lowest recent horizon on the frame clock, so uneven packets don't move the
avatar and new input never changes a tick already drawn. Remote players and
everything else stay interpolated, so your avatar is drawn about an input lead
ahead of them. See [the prediction](../app/src/prediction.rs).

When the prediction for the drawn tick changes, the difference is kept as a
visual offset that halves every 50 ms (or snaps past 1.5 units), and is logged
as a `mispredict` correction. The replay can't know about late inputs the host
moved to a later tick, hits and knockback, damage, pile jobs, terrain changed
by anyone (your own digs included), respawning after death, or scene changes,
so those still correct. Click-to-move isn't predicted: while one is pending,
the confirmed avatar is drawn.

## Predicted laser shots

A client also draws its own laser shots predicted. When the drawn time
reaches the tick a laser swing resolves on in the replay, two ticks after the
press, the shot is aimed with the simulation's own targeting
(`Settlement::predict_laser`) from the predicted avatar against the confirmed
world, and drawn at once: the beam from the drawn avatar's gun, the laser
sound, and the impact spark and sound. The beam stops at walls, cover, the
ground, and enemies and other players where they are drawn. Damage, flinches,
deaths and cover breaking still wait for the confirmed shot, about an input
lead after the drawn beam hit (if that beam is still in flight then, the
cover waits for it to arrive). See
[the shot matching](../app/src/laser_prediction.rs).

The confirmed `LaserFired` events of your avatar are matched to the drawn
shots in order and not drawn again. If one ends elsewhere, the drawn beam
stays and the results apply where the confirmed shot hit; that is logged as a
`laser` misprediction. A drawn shot the confirmed world is 10 ticks past
without firing (the swing didn't happen there, or the avatar died) is given
up and logged the same way. So is one the replay stops firing once a later
shot resolves: the replay's shots are told apart by their command's aim and
muzzle, so a shot the host moved later stays the same shot. A confirmed shot with nothing drawn for it (fired
while a click-to-move was pending, for example) is drawn when it arrives.
Other players' shots are drawn when confirmed.

A shot fired from the predicted position ends where the confirmed one does
unless the replay was wrong: an input the host moved to a later tick shifts
the shot a tick (about 0.07 units when running), and a target that moves or
cover that breaks between the drawn tick and the shot also changes where it
ends.

On a joined client the mouse aims from the drawn avatar, where its shots fire
from. The host and single player draw their avatar where it is confirmed and
aim from that, as before. Only the aim in the `Attack` command changes; the
protocol does not.

Graphs use local monotonic wall time, with per-frame p95 values over the retained
history. The slowest tick graph has no sample on frames without a tick.
Traffic rates use up to one second of local observations. They are unavailable
until two samples exist. Packet loss, remote assigned ticks on the client, and
per-shot animation and hit timelines are not measured in this first window.

## Feeling a slower network

On Windows, [`tools/network-lab.ps1`](../tools/network-lab.ps1) opens a host
and a client window on one machine, with a localhost relay between them that
delays every datagram:

```powershell
.\kestrel\tools\network-lab.ps1 -DelayMs 75 -JitterMs 20 -Seed 42
```

From cmd, `kestrel\tools\network-lab.bat` takes the same arguments.
`-InputMarginMs 133` pins the client's input margin (it passes
`--input-margin-ms` to the client only), for comparing against the adaptive
one; each run's `run.json` records it (`null` when adaptive).

Each datagram waits `max(0, DelayMs + uniform(-JitterMs, +JitterMs))` ms in
each direction, so round trips grow by about `2 × DelayMs`. It reuses the built
game; `-Build` builds it once first. Each run's parameters, relay counters and
logs land in `tools/network_lab/runs/`. The two windows share one CPU and GPU,
so read frame time next to the network graphs.

## First investigation

1. On localhost, record the input margin and pending command count
   during movement and firing. Observe whether visible hitches coincide with
   missing-turn waits or expensive frames/ticks.
2. Repeat over LAN and the connection with poor feel. Compare ready turns, RTT,
   packet age, catch-up activity, and movement preview offsets.
3. Inspect the host's peer section, or the client's locally detected late
   inputs, for late inputs and shifted ticks. These distinguish inputs missing
   their requested turn from commands intentionally scheduled ahead of time.

Use [NetSim](../net/src/netsim.rs) for repeatable outages, jitter, loss, and spikes.
The [networking tests](../net/src/tests.rs) verify receipt versus application, starvation followed by catch-up,
and that host/client hashes still agree. Per-action shot traces and replayable
desync captures are follow-up work; this window does not export replay evidence.
