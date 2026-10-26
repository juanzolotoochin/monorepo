# Kestrel ghoul attack handoff

## Goal

Ghouls (threats with a `FollowRoute` brain) hit players in passing, so a
player can be hurt by them and not only by lasers. This is part of character
step 5 (`docs/character-definition.md`, "Threats attacking agents"), which
records the decisions. This file has how to build it and the order.

## Agreed decisions

- **In passing.** A threat never leaves its route. When a player is in
  reach, it stops, winds up, swings, then walks on along its route. Chasing
  would be a separate hunter brain, later.
- **Players only.** Haulers have no health (the `Hauler` schema forbids
  `health`). Making them targets is a separate step.
- **Only hurtable players.** With the scenario's `player_health` at 0,
  players can't be hurt, and threats don't attack them, the same rule the
  laser follows (`combat.rs`, the laser player damage).
- **Latency.** The windup (about 400–500 ms, so 24–30 ticks) is longer than
  a typical input lead (50–450 ms), so a player who sees it can step away.
  Reach is checked when the swing lands, against where the player is then.
  A hit deals damage and a small knockback; it doesn't stun. Clients smooth
  the push with the `Knockback` correction (`app/src/prediction.rs`, D6 in
  `ABILITY_PREDICTION_HANDOFF.md`).
- **Tuning.** An optional `attack` section on `FollowRoute` characters:
  `damage` (integer), `reach` (m, from the threat's center to the player's
  center), `windup` and `cooldown` (frames at 60 Hz, like every other
  duration in the format), `knockback` (m/s). A character without it never
  attacks. Variants merge it like `health`.
- **Targeting.** The nearest hurtable player in reach of the threat, with
  the lowest agent index on a tie. Each player scans the threats for ones in
  reach, the way each projectile already scans them (`Settlement::fly`), so
  the cost is threats × players with no players near meaning one distance
  check each.

## How it fits the tick stages

| Stage | What happens |
|---|---|
| 2. Self update | A threat winding up stands still (`threats_self_update`). Stagger and shock still win over everything. |
| 3. Observe | For each hurtable player, threats with an `attack`, in reach, ready (cooldown over, not staggered or shocked, not already winding up) emit a "start swing" message. A threat whose windup ends this tick emits a "swing lands" message, a hit only if its target is still alive and in reach. |
| 5. Apply | "Start swing" sets the threat's windup and target, keeping the nearest player (lowest index on a tie) when several messages name one threat. "Swing lands" damages the player, pushes it, starts the threat's cooldown, and kills the player at 0 health through the same code the laser uses. |

Threat state gains the windup (target agent, tick it lands) and the tick its
cooldown ends. It is hashed and saved, so `FORMAT_VERSION` goes up.

## Order

Each task is its own commit on branch `kestrel-ghoul-attacks`, in order.

1. **Release a transfer's reservation on death.** A player killed in stage 5
   (today only by the laser, `combat.rs`) keeps `avatar.job`, so its depot
   reservation is never released (the respawn fix in #114 covered only the
   `Respawn` action). Pull the laser's kill code into one helper that the
   ghoul hit will reuse, and have it release the job's units the way
   `drop_transfer` in `avatar.rs` does (stage 5 may write the depot
   directly). Compare with what the `Respawn` action clears (pile haul,
   memory trial) and fix or file in `todo/characters.md` what death misses.
   Test with `Settlement::validate`. Gameplay fix, own commit.

   Done in task 1: the kill code is `Settlement::hurt_player(&mut self,
   agent: usize, damage: u32, tick: u64) -> bool` in
   `sim/src/settlement/avatar.rs`. It takes the damage, and at 0 health it
   kills the player and releases its job's remaining units through
   `transfer_stock`. It returns whether the player died. The laser calls it;
   the ghoul hit should too, then add its knockback. Test:
   `dying_releases_a_transfer_reservation`. The pile haul, memory trial and
   dig are filed in `todo/characters.md` ("Decide what a player's death
   ends besides its transfer"). Golden file and hashes unchanged.
2. **The `attack` format.** Add the optional `attack` section to
   `content/src/character.rs` (file, merge, validation, converted
   definition), its tests in `content/src/lib.rs`, the behaviour digest if
   the rules hash needs it, and the schema in `docs/character-definition.md`.
   No content file uses it yet, so the golden file and hashes stay unchanged.

   Done in task 2: `CharacterDef.attack: Option<AttackDef>`, with
   `AttackDef { damage: u32, reach: Fx (m), windup: u64 (ticks), cooldown:
   u64 (ticks), knockback: Fx (m/s, not per tick, like a weapon's) }`.
   Ranges: damage 1..=1000, reach [0.1, 8] m, windup 1..=600 frames,
   cooldown 0..=600 frames, knockback [0, 40] m/s. Only `FollowRoute` may
   have it. The field is `#[serde(skip)]`, and `rules_hash` appends every
   character's attack only when one has it, so today's rules hash holds.
   Tests: `attacks_parse_in_simulation_units`,
   `a_variant_merges_its_attack_field_by_field`, `attacks_are_validated`,
   `attacks_feed_the_rules_hash`.
3. **Ghouls hit players.** The sim work in the table above, with tests in
   `sim/src/settlement/combat_tests.rs`: a windup then hit, a player who
   steps out of reach during the windup, the nearest-player tie-break,
   cooldown, `player_health` 0, the route resuming after the swing,
   determinism. Give the ghoul content (`content/data/character/ghoul*.ron`)
   an `attack`. Update the golden file and hashes only where the change
   explains them, and say why in the commit.

   Done in task 3:
   - **State** (`sim/src/settlement/combat.rs`): `Threat.windup:
     Option<Windup { agent: AgentIx, lands: u64 }>` and `Threat.ready_at:
     u64`, the first tick a new windup may start (`lands + 1 + cooldown`,
     set hit or miss). Save `FORMAT_VERSION` 8; no migration, as before.
   - **Messages** (`sim/src/messages.rs`, after `projectile_expired` and
     before `breach`): `StartSwing { threat, agent, distance }` from each
     hurtable player's scan (`threats_in_reach`), and `SwingLands { threat,
     agent, hit }` from the threat whose `windup.lands` is this tick
     (`swing_reaches`: target alive and within `reach`). Stage 5
     `start_swings` sorts by (threat, distance, agent) and keeps the first
     per threat, so the queue order doesn't matter; it also turns the
     threat to face its target. `swing_lands` clears the windup, starts the
     cooldown, and on a hit calls `hurt_player`, then pushes a survivor.
   - **Stagger rule:** a stagger or shock cancels the windup (`strike` and
     the bolt pierce in `projectile_hit` clear `windup`). No swing lands
     and no event says so: the windup just goes away. No windup starts
     while staggered or shocked; after that it may start again at once
     (the cooldown runs only from a landed swing).
   - **Knockback:** `avatar.velocity += away * knockback` (m/s, away from
     the threat). `walk` brakes it out at `braking`, so 3 m/s moves an idle
     player about 0.14 m. A killed player isn't pushed. The client's
     `prediction::StageFive` needs no change: the replay doesn't run threat
     swings, so the confirmed tick with the new health and velocity differs
     from the replay and is logged as touched (a `Knockback`).
   - **Events** (`Event`, all `Prediction::Confirmed` through the default
     arm): `ThreatWindup { threat, agent, lands }` when a windup starts,
     and `ThreatSwing { threat, agent, hit, damage, killed }` when it
     lands. The app's legacy mapping drops both for now (`legacy.rs`).
   - **Tuning** (`content/data/character/ghoul.ron`, inherited by
     `ghoul_brute` and `ghoul_forest`): damage 1, reach 0.9 m, windup 27
     frames, cooldown 60, knockback 3 m/s. At 3.8 m/s a player next to a
     ghoul (0.6 m apart) needs about 0.2 s to get past 0.9 m, so stepping
     away works with a short input lead and gets hard near the 450 ms one.
   - **Tests** (`combat_tests.rs`): the eight in the list above plus
     `ghoul_attacks_are_deterministic` (1 vs 4 threads).
   - **Golden and hashes:** no golden scene has both ghouls and
     `player_health` above 0, so no ghoul attacks there; the behaviour
     lines are unchanged. Totals change because the threat state is longer
     (forest, arena) and the pending `Messages` hash has two more queues
     (every scenario, the mover-only `small`/`large` hashes too).
   - **For task 4:** the windup is in confirmed state
     (`Settlement::threats[i].windup`, with `lands`; the windup lasts
     `characters[waves[t.wave].character].attack.windup` ticks), and the
     events are in `Settlement::events`. The app draws threats in
     `app/src/settlement.rs` (the `sim.threats` loop building a `View` from
     the wave's skin, with `activity`, `injury`, `recoil`) from the legacy
     `Simulation` snapshot (`legacy.rs`, `Threat`), which doesn't carry the
     windup yet. Sounds come from the legacy `Event` match in
     `settlement.rs`. No scene shows it yet: see "Give a scene ghouls and
     hurtable players" in `todo/characters.md`.
4. **Show the windup.** The client draws the threat's windup so the player
   can read it (a pose or tell on the ghoul's skin) and presents the hit on
   the player (sound, flash or similar, through the existing event path).
   Confirmed state only: the threat is not the local player's action.

   Done in task 4:
   - **Snapshot** (`app/src/legacy.rs`): `Threat.windup: Option<Windup {
     agent, lands, ticks }>` (`ticks` is the character's `attack.windup`)
     and `Threat.swung: Option<u64>`, the tick its last swing landed,
     derived from `ready_at - 1 - cooldown` (no new sim state). Positions
     are drawn between the previous tick and the last one, so a windup in
     the previous tick that landed on the last one is carried; one a
     stagger cancelled (landing later) isn't. `Simulation::drawn_time()`
     is that drawn tick (`tick - 1 + alpha`). `ThreatWindup` still maps to
     nothing (the state draws it, also for a client joining mid-windup);
     `ThreatSwing` maps to `Event::ThreatSwing { threat, agent, from, hit,
     killed }`.
   - **Pose** (`app/src/rig.rs`): `Rig::pose` takes a `Strike { windup,
     swing }`. A biped raises both arms overhead and leans back as the
     windup builds (eased out, so it reads early), then on landing snaps
     its arms down in front and lunges, drawn back over 0.3 s, hit or miss.
     `settlement::threat_strike` computes it from `drawn_time`.
   - **Hit and miss** (`app/src/settlement.rs`): a hit plays `GhoulHit`
     and sprays blood off the player, away from the threat (at the drawn
     local avatar for the local player); the local player also gets camera
     shake (trauma 0.3, 0.45 on a kill). A miss plays `Swing`. No new
     assets. No hurt flash exists yet: filed as "Flash the player's screen
     or body when it is hurt" in `todo/characters.md`.
   - **Tests** (`settlement.rs`):
     `the_view_carries_a_threats_windup_until_its_drawn_tick_passes_it`,
     `a_threats_swing_event_names_the_player_and_where_the_threat_stands`,
     `a_threats_windup_builds_with_its_drawn_tick_then_its_swing_follows_through`.
     Not checked visually: no scene has ghouls and hurtable players.
5. **The knockback push test.** Do the todo "Test a knockback that pushes
   the joined player" (`todo/characters.md`) with a ghoul hit, then move it
   to Done.

   Done in task 5:
   - **Test** (`app/src/lockstep.rs`):
     `joined_player_pushed_by_a_ghoul_is_knocked_back_not_mispredicted`,
     under 100 ± 20 ms. `RelayedGame::in_scene_with` (and
     `scene_app_with`) edit a scene's pack on both machines first: the
     laser pit (player health 6) gets one `ghoul_forest` wave walking from
     beside the joiner toward the stage, and the forest's ghoul skin with
     its materials. The pit's follow camera turns the keys, so the test
     measures where D and W go, then runs (Shift held) back and forth
     across the ghoul on both axes until a swing lands, and 60 frames on.
   - **Asserts:** a hit while the avatar moves (> 1 m/s before the hit's
     tick), only `Knockback` corrections, none snapped or 3 u or more,
     their distance above 0.05 u, and no misprediction
     (`preview_corrections` and `preview_mispredicted_frames` 0).
   - **Measured** (same every run): the hit lands on frame 65 at
     1.52 m/s, mid-turn; one knockback of 0.695 u over 1 frame. That is
     more than the 0.14 m an idle player moves: the turning avatar
     accelerates toward its new direction, so the push's sideways part
     isn't braked out until it gets there.
   - **No prediction change.** Only `StageFive`'s doc comment changed. The
     prediction units still fake their pushes: they need 2 and 4 u ones
     to cross the snap distances.

## Rules

- Bazel: `bazel test --config=remote ...` (see `AGENTS.md`).
- Things found but out of scope go in `kestrel/todo/`.
- When a task is done, add a "Done in task N" note under it here.
