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
  mispredicted frames, snaps and knockback frames. These are view observations, not
  deterministic state mismatches. A line per event kind (`laser`)
  counts the client's predicted events: drawn, confirmed, mispredicted, and
  how long before its confirmation the last one was drawn (about one input
  lead).
- **Preview corrections:** a misprediction moves the drawn avatar; consecutive
  mispredicted frames form one `mispredict` correction, whose distance is the
  sum of each frame's starting error. A `snap` is a correction over 1.5 units.
  Corrections under 0.05 units are dropped, except snaps, but their count and
  total distance are shown. **Show corrections in the world** draws, for two
  seconds, a box where the avatar was drawn when the correction started and a
  line to where the new prediction pulls it. The copied report lists them
  under `preview_corrections`, with duration, distance, positions, local tick,
  pending inputs and RTT at the start.
- **Knockbacks:** a change of prediction that something the replay never
  runs explains (a hit, a death, a push: the simulation's stage 5; see
  [movement prediction](#movement-prediction)) is a `knockback`, not a
  misprediction. It slides back over a longer offset (halving every 100 ms,
  snapping past 3 units) and is always kept, drawn in blue. It is left out
  of the mispredicted frames, snaps, small corrections and the
  corrections count; the window counts knockback frames and kept
  knockbacks on their own. In the copied report it is listed under
  `preview_corrections` with `kind: "knockback"` and `snapped`, and counted
  in `preview_knockbacks` and `preview_knockback_frames`; the top-level
  `preview_corrections` counts mispredictions only.
- **Event mispredictions:** a correction named by an event kind (`laser`)
  is one predicted event (see below) that the confirmed one didn't
  match: it ended elsewhere, by any distance, or never confirmed. It is
  always kept. In the world it is a red ball where the presented event ended
  (a laser beam's end; for an event without an end, where the avatar was
  drawn) and a line to where the confirmed one ended. In the copied report
  it has `event_mispredict: true`, `from` is the presented end, `to` the
  confirmed end (the same point when `never_fired` is true) and `distance`
  the gap between them. The report's top level has, per kind,
  `predicted_events.<kind>` with `presented`, `confirmed`, `mispredicted` and
  `last_lead_ms`, and for the laser also `laser_shots_drawn`,
  `laser_shots_confirmed`, `laser_mispredictions` and `laser_last_lead_ms`.

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
moved to a later tick, hits and knockback, damage, deaths, pile jobs, terrain
changed by anyone (your own digs included), or scene changes, so those still
correct.

Hits, deaths and pushes are the simulation's stage 5, which the replay never
runs, so the correction they cause is expected: a `knockback`, not a
misprediction. Its offset halves every 100 ms and snaps past 3 units. A
client tells them apart by checking each confirmed tick against the replay
([`StageFive`](../app/src/prediction.rs)): if the avatar after the tick isn't
what replaying that tick from the confirmed world before it gives, with the
commands that really applied on it, something the replay doesn't run touched
it. A change of prediction on the frame such a tick is confirmed is a
knockback; a late input confirmed on the same frame is counted with it. A
late input alone is not a touch: the check uses the tick the host applied it
on. A tick run on terrain that has changed since isn't checked, so terrain
changes stay mispredictions. Today a laser hit from another player only
lowers health, which moves nothing; a death stops the avatar where it was
hit, which is a knockback. The respawn two seconds later is the avatar's own
step, so the replay draws it on its tick, a teleport with no correction (a
`Respawn` command is predicted the same way; if the host moves it later, that
is a snap).

Pending commands go through the same actions the
simulation applies (`settlement::action`), so walks to a point (the depot
dock walk, scripted walks) are predicted too: the route is planned from the
confirmed terrain when the walk starts. The units it then moves at the dock are not.
Pile jobs read pile lab state the replay doesn't run, so they aren't
predicted: while one is pending, the confirmed avatar is drawn.

Everything about your avatar that lasts is drawn from the drawn avatar's
state ([`DrawnAvatar`](../app/src/prediction.rs)), not from the confirmed
world: the jetpack flame, thrust and landing sounds and the flight HUD, the
jump crouch, the dig pose, bar, dust and chip sound, footsteps, and facing.
So they start with the predicted avatar, about an input lead before the
confirmed one, and when a prediction is withdrawn or corrected they end or
change on the same frame. A swing starts on the press, before even the
drawn avatar swings; once the drawn avatar swings for it, the swing takes
that swing's stroke (the sword combo is the avatar's), and if the drawn
avatar shows no swing for it two ticks after the tick it asked for (the
simulation refused it: you were digging, dead, or the scene doesn't offer
the weapon), it ends. On the host and in single player the drawn avatar is
the confirmed one.

## Predicted events

A client also presents some of its own avatar's events predicted: so far,
laser shots. The replay reads the confirmed world for the predicted avatar
as the simulation's stage 3 does (`read_avatar`), and the resulting messages
become events through the same function the simulation applies them with
(`strike_event`). When the drawn time first reaches one, it is presented at
once. A laser shot, two ticks after the press, is drawn from the drawn
avatar's gun with the laser sound, and the impact spark and sound. The beam
stops at walls, cover, the ground, and enemies and other players where they
are drawn. Damage, flinches, deaths and cover breaking still wait for the
confirmed event, about an input lead after (if a drawn beam is still in
flight then, the cover waits for it to arrive). Bow shots and the arc coil's
arc are predicted too, but still presented only once confirmed: the arrow is
a projectile in the confirmed world, so its sound an input lead before the
arrow appears would feel worse than late, and the arc chains into the hits
that follow it. See
[the predicted-events ledger](../app/src/event_prediction.rs).

Each predicted event is told apart by the command that caused it and its
kind. Its start tick is the tick its command applied on: in the replay, for
a command still pending, and in the confirmed world, from the client's
record of which of its commands applied on which tick
(`net::Client::applied_commands`). So a shot the host moved later stays the
same shot, and two presses with the same aim are still two shots. Several
commands applied on one tick are told apart in order.

The confirmed events of your avatar are matched to the presented ones and
not presented again. If one ends elsewhere, the drawn beam stays and the
results apply where the confirmed shot hit; that is logged as a misprediction
of its kind (a `laser` correction). An event the confirmed world is 10 ticks
past without confirming (the swing didn't happen there, or the avatar died)
is given up and logged the same way. So is one the replay stops showing once
a later command's event of its kind shows. A confirmed event with nothing
presented for it (fired while a pile job was pending, for example) is
presented when it arrives. Other players' events are presented when
confirmed.

Each kind has a miss policy. Laser shots are `Keep`: what was
presented stays. An event that starts something lasting would be `Cancel`:
the presenter stops it when it misses, and presents the confirmed one
instead. No event is `Cancel`: lasting effects are drawn from the drawn
avatar's state instead (see above).

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
