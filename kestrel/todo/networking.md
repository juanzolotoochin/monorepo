# Networking

## Cap how far ahead a client may request a tick

- **Status:** open
- **Why:** each input carries the tick the client wants it applied on, and
  the host files it under any tick that hasn't run yet, with no upper bound
  (`Host::inputs`, `net/src/host.rs`). A broken or hostile client could
  request ticks far in the future: the host keeps those commands in `open`
  until then, which grows memory without limit and lets a client schedule
  actions long in advance. Harmless with our own client, but worth closing
  before playing with strangers.
- **What to do:** reject or clamp inputs whose requested tick is more than
  a limit past the host's open tick (for example 2 s, well above any sane
  input margin), and count them in the remote's diagnostics next to
  `late_inputs`. Decide whether to clamp (keep the input) or drop it. Add a
  `net` test for both an over-limit input and one just under the limit.
- **Where:** `Host::inputs` in `kestrel/net/src/host.rs`; diagnostics in
  `kestrel/net/src/lib.rs`; Network window rows in
  `kestrel/app/src/telemetry/network.rs`.
- **Found in:** review of the adaptive input margin (PR #111).

## Let the input margin shrink back faster after a lag spike

- **Status:** open
- **Why:** the adaptive margin grows at once on a late input but shrinks by
  only 1 tick after each 3 s quiet period. After a 250 ms lag spike it takes
  about 40 s to return to normal, and confirmed results (hits, damage) arrive
  later than needed the whole time.
- **What to do:** shrink faster when the measured one-way time plus safety
  is far below the current margin (for example several ticks per quiet
  period, or straight to the measured value plus a tick), while keeping
  requested ticks monotonic. Check it doesn't oscillate (grow, shrink, late,
  grow) at 100 and 150 ± 20 ms.
- **Where:** `InputMargin` in `kestrel/net/src/margin.rs`; its `net` tests.
- **Found in:** stage 3 of PR #111 (notes in `kestrel/LASER_PREDICTION_HANDOFF.md`).

## Bring the input margin closer to the old fixed value at 75–100 ms

- **Status:** open
- **Why:** at 75–100 ms one way the adaptive margin settles 1–4 ticks above
  the old fixed 8, so confirmed results arrive 17–67 ms later there.
  Prediction hides it for the player's own movement and shots, not for hits
  on others.
- **What to do:** measure how much safety is actually needed (it is 1 tick
  plus 4× the late-side deviation; 3× was also clean in tests) on
  network-lab runs, and tune it down only if late inputs stay at zero.
- **Where:** `kestrel/net/src/margin.rs`; network-lab runs with
  `-DelayMs 75/100 -JitterMs 20`.
- **Found in:** stage 3 of PR #111.

## Re-measure the 67 ms jitter buffer

- **Status:** open
- **Why:** decision C4 left the client's jitter buffer alone until the
  adaptive margin landed. Now that it has, the buffer may be larger than
  needed, and it delays everything the client draws as confirmed (other
  players, enemies, hits).
- **What to do:** measure buffer underruns on network-lab runs (LAN, 75,
  100, 150 ± 20 ms) and decide whether to shrink it or make it adaptive
  like the margin.
- **Where:** the client's turn buffering in `kestrel/net/src/client.rs`;
  `kestrel/docs/network-diagnostics.md`.
- **Found in:** decision C4 in `kestrel/LASER_PREDICTION_HANDOFF.md`.

## Test laser prediction against moving targets and broken cover

- **Status:** open
- **Why:** the laser tests run in the laser pit, which has no enemies, so
  decision B1 (the predicted beam stops at targets as drawn) and cover
  broken just before a shot are untested. Those are the cases where the
  predicted and confirmed beams can disagree.
- **What to do:** add scenarios with a moving enemy or a second player
  crossing the line of fire, and with cover broken a few ticks before the
  shot. Measure how often a laser mispredict is logged and how far apart the
  ends are; decide whether that is acceptable.
- **Where:** laser scenarios in the `kestrel/app/src/lockstep.rs` tests;
  `kestrel/app/src/event_prediction.rs`.
- **Found in:** stage 2 of PR #111.

## The `Join` command is always late on a delayed link

- **Status:** open (low priority)
- **Why:** a joining client sends `Join` before it has a round-trip
  measurement, so on a delayed link the host always moves it to a later
  tick. Harmless today, but it counts as a late input in diagnostics and
  can hide real ones.
- **What to do:** either don't count `Join` as late, or send it only once
  the first round trip is measured.
- **Where:** `kestrel/net/src/client.rs` (queue, late-input detection);
  `Host::inputs` in `kestrel/net/src/host.rs`.
- **Found in:** stage 3 of PR #111.

## Done
