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

## Notes from stage 1 (avatar prediction)

Stage 1 landed on `kestrel-avatar-prediction`. See
`AVATAR_PREDICTION_HANDOFF.md` for its results.

### What exists

- `Session::pending_commands()` (`session/src/lib.rs`) wraps
  `Client::pending_commands()` (`net/src/client.rs`). It returns this
  client's commands that haven't been simulated here yet, oldest first, as
  `(requested_tick, &CommandKind)`. A host or replay returns none.
- `Session::input_horizon()` wraps `Client::input_horizon()`: the tick a
  command queued now asks for, `host_tick + one_way + input_delay`. `queue`
  uses it too, so it is the one place to change the margin.
- `Settlement::replay_avatar(agent, tick, terrain) -> Option<AvatarReplay>`
  (`sim/src/settlement/prediction.rs`). The replay copies one player avatar
  and never writes to the world. It provides:
  - `apply(&CommandKind) -> bool`: the command's effect on this avatar only.
    It returns false for GoTo and Pile commands, which it doesn't predict.
  - `step()`: runs one tick.
  - `step_with(commands)`: applies the commands, then runs the tick.
  - `state()`, `tick()`, `settlement()` and `terrain()`.
- The avatar's part of each command and of housekeeping is now in shared
  `pub(super)` functions in `sim/src/settlement/avatar.rs`, used by both the
  real sim and the replay:
  - `set_intent`, `attack`, `start_queued`, `jump`, `jetpack`, `dig`,
    `finish_dig`, `release`, `respawn`;
  - `resolving(a, tick)`: the strike gate `read_avatar` uses (`!resolved`,
    `resolve_at == tick`, `attack_ready <= tick`);
  - `swing_resolved(a, tick, hits, def)`.
- App: `prediction::AvatarPrediction` (`app/src/prediction.rs`) is a `Local`
  of `follow_avatar` (`app/src/lockstep.rs`), which calls it through
  `predict`. Every frame, `replay()` in `prediction.rs` replays the avatar
  from the confirmed tick T to the drawn tick. Each pending command applies
  on `max(requested, T)`.
- Drawn time: the avatar is drawn between the predicted states at
  `floor(drawn)` and `floor(drawn) + 1`. The drawn time follows the lowest
  horizon of the last second on the frame clock, minus one frame and half a
  tick, so a new command never changes a tick already drawn.
- Corrections: a misprediction (the prediction for the drawn tick changed)
  becomes a visual offset that halves every 50 ms and snaps past 1.5 u. It
  is logged as `CorrectionKind::Mispredict` in `app/src/telemetry/network.rs`.
  Add B3's laser misprediction kind next to it.

### Hooking in the predicted shot (B)

- `AvatarReplay::apply` already runs `Attack` through the sim's own
  `avatar::attack` and `start_swing`, with the `has_weapon` and alive checks.
  A press during a swing is queued, and `step()` starts it the same way
  `avatar_housekeeping` does.
- `step()` runs the avatar's own step, then the `resolving()` check. On a
  resolving swing it sets `attack_ready = tick + cooldown` and marks the swing
  resolved, with hits assumed 0 (a bow shot counts as 1). So attack gating is
  already replicated.
- To get the shot, have `step()` report what `resolving()` returned (the
  swing) and the `AgentState` right after the avatar's own step, before it is
  marked resolved. That is the state the real sim passes to
  `strike_targets(me, a.position, swing.aim, swing.weapon, terrain)`.
- Run `strike_targets` against `replay.settlement()` and `replay.terrain()`
  with this player's agent replaced by that state, because `strike_targets`
  reads the shooter's height from `agents[shooter]`. Either clone the
  settlement once per shot or add a variant that takes the shooter's state.
- The app's `replay()` replays the same ticks again every frame, so a
  predicted shot is seen on many frames. Deduplicate by its tick, and emit it
  once, when the drawn time first passes `resolve_at` (B4).
- `attack_input` (`lockstep.rs`) still aims from the confirmed position
  (`player_view_position`). The drawn avatar is about an input lead ahead, so
  B4's "the beam comes out where the drawn avatar is" may need the aim taken
  from the drawn position. That changes the gameplay command, so decide it
  deliberately.
- A pending click-to-move makes `apply` return false, and the app then draws
  the confirmed avatar. A shot predicted during that time has nothing to
  start from.

### For C (adaptive input margin)

- Detect late inputs at the drain in `Client::update` (`net/src/client.rs`,
  `self.pending.drain(..mine…)`). Each drained entry's requested tick against
  the tick whose turn contained it is exactly "applied later than requested".
- The drain assumes a player's commands apply in send order, so requested
  ticks must never go backwards. C3's clamp, at least the previous input's
  tick, guarantees that. Today a drop in the one-way estimate between two
  sends could break it (not observed in tests).
- Change the margin in `Client::input_horizon` only. Prediction reads the
  same value through `Session::input_horizon`, so the drawn tick follows
  automatically.
- The reproduction test is split in `app/src/lockstep.rs` tests:
  `joined_player_movement_needs_no_corrections_under_latency` (no delay and
  75 ± 20 ms) is enabled. `joined_player_movement_needs_no_corrections_at_100_ms`
  is `#[ignore]`d only because the fixed 133 ms margin lets an occasional
  input arrive one tick late at 100 ± 20 ms. On the seeded run, zig-zag has 1
  late input and 1 kept 0.08 u mispredict. Once C lands, remove that ignore,
  keeping the same pass condition and seed. The test already prints host late
  inputs and client RTT per case. The planned 150 ± 20 ms and LAN cases fit
  `assert_no_corrections`.

## Notes from stage 2 (laser prediction)

Stage 2 (B) landed on `kestrel-laser-prediction`, stacked on stage 1.

### What landed

- **Sim** (`sim/src/settlement/prediction.rs`): `AvatarReplay::step` records
  the strike that resolved on the tick (`struck() -> Option<&ReplayedStrike>`:
  tick, weapon, aim, muzzle, and the position and height after the avatar's
  own step, which is what `read_avatar` reads).
  `Settlement::predict_laser(agent, &ReplayedStrike, terrain) ->
  Option<PredictedLaser>` returns `LaserFired`'s `from`, `to`, `hit_actor`
  and `cover`. No settlement clone: `strike_targets` and `laser_player` now
  take the shooter's height (`strike_targets_from`, `combat.rs`), and the
  `hit_actor` check is shared (`laser_hits_actor`). Golden tests unchanged.
- **App**: `prediction::Drawn::strikes` lists the strikes the replay resolved
  through the drawn tick. `laser_prediction::LaserPrediction` (a resource)
  keeps the drawn shots not yet confirmed, in order. Each client frame,
  `predict_lasers` in `lockstep.rs` (called from `follow_avatar`):
  1. matches this frame's confirmed `LaserFired` of the local avatar to the
     oldest drawn shot (exact `FxVec2` compare of the ends);
  2. gives up drawn shots the confirmed tick is `TIMEOUT_TICKS` (10) past;
  3. maps the replay's laser strikes, in order, onto the outstanding drawn
     shots (an input the host moved later moves its shot's tick instead of
     drawing another), and draws the rest: a `LaserShot` with
     `predicted: Some(id)`, `Cue::Laser`, `laser_drawn()`.
  `settlement.rs` skips the beam and sound of a matched confirmed shot. A
  predicted bolt sparks on cover but holds no `CoverContacts`; a matched
  confirmed cover hit goes through `effects::hand_over_cover`, which holds
  the cover only while the drawn bolt is still flying.
- **Telemetry**: `CorrectionKind::LaserMispredict` ("laser"), always kept,
  `from` = drawn end, `to` = confirmed end, `never_fired`. Window line with
  drawn / confirmed / mispredicted counts and the last lead; report fields
  `laser_shots_drawn`, `laser_shots_confirmed`, `laser_mispredictions`,
  `laser_last_lead_ms`.
- **Aim** (arbiter decision): `attack_input` aims from the drawn avatar on a
  client (`aim_origin`); host and single player still use
  `player_view_position()` exactly. Wire format unchanged.

### Tests

- Sim: `predicted_laser_matches_the_simulation_with_a_jetpack` and
  `..._with_jumps` fire the laser at random through rocks, cover, a ledge and
  a second player for 3 seeds each and assert every replayed shot equals
  `LaserFired` (compared when the rest of the world the targeting reads
  didn't change, about 300 per seed); `only_a_laser_strike_predicts_a_shot`.
- App units: `laser_prediction::tests` (matching, dedup across frames, a late
  input moving a shot, the timeout logged once, reset),
  `laser_mispredictions_are_kept_with_both_ends_and_reported`,
  `a_confirmed_cover_hit_waits_for_the_predicted_beam_still_in_flight`,
  `mouse_aim_starts_at_the_drawn_avatar_only_when_it_is_predicted`.
- Harness (`lockstep.rs`): `joined_player_laser_shots_are_predicted_under_latency`
  (no delay, 75 ± 20 ms; enabled) and
  `joined_player_laser_shots_are_predicted_at_100_ms` (`#[ignore]`d), in the
  laser pit (`field` offers no laser): standing still (5 shots, one on the
  mixing desk), running (4), right after reversing (6, the second of each pair
  queued during the first swing). The host's avatar is walked into the
  southwest corner first. Pass: every shot confirmed, drawn early, exactly one
  beam and one `Cue::Laser` per shot, all beams predicted, ends equal exactly
  (tolerance 0: same fixed-point code), lead > 0, no correction of any kind.

| Case | Laser mispredictions | End difference | Drawn before confirmation |
|---|---|---|---|
| no added delay (RTT 17 ms) | 0 / 15 shots | 0 | 150 ms |
| 75 ± 20 ms (RTT 174 ms) | 0 / 15 | 0 | 283 ms |
| 100 ± 20 ms (RTT 241 ms), committed seed | 0 / 15 | 0 | 283–300 ms |

Six more seeds per delayed case, not committed: 75 ± 20 ms has no late input
and no misprediction. At 100 ± 20 ms, 2 of 6 seeds have one laser
misprediction (0.03 u and 0.07 u, running), each in a scenario where the host
reported a late input; standing still, a late input changes nothing. So the
100 ms test stays ignored with the same reason as stage 1's.

### For stage 3 (C)

- Remove the ignore on `joined_player_laser_shots_are_predicted_at_100_ms`
  once C lands, keeping its seed and pass condition. A 150 ± 20 ms case fits
  `assert_lasers_predicted` too.
- Nothing in B reads the margin; shots follow the drawn time, which follows
  `input_horizon`. The lead in the window (`laser_last_lead_ms`) should drop
  on LAN as the margin shrinks.
- B1's moving-target and breaking-cover cases are not covered by the tests
  (`field`/laser pit have no enemies). They mispredict by design and are
  logged.

## Notes from stage 3 (adaptive input margin)

Stage 3 (C) landed on `kestrel-input-margin`, stacked on stage 2.

### What landed

- **`net/src/margin.rs`** (`InputMargin`, client only). Margin = one way +
  safety, rounded up to ticks. The safety is one tick (`SEND_WAIT`: an input
  waits up to a tick to be sent, which the round trip doesn't see) plus 4×
  the round trip's late-side deviation, at most 100 ms of that. So the
  margin is never below one way + 1 tick. It starts at the host's
  `input_delay` (133 ms) until 32 samples, then:
  - it grows at once when the estimate rises, or on a late input, to that
    input's headroom plus its lateness;
  - it shrinks one tick after 3 s (`QUIET`) without a late input or a
    change, down to the estimate;
  - `NetConfig::input_margin` pins it.
- **`client.rs`**:
  - Late inputs are found at the drain: a pending command drained from a
    turn later than its requested tick was late. They are counted in
    `NetDiagnostics::{late_inputs, shifted_ticks}`, and the app tests assert
    the counts equal the host's.
  - An input sent before any RTT sample has no headroom and doesn't grow
    the margin (that is the `Join`).
  - `input_horizon` = `max(host_tick + one_way + margin, last requested)`,
    so requested ticks never go backwards.
  - The deviation tracks only samples above the smoothed RTT (gain 1/16
    per sample), next to `rtt`.
- **App**: `--input-margin-ms` (`main.rs` → `flow::Launch`/`Defaults` →
  `NetConfig`). The Network window gets margin `adaptive`/`pinned` and a
  "Late inputs (detected locally)" row. The F3 line reads
  `NET RTT … · margin 117 ms`. The report gets `input_margin_pinned`,
  `late_inputs_detected_locally`, `late_input_shifted_ticks_detected_locally`,
  and `input_margin_ticks` in the history.
- **network-lab**: `-InputMarginMs` (client only, in `run.json`, printed).
- The protocol, the deterministic simulation, the host and C4's jitter
  buffer are unchanged.

### Results (committed seeds, in the scenarios)

| Case | Late inputs | Corrections | Laser mispredicts | Margin |
|---|---|---|---|---|
| no delay (RTT 17 ms) | 0 (before: 0) | 0 (0) | 0/15 (0/15) | 2 ticks, 33 ms (133) |
| 75 ± 20 ms (RTT 175) | 0 (0) | 0 (0) | 0/15 (0/15) | 9–10 (8) |
| 100 ± 20 ms (RTT 232–241) | 0 (1) | 0 (1) | 0/15 (0/15) | 10–12 (8) |
| 150 ± 20 ms (RTT 336) | 0 (76) | 0 (53) | 0/15 (10/15) | 14–15 (8) |

- The predicted-shot lead on a LAN fell from 150 ms to 50 ms.
- The 100 ms tests are no longer ignored, and the 150 ms cases are added,
  with the same pass conditions and seeds.
- Outside the scenarios, each delayed case still has the `Join` late
  (5–13 ticks), plus 1–2 inputs right after joining at 150 ms.

**Extra seeds** (6 per case, movement and lasers, 36 runs): no late input
and no correction at 75, 100 or 150 ± 20 ms, at the final constants.
Margins: 75: 9–10, 100: 10–12, 150: 13–15 ticks.

### Why these constants

- **Where late inputs start**, swept the same way:
  - With the send tick: K = 0 gives late inputs in 5 of 36 runs, 3 of them
    with 2–4 kept corrections; K = 1 gives one late input; K = 2 is clean in the
    app but has a late input in the net test with all jitter on one leg; K = 3
    and 4 are clean everywhere.
  - Without the send tick (safety = max(tick, K·dev)), K = 4 had one late
    input per ~6400 inputs at the floor in that net test.
- K = 4 is about the design's "twice the mean deviation" for symmetric
  jitter. K = 3 would cost about half a tick less.
- **Late-side, gain 1/16.** With TCP's mean deviation, a link getting faster
  reads as jitter and grew the margin from 11 to 14 ticks. With gain 1/4,
  the max-held margin sat 1–2 ticks higher from estimator noise alone.
- **100 ms cap.** A 250 ms lag spike read as ~140 ms of deviation, and the
  margin went to 40 ticks. Capped, it goes to ~17–18, about what late inputs
  ask for anyway.
- **Quiet period 3 s.** 2 s and 4 s gave the same late counts (none), so 3 s
  sits between them. It shrinks one tick per period, so after a 250 ms spike
  the margin takes about 40 s to come back down.

### Prediction stability

- I stepped the relay delay up mid-run at 75 ± 20 ms (2 seeds each):
  - **+60 ms each way:** the margin grows 9–10 → 15–17 ticks with no late
    input and no correction, both idle-then-moving and while running.
  - **+150 ms:** it grows → 21–22 ticks with no correction while running.
    Idle-then-moving had 1 late input (4 ticks) and 1–2 kept mispredicts
    (0.37 u; 0.28 + 0.09 u). That input left before any slower RTT sample
    could arrive, so the network changed, not the grow.
- So a grow by several ticks causes no misprediction itself: drawn time
  follows the lowest horizon of the last second, then jumps or eases.
- Shrinks are covered by the net test: no late input, the requested ticks
  are monotonic, and the order is kept.
- One 0.007 u under-threshold correction showed at 100 ms (quick taps),
  dropped as usual.

### Laser matching fix (stage 2 edge case)

- `LaserPrediction::predict` now matches a replay strike to the next
  outstanding drawn shot from the same command (the `Attack`'s aim and
  muzzle, which a moved shot keeps), instead of by position.
- Drawn shots a strike passes over are given up at once (`never fired`).
- Unit test:
  `a_shot_that_drops_out_of_the_replay_does_not_take_the_next_ones_place`.
- Two commands with exactly the same aim and muzzle still can't be told
  apart (the old behaviour).
- Superseded: `LaserPrediction` and `laser_prediction.rs` were replaced by
  the generic predicted-events ledger (`app/src/event_prediction.rs`),
  which matches by the command that caused the shot, so identical presses
  are told apart. Telemetry is `CorrectionKind::EventMispredict { kind:
  "laser" }`; the report fields above are unchanged. See D7 and D8 in
  `ABILITY_PREDICTION_HANDOFF.md`.

### Open issues

- **More input delay at 75–100 ms.** The margin is 1–4 ticks above the old
  fixed 8, so confirmed results arrive 17–67 ms later there. The drawn
  avatar and shots are predicted, so mostly hits and damage feel it. In
  exchange, no input is late.
- **The `Join` is always late on a delayed link.** It is sent before any
  RTT sample; harmless.
- **Slow recovery after a lag spike** (above): this is the agreed "shrink
  slowly".
- **A link that suddenly gets much faster:** samples still in flight on
  the old path read as late and grow the margin (capped at 100 ms of
  safety). This was seen with a 150 → 10 ms step in a net test, which was
  dropped as unrealistic.
- **The monotonic clamp rarely acts.** Shrinks land as turns arrive, which
  advance `host_tick`. The net test needs reordered turns (40 ms jitter) to
  exercise it.
- **C4 (jitter buffer) was not re-measured.**
- **Real-network check pending:** all numbers come from the 60 Hz
  virtual-clock relay. Next is a network-lab playtest with and without
  `-InputMarginMs 133`.

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
