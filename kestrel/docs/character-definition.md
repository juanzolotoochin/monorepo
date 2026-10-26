# Character definitions

New to RON or visual assets? [From a cube to a playable character](asset-walkthrough.md) introduces those concepts before this schema reference.

A character definition is the gameplay data for **one kind of character**. Every ghoul can use the same definition, while each ghoul in the world has its own position, health, and armor state. The file name is the kind's ID: `kestrel/content/data/character/ghoul.ron` defines `ghoul`.

RON is the file syntax, not a schema shared by all game objects. Character files use the schema on this page. Boulder recipes, material palettes, style profiles, and scene layouts are different kinds of RON files with different fields. [Visual assets and characters](asset-system.md) shows how those files fit together.

## Start with one complete file

This is the `ghoul` character definition:

```ron
(
    sim: (
        brain: FollowRoute,
        body: (radius: 0.27),
        movement: (walk: 0.6),
        health: (hp: 3, breach_damage: 1),
    ),
    view: (skin: "ghoul"),
)
```

Read it from the outside in:

1. The outer `(...)` is one **character file**. It has a required `sim` section, a required `view` section, and optionally `base`.
2. `sim` holds gameplay rules. This ghoul follows a route, has a collision radius of 0.27 meters, walks at 0.6 meters per second, and starts with 3 hit points.
3. `view` says how the character is drawn. This ghoul is drawn by the `ghoul` skin, which each scene's style supplies: the character picks the skin by name, and the style holds its meshes.

The loader rejects unknown fields. A value left out of a standalone file is valid only when the schema gives it a default; a variant can also inherit it from `base`.

## The file schema

| Outer field | Type | Meaning |
| --- | --- | --- |
| `base` | Optional character ID string | Inherit gameplay fields from another character file, then override the fields written here. |
| `sim` | Required section | Gameplay behavior and tuning, detailed below. |
| `view` | Required section | How the character is drawn, detailed below. Never affects gameplay or the rules hash. |

The `sim` section has the same **shape** for every character kind, but `brain` determines which role-specific section is required:

| `sim` field | Shape | Rule |
| --- | --- | --- |
| `brain` | `Player`, `FollowRoute`, or `Hauler` | Required, unless inherited. Selects behavior implemented by the engine. |
| `body` | `(radius: number)` | Required collision radius in meters. |
| `movement` | `(walk: number, run: number)` | `walk` is required in meters per second. `run` is optional and defaults to `walk`. |
| `health` | `(hp: integer, breach_damage: integer, armor: (...))` | Required for `FollowRoute`; forbidden for `Player` and `Hauler`. `armor` is optional. |
| `avatar` | `(acceleration: ..., braking: ..., attack_braking: ..., attack: (...))` | Required for `Player`; forbidden for `FollowRoute` and `Hauler`. |
| `carry` | `(transfer: integer)` | Required for `Player` and `Hauler`; forbidden for `FollowRoute`. `transfer` is frames to load or unload one unit. |
| `abilities` | List of abilities, such as `[Jetpack(...), Dig(...)]` | Optional, `Player` only. Each ability at most once, and never both `Jump` and `Jetpack`. A variant replaces the whole list. |

`health.hp` is starting hit points. `health.breach_damage` is damage dealt on reaching the route's target. If supplied, `health.armor` must contain both `points` and `radius`; the armor absorbs hits and enlarges the collision radius until it breaks.

`avatar.acceleration`, `braking`, and `attack_braking` are rates in meters per second squared. `avatar.attack` requires seven frame counts: `windup`, `aim`, `swing`, `sword_windup`, `sword_swing`, `rebound`, and `combo_window`. Durations, including `carry.transfer`, are written as whole frames at 60 Hz. The loader validates ranges and converts authored values to simulation units.

Abilities are moves the engine implements; the file picks them by name and tunes them:

| Ability | Fields | Meaning |
| --- | --- | --- |
| `Jump` | `prep`, `land` (frames), `velocity` (m/s), `gravity` (m/s²) | A grounded jump with a crouch before takeoff and a recovery after landing. |
| `Jetpack` | `hover` (m), `climb`, `sink` (m/s) | Hold to hover over the ground ahead. |
| `Dig` | `time` (frames), `centering` (m/s) | Dig out the tile underfoot, where the scene's ground can be dug (`Digging`). |
| `SwiftSword` | none | Swing the sword with `sword_windup` and `sword_swing` instead of `windup` and `swing`. |

Commands for an ability the character lacks do nothing. A player with neither `Jump` nor `Jetpack` stays on the ground.

The `view` section has three fields, each required or forbidden by role:

| `view` field | Shape | Rule |
| --- | --- | --- |
| `skin` | Skin ID string, such as `"ghoul"` | Required for `FollowRoute` and `Hauler`; forbidden for `Player`, which its `actor` draws. Every scene that spawns the character must have a style with that skin. |
| `actor` | Actor name string, such as `"exosuit"` | Required for `Player`; forbidden otherwise. Names the articulated body the player is built from, a file in `kestrel/app/actors/`. |
| `motion` | `(motion_style: Natural, arm_swing: 0.43, ...)` | Optional, `Player` only. Tunes how the rig poses; omitted fields keep their defaults. |

`motion` holds `motion_style` (`Classic` or `Natural`), `arm_swing`, `leg_swing`, `torso_turn`, `pose_response`, `run_crouch`, `elbow_drive`, `step_lift`, `step_distance`, `plant_feet`, `leg_reach`, and `turn_inertia`; the [animation guide](character-animation.md) explains them. It has no speeds: the rig poses at the character's `movement` speeds, so what you see always matches how fast the simulation moves it. The rig a style bakes can still adapt the tuning, for example planting the feet of a legged actor.

## The other roles

The player uses the same outer file schema. Its `Player` brain requires `avatar` and `carry` rather than `health`:

```ron
(
    sim: (
        brain: Player,
        body: (radius: 0.32),
        movement: (walk: 3.8, run: 5.6),
        carry: (transfer: 24),
        abilities: [
            Jetpack(hover: 1.8, climb: 5.5, sink: 3),
            Dig(time: 120, centering: 1.5),
        ],
        avatar: (
            acceleration: 26, braking: 32, attack_braking: 35,
            attack: (
                windup: 13, aim: 10, swing: 35,
                sword_windup: 7, sword_swing: 26,
                rebound: 25, combo_window: 75,
            ),
        ),
    ),
    view: (actor: "exosuit"),
)
```

This is the built-in `exosuit` definition. Player health is a scenario rule, so it does not appear here. Its body is the `exosuit` actor, `kestrel/app/actors/exosuit.ron`: a tree of joints, each with a pivot relative to its parent and the boxes drawn on it. The player is posed by joint kind (`Body`, `LeftArm`, `RightHand`, `Weapon`, and so on), and a scene's style may replace any joint's boxes with its own baked meshes.

Each scene names its player character with `player: "exosuit"`, and every player avatar in that scene uses it. A scenario's controlled agents must name the same character. The moonlit glade's player is `exosuit_moonlit`: it inherits `exosuit`, walks at 1.6 and runs at 2.6 m/s, lists `[Jump(prep: 8, land: 10, velocity: 4, gravity: 14), SwiftSword]` instead of the jetpack and dig, and sets its own `view.motion`. The laser pit's `exosuit_laser_pit` changes only `view.motion`, for a planted-foot gait: a scene that wants a different feel uses a character variant.

A hauler uses `Hauler` and `carry`, without an `avatar` or `health` section:

```ron
(
    sim: (
        brain: Hauler,
        body: (radius: 0.28),
        movement: (walk: 1.5),
        carry: (transfer: 27),
    ),
    view: (skin: "worker"),
)
```

This is the `worker` definition. The scenario separately chooses each worker's position, cargo capacity, links, and equipment. The `runner` variant inherits `worker` and changes its walk speed; the `ox` is another hauler kind.

## Variants reuse a definition

`ghoul_brute.ron` starts with `ghoul` and overrides only movement speed, hit points, and armor:

```ron
(
    base: "ghoul",
    sim: (
        movement: (walk: 0.59),
        health: (hp: 5, armor: (points: 2, radius: 0.38)),
    ),
    view: (skin: "ghoul_brute"),
)
```

It inherits `FollowRoute`, `body.radius`, and `health.breach_damage`, and draws with its own skin. Nested sections, `view` included, merge field by field. An `armor` value replaces the entire inherited armor value. The base file remains a usable character kind. Missing required fields after inheritance, unknown bases, and inheritance cycles are errors.

## Where definitions and visuals meet

| Object | Character definition | Visual selection |
| --- | --- | --- |
| Player avatar | The scene's `player`, such as `exosuit` | The character's `view.actor` builds the body, the scene's style may re-dress its joints, and `view.motion` tunes its pose. |
| Wave enemy | A kind such as `ghoul_forest` | The character's `view.skin`, from the scene's style. |
| Hauler | A kind such as `worker`, `runner`, or `ox` | The character's `view.skin`, from the scene's style. |
| Defender, machine, cart, item | No character definition | The scenario names a skin directly. |

Each spawned character has its own position and other instance state. Scenario agents can also have different capacities and equipment while sharing a character definition. The character names a skin and the style supplies it, so one character can look different in each scene's style; [visual assets and characters](asset-system.md) explains the visual path. What a player can do comes from its character's `abilities`; whether the ground can be dug is the scene's `Digging` feature.

## Next steps

Character definitions are moving in steps, each its own PR against `master`. Steps 1 to 4 are done: characters carry their gameplay, haulers and abilities moved into them, and the `view` section builds and poses every player body through one path. Each step follows the same commit order:

1. Extend the behaviour digest if the step needs it.
2. Change the format.
3. Read the definitions, with the golden file unchanged and bit-equality tests.
4. Make gameplay changes last, each in its own commit.

### Step 5: one set of actions for players and AI

- **Actions:** one vocabulary (move, go to, attack, interact) that player commands and AI brains both produce. `CommandKind` variants stay append-only, so recorded input logs still replay. An action changes only its agent; what it does to the world is a message applied in stage 5 (done for player avatars, `sim/src/settlement/action.rs`).
- **Haulers:** their `Phase` state machine emits actions instead of moving them directly.
- **Threats:** they become characters with a `FollowRoute` brain. They keep their own table, for the 100k-entity budget, but each stores a `CharacterIx` instead of reaching its kind through its wave.
- **Shared body:** `AgentState` splits into the body every agent has (position, heading, velocity, radius, health) plus data for each role. Cargo has three models today (hauler `cargo`, the avatar's `ManualJob`, pile jobs), and they merge here.
- **Prediction:** the sim and the client's replay apply actions through the same code. An ability's own state is always predicted. Its effects on the world are only shown early, as events built from its messages, and each event kind says whether to keep or cancel it when it never confirms. `kestrel/ABILITY_PREDICTION_HANDOFF.md` has the decisions and the order.
- **Out of scope:** possession. Defenders stay as they are.

### Open questions

- **Threats attacking agents:** should ghouls attack agents, so the player can be hurt by them and not only by lasers? It is a gameplay change, so it gets its own commit.
- **Where meshes live:** do styles stay the mesh and material libraries that a character's `view` refers to, or does each character carry its own meshes?
