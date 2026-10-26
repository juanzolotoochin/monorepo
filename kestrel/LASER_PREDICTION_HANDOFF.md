# Kestrel laser prediction and adaptive input margin handoff

## Goal and order

On a joining client, the laser beam and its sound appear only when the
confirmed simulation fires the shot: the input lead plus a 2-tick windup,
about 250–320 ms at 160 ms RTT, and still about 170–230 ms on localhost. The
user's complaint is specifically the **beam appearing late**.

This is step 2 of the client-feel work. Do the steps in this order, each
finished and tested before the next:

1. **Avatar prediction** (`kestrel/AVATAR_PREDICTION_HANDOFF.md`). Required
   first: B uses its replay.
2. **B: laser prediction.** Draw the beam from the predicted avatar when it
   fires.
3. **C: adaptive input margin.** The client sizes its input margin from the
   measured network instead of the fixed 133 ms.

The design was agreed with the user on a design page:
https://claude.ai/artifact/NnxUtKTmeQgwhY6TL4WfgA. This document is
authoritative where they differ.

The deterministic simulation and the wire protocol stay unchanged. Work on the
branch `network-lab-iteration`.

## How a laser shot works today

1. `PlayerControl::strike` (`app/src/lockstep.rs`) queues
   `CommandKind::Attack { agent, weapon, aim, muzzle }` and starts the local
   arm animation (`Controls::strike`, `app/src/gameplay.rs`).
2. When the command applies, `start_swing` (`sim/src/settlement/avatar.rs`)
   sets `avatar.swing` with `resolve_at = tick + 2` for the laser
   (`ticks_from_60hz(2)`) and `until = tick + 8`. The laser does not lock
   movement.
3. On `resolve_at`, `read_avatar` (same file) calls
   `Settlement::strike_targets(me, a.position, swing.aim, swing.weapon,
   terrain)` (`sim/src/settlement/combat.rs`). It returns threat targets, the
   beam end and the cover index. Laser targeting checks static obstacles,
   destructible cover, ground and other players (`laser_player`). It is gated
   by `a.attack_ready <= tick` and `!swing.resolved`.
4. Combat resolution emits `Event::LaserFired { agent, from, to, hit_actor,
   cover }`, then `Event::Hit` for damage.
5. In the app, `app/src/settlement.rs` (around line 341) turns `LaserFired`
   into a `PendingLasers` entry and calls `CoverContacts::begin` for cover hits.
   The laser sound cue is played from the same event (around line 468).
   `effects::emit_lasers` spawns the `LaserBolt` from the drawn avatar's weapon
   muzzle marker to the event's end point. The bolt's impact plays sparks and
   `Cue::LaserImpact` (`app/src/effects.rs`, around line 547).

## Step 2: B, the predicted shot

When the step-1 replay reaches a laser swing's `resolve_at` tick, compute the
shot as the simulation would and draw it immediately.

- **Targeting.** Run the simulation's own laser targeting, `strike_targets`
  with the predicted avatar's position, height and aim, against the
  **confirmed** settlement and terrain. `strike_targets` reads the shooter's
  height from `self.agents[shooter]`. Either run it on a copy of the confirmed
  settlement with the player's agent replaced by the predicted `AgentState`, or
  add a variant that takes the shooter's state. Shots are rare, so one
  settlement clone per shot is fine. Expose this through the read-only
  prediction entry point added in step 1; never write to the real world.
- **Replay must also replicate attack gating:** the swing only resolves if
  `attack_ready <= tick` and it hasn't resolved yet. Apply `Attack` in the
  replay with the same `attack`/`start_swing` logic on the cloned agent,
  skipping world effects.

### Agreed decisions

- **B1. What the predicted beam stops at: walls, cover, ground and also
  enemies and other players as drawn.** The beam ends on the target the player
  sees. When that target moves between the drawn (confirmed) tick and the
  predicted tick, the beam shows a stop the confirmed shot doesn't make. That
  is accepted and logged (B3).
- **B2. Immediate effects: beam, laser sound, impact spark and impact sound**
  at the predicted end. Damage, enemy flinch, death and cover breaking wait for
  the confirmed shot. Cover already stays intact while a hit is in flight
  (`CoverContacts`). Make sure a predicted cover impact doesn't break or begin
  the cover early; only the confirmed `LaserFired` drives `CoverContacts`.
- **B3. When the confirmed shot ends elsewhere: keep the predicted beam and
  apply results where the confirmed shot hit.** Nothing is redrawn. Log each
  disagreement as a laser misprediction in the correction log
  (`app/src/telemetry/network.rs`): a new kind next to step 1's movement
  mispredict, with both end points, shown in the overlay, the window list and
  the copied report.
- **B4. When the beam appears: at the predicted fire tick,** 2 ticks (33 ms)
  after the press, consistent with the replay. The beam comes out of the gun
  where the drawn avatar is at that tick.

### Matching predicted and confirmed shots

The local player's confirmed `LaserFired` events must not spawn a second beam
or sound when a predicted shot already did. Match them in order: the n-th
predicted laser shot against the n-th confirmed `LaserFired` for the local
agent. Handle both mismatches:

- A predicted shot that never fires (gating differed, the avatar died, the host
  moved the input) times out after a few ticks past its expected tick, plus the
  input lead. Log it as a laser misprediction.
- A confirmed shot with no predicted match is drawn as today.

Other players' shots are unchanged.

## Step 3: C, adaptive input margin

### Why the margin matters

The client requests `host_tick + one_way + input_delay` for each input
(`net/src/client.rs`, `queue`). `host_tick` comes from the last `Turns` packet
and is already one one-way trip old, so `+ one_way` only brings it to the
host's present. The input then needs another one-way trip to reach the host,
which the margin pays for. An input is late whenever `one_way + jitter >
margin`. The host then moves it to its open tick (`net/src/host.rs`,
`inputs`, counting `late_inputs` / `shifted_ticks`). With step 1, every late
input is a misprediction.

Rough effect, from the formulas, not measured:
- LAN: about 80 ms sooner to the confirmed result.
- 75–100 ms one way: 0–16 ms.
- Beyond ~120 ms one way: today every input is late; adaptive keeps them on
  time.

### Agreed decisions

- **C1. Adaptive, with a command-line override.** The client computes
  `margin = one_way + safety`. A flag (`--input-margin-ms`) pins it. The
  `input_delay` the host sends in `Welcome` becomes the starting value until
  RTT is measured. This is client-side only: the host already accepts any
  requested tick. Parse the flag in `app/src/main.rs` and pass it through
  `NetConfig` / `lockstep.rs` (around line 167) to the client. The host's own
  inputs are unaffected (it applies them on its next tick).
- **C2. Safety from measured jitter, at least 1 tick.** For example twice the
  mean deviation of recent RTT samples, tracked next to the smoothed RTT
  (`client.rs`, around line 352; similar to TCP's RTTVAR).
- **C3. Grow at once on a late input, shrink slowly.**
  - The client detects its own late inputs without a protocol change: its
    commands appear in turns at a tick, and step 1's pending-command queue
    knows each one's requested tick. Applied later than requested means late.
  - Grow the margin immediately on a late input. Shrink by 1 tick after a few
    seconds without late inputs.
  - **Requested ticks never go backwards.** Clamp each requested tick to at
    least the previous input's. Otherwise a shrinking margin could reorder
    inputs (a stop applied before the walk it follows).
- **C4. Leave the 67 ms jitter buffer alone** for now. Revisit after measuring
  with the margin change in place.

### Tooling and diagnostics

- **network-lab:** add `-InputMarginMs` to `tools/network-lab.ps1`, which
  passes `--input-margin-ms` to the client process only. Validate it like the
  other numeric parameters, record it in `run.json`, and print it in the
  summary. The `.bat` wrapper passes arguments through and needs no change.
  Mention it in `docs/network-diagnostics.md`.
- **Network window:** the "Input scheduling margin" row (`d.input_delay`) shows
  the adaptive value as it changes. Add a client row "Late inputs (detected
  locally)" with count and ticks shifted.
- **F3 overlay:** add the margin to the client's `NET` line
  (`network_line` in `app/src/telemetry.rs`), for example
  `NET RTT 152 ms / last 160 · margin 117 ms`, and update its test.

## Tests

Extend the step-1 reproduction test (`app/src/lockstep.rs` tests; in-test
delay relay on the virtual clock; latency cases no added delay, 75 ± 20 ms and
100 ± 20 ms).

- **Laser scenarios (after B):** fire at a wall or obstacle while standing
  still, while running, and right after reversing. Pass condition: zero laser
  mispredictions; the predicted beam ends where the confirmed one does. `field`
  has no enemies; moving targets are out of scope for now (the user may revisit
  for accuracy testing).
- **Margin cases (after C):**
  - Add a 150 ± 20 ms case. Today every input is late there. With C, it shows
    no late inputs and zero movement corrections.
  - Add a LAN case (no added delay) checking that the margin settles near the
    one-way time plus 1 tick, well under 133 ms.
- **Unit tests for C** in the `net` crate: the margin grows on a detected late
  input, shrinks only after the quiet period, never drops below one-way + 1
  tick, honours the override, and requested ticks stay monotonic across a
  shrink.

Run tests from PowerShell:

```powershell
bazel test --config=remote --shell_executable=/bin/bash //kestrel/app:movement_test --test_output=all
```

`--shell_executable=/bin/bash` is needed on this Windows machine: without it,
remote Linux genrules get the local MSYS bash and fail. Git Bash rewrites
`/bin/bash` into a Windows path, which is why the command is run from
PowerShell. Run `net` crate tests the same way with their own target.

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
