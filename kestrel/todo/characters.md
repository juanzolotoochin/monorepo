# Characters

## Move pile hauls and piano keys onto actions

- **Status:** open
- **Why:** the `Pile` and `PlayPiano` commands still bypass
  `settlement::action`. `Settlement::pile_action` checks and writes pile lab
  state (the hauler's job, serial and notice) around planning the avatar's
  route, so its agent part can't be split off cleanly, and the replay keeps
  returning false for it. A piano key changes only `piano_notes`, nothing on
  the avatar.
- **What to do:** now that ending a haul is a message (`DropHaul`, handoff
  D3, task 7), or once pile jobs merge into
  the shared body's cargo (step 5), make a pile haul an action (step 5's
  "interact") whose agent part plans the walk, and let the replay predict
  it. Decide whether a piano key is an "interact" action with no agent part
  or stays a world command.
- **Where:** `kestrel/sim/src/settlement/action.rs` (`Action::of`),
  `piles.rs` (`pile_action`), `piano.rs` (`play_piano`),
  `prediction.rs` (`AvatarReplay::apply`).
- **Found in:** ability prediction task 2, the shared action path
  (`kestrel/ABILITY_PREDICTION_HANDOFF.md`, D2).

## Turn the memory commands' world writes into messages

- **Status:** open
- **Why:** since task 7 a `Respawn` restarts the memory trial in stage 5,
  but `MemoryRecord`, `MemoryDeploy` and `MemoryClear` (and `Pile` and
  `PlayPiano`) still write the world when the command applies. In one turn,
  a Respawn followed by `MemoryRecord` now loses the recording, which used
  to start after the restart. `MemoryDeploy` also cancels the avatar's pile
  haul and moves the avatar directly.
- **What to do:** give the memory commands the same split as actions (the
  avatar's part at once, the trial's part as a message applied in stage 5
  in command order with `RestartMemory`), or decide they stay world
  commands and order `RestartMemory` among them.
- **Where:** `kestrel/sim/src/settlement/memory.rs`, `world.rs`
  (`apply_commands`), `messages.rs` (`ActionMessages`).
- **Found in:** ability prediction task 7 (`kestrel/ABILITY_PREDICTION_HANDOFF.md`, D3).

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

## Look at the hurt flash in play

- **Status:** open
- **Why:** the red screen-edge flash (`app/src/hurt.rs`) is unit tested
  for when it fires, but nobody has seen it drawn. Its strength (0.6
  alpha at the corners), its clear middle (55%) and its fade (0.45 s) are
  guesses.
- **What to do:** get hit by another player's laser in the laser pit, and
  by a ghoul in the forest or arena (player health 6), and tune the
  constants. Check it stays under the HUD
  text and doesn't show in captures where nobody is hurt.
- **Where:** `kestrel/app/src/hurt.rs`.
- **Found in:** branch `kestrel-ghoul-followups`.

## Give the body a height

- **Status:** open
- **Why:** height lives on the avatar (`AvatarState::height`, with the
  jump and jetpack), so a hauler has none. A threat's swing treats a hauler
  as standing on the ground (`within_swing_height`), and the behaviour
  digest hashes its height as 0. Haulers on raised ground would be wrong
  once threats hit them.
- **What to do:** when haulers become targets, move `height` (and the
  ground it last stood on) into `Body`, and have haulers keep it on the
  ground as they walk. Keep the digest's value for agents that don't move
  vertically.
- **Where:** `kestrel/sim/src/settlement/agent.rs`, `avatar.rs`,
  `haulers.rs`, `within_swing_height` in `combat.rs`.
- **Found in:** branch `kestrel-shared-body`.

## Trim body fields one role ignores

- **Status:** open
- **Why:** `Body::speed` and `travel_heading` are used only by haulers: an
  avatar moves at its character's `movement` speeds and has no travel
  heading. They sit in the body so every agent has the same shape.
- **What to do:** once actions drive haulers too (step 5, "Haulers"), read
  the speed from the character for both roles, and decide whether the
  travel heading belongs to the hauler. A save format change.
- **Where:** `Body` in `kestrel/sim/src/settlement/agent.rs`, `speed_now`
  in `haulers.rs`.
- **Found in:** branch `kestrel-shared-body`.

## Fix the flaky two-player session test

- **Status:** open
- **Why:** `session:session_test` `two_players_walk_around_together` failed
  1 run in 4 on the remote executor: the client was one tick behind the host
  (240 vs 239) when the test compared them. The client gets two wall-clock
  seconds to catch up, which a busy executor can miss. It has nothing to do
  with the state being tested.
- **What to do:** wait for the client by ticks, not wall time, or give it a
  bound in updates rather than seconds.
- **Where:** `kestrel/session/src/lib.rs` (`two_players_walk_around_together`).
- **Found in:** inventory step 3 (`kestrel/INVENTORY_HANDOFF.md`).

## Done

### Keep a player's health over the Respawn action

- **Status:** done (branch `kestrel-shared-body`)
- **Why:** the `Respawn` action resets the avatar to a fresh state, and that
  has always included its health: `health` and `max_health` go to 0. A
  player who respawns by command in a scene with `player_health` above 0
  can't be hurt again. A death's respawn (`self_update`) restores health
  instead. The shared body split kept this as it was.
- **What to do:** have `respawn` restore `max_health` and fill `health`, as
  the death respawn does. It is a gameplay change: test it, and check the
  replay still predicts the command.
- **Where:** `respawn` in `kestrel/sim/src/settlement/avatar.rs`.
- **Found in:** branch `kestrel-shared-body`.

### Flash the player's screen or body when it is hurt

- **Status:** done (branch `kestrel-ghoul-followups`)
- **Why:** nothing shows the local player being hurt beyond its nametag's
  health, a sound and (for a ghoul hit) blood and camera shake. A laser hit
  on a player plays only `LaserImpact`. `effects::Feedback::flash` is set by
  the player's own hits (`combat.rs`) and decayed (`effects.rs`), but
  nothing draws it, so it can't be reused for a hurt flash.
- **What to do:** draw a short hurt cue for the local player, for any
  damage (laser or ghoul): a red screen-edge flash from `Feedback::flash`,
  or an emissive flash on the actor's materials like `ImpactTarget` slots.
  Drive it from the confirmed health drop so every damage source gets it.
- **Done:** `app/src/hurt.rs` draws a red radial-gradient UI node over the
  screen's edges, fading over 0.45 s, when the local avatar's confirmed
  health (`WorldMirror::current`) drops. A respawn or a new avatar doesn't
  flash (`hurt`, unit tested). `Feedback::flash` was dead since it was
  added (set on the player's own sword contacts, never read), so it is
  removed. Not checked on screen.
- **Where:** `kestrel/app/src/effects.rs` (`Feedback`), `settlement.rs`
  (the `ThreatSwing` presentation), `combat.rs`.
- **Found in:** task 4 of `kestrel/GHOUL_ATTACK_HANDOFF.md`.

### Decide whether a ghoul reaches a flying or busy player

- **Status:** done (branch `kestrel-ghoul-followups`)
- **Why:** a ghoul's reach is measured on the ground plane only, so it hits
  a player hovering 1.8 m up on the jetpack. And a ghoul's push is a
  velocity that `walk` brakes out; during the player's own non-laser swing
  or a dig, `walk` doesn't move the avatar and brakes that velocity away,
  so the hit lands with no push.
- **What to do:** decide whether reach checks height (skip players whose
  `avatar.height` is well above the ground under the threat) and whether a
  push should move a swinging or digging avatar. Both are rules choices.
- **Done:** a ghoul can't reach feet more than `SWING_HEIGHT` (1 m) above
  the ground under the player, at the windup's start or the swing's
  landing. A push ends a dig, and during the player's own melee swing it
  goes to the new `AvatarState::push`, which `walk` brakes and moves by the
  walking rules (save format 9). Tests in `combat_tests.rs`:
  `a_ghoul_does_not_reach_a_hovering_player`,
  `a_player_who_lifts_off_during_the_windup_is_missed`,
  `a_jump_does_not_dodge_a_ghoul`,
  `a_ghoul_pushes_a_swinging_player_as_far_as_an_idle_one`,
  `a_ghoul_push_ends_a_dig_and_moves_the_player`.
- **Where:** `kestrel/sim/src/settlement/combat.rs` (`swing_reaches`,
  `threats_in_reach`, `swing_lands`), `avatar.rs` (`walk`).
- **Found in:** task 3 of `kestrel/GHOUL_ATTACK_HANDOFF.md`.

### Decide what a player's death ends besides its transfer

- **Status:** done (branch `kestrel-ghoul-followups`)
- **Why:** a player killed in stage 5 (`Settlement::hurt_player`) now drops
  its transfer job, but not the rest of what the `Respawn` action clears.
  A pile haul keeps running: the dead avatar still loads units when in
  reach (stage 3), and after the respawn at home the haul walks it back to
  the pile. The memory trial doesn't restart. A dig in progress still
  finishes while the player is dead.
- **What to do:** decide which of these death should end. For the haul,
  call `cancel_pile_job` from `hurt_player` with a notice that says the
  player died (today's says "canceled by player input"). For the memory
  trial, decide whether only the `Respawn` action restarts it. Clear `dig`
  if a dead player shouldn't dig.
- **Done:** death ends what a respawn ends. `hurt_player` clears the dig
  and the route, ends the pile haul (`end_pile_job`, the core of
  `cancel_pile_job`, with "You died. Haul canceled; cargo is retained.")
  and restarts the memory trial at once (`restart_memory`, which
  `RestartMemory` also calls). `strike` and `swing_lands` take the terrain
  for the door. Tests: `dying_ends_a_haul`, `dying_stops_a_dig`,
  `dying_restarts_the_memory_trial_and_closes_its_door`.
- **Where:** `kestrel/sim/src/settlement/avatar.rs` (`hurt_player`,
  `self_update`, `act`), `piles.rs` (`cancel_pile_job`), `memory.rs`
  (`restart_memory`).
- **Found in:** task 1 of `kestrel/GHOUL_ATTACK_HANDOFF.md`
  (branch `kestrel-ghoul-attacks`).

### Give a scene ghouls and hurtable players

- **Status:** done (branch `kestrel-ghoul-attacks`, PR #115)
- **Why:** ghouls hit players only when the scenario's `player_health` is
  above 0, and no scene has both: the arena and forest have ghouls with
  `player_health` 0, and the laser pit has health but no waves. So the
  ghoul attack (task 3 of `kestrel/GHOUL_ATTACK_HANDOFF.md`) is reached
  only by the sim tests, and nobody can see it in play.
- **What to do:** decide which scene gets it: health in the combat arena,
  or a wave in the laser pit, or a new scene. Changing a golden scene
  (arena, forest) changes the golden file's behaviour lines, so explain
  that in the commit.
- **Where:** `kestrel/app/assets/levels/*.ron`, `sim/src/scenario.rs`
  (`golden_report`).
- **Found in:** task 3 of `kestrel/GHOUL_ATTACK_HANDOFF.md`.


### Test a knockback that pushes the joined player

- **Status:** done (branch `kestrel-ghoul-attacks`)
- **Why:** nothing in the simulation pushed a player avatar yet: stage 5
  only damaged, killed and respawned one. So the knockback correction (D6)
  was tested in the harness only with a death stopping a running avatar
  (`joined_player_hit_by_the_laser_is_knocked_back_not_mispredicted`), and
  with a push faked on the confirmed settlement in the prediction units.
  The detection (`prediction::StageFive`) needs no change for real pushes.
- **What to do:** once ghouls attack agents, or anything else knocks a
  player back (the open question in `docs/character-definition.md`), add a
  joined-client harness case under latency where the push moves the
  running avatar: logged as `Knockback`, smoothed below 3 u, no
  misprediction.
- **Done:** `joined_player_pushed_by_a_ghoul_is_knocked_back_not_mispredicted`
  gives the laser pit one `ghoul_forest` wave and has the client run back
  and forth across the ghoul until a swing lands. No prediction change was
  needed. The prediction units still fake their pushes: they need 2 and
  4 u ones to cross the snap distances, far more than a ghoul's.
- **Where:** `kestrel/app/src/lockstep.rs` tests (`RelayedGame`),
  `kestrel/app/src/prediction.rs`.
- **Found in:** task 6 of `kestrel/ABILITY_PREDICTION_HANDOFF.md`
  (branch `kestrel-ability-prediction`).

### Release a transfer's reservation when its avatar respawns

- **Status:** done (branch `kestrel-respawn-transfer`)
- **Why:** `Respawn` resets the avatar's state, dropping a running manual
  transfer job (`AvatarState::job`) without releasing the units it still
  had reserved at its depot (`out` or `incoming`). Those units stay promised
  to nobody, so the depot can never hand them out or take them in.
- **What to do:** have `Respawn` emit a `TransferStock` release for the job
  it drops, as `CancelTransfer` does, and check `Settlement::validate` with
  a test that respawns mid-transfer. It changes gameplay, so it gets its own
  commit.
- **Where:** `kestrel/sim/src/settlement/avatar.rs` (`act`, `respawn`,
  `Settlement::transfer_stock`).
- **Found in:** ability prediction task 7 (`kestrel/ABILITY_PREDICTION_HANDOFF.md`, D3).

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
