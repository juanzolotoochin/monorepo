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
| `attack` | `(damage: integer, reach: number, windup: integer, cooldown: integer, knockback: number)` | Optional, `FollowRoute` only. How the character hits players in passing. |

`health.hp` is starting hit points. `health.breach_damage` is damage dealt on reaching the route's target. If supplied, `health.armor` must contain both `points` and `radius`; the armor absorbs hits and enlarges the collision radius until it breaks.

`attack` makes a threat hit players it passes, only in scenarios where players have health (`player_health` above 0). It stops beside the nearest one in reach, winds up for `windup` frames, then swings. A stagger or shock during the windup cancels the swing. The swing hits if the player is still within `reach` meters, measured center to center. A hit takes `damage` health and pushes the player away at `knockback` meters per second. The next swing can start `cooldown` frames after the hit. All five fields are required, after inheritance. A variant merges them field by field, like `health`. A character without `attack` never attacks.

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
- **Haulers:** their `Phase` state machine emits actions instead of moving them directly. The phase machine stays in stage 2 and picks `Move(GoTo)` actions, an accepted job's route goes through a new `Follow` action, and one `act` dispatches by role. Behaviour, the golden file and the save format don't change. [The hauler actions design](../HAULER_ACTIONS_HANDOFF.md) has the decisions and the order.
- **Threats:** they become characters with a `FollowRoute` brain. They keep their own table, for the 100k-entity budget, but each stores a `CharacterIx` instead of reaching its kind through its wave.
- **Shared body:** done. An agent is `AgentState { id, character, body, carry, role }` (`sim/src/settlement/agent.rs`):
  - `Body` is what every agent has: home, speed, radius, position, headings, route, velocity, the knockback `push`, and health.
  - `Carry` is the cargo, since players and haulers both carry (a depot transfer lands in the player's cargo).
  - `Role` is `Hauler(HaulerState)` or `Avatar(AvatarState)`. It is set when the agent is made and never changes, so `controlled` checks became matches. The avatar keeps the player's owner, name, weapon cooldown, respawn tick, height and abilities.
  - A hauler's health is 0, so it can't be hurt, and its velocity stays 0, since it walks its route without one. Giving haulers health is a later step that changes the `Hauler` schema.
  - **Inventory:** done. Depots, agents' cargo, piles and the collector each hold an `Inventory` (counts, one capacity, reservations out and in), and every unit moves through one path: a `CarryJob` on the agent's `Carry` holds its units on both sides when it starts, each unit flies as a `Transfer`, and `drop_carry` is the one early end. A player's cargo has one capacity for any mix of items and minerals. [The inventory design](../INVENTORY_HANDOFF.md) has the decisions and what each step did.
- **Prediction:** the sim and the client's replay apply actions through the same code. An ability's own state is always predicted. Its effects on the world are only shown early, as events built from its messages, and each event kind says whether to keep or cancel it when it never confirms. `kestrel/ABILITY_PREDICTION_HANDOFF.md` has the decisions and the order.
- **Threats attacking agents:** ghouls hit players in passing (`kestrel/GHOUL_ATTACK_HANDOFF.md` has the order):
  - **Behaviour:** a ghoul never leaves its route. When a player is in reach, it stops, winds up, swings, and then walks on. Chasing a target would be a separate hunter brain, later.
  - **Targets:** players only. Haulers have no health, so making them targets is a separate step that changes the `Hauler` schema.
  - **Latency:** the windup (about 400–500 ms) is longer than a typical input lead, and the ghoul shows it. Reach is checked when the swing lands, against where the player is then. A hit deals damage and a small knockback, smoothed on clients by the `Knockback` correction. It doesn't stun.
  - **Tuning:** an optional `attack` section on `FollowRoute` characters (`damage`, `reach`, `windup`, `cooldown`, `knockback`). A ghoul without it never attacks, as before.
  - **Targeting:** the nearest player in reach, with the lowest agent index on a tie. Each player scans the threats for ones in reach, as a projectile already does, so a threat with no player near costs one distance check.
  - **Height:** a ghoul can't reach a player whose feet are more than 1 m above the ground under it (`SWING_HEIGHT` in `combat.rs`). This holds when the windup starts and when the swing lands, so a player who lifts off during the windup is missed. A jump (apex about 0.57 m) doesn't dodge; the jetpack's hover (1.8 m) does.
  - **Push:** the push moves a busy player as far as an idle one, about 0.14 m at 3 m/s, braked at the walking rate and by the walking rules. It ends a dig. During the player's own melee swing it is kept apart from the avatar's velocity (`Body::push`), since the swing keeps that velocity from moving the avatar.
- **Out of scope:** possession. Defenders stay as they are.

### Open questions

- **Where meshes live:** do styles stay the mesh and material libraries that a character's `view` refers to, or does each character carry its own meshes?
