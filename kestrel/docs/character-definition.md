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
    view: (),
)
```

Read it from the outside in:

1. The outer `(...)` is one **character file**. It has a required `sim` section, a required `view` section, and optionally `base`.
2. `sim` holds the gameplay rules. Here the ghoul follows a route, has a collision radius of 0.27 meters, walks at 0.6 meters per second, and starts with 3 hit points.
3. `view: ()` is required but currently has **no fields**. This file does not select a mesh, skin, rig, or animation. Those visuals come from scene style assets.

The loader rejects unknown fields. A value left out of a standalone file is valid only when the schema gives it a default; a variant can also inherit it from `base`.

## The file schema

| Outer field | Type | Meaning |
| --- | --- | --- |
| `base` | Optional character ID string | Inherit gameplay fields from another character file, then override the fields written here. |
| `sim` | Required section | Gameplay behavior and tuning, detailed below. |
| `view` | Required empty section `()` | Reserved for visual selection; it cannot yet contain fields. |

The `sim` section has the same **shape** for every character kind, but `brain` determines which role-specific section is required:

| `sim` field | Shape | Rule |
| --- | --- | --- |
| `brain` | `Player` or `FollowRoute` | Required, unless inherited. Selects behavior implemented by the engine. |
| `body` | `(radius: number)` | Required collision radius in meters. |
| `movement` | `(walk: number, run: number)` | `walk` is required in meters per second. `run` is optional and defaults to `walk`. |
| `health` | `(hp: integer, breach_damage: integer, armor: (...))` | Required for `FollowRoute`; forbidden for `Player`. `armor` is optional. |
| `avatar` | `(acceleration: ..., braking: ..., attack_braking: ..., attack: (...))` | Required for `Player`; forbidden for `FollowRoute`. |

`health.hp` is starting hit points. `health.breach_damage` is damage dealt on reaching the route's target. If supplied, `health.armor` must contain both `points` and `radius`; the armor absorbs hits and enlarges the collision radius until it breaks.

`avatar.acceleration`, `braking`, and `attack_braking` are rates in meters per second squared. `avatar.attack` requires all seven frame counts: `windup`, `aim`, `swing`, `sword_windup`, `sword_swing`, `rebound`, and `combo_window`. Durations are written as whole frames at 60 Hz. The loader validates ranges and converts authored values to simulation units.

## The other role: a player

The player uses the **same outer file schema**. Its `Player` brain requires `avatar` rather than `health`:

```ron
(
    sim: (
        brain: Player,
        body: (radius: 0.32),
        movement: (walk: 3.8, run: 5.6),
        avatar: (
            acceleration: 26,
            braking: 32,
            attack_braking: 35,
            attack: (
                windup: 13, aim: 10, swing: 35,
                sword_windup: 7, sword_swing: 26,
                rebound: 25, combo_window: 75,
            ),
        ),
    ),
    view: (),
)
```

This is the built-in `exosuit` file. Player health is a scenario rule, so it does not appear here.

## Variants reuse a definition

`ghoul_brute.ron` starts with `ghoul` and overrides only movement speed, hit points, and armor:

```ron
(
    base: "ghoul",
    sim: (
        movement: (walk: 0.59),
        health: (hp: 5, armor: (points: 2, radius: 0.38)),
    ),
    view: (),
)
```

It inherits `FollowRoute`, `body.radius`, and `health.breach_damage`. Nested sections merge field by field. An `armor` value replaces the entire inherited armor value. The base file remains a usable character kind. Missing required fields after inheritance, unknown bases, and inheritance cycles are errors.

## Where this definition applies

| Object | Character file? | Where its instance state lives |
| --- | --- | --- |
| Player avatar | Yes: currently `exosuit` | The running world tracks its position and actions. |
| Wave enemy | Yes: the wave names a kind such as `ghoul` | Each spawned enemy tracks its own position, health, and armor. |
| Hauler | No | Scenario and simulation data currently supply its stats and behavior. |
| Defender, machine, cart, projectile | No | Each uses its own game system. |

A wave names the enemy kind it spawns; the player currently uses `exosuit`. Peers in a network game load matching simulation definitions. Jump, jetpack, and digging availability currently come from scene rules. The empty `view` section does not yet connect a character kind to a skin; [visual assets and characters](asset-system.md) explains the current visual path.
