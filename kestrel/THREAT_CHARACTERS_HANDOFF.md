# Kestrel threat characters (design)

Status: done (branch `kestrel-threat-characters`, steps 1–4). Part of character step 5 ([character definitions](docs/character-definition.md)).

## Goal

A threat reaches its kind through its wave today: `self.waves[t.wave].character`
for its radius and attack, and the wave's copies of the character's tuning
(`speed`, `hp`, `armor`, `damage`) for its walk, limp and breach. Make each
threat store its own `CharacterIx`, set when its wave spawns it, and read
every kind lookup through it. Waves keep naming the character they send,
and stop copying its tuning. The threat's stage 2 becomes the `FollowRoute`
brain by name.

Threats keep their own table (`Settlement::threats`), not `AgentState`.

**Behaviour stays identical.** The golden file's behaviour lines and
`testdata/*.hashes` don't change. The forest and arena **total** hashes in
`golden.txt` do, since the saved state changes shape, and `FORMAT_VERSION`
goes from 19 to 20.

## What exists today

| Where | Reads | Through |
|---|---|---|
| `Brain::FollowRoute` (`content/src/character.rs`) | the brain enum exists; the loader requires `health` for it and forbids `attack` elsewhere | — |
| `ghoul`, `ghoul_brute`, `ghoul_forest` (`content/data/character/`) | already `FollowRoute` character files | — |
| `Settlement::new` waves (`settlement/mod.rs`) | resolves `Wave::character` to `WaveState::character`; copies `speed = walk / per_second`, `hp`, `armor.points`, `breach_damage`; rejects a character without `health`, never checks the brain | the scenario |
| `combat_housekeeping` spawn (`combat.rs`) | `wave.hp`, `wave.armor`; sets `Threat::wave` | wave |
| `threats_self_update` (stage 2) | `wave.speed`, `wave.hp` (limp at 55% below full) | `t.wave` |
| `threat_radius` (stage 3) | `def.body.radius`, `def.health.armor.radius` | `waves[t.wave].character` |
| `threat_attack` (stages 3, 5) | `def.attack` | `waves[t.wave].character` |
| `breach` (stage 5) | `wave.damage` | `t.wave` |
| `behaviour_digest` (`mod.rs`) | hashes `t.wave`, `w.spawned`, `pinned` (with `wave`); no tuning | — |
| `set_characters` (`mod.rs`), `remap_characters` (`save/src/lib.rs`) | check and remap agents' and waves' characters | — |
| `legacy.rs` threats (app) | `current.characters[current.waves[t.wave].character].attack` for the windup length | `t.wave` |
| `settlement.rs` threat and pinned views (app) | skin and full hp from `sim.config().waves[t.wave]`, the legacy scenario, built from the wave's character by name | `t.wave` |
| `session/src/lib.rs` test bot | `t.wave` as a lane | wave as origin |

`Threat` is about 144 bytes on 64-bit: a 2-byte `wave` beside 4-byte and
1-byte fields leaves padding. Net join and autosave carry the settlement
through `save::write`/`read`; nothing else serializes threats.

## Decisions

### D1. A threat stores `character: CharacterIx`; it keeps `wave`

- `Threat::character: CharacterIx` (`u16`, the type agents and waves use),
  set at spawn from `WaveState::character`. Put it beside `wave`.
- **Why `u16` costs nothing:** it fits in `Threat`'s existing padding, so
  `size_of::<Threat>()` stays the same (expected 144; measure it in step 1
  and pin it in a test). `u8` would save nothing and differ from every
  other `CharacterIx`.
- **Why keep `wave`:** it is where the threat came from, not what it is.
  The behaviour digest hashes it, `Pinned` copies it, the app's legacy
  `Threat` and its traces carry it, and the session test bot uses it as a
  lane. After this work nothing in the sim reads tuning through it.
- **Why not a `Body`/`AgentState`:** the doc's call, kept. A threat has no
  carry, role, home or velocity, and `AgentState` is far larger. What
  threats share with agents is the definition: both read their tuning from
  `self.characters[ix]`.

### D2. Every kind lookup goes through `t.character`

- `Settlement::threat_def(&self, t) -> &CharacterDef` replaces the three
  `self.waves[t.wave].character` lookups (`threat_radius`, `threat_attack`)
  and the wave's `damage` in `breach` (`health.breach_damage`).
- Stage 2 builds a per-character table once per tick,
  `(def.movement.walk / per_second, health.hp)` indexed by `CharacterIx`,
  and each threat reads its speed and full hp from it.
  - **Bit equality:** use exactly the wave's expression,
    `def.movement.walk / Fx::from_int(TICKS_PER_SECOND)`. Agents use
    `div_int`; the two may round differently.
  - **Why a table:** a fixed-point division per threat per tick is real
    cost at large threat counts; the character list has about ten entries.
- App: `legacy.rs`'s attack lookup reads `current.characters[t.character]`.

### D3. Waves name a character and stop copying its tuning

- `WaveState` drops `speed`, `hp`, `armor` and `damage`. Spawn reads
  `hp` and `armor.points` from the character.
- The scenario's `Wave` schema doesn't change: `character` stays.
- The loader requires `def.brain == Brain::FollowRoute` for a wave's
  character, with a message naming the wave, the character and its brain
  (as agents do). It replaces the "has no health" check; the content
  loader already makes `FollowRoute` imply `health`.
- **Why:** two copies of the tuning is the "kind through the wave" this
  step removes. Agents already read theirs from the definition.
- **Consequence:** a save loaded with *changed* content now uses the new
  speed, hp and breach damage for its threats and waves, as it already did
  for radius and attack. Peers in a session share the rules hash, so
  lockstep is unaffected.

### D4. The `FollowRoute` brain is the threat's stage 2, by name

- The body of `threats_self_update`'s per-threat closure moves into
  `fn follow_route(t: &mut Threat, speed: Fx, full_hp: u32, tick, terrain) -> Fx`
  (returns the knockback meters, as the closure does). Same order: shock,
  stagger, windup, walk.
- **No dispatch on `Brain`:** every threat is `FollowRoute` (D3 checks it
  at load), so a `match` would have one arm. A second threat brain (a
  hunter) adds the match, keyed on `threat_def(t).brain`, in the same
  closure.
- **No actions for threats yet:** `action::act` takes an `AgentState`, and
  a route follower decides nothing a player would. A hunter brain is where
  threats would pick `Move` (todo below).

### D5. Golden file, digest and format

- **Digest:** no change. It hashes `t.wave` and leaves out definition
  references, so `t.character` stays out. The removed `WaveState` tuning
  was never in it. Update its doc comment to name `Threat::character` as
  left out.
- **Format:** `FORMAT_VERSION` 20, one bump for the PR: "threats keep
  their character (`Threat::character`); waves no longer copy its speed,
  health, armor or breach damage." `remap_characters` remaps threats too;
  `set_characters` checks them.
- **Golden:** in `golden.txt` only the second column (total) changes:
  the `arena` lines in step 1 (the forest's first wave spawns at tick 600
  and the report stops at 599, so the forest never has a threat), and the
  `forest` and `arena` lines in step 3 (both carry waves). The `behaviour`
  column, the small scenario's `tick` lines and the subsystem lines stay
  (that scenario has no settlement). `testdata/*.hashes` stay (no
  settlements). Step 2 changes nothing: its golden file is byte-equal to
  step 1's.
- **Rules hash:** unchanged; no character file changes.
- **No gameplay commit.**

## Todos

- None of the open entries in `todo/*.md` belongs here. "Give the body a
  height" and "Trim body fields one role ignores" are about agents; they
  stay.
- Add to `todo/characters.md`: **Let threat brains pick actions.** Why:
  threats move themselves outside the action vocabulary (D4). What: when a
  hunter brain (or possessing a threat) lands, give threats `Move` through
  a threat `act`. Where: `combat.rs` (`follow_route`), `action.rs`. Found
  in: this handoff.

## Commit order

Step 5's order: digest (not needed, D5), format (steps 1 and 3), read the
definitions with the golden file unchanged (step 2), gameplay (none). Each
step is one commit on `kestrel-threat-characters`.

1. **Threats store their character.** Add `Threat::character`, set it at
   spawn. `remap_characters` and `set_characters` cover threats.
   `FORMAT_VERSION` 20 with its doc line (D5). Add `character` to every
   `Threat` literal. Print `size_of::<Threat>()` before and after; it must
   not grow. Nothing reads the field yet. Regenerate:
   `bazel run --config=remote //kestrel/sim:update_golden`; the diff must
   be the forest and arena total columns only. Don't run tests yet.
   - Where: `sim/src/settlement/combat.rs` (`Threat`, `combat_housekeeping`),
     `settlement/mod.rs` (`set_characters`), `save/src/lib.rs`
     (`remap_characters`, `FORMAT_VERSION`), `combat_tests.rs` (`threat`
     helper, `brute_radius_follows_its_armor`), `app/src/settlement.rs`
     (`forest_with_a_ghoul`).
   - Done in step 1: `Threat::character` beside `wave`, format 20.
     `size_of::<Threat>()` is 144 before and after (measured with a local
     `rustc` mirror of the struct). Golden: only the arena total columns
     changed; the forest lines didn't, since its first wave spawns at tick
     600 and the report stops at 599, so it never has a threat.
     `testdata/*.hashes` unchanged.
2. **Read the kind through the threat.** `threat_def` (D2) in
   `threat_radius`, `threat_attack` and `breach`; the per-character table
   and `follow_route` in stage 2 (D2, D4); the app's attack lookup.
   `WaveState` keeps its fields, unread except at spawn. **Run
   `bazel test --config=remote //kestrel/sim:sim_test` here:** it is the
   bit-equality proof, and `golden.txt` must match step 1's byte for byte.
   - Where: `combat.rs` (`threat_radius`, `threat_attack`, `breach`,
     `threats_self_update`), `app/src/legacy.rs` (threats).
   - Done in step 2: `threat_def`, the per-character `(speed, full hp)`
     table and `follow_route` as planned; `sim_test` passes and
     `golden.txt` is byte-equal to step 1's. Deviation: the
     `melee_staggers_...` test's switch from the wave's `speed` to the
     character's `movement.walk` moved here from step 3, since stage 2
     no longer reads the wave and the test would fail `sim_test`.
3. **Waves stop copying tuning.** Drop `speed`, `hp`, `armor`, `damage`
   from `WaveState`; spawn reads the character (D3). The loader checks
   `Brain::FollowRoute`. Tests that used wave fields read the character:
   `melee_staggers_a_wounded_ghoul_then_it_resumes_advancing` sets the
   test ghoul's `movement.walk` to 1 m/s in `s.characters` (its checks
   have tolerances), and `brute_radius_follows_its_armor` finds the brute
   wave by its character's armor. Regenerate the golden file; again totals
   only. `bazel build` the sim.
   - Where: `combat.rs` (`WaveState`, `combat_housekeeping`),
     `settlement/mod.rs` (`Settlement::new` waves), `combat_tests.rs`.
   - Done in step 3: `WaveState` lost `speed`, `hp`, `armor` and `damage`;
     spawn reads `hp` and `armor.points` from the character; the loader
     requires `Brain::FollowRoute`. `brute_radius_follows_its_armor` finds
     the brute wave by its character's armor (the `melee_staggers_...`
     change was done in step 2). Golden: the 16 forest and arena total
     columns changed (the forest's saved waves changed shape too), nothing
     else; `testdata/*.hashes` unchanged. `//kestrel/sim/...` builds.
4. **Tests and docs.** The tests below. Docs: the "Threats" bullet in
   `docs/character-definition.md` (done, link here), `Brain::FollowRoute`'s
   comment ("Walks its wave's route": the threat walks the route its wave
   planned), the `behaviour_digest` comment, the todo above, "Done in step
   N" notes here. Run the full suite once. Add the `docs/test-time.md` row.
   - Done in step 4: in `combat_tests.rs`,
     `threats_carry_their_waves_character`,
     `threat_tuning_follows_its_character_not_its_wave` (the test brute's
     walk and breach damage are set apart from the test ghoul's, since the
     builtin brute breaches for 1 like a ghoul),
     `a_wave_must_send_a_follow_route_character` and
     `threat_size_is_pinned` (144); in `settlement/tests.rs`,
     `set_characters_rejects_an_unknown_threat_character`; in
     `save/src/tests.rs`, `settlement_characters_remap_by_name` steps the
     forest until a threat exists. Docs and the todo as planned. The full
     suite (`--build_tests_only //kestrel/...`) passed, 24 of 24; the app
     needed no fixes.

## Tests

In `settlement/combat_tests.rs` unless noted:

- **Threats carry their wave's character:** run the arena until each wave
  has spawned; every threat's `character` is its wave's, and a brute
  starts with the brute's hp and armor.
- **Tuning follows the character, not the wave:** a threat with
  `wave: 0` (a ghoul wave) and the brute's `character` has the brute's
  plated radius, walks at the brute's speed, and a breach costs the
  brute's `breach_damage`. This proves no lookup still goes through the
  wave.
- **A wave must send a `FollowRoute` character:** a wave naming a hauler
  character fails `Settlement::new` with the wave's id in the message.
- **Unknown threat character:** `set_characters` rejects a threat whose
  index is out of range (`settlement/tests.rs`).
- **Save remap** (`save/src/tests.rs`): extend
  `settlement_characters_remap_by_name` to step the forest until a threat
  exists, then check threats' character names survive the shifted
  content, like waves'.
- **Size:** `size_of::<Threat>()` equals the value measured in step 1.
- **Bit equality:** `golden_report_matches` (behaviour columns unchanged
  across all steps, step 2 byte-equal), and
  `settlements_are_deterministic_across_thread_counts`.

## Open questions

- **Should the app draw a threat by its character instead of its wave's
  config?** `settlement.rs` reads skin and full hp from the legacy
  scenario's wave, which it already built from the wave's character, so
  the answer is the same. Recommendation: leave it; moving it needs a
  name table in the legacy view for no visible change.
- **Should `Pinned` store a character?** It is visual only and drawn
  through the wave's skin. Recommendation: leave it.
- **Drop `Threat::wave`?** Recommendation: no (D1).

## Rules for the work

- Each numbered step is its own commit, in order. Tests run once at the
  end, plus `sim_test` in step 2 (`AGENTS.md`, "When to run tests").
  Golden regeneration in steps 1 and 3 is a `bazel run`; count it.
- Bazel: `bazel test --config=remote ...` (`AGENTS.md`).
- Only the forest and arena total columns of `golden.txt` may change, and
  only in steps 1 (arena only) and 3. If a behaviour column, a small-scenario line or a
  `.hashes` file changes, the refactor slipped: find it, don't regenerate.
- Things found but out of scope go in `kestrel/todo/`.
- When a step is done, add a "Done in step N" note under it here.
