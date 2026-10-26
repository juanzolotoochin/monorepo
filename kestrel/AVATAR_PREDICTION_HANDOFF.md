# Kestrel client avatar prediction handoff

## Goal

On a joining client, the local avatar visibly corrects itself when the player
reverses direction or stops. Replace the current leashed movement preview with
real client-side prediction. Every frame, replay the player's own unapplied
movement commands through the simulation's per-avatar step on a copy of the
confirmed avatar. Corrections should then disappear in normal play.

The design was agreed with the user in a design page:
https://claude.ai/artifact/MNbXm3PnqMTLwwUYKrBqtR (the page's D6 and D4 options
were later overridden; this document is authoritative).

Work happens on the branch `network-lab-iteration`. Do it in this order:

1. Write the reproduction test (below), land it **disabled**, and run it
   explicitly to show it detects the clamps and eases today.
2. Implement prediction.
3. Run the same test, show zero corrections, and enable it.

The deterministic simulation, the wire protocol and the host stay unchanged.
Remote players stay interpolated.

## What the player feels, and why

Measured with the correction overlay (Diagnostics → Network → Movement preview
→ **Show corrections in the world**) at `network-lab.bat -DelayMs 75`:

- **Clamp on every reversal.** `follow_avatar` in `app/src/lockstep.rs` draws a
  preview that may lead the confirmed position by at most 0.9 units. The input
  lead is the input margin (133 ms) + one-way time + the jitter buffer: about
  13–17 ticks (~250 ms) at 160 ms RTT. That is ~0.95 u of lead walking (exosuit
  3.8 u/s) and ~1.4 u running (5.6 u/s). After a reversal, the confirmed avatar
  keeps going the old way for the whole lead, and the leash drags the preview
  with it.
- **Ease on every stop.** On release, the preview velocity is zeroed at once
  (the simulation brakes at 32 u/s² instead, ~0.23 u walking, ~0.49 u running).
  The preview then blends toward the confirmed position as it is *now*, which
  is still behind and moving toward it: a pull back, then a pull forward when
  the confirmed avatar arrives and overshoots by its braking distance.

The correction log already exists (`app/src/telemetry/network.rs`):
consecutive frames pulled the same way form one `PreviewCorrection` with kind
ease, clamp or snap. Episodes under 0.05 u are dropped (snaps always kept).
The overlay draws a box for 2 s where the avatar was before each correction.
The copied report lists them under `preview_corrections`.

## Step 1: reproduction test

A new test beside `joined_player_movement_response_is_measured_in_frames` in
the `tests` module of `app/src/lockstep.rs` (part of
`//kestrel/app:movement_test`). It is `#[ignore = "..."]` when it lands. The
prediction change removes the ignore.

### Harness

- Reuse `field_app(Some(Network::Host("127.0.0.1:0"...)), 60.0)` and
  `field_app(Some(Network::Join(address)), 60.0)`. The `field` scene is the
  default networked map and the simplest.
- The network session runs on the app's virtual clock (`service.clock`
  advances by `time.delta()`, and these tests use
  `TimeUpdateStrategy::ManualDuration`). So latency can be added
  deterministically by an **in-test UDP relay** pumped from the test loop on
  the same clock. Do not use wall-clock sleeps or the Node relay.
- The relay mirrors `tools/network_lab/relay.mjs`. The client joins the relay's
  listen address. The relay forwards to the host from a second socket and sends
  replies back from the listen socket. The client checks the reply's source
  address (`net/src/client.rs`, `from != self.host`), so replies must come from
  the address the client sent to. Each datagram is held for
  `max(0, delay + uniform(-jitter, +jitter))` of virtual time, using a small
  seeded PRNG so runs repeat exactly. Use `std::net::UdpSocket` with
  non-blocking reads.
- Per frame: `client.update()`, pump the relay, `host.update()`, pump the relay.
  The relay's clock is frame count × 1/60 s.
- Wait for the client to have an avatar and an active world before starting
  the script, as the existing test does.

### Scenarios (client input only)

Only the client's avatar is predicted. The host's avatar has no input delay and
never corrects. Drive the client with `ButtonInput<KeyCode>` (WASD; run is
`ShiftLeft`):

1. **Base, running:** run right 1 s, run left 0.6 s, release 0.6 s; three times.
2. **Base, walking:** the same without Shift.
3. **Quick taps:** press right for 3 frames, release, wait 0.3 s; ten times.
4. **90° turn:** run right 1 s, then switch to up without releasing.
5. **Zig-zag:** alternate left and right every 150 ms for 2 s.
6. **Speed change:** hold right and toggle Shift every 0.4 s for 2 s.

End each scenario with enough idle frames for the confirmed avatar to settle,
so a final ease is caught.

### Latency cases

Each scenario runs under each case:

- no added delay (input margin only)
- 75 ms ± 20 ms each way
- 100 ms ± 20 ms each way

The user asked for "100 ms each way, in both cases add 20 ms jitter". This
document reads that as the two delayed cases both having ±20 ms jitter. With
the 133 ms input margin, ±20 ms jitter should not make inputs late, so zero
corrections stays achievable.

### Pass condition

No kept correction (the log's 0.05 u threshold) in the client's
`Telemetry.network.corrections()` for any scenario and case. Print a summary
table to the test log for every scenario × case: count, total distance and
largest correction per kind. Also print the total of sub-threshold corrections
so anything left below 0.05 u stays visible. Clear the log between scenarios
(or read it per scenario by time range) so counts are attributed correctly.

### Running it

From PowerShell (Git Bash rewrites `/bin/bash` into a Windows path):

```powershell
bazel test --config=remote --shell_executable=/bin/bash //kestrel/app:movement_test --test_arg=--ignored --test_arg=<test name> --test_output=all
```

`--shell_executable=/bin/bash` is needed on this Windows machine. Without it,
remote Linux genrules (for example `rules_distroless` `libxcb1`) get the local
MSYS bash and fail. Print the BuildBuddy link as soon as it appears and again
with the results (user rule). Record today's failing counts in the commit
message or this document before starting step 2.

### Baseline (leashed preview, before prediction)

The test is `lockstep::tests::joined_player_movement_needs_no_corrections_under_latency`.
Each case is a fresh host and client joined through the relay. Each scenario
starts with a respawn and 1.5 s of idle, then clears the log. Kept
corrections, as count (total distance, largest):

| Case | Scenario | Kept | Under 0.05 u |
|---|---|---|---|
| no added delay | base running | clamp 3 (0.26, 0.13), ease 1 (0.87) | 5 (0.167 u) |
| no added delay | base walking | none | 0 |
| no added delay | quick taps | none | 0 |
| no added delay | 90° turn | none | 3 (0.101 u) |
| no added delay | zig-zag | none | 0 |
| no added delay | speed change | none | 1 (0.033 u) |
| 75 ± 20 ms | base running | clamp 6 (7.87, 1.59), ease 3 (1.22, 0.76) | 0 |
| 75 ± 20 ms | base walking | clamp 6 (2.92, 0.73), ease 3 (0.81, 0.30) | 0 |
| 75 ± 20 ms | quick taps | none | 1 (0.033 u) |
| 75 ± 20 ms | 90° turn | clamp 3 (2.19, 1.10), ease 1 (0.90) | 0 |
| 75 ± 20 ms | zig-zag | clamp 2 (5.42, 5.28), ease 1 (0.90) | 0 |
| 75 ± 20 ms | speed change | clamp 2 (0.93, 0.87), ease 1 (0.90) | 0 |
| 100 ± 20 ms | base running | clamp 6 (8.64, 1.74), ease 3 (1.17, 0.73) | 0 |
| 100 ± 20 ms | base walking | clamp 6 (3.38, 0.85), ease 3 (0.92, 0.36) | 0 |
| 100 ± 20 ms | quick taps | ease 1 (0.19) | 0 |
| 100 ± 20 ms | 90° turn | clamp 3 (2.52, 1.23), ease 1 (0.90) | 0 |
| 100 ± 20 ms | zig-zag | clamp 2 (5.43, 5.14), ease 1 (0.90) | 0 |
| 100 ± 20 ms | speed change | clamp 1 (0.97), ease 1 (0.90) | 1 (0.033 u) |

## Step 2: prediction (agreed decisions)

### D1: draw at the input horizon, replaying in simulation ticks

- The confirmed world on the client is at tick T. Each pending command has a
  requested tick. The client computes it as
  `host_tick + one_way + input_delay` (`net/src/client.rs`, `queue`).
- Replay from the confirmed avatar at T to the horizon H: the tick a command
  sent now would apply at. Apply each pending command on its requested tick.
  A pending command whose requested tick is ≤ T was moved later by the host;
  apply it at T + 1, which is the best guess available.
- Draw the avatar between the predicted states at H − 1 and H, using the
  frame's fraction, so it moves smoothly.
- Your avatar is then drawn about one input lead ahead of other players and
  enemies. That is accepted.
- **Pitfall:** H as computed from the last host packet can jump by more than
  one tick when packets arrive unevenly. A jump moves the drawn tick, not the
  prediction. Advance the drawn tick smoothly with the local clock. Measure
  mispredictions by comparing predictions **for the same tick**: the state
  previously predicted for tick t against the state now predicted for t, or
  against the confirmed state once t is confirmed. Do not compare drawn
  positions frame to frame, or horizon jitter will look like mispredictions.
  Cap the replay (for example 60 ticks past T); beyond it, draw the confirmed
  position.

### D2: pending commands come from the net client

- Today `Client::unacked` (`net/src/client.rs`) drops inputs on the host's
  receipt acknowledgement, which is before the client simulates them.
  `pending_inputs()` is `queued - applied`. `applied` grows as this player's
  commands appear in turns (around line 236), in send order.
- Keep each sent input (requested tick + `CommandKind`) in a separate queue
  until it is applied: drop from the front as `applied` grows, and clear it
  where `queued`/`applied` reset on `Welcome`. Expose it through
  `session::Session` as `pending_commands()` returning
  `(requested_tick, CommandKind)`; a host or replay returns none.
- This is an API change in the `net` and `session` crates only. The wire
  format does not change.

### D3: replay the whole per-avatar step except pile jobs

- `self_update` in `sim/src/settlement/avatar.rs` is the per-avatar step:
  respawn, pile job, `walk`, then `fly` or `jump_step`. It is `pub(super)` and
  works on one `AgentState` plus terrain and the character definition.
- Add a read-only prediction entry point in `sim` (for example on
  `Settlement`). It clones the player's `AgentState`, applies pending commands'
  **effect on this avatar only** at their ticks, and runs the per-avatar step
  for each tick, skipping pile jobs. Commands are applied in
  `sim/src/world.rs` (around lines 414–450).
  - `MoveAvatar` with `Walk`/`Stop`: the same intent logic as
    `Settlement::move_avatar`, minus pile-job cancel.
  - `Jump` and `Jetpack`: the avatar part only.
  - `Attack`: set the swing so sword swings lock movement. No damage, events,
    projectiles or messages.
  - `DigDown` / `Dig`: the dig lock, without changing terrain or spawning ore.
  - `AimAvatar` and `ReleaseDirectControl`: the avatar part.
- It must never write to the real world. Use the same fixed-point code as the
  simulation, so the replay matches the host exactly when inputs apply on their
  requested ticks.
- Things the replay cannot know stay corrections: late inputs moved by the
  host, hit rebounds and knockback, pile jobs, terrain changed by other
  players, respawns and scene changes.

### D4: click-to-move is not predicted

Right-click move (`Intent::GoTo`) is going away. Do not replay it or plan
routes in the prediction. While it still exists, a pending GoTo can simply
show the confirmed position. The routed part of
`joined_player_movement_response_is_measured_in_frames` ("routed movement gave
no immediate visual response", "routed movement did not show prediction
before confirmation") will need to be removed or relaxed accordingly.

### D5: decay visual error, snap past 1.5 u

- When the prediction for the drawn tick changes (a misprediction), keep the
  difference as a visual offset on the drawn transform only. Halve it every
  50 ms. Snap if it exceeds 1.5 u.
- Log each one in the correction log as a new kind (for example `mispredict`)
  with its starting error, in the overlay, the window list and the copied
  report. Ease and clamp go away with the old preview. Keep snap for
  teleports.

### D6: remove the old preview now

Delete the leash, ease and stop logic in `follow_avatar` and `LocalAvatarPreview`.
No toggle and no side-by-side comparison.

### Other notes

- The ground-step smoothing and heading logic in `follow_avatar` should keep
  working on the predicted position. The heading comes from preview velocity
  today, so use the predicted velocity instead.
- Update `docs/network-diagnostics.md` (Movement preview and Preview
  corrections) to describe prediction and the new correction kind.
- Add a `sim` test: for random command sequences, the replayed avatar at tick
  H equals the real avatar after the world reaches H.

## Step 3: verify and enable

Run the reproduction test with `--test_arg=--ignored` again. Every scenario ×
case should show zero kept corrections. Then remove the `#[ignore]`, run the
whole `//kestrel/app:movement_test`, and check the result by feel with
`kestrel\tools\network-lab.bat -Build -DelayMs 75 -JitterMs 20` with the
overlay on. Reversing should draw no boxes.

## Rules from the user and repository

- Bazel: always `--config=remote`; never `--config=full_local` or disable
  BuildBuddy. Run tests with `bazel test`, never `bazel run` on a test. Keep
  `.bazelrc` defaults; explain any extra flag (see `kestrel/AGENTS.md`).
- Print the BuildBuddy invocation link as soon as it is available, and again
  with the results.
- Bazel commands must run outside the sandbox on this machine. Use
  `git -c core.fsmonitor=false` if git reports an fsmonitor IPC error.
- Leave any game windows the user has open alone.
- Commit on `network-lab-iteration`. Do not open a PR unless asked.
