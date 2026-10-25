# Character definitions

Status, 25 October 2026:
- **Steps 1 and 2 are implemented.**
- **Steps 3 to 5** are planned; see [Next steps](#next-steps).

Background:
- The survey of how characters were assembled before this is
  [character architecture](character-architecture.md).
- The decisions are recorded in [the discussion page](design/character-definition.html).

## Decisions

| Question | Decision |
|---|---|
| Data or code | Stats and timings are data. Brains and abilities are Rust code, picked by enum name. |
| Variants | Separate character files. A file may name a `base` and override fields. |
| Scope | Player avatars, haulers and enemies. Defenders, machines, carts and projectiles are out. |
| Brains | A shared action vocabulary in step 5. No possession. |
| Player rig | The exosuit becomes an actor recipe on one rig path (step 4). |
| Golden hashes | Simulation results stay identical while values move. Values change only in separate commits. |
| Player radius | One radius: the character's body radius. |
| Ghoul speed | Every ghoul walks at 0.6 m/s, the forest speed. 0.75 was tried and dropped: the forest no longer held its first minute. |
| Player health | Stays a scenario rule (`player_health`). |
| Timing units | Whole 60 Hz frames, now a project-wide guideline (`architecture.md`). |
| Player character | Each scenario agent names its character; the player agents name `exosuit`. The view still assumes `exosuit` (`PLAYER_CHARACTER`) until scenes name their player (step 3). |
| Hauler capacity | Stays on the scenario agent: haulers of one kind carry different loads by job. To revisit with better tooling. |
| Transfer time | A character stat (`carry.transfer`), for haulers and the player. |
| Carts | Stay in the scenario agent's `equipment`. |
| Base-only files | Ordinary characters. A base may also be used on its own. |
| Jump, jetpack, dig | Move with abilities in step 3. |
| Spawn fallout | Allowed to move a spawn point. None needed to move. |
| Studio timings | The view reads the builtin content's exosuit. Every session uses builtin content. |

## The migration

1. **Done:** character files, the player and enemies.
2. **Done:** haulers become characters.
3. Abilities move from scene flags to characters.
4. A `view` section and one rig path.
5. A shared brain and action vocabulary.

[Next steps](#next-steps) below details each one.

## Files

```
kestrel/content/data/character/
  exosuit.ron           the player avatar
  ghoul.ron             hp 3, walk 0.6; the base for the others
  ghoul_forest.ron      base: ghoul; hp 2
  ghoul_brute.ron       base: ghoul; hp 5, walk 0.59, armor
  worker.ron            hauler; walk 1.5, radius 0.28, transfer 27 frames
  runner.ron            base: worker; walk 1.7
  ox.ron                hauler; walk 1.1, radius 0.75, transfer 27 frames
```

Files are named by path (`character/ghoul`). Scenarios and `base` use the short
name (`ghoul`). They are compiled into the binary through `content::BUILTIN`.

```ron
// character/ghoul_brute.ron
(
    base: "ghoul",
    sim: (
        movement: (walk: 0.59),
        health: (hp: 5, armor: (points: 2, radius: 0.38)),
    ),
    view: (),
)
```

- Character files are parsed with RON's `implicit_some`, so optional values are
  written bare.
- `view` is empty until step 4.

## Types

All of these are in `content::character`.

- `CharacterDef { brain, body, movement, health, avatar, carry }`
  - `brain`: `Player`, `FollowRoute` or `Hauler`.
  - `body.radius`: meters.
  - `movement`: `walk` and `run` in m/s. `run` defaults to `walk`.
  - `health`: enemies only. Holds `hp`, `breach_damage`, and optional
    `armor { points, radius }`. Breaking the armor shrinks the threat to its body radius.
  - `avatar`: Player characters only.
    - `acceleration`, `braking`, `attack_braking`: written in m/s², stored per tick.
    - `attack: AttackTiming`: windup, aim, swing, sword windup and swing,
      rebound and combo window. Written as frames, stored as ticks.
  - `carry.transfer`: players and haulers only. Ticks to load or unload one unit,
    written as frames. How much a character carries is set by the scenario.
- `CharacterIx = u16`, indexing `Rules::characters` in name order.
- `PLAYER_CHARACTER = "exosuit"`.

The loader works in four stages:
1. It resolves `base` chains. Unknown bases and cycles are errors that name the file.
2. It merges leaf fields: a variant's `movement: (walk: 0.59)` keeps its
   base's `run`. Armor is replaced whole.
3. It validates the merged definition: required fields, ranges, `Player` with
   `avatar` and `carry` and no `health`, `FollowRoute` with `health` only,
   `Hauler` with `carry` only, and armor radius at least body radius.
4. It converts units.

`Rules`, `Names` and `Views` gain `characters`. `Rules::max_body_radius` and
`max_speed` stay movers-only, since they size the mover chunk queries.

## Simulation

- **World builders:** `World::new_scene` and `new_settlement` take `&Content`, so
  scenarios can resolve character names.
- **Rules on the settlement:** `Settlement::characters` holds the rules. It is
  `#[serde(skip)]`, so it is left out of saves and hashes, and
  `World::from_state` restores it with `set_characters`, which checks every index.
- **Index fields:** every `AgentState` has a `character: CharacterIx`, named by
  the scenario agent's `character:`. Avatars must name a `Player` character and
  haulers a `Hauler` one. `WaveState.character` names each wave's enemy.
- **Copied tuning:** agents copy walk speed, body radius and transfer time into
  `AgentState` (`speed`, `base_radius`, `transfer_ticks`). Waves copy `hp`,
  `armor`, `speed` and breach `damage` into `WaveState`. Both copy from the definition.
- **Scenario agents** keep `id`, `label`, `skin`, `position`, `capacity`, `links`,
  `equipment` and the cargo view fields. Capacity and carts stay there.
- **Avatars:**
  - Movement, piles routing and the laser's hit test read the avatar's character.
    The character table reaches them through `haulers::Ctx`.
  - A controlled agent's `base_radius` is its body radius, so haulers avoid the
    avatar at the size it collides with.
- **Threats:** `Settlement::threat_radius` gives the armor radius while
  plating is left and the body radius after.
- **View:**
  - `SessionService::player_character()` serves prediction and route checks.
  - The swing phases come from the builtin exosuit, through `lockstep::builtin_content()`.
  - The view's scenario copy (`legacy::Scenario`) takes agent speed, radius and
    transfer time from the same builtin characters.

Saves are format 4: format 3 added character indices, and format 4 gives every
agent one. Character indices are remapped by name, and only the characters a
save uses must exist. The rules hash covers characters. Input logs are unchanged.

## How results were kept

| Commit | Golden hashes | Behaviour digest |
|---|---|---|
| Behaviour digest added to `golden.txt` (and the combat arena) | forest unchanged | new |
| Definitions, wave references, index fields, save format 3 | change (encoding) | unchanged |
| Code reads definitions | unchanged | unchanged |
| One player radius | change | unchanged |
| All ghouls at 0.6 m/s (gameplay change) | change | arena changes, forest unchanged |
| Step 2: hauler characters, every agent indexed, save format 4 | change (encoding) | unchanged |
| Step 2: agents read their characters | unchanged | unchanged |

- `builtin_characters_keep_the_former_constants` pins every moved value to the
  bits of the constant it replaced.
- `builtin_haulers_keep_the_former_scenario_values` does the same for the
  forest haulers' speed and radius. It also checks that 27 and 24 frames give
  the same ticks as the former 0.45 s and 0.4 s.
- The sim's test fixtures keep their own values (2 m/s, 0.18 m, 6 frames)
  through `test_hauler` and `test_player` characters.
- The parity probe (two unattended minutes) is the same before and after the radius
  fix: forest 99 delivered and 23 kills, arena 10 kills and 22 breaches. In idle runs no hauler passes close to
  the player. The radius matters once the player walks among the haulers.
- **Ghoul speed:** 0.75 m/s was tried first. The forest base then fell to 13 of 20 in
  its first minute unattended, and kills dropped from 23 to 13 over two minutes.
  At 0.6 the forest is unchanged. The arena gets easier: over two minutes,
  kills rise from 10 to 14, breaches fall from 22 to 18, and base health ends at 12
  instead of 8. The arena demo's strikes are retimed for the slower arrivals.

## Next steps

Each step is its own PR against `master`. Each follows step 1's commit order:
1. Extend the behaviour digest if the step needs it.
2. Change the format.
3. Read the definitions, with the golden file unchanged and bit-equality tests.
4. Make gameplay changes last, each in its own commit.

### Follow-ups from step 1

These are manual checks the tests can't make:
- **Combat arena:** do the ghouls at 0.6 m/s pace well?
- **Forest outpost:** walk through the hauler lanes. Haulers now keep 0.32 m
  from the player instead of 0.18.
- **Swing timing:** the view's swing phases follow the sim's frames since #86.
  They are under 10 ms off the old values, so check that swings still look right.

### Step 2: haulers become characters (done)

| Character | Walk | Radius | Transfer | Used by |
|---|---|---|---|---|
| `worker` | 1.5 | 0.28 | 27 frames | forester, lumberjack, ironmonger |
| `runner` (base `worker`) | 1.7 | | | runner |
| `ox` | 1.1 | 0.75 | 27 frames | bramble |
| `exosuit` | | | 24 frames | the player |

Capacity stays on the scenario agent, so no `porter` file was needed. Agent
`skin` fields stay until step 4.

### Step 3: abilities move to characters

- **Abilities:** a character lists them as code picked by name:
  - `Jump` replaces the `JUMP_*` constants.
  - `Jetpack` replaces `HOVER`, climb and sink.
  - `Dig` replaces `DIG_TICKS` and centering.
  - `SwiftSword` replaces the scene flag that picks the short sword timings.

  Their tuning moves into the character files, in frames and per-second rates.
- **Scene flags:** the `Jump`, `SwiftSword` and `Digging` scene features go away. Scenes name
  their player character, for example `player: "exosuit"`, which replaces `PLAYER_CHARACTER`.
- **Moonlit glade:** it is the only style that feeds speeds into the sim
  (`character_motion.walk_speed 1.6`, `run_speed 2.6`), and the only scene with
  Jump and SwiftSword. It becomes a player variant, for example
  `exosuit_moonlit` (base `exosuit`; walk 1.6, run 2.6, Jump, SwiftSword).
  `lockstep::scene_setup` then stops reading locomotion from the style, which
  closes the rules-hash gap from the architecture survey.
- **Character Studio** reads capabilities (can jump, weapon) from the definition
  instead of guessing from mesh coverage (`character_catalog.rs`).
- **To decide:**
  - Jump and jetpack are exclusive per scene today. Can a character have both?
  - Do the scenario's `weapons` and `weapon_tuning` move to the character, or
    stay arena rules? Player health already stays a scenario rule.

### Step 4: the view section and one rig path

- **View section:** `view` gains the skin or actor reference, rig kind, scale, motion tuning
  (today's `character_motion`), sounds and nametag. Scenario and wave `skin`
  fields go away.
- **The exosuit:** its hand-built hierarchy (about 450 lines in `scene.rs`
  `spawn_player`) becomes an actor recipe. Every character is posed through one rig
  path, and rig pivots get a single source.
- **One pose builder** for the local player, remote players and Character Studio
  replaces the three today (`animate_character`, `peers::update`, `viewer.rs`). Remote players
  stop deep-copying the local player's entity tree.
- **Pose parity:** check it against today's captures with `--motion-showcase`,
  `--jump-combo-showcase` and the Studio clips before deleting the old path.
- **To decide:** do styles stay as mesh and material libraries that a character's
  view section references, or does each character carry its own meshes?

### Step 5: a shared brain and action vocabulary

- **Actions:** one vocabulary (move, go to, attack, interact) that player commands and
  AI brains both produce. `CommandKind` variants stay append-only for input logs.
- **Haulers:** their `Phase` state machine emits actions instead of moving directly.
  Threats become characters with a `FollowRoute` brain. They keep their own
  table for the 100k-entity budget but store a `CharacterIx` instead of going
  through their wave.
- **Shared code:** `AgentState` splits into a shared body (position, heading,
  velocity, radius, health) plus role data. Cargo has three models today (hauler
  `cargo`, avatar `ManualJob`, pile jobs); they merge here.
- **Out of scope:** possession. Defenders stay as they are.
- **To decide:** do threats attack agents, so the player can be hurt by ghouls
  and not only by lasers? This is a gameplay change, so it gets its own commit.

## Open

- Hauler capacity is set per scenario agent. Revisit it once there is better
  content tooling.
