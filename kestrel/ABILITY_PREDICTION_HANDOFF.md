# Kestrel ability prediction handoff

## Goal

Make client-side prediction part of how abilities are built, so a new
ability is predicted without its own replay code, matcher or telemetry. Today
`AvatarReplay::apply` (`sim/src/settlement/prediction.rs`) keeps its own copy
of the sim's command handling, and the laser has a dedicated path end to end
(`ReplayedStrike`, `predict_laser`, `app/src/laser_prediction.rs`,
`CorrectionKind::LaserMispredict`). That doesn't scale to more abilities.

This belongs to character step 5, "one set of actions for players and AI"
(`docs/character-definition.md`). It builds on the actions vocabulary.
Before starting, do the first task in `todo/characters.md` (remove the
right-click movement leftovers).

## How prediction works today

The game uses lockstep, so the confirmed world is never wrong. Each frame the
client replays its own avatar from the confirmed tick through its pending
commands, using the sim's code (`AvatarReplay`). The avatar is drawn at the
input horizon. Reconciliation is therefore free: the next replay is correct.
What the player saw is smoothed by a visual offset (`app/src/prediction.rs`).
A prediction can be wrong for three reasons:

- something outside the avatar changed it (hits, knockback, terrain,
  depot stock, other players);
- the host moved a late input;
- the command can't be predicted. `Pile` makes `apply` return false, and
  the client then draws the confirmed avatar. (Before D2, `GoTo` did too.)

## Agreed decisions

### D1. The tick stages are the prediction contract

| Stage | Avatar code | In the replay | When the prediction is wrong |
|---|---|---|---|
| 2. Self update | `self_update` (walk, fly, jump, dig progress, swing timers) | Always run, with the same code | Next frame's replay is correct; the offset smooths what was drawn |
| 3. Observe and emit | `read_avatar` (strikes, transfers, pile steps) | Run against the **confirmed** world. Its messages become predicted events and are never applied | The event kind's miss policy (D4) |
| 5. Apply messages | damage, knockback, terrain, stock | Never | Arrives as a correction (D6 for knockback) |

An ability writes only its own agent in stages 2 and 3, which the chunked
sim already requires.

### D2. One path for commands: command → action → ability

- A shared mapping turns a `CommandKind` into a step 5 action.
  `CommandKind` stays append-only, so recorded logs still replay.
- `avatar::act(a, action, view, def)` applies an action to the agent. `view`
  is read-only access to the settlement. The sim and the replay both call
  it. `AvatarReplay::apply`'s own `match` is removed.
- An action changes only its agent. Anything it does to the world is a
  message, applied in stage 5.

### D3. Commands that write the world become messages

`Transfer` reserves depot stock when the command applies
(`Settlement::transfer`, `depots[d].incoming/out += units`, then sets
`avatar.job`). Instead, the action sets the avatar's job and emits a
reservation message, which stage 5 applies. Check the other command
handlers for the same pattern. This changes gameplay order, so it gets its
own commit, last, with the golden file updated in that commit only.

### D4. Predicted events come only from stage 3 messages

- Each message type gets one shared function that turns it into the
  `Event` the player sees. The sim's apply stage and the replay both use it.
  It replaces `predict_laser` and `ReplayedStrike`. Today `LaserFired` is
  built in `combat.rs` (around line 541) from the `Strike` message's
  targets, beam end and cover, so it already qualifies.
- Events that need stage 5 results (`Hit`, deaths, cover breaking, stock
  moving) are `Confirmed` by construction and never predicted.
- Done in task 3: `strike_event` (`sim/src/settlement/combat.rs`) turns a
  `Strike` into `LaserFired`, `ArcFired` or `BowFired` (the bow's former
  `Shot { defender: None }`); sword and spear strikes show only `Hit`.
  `ManualUnit` and `PileStep` show nothing until stage 5 moves units. A
  laser's `hit_actor` now comes from stage 3 (`Strike::hit_player`), so a
  player killed earlier in the same tick's stage 5 still counts as hit.
  `Event::prediction` returns `Confirmed` or `Predict { agent, started,
  on_miss }`; the three strike events are `Keep`. `AvatarReplay::events`
  holds the last tick's predicted events.
- `AvatarReplay::step` returns the events it predicted that tick.
- Each predictable event kind has a miss policy:
  - `Keep`: a one-shot such as the laser beam, a swing sound or sparks. If
    it never confirms, or confirms elsewhere, leave it, apply the confirmed
    results where the confirmed event says, and log it. This is the
    laser's current behaviour (B3).
  - `Cancel`: an event that starts something lasting, such as a looping
    sound. If it never confirms, stop it.

### D5. Ongoing effects are driven by state (hard rule)

Anything that lasts (jetpack flame, dig pose and dust, jump crouch, a held
repair) is drawn from the drawn avatar's state. That state is already
predicted and smoothed, so a misprediction ends the effect on the next frame
with no extra code. Events are for one-shots. `Cancel` exists only for the
rare lasting effect an event must start.

### D6. Knockback has its own correction kind

Knockback and other pushes from stage 5 are expected corrections, not
mispredictions:

- `CorrectionKind::Knockback`, with its own smoothing: a longer half-life
  (around 100 ms) and a larger snap distance (around 3 u). The current
  values are 50 ms and 1.5 u.
- It is told apart from a misprediction by the confirmed state showing a
  hit applied to the avatar on that tick.
- It is not counted in the mispredict totals.

### D7. Matching key: the command that caused the event (option A, client-side)

Laser matching today goes by aim and muzzle, so two identical commands are
ambiguous.

- Each predictable event carries the tick its action started, for example
  the swing's start tick. Events are local sim output, so the wire format
  doesn't change.
- The client already learns at the drain in `Client::update`
  (`net/src/client.rs`, around line 293) which pending command applied on
  which tick. Expose that record through `Session`, then map confirmed
  start tick → command → predicted event.
- A sim-side per-player command counter (option B) was rejected because it
  would add hashed state only for client bookkeeping.
- Done in task 3: `started` is `Swing::pressed()`, the tick the `Attack`
  applied on. A press queued during a swing keeps its press tick in
  `AvatarState::queued_at` and then `Swing::queued_at`, which are not saved
  or hashed, so a queued swing in flight across a save or a join keys by
  its start tick instead. Until task 4, the app's `LaserPrediction` still
  tells commands apart by the resolving swing's aim and muzzle
  (`PredictedEvent` in `app/src/prediction.rs`).

### D8. One generic predicted-events ledger in the app

It replaces `LaserPrediction`:

- match by key (D7);
- deduplicate across frames, since the replay re-runs the same ticks every
  frame;
- time out events that never confirm;
- skip the duplicate when the confirmed event arrives;
- apply the miss policy.

Telemetry uses one `Mispredict { kind }` per event kind instead of a
dedicated kind per ability. The laser is the first user and must keep its
current results (0 mispredictions in the latency tests).

## Order

Each item is a commit or a PR, following step 5's commit order: digest,
then format, then reading with the golden file unchanged, then gameplay
changes last.

1. `todo/characters.md`: remove the right-click leftovers.
2. D2: the action path shared by the sim and the replay. Golden file
   unchanged. The replay tests (`replayed_avatar_matches_the_simulation_*`)
   still pass.
3. D4 and D7 in the sim: message → event, the replay returns events, and
   events carry their start tick. The laser's sim tests pass unchanged.
4. D8 in the app: the generic ledger and telemetry. Remove
   `LaserPrediction`. The laser harness tests in `lockstep.rs` pass with the
   same seeds and pass conditions.
5. D5: check presentation for lasting effects started by events, and move
   them to state.
6. D6: the knockback correction kind. This matters most once ghouls attack
   agents (the step 5 open question).
7. D3: `Transfer` and any other command that writes the world become
   messages (gameplay commit).

## Tests

- Extend `replay_matches_simulation` (`sim/src/settlement/prediction.rs`) to
  compare events too: the replay's predicted events equal the sim's
  whenever what they read of the world didn't change. That gives every
  ability coverage once its commands are in `random_command`. The laser's
  dedicated test becomes one case. (Done in task 3: it runs in the laser
  range, with cover, a ledge and a second player.)
- App units for the ledger: matching by key, a moved input moving its
  event, a timeout logged once, `Keep` versus `Cancel`.
- A knockback case once enemies can hit agents: smoothed, not snapped,
  logged as `Knockback`, with no mispredict counted.

## Rules

Same as `LASER_PREDICTION_HANDOFF.md`: Bazel with `--config=remote`, tests
through `bazel test`, PowerShell for the `--shell_executable=/bin/bash`
runs, and leave the user's game windows alone.

## Open questions

- **Predicting the dock walk.** Decided in D2: predicted. `set_intent` plans
  a `GoTo` route from the terrain and the avatar alone (`nav::path_with`,
  flying or walkable for its radius), and the walk itself reads only the
  terrain, so the replay plans it once when the action applies, from the
  confirmed terrain. The `Transfer` sent with it is predicted too: it sets
  the avatar's job from the confirmed depot stock; the units that then move
  at the dock are stage 5 and correct like a hit. Pile commands still read
  and write pile lab state and stay unpredicted (`todo/characters.md`).
