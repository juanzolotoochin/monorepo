# Characters

## Move pile hauls and piano keys onto actions

- **Status:** open
- **Why:** the `Pile` and `PlayPiano` commands still bypass
  `settlement::action`. `Settlement::pile_action` checks and writes pile lab
  state (the hauler's job, serial and notice) around planning the avatar's
  route, so its agent part can't be split off cleanly, and the replay keeps
  returning false for it. A piano key changes only `piano_notes`, nothing on
  the avatar.
- **What to do:** once pile jobs are messages (handoff D3) or merge into
  the shared body's cargo (step 5), make a pile haul an action (step 5's
  "interact") whose agent part plans the walk, and let the replay predict
  it. Decide whether a piano key is an "interact" action with no agent part
  or stays a world command.
- **Where:** `kestrel/sim/src/settlement/action.rs` (`Action::of`),
  `piles.rs` (`pile_action`), `piano.rs` (`play_piano`),
  `prediction.rs` (`AvatarReplay::apply`).
- **Found in:** ability prediction task 2, the shared action path
  (`kestrel/ABILITY_PREDICTION_HANDOFF.md`, D2).

## Present the arc coil's shot early

- **Status:** open
- **Why:** `ArcFired` is predicted (`Event::prediction` says `Keep`), but a
  client still draws it only once confirmed. Its arc chains into the
  `Hit { impact: Arc }` events that follow it in the same tick
  (`arc_source` and `arc_fired` in `app/src/settlement.rs`): skipping a
  confirmed `ArcFired` already drawn would make the next hit draw an extra
  arc from the player. The legacy `ArcFired` also doesn't carry the agent.
- **What to do:** give the legacy event its agent, draw the predicted arc
  and its `Cue::Arc` from the ledger, and let a matched confirmed
  `ArcFired` still set the chain's start for the hits without drawing again.
  Then add `"arc"` to `PRESENTED_EARLY`.
- **Where:** `kestrel/app/src/event_prediction.rs` (`PRESENTED_EARLY`),
  `kestrel/app/src/lockstep.rs` (`predict_events`),
  `kestrel/app/src/settlement.rs`, `kestrel/app/src/legacy.rs`.
- **Found in:** ability prediction task 4, the predicted-events ledger
  (`kestrel/ABILITY_PREDICTION_HANDOFF.md`, D8).

## Draw a predicted arrow in flight

- **Status:** open
- **Why:** a client's own bow shot is presented only once confirmed, about
  an input lead after the drawn avatar looses it, because the arrow is a
  projectile in the confirmed world. Task 4 played the bow's sound early,
  but sound an input lead before the arrow appears felt worse than late, so
  task 5 made the bow confirmed-only again (`BowFired` is not in
  `PRESENTED_EARLY`).
- **What to do:** decide whether to draw a predicted arrow from `BowFired`
  (it has no aim or speed yet) that hands over to the confirmed projectile,
  or accept the gap. If it is drawn, add `"bow"` to `PRESENTED_EARLY`,
  present the arrow with its `Cue::Bow` in `predict_events`, give the legacy
  `BowFired` its agent back, and skip the matched confirmed shot's arrow and
  sound in `settlement.rs` (`presented_early.next("bow")`). Ghouls hitting agents
  (step 5) may change what a bow shot needs.
- **Where:** `kestrel/app/src/event_prediction.rs` (`PRESENTED_EARLY`),
  `kestrel/app/src/lockstep.rs` (`predict_events`), projectile
  views in `kestrel/app/src/settlement.rs`, `kestrel/app/src/legacy.rs`,
  `strike_event` in
  `kestrel/sim/src/settlement/combat.rs`.
- **Found in:** ability prediction task 4, the predicted-events ledger
  (`kestrel/ABILITY_PREDICTION_HANDOFF.md`, D8).

## Follow a matched swing to its end

- **Status:** open
- **Why:** the local swing starts on the press and, once the drawn avatar
  swings for it, takes that swing's stroke; then it plays on the view's own
  clock (`combat::follow_drawn_swing`). So a matched swing the prediction
  later withdraws plays out, and a confirmed sword or spear hit, which
  shortens the simulated swing to the rebound in stage 5, isn't reflected
  in the pose (the view rebounds only off its own obstacles). Ending a
  matched swing when the drawn one ends would cut its recovery short,
  since the press starts the view's swing a few ticks before the drawn
  avatar's.
- **What to do:** time a matched swing from the drawn swing (`started`,
  `until`, `resolved`) at the drawn time, keeping the press's head start
  only until the drawn swing shows, and end it when the drawn avatar no
  longer swings.
- **Where:** `kestrel/app/src/combat.rs` (`follow_drawn_swing`, `combat`,
  `Attack`), `kestrel/app/src/gameplay.rs` (`Controls`).
- **Found in:** ability prediction task 5, lasting effects from the drawn
  state (`kestrel/ABILITY_PREDICTION_HANDOFF.md`, D5).

## Make the control snapshot read one avatar consistently

- **Status:** open
- **Why:** after task 5 of the ability prediction work, the automation
  control snapshot's `jetpack` field reports the drawn (predicted) avatar,
  while `digging` still reads the confirmed one. They agree on the host and
  in single player, but differ on a joined client.
- **What to do:** pick one (probably the drawn avatar, which is what the
  player sees) and read every avatar field of the snapshot from it.
- **Where:** `kestrel/app/src/control/mod.rs` (the snapshot), `DrawnAvatar`
  in `kestrel/app/src/prediction.rs`.
- **Found in:** task 5 of `kestrel/ABILITY_PREDICTION_HANDOFF.md`
  (branch `kestrel-ability-prediction`).

## Test a knockback that pushes the joined player

- **Status:** open
- **Why:** nothing in the simulation pushes a player avatar yet: stage 5
  only damages, kills and respawns one. So the knockback correction (D6)
  is tested in the harness only with a death stopping a running avatar
  (`joined_player_hit_by_the_laser_is_knocked_back_not_mispredicted`), and
  with a push faked on the confirmed settlement in the prediction units.
  The detection (`prediction::StageFive`) needs no change for real pushes.
- **What to do:** once ghouls attack agents, or anything else knocks a
  player back (the open question in `docs/character-definition.md`), add a
  joined-client harness case under latency where the push moves the
  running avatar: logged as `Knockback`, smoothed below 3 u, no
  misprediction.
- **Where:** `kestrel/app/src/lockstep.rs` tests (`RelayedGame`),
  `kestrel/app/src/prediction.rs`.
- **Found in:** task 6 of `kestrel/ABILITY_PREDICTION_HANDOFF.md`
  (branch `kestrel-ability-prediction`).

## Reuse the mirror's settlement copy for stage-5 detection

- **Status:** open
- **Why:** `prediction::StageFive` keeps the last confirmed settlement by
  cloning it once per frame on clients, on top of the per-tick copies
  `lockstep.rs` already takes for the mirror. In a large world that is a
  second full settlement copy per frame.
- **What to do:** hand `StageFive` the copy already taken (move it, or share
  it behind an `Arc`) instead of cloning again; measure the client frame
  time in a large scene before and after.
- **Where:** `StageFive::observe` in `kestrel/app/src/prediction.rs`;
  `observe_stage_five` and the session update closure in
  `kestrel/app/src/lockstep.rs`.
- **Found in:** task 6 of `kestrel/ABILITY_PREDICTION_HANDOFF.md`
  (branch `kestrel-ability-prediction`).

## Done

### Remove the right-click movement leftovers

- **Status:** done (branch `kestrel-ability-prediction`)
- **Why:** right-click path movement was replaced by explicit WASD movement
  in `50cd1a3`, but its drawing and comments remain. GoTo itself stays: the
  depot transfer's walk to the dock, pile jobs, the automation `Walk`
  command and the character preview still use it. Do this first when the
  step 5 ability prediction work starts
  (`kestrel/ABILITY_PREDICTION_HANDOFF.md`).
- **What to do:**
  - Remove `Controls::route` and the waypoint gizmo that draws it.
  - Update the character preview shot that sets it, and the automation's
    movement readout.
  - Fix the comments that say click-to-move "is going away": GoTo stays,
    only the player's right-click input went.
- **Where:** `kestrel/app/src/gameplay.rs` (`Controls::route`, the gizmo
  around line 89); `kestrel/app/src/preview.rs:68`;
  `kestrel/app/src/control/mod.rs:623`; comments at
  `kestrel/app/src/lockstep.rs:1434` and `kestrel/app/src/prediction.rs:230`.
- **Found in:** design session for step 5 ability prediction (2026-10-26).
