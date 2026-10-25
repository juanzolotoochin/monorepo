# Character definitions

Status, 25 October 2026:
- **Step 1 is implemented.**
- **Steps 2 to 5** are planned.

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
| Player character | A fixed default (`exosuit`) until scenes name their player (step 3). |
| Base-only files | Ordinary characters. A base may also be used on its own. |
| Jump, jetpack, dig | Move with abilities in step 3. |
| Spawn fallout | Allowed to move a spawn point. None needed to move. |
| Studio timings | The view reads the builtin content's exosuit. Every session uses builtin content. |

## The migration

1. **Done:** character files, the player and enemies.
2. Scenarios name characters for haulers too.
3. Abilities move from scene flags to characters. Scenes name their player, and
   styles stop feeding walk and run speed into the sim.
4. A `view` section, the exosuit as an actor recipe, and one shared pose builder.
5. A shared brain and action vocabulary. Threats become characters with a brain.

## Files

```
kestrel/content/data/character/
  exosuit.ron           the player avatar
  ghoul.ron             hp 3, walk 0.6; the base for the others
  ghoul_forest.ron      base: ghoul; hp 2
  ghoul_brute.ron       base: ghoul; hp 5, walk 0.59, armor
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

- `CharacterDef { brain, body, movement, health, avatar }`
  - `brain`: `Player` or `FollowRoute`. `Hauler` arrives in step 2.
  - `body.radius`: meters.
  - `movement`: `walk` and `run` in m/s. `run` defaults to `walk`.
  - `health`: enemies only. Holds `hp`, `breach_damage`, and optional
    `armor { points, radius }`. Breaking the armor shrinks the threat to its body radius.
  - `avatar`: Player characters only.
    - `acceleration`, `braking`, `attack_braking`: written in m/s², stored per tick.
    - `attack: AttackTiming`: windup, aim, swing, sword windup and swing,
      rebound and combo window. Written as frames, stored as ticks.
- `CharacterIx = u16`, indexing `Rules::characters` in name order.
- `PLAYER_CHARACTER = "exosuit"`.

The loader works in four stages:
1. It resolves `base` chains. Unknown bases and cycles are errors that name the file.
2. It merges leaf fields: a variant's `movement: (walk: 0.59)` keeps its
   base's `run`. Armor is replaced whole.
3. It validates the merged definition: required fields, ranges, `Player` with
   `avatar` and no `health`, `FollowRoute` with `health` and no `avatar`, and
   armor radius at least body radius.
4. It converts units.

`Rules`, `Names` and `Views` gain `characters`. `Rules::max_body_radius` and
`max_speed` stay movers-only, since they size the mover chunk queries.

## Simulation

- **World builders:** `World::new_scene` and `new_settlement` take `&Content`, so
  scenarios can resolve character names.
- **Rules on the settlement:** `Settlement::characters` holds the rules. It is
  `#[serde(skip)]`, so it is left out of saves and hashes, and
  `World::from_state` restores it with `set_characters`, which checks every index.
- **Index fields:** `AgentState.character: Option<CharacterIx>` is set for avatars;
  haulers get one in step 2. `WaveState.character` names each wave's enemy.
- **Waves:** they still copy `hp`, `armor`, `speed` and breach `damage` into
  `WaveState`, now from the definition.
- **Avatars:**
  - Movement, piles routing and the laser's hit test read the avatar's character.
    The character table reaches them through `haulers::Ctx`.
  - A controlled agent's `base_radius` is its body radius, so haulers avoid the
    avatar at the size it collides with.
  - A scenario `radius` on a controlled agent is an error.
- **Threats:** `Settlement::threat_radius` gives the armor radius while
  plating is left and the body radius after.
- **View:**
  - `SessionService::player_character()` serves prediction and route checks.
  - The swing phases come from the builtin exosuit, through `lockstep::builtin_content()`.

Saves move to format 3. Character indices are remapped by name, and only the
characters a save uses must exist. The rules hash covers characters. Input
logs are unchanged.

## How results were kept

| Commit | Golden hashes | Behaviour digest |
|---|---|---|
| Behaviour digest added to `golden.txt` (and the combat arena) | forest unchanged | new |
| Definitions, wave references, index fields, save format 3 | change (encoding) | unchanged |
| Code reads definitions | unchanged | unchanged |
| One player radius | change | unchanged |
| All ghouls at 0.6 m/s (gameplay change) | change | arena changes, forest unchanged |

- `builtin_characters_keep_the_former_constants` pins every moved value to the
  bits of the constant it replaced.
- The parity probe (two unattended minutes) is the same before and after the radius
  fix: forest 99 delivered and 23 kills, arena 10 kills and 22 breaches. In idle runs no hauler passes close to
  the player. The radius matters once the player walks among the haulers.
- **Ghoul speed:** 0.75 m/s was tried first. The forest base then fell to 13 of 20 in
  its first minute unattended, and kills dropped from 23 to 13 over two minutes.
  At 0.6 the forest is unchanged. The arena gets easier: over two minutes,
  kills rise from 10 to 14, breaches fall from 22 to 18, and base health ends at 12
  instead of 8. The arena demo's strikes are retimed for the slower arrivals.

## Open

- `speed` on a controlled scenario agent is still required but unused. Step 2 removes it.
