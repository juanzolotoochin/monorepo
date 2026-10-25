# Characters today: how they are assembled

A snapshot (25 October 2026) of how Kestrel builds a character from data, sim
state, input, AI and visuals. It is the starting point for moving to one joint
character definition. Paths are relative to `kestrel/`.

## Summary

There is no "character" concept in the code. What a player sees as one
character is put together from **five independent sources**, joined by
string ids and by which scene is loaded:

| Concern | Where it lives | Keyed by |
|---|---|---|
| Gameplay stats (haulers, enemies) | scenario RON (`levels/*_supply.ron`) via `content/src/scenario.rs` | agent/wave `id` |
| Gameplay stats (player) | Rust consts in `sim/src/settlement/avatar.rs:24-74` | none (global) |
| Capabilities (jump, swift sword, dig, jetpack) | scene feature flags in the layout RON (`SceneFeature`, `tools/assetc/core/src/lib.rs:281`) | scene |
| Visuals and motion tuning | style RON (`styles/*.ron`): `actor`, `skins`, `materials`, `character_motion` | style / `skin` string |
| Animation code | `app/src/character.rs` (player) **or** `app/src/rig.rs` (everything else) | which path spawned it |

`app/assets/BUILD.bazel` is what binds a layout, style and scenario into one
`ScenePack` per scene. So **a character's identity is a property of the
scene**: each scene has exactly one player actor (its style's `actor`), and a
"ghoul" in forest and a "ghoul" in combat are two separately authored skins.

## The kinds of character

| Kind | Sim type | Brain | Visual | Animation |
|---|---|---|---|---|
| Local player | `AgentState { controlled: true }` + `AvatarState` | player commands | procedural exosuit (`scene.rs:584 spawn_player`) dressed by style `actor` meshes (`style.rs:101 apply`) | `character::Pose` / `animate_character` |
| Remote players | same as local | their commands | deep copy of the local player's entity tree (`peers.rs:365 copy`) | `Pose`, built again in `peers.rs:109 update` |
| Haulers (worker, ox) | `AgentState { controlled: false }` | `haulers::decide` plus `Phase` FSM | baked `SkinMesh` | `rig::Rig::pose`, chosen by the skin's `Motion` |
| Enemies (ghouls) | `combat::Threat` (separate table) | none: follow the wave route | baked `SkinMesh` | `Rig::pose` |
| Defenders | `combat::DefenderState` | nearest threat in range | baked `SkinMesh` (Turret) | `Rig::pose` |
| Memory ghost | `memory::Sample` replay | recording | — | — |
| Movers (Milestone 0) | `sim::mover` + `content/data/mover/*.ron` | own | spheres in `kestrel/view` | none |

The view-side `Rig` is shared by NPCs, enemies, machines, turrets and
projectiles (`settlement.rs:500-650`). It picks a motion by asset metadata,
not by sim role. That part is already close to the "one definition" idea.

## Flow, end to end

```
Authoring                      Build                     Runtime
---------                      -----                     -------
levels/<scene>.ron  ─┐
levels/<x>_supply.ron├─ assetc ─▶ ScenePack ─▶ SceneLibrary (assets.rs:31)
styles/<style>.ron  ─┘   (BUILD.bazel)          │
                                                ├─▶ lockstep::scene_setup ─▶ SceneSetup ─▶ sim Settlement (tick 0)
                                                │      (features, style walk/run speed)    AgentState / Threat / Defender
                                                └─▶ view: spawn_player + style::apply, RigTemplates per skin

Input:  devices ─▶ InputService::sample ─▶ ActionFrame ─▶ gameplay::dispatch ─▶ PlayerControl helpers
        ─▶ SessionService::command(CommandKind) ─▶ net turn ─▶ World::apply_commands ─▶ Settlement::{move_avatar, attack, …}

Tick:   self update (avatar::walk/fly/jump_step, haulers::self_update, threats)
        ─▶ read (haulers::decide, read_avatar, read_combat) ─▶ messages ─▶ apply ─▶ housekeeping

View:   WorldMirror ─▶ follow_avatar (local, with prediction) ─▶ animate_character
                    ─▶ peers::update (remote players)
                    ─▶ legacy::Simulation ─▶ settlement "desired" map ─▶ Rig::pose (NPCs, enemies)
```

### Input and possession

- Devices → `InputService::sample` (`app/src/input/mod.rs:384`) → `ActionFrame`
  → `input::gameplay::dispatch` (`gameplay.rs:155`) → `PlayerControl` helpers
  (`lockstep.rs:709-819`) → `CommandKind` (`sim/src/world.rs:63-135`).
- There is no possession component. The controlled character is found on
  every call by `avatar_of(player)`, which matches `AgentState.owner`.
  `Settlement::player()` (first controlled agent) is a second, different lookup.
- Commands that bypass `InputService`: the automation protocol
  (`control/mod.rs:285`), showcases (`preview.rs`), palimpsest, piles and
  settlement transfers.

### AI

- No shared brain or action vocabulary. Players speak `CommandKind`/`Intent`.
  Haulers speak `Phase` plus economy messages. Threats only follow routes.
  Defenders aim at the nearest threat. Only players can `Strike`, and threats
  never attack agents.
- `AgentState` (`haulers.rs:68`) is one struct for haulers and avatars. Player-only
  fields (`avatar`, `attack_ready`) sit next to hauler-only ones (`links`,
  `assignment`, `trailer`, `phase`), and every avatar method re-checks
  `a.controlled`.

### Visuals and animation

- **Two rig systems.**
  - Player: `character::Part`/`PartKind`, a hand-built exosuit hierarchy (about 450 lines in
    `scene.rs`), dressed by mapping `PartKind` to `ActorPart` (`style.rs:280-360`),
    posed by `Pose::place`.
  - Everything else: `rig::RigTemplate`/`Rig` over `ActorPart`, posed by
    `Rig::pose` with default pivots in `rig.rs:105-126`.
  - Character Studio (`viewer.rs`) has to drive both.
- **The player's posing is written three times.** `animate_character` covers the local player,
  `peers::update` covers remote players, and `viewer.rs:900-1020` covers Studio. Each one builds its own `Pose`
  from slightly different inputs.
- Weapons, nametags, effects and sound each take their colours and cues from their
  own constants (`SceneArt`, `nametags.rs`, `impact::response`, per-event cues).

## Problems that make maintenance hard

Duplicated values, kept in sync by hand:

1. **Player speed** is set in 5 places: `avatar.rs:28-29` (3.8 / 5.6),
   `CharacterMotion` defaults in assetc `style.rs:74-75`, `gameplay.rs:9`,
   scenario `speed` (ignored for the avatar), and `player_only` (`19/5`).
2. **Player radius**: the scenario and `player_only` say 0.18, but movement and hits use
   `BODY_RADIUS` 0.32. Client prediction in `follow_avatar` (`lockstep.rs:880-930`)
   hard-codes 0.32 and the acceleration values again.
3. **Attack timing** is defined twice: in ticks in the sim (`avatar.rs:48-56`) and in f32
   seconds in the view (`app/src/combat.rs:13-20`).
4. **The sword combo cycle** is implemented in the sim (`start_swing`) and again in
   `Controls::strike` (`app/src/gameplay.rs:30`).
5. **Jetpack `HOVER` 1.8** appears in both `avatar.rs:61` and `jetpack.rs:7`.
6. **Rig pivots** (0.94, 0.58, 1.49, …) are repeated in `rig.rs`, `scene.rs`, `viewer.rs` and
   tests.
7. **Enemy collision radius** is set by armour class (`combat.rs:23-24`) and does not
   follow the skin's `scale`.
8. **`forest.ron` and `combat.ron`** are near-copies, and their ghoul and arrow skins
   have drifted apart.

Gameplay and view mixed together:

9. **Walk and run speed reach the sim from a visual style file**
   (`lockstep.rs:380-388`), and only when they differ from the defaults. That
   is a gameplay rule outside the content rules hash, so it is a desync risk.
10. **`app/src/combat.rs` calls itself "authoritative"** and runs its own f32 swept
    collision for rebound and hit-stop. Meanwhile the sim resolves hits independently.
11. **The dispatcher makes gameplay decisions from view mirrors**: busy state, pile
    refusals, and transfer limits followed by a separate `GoTo` command.

Identity and capabilities:

12. **A character's abilities are scene flags, not character traits.** Jump,
    SwiftSword, Digging and jetpack are chosen per scene. Studio guesses
    capabilities (`character_catalog.rs`) from mesh coverage and skin `Motion`.
13. **The player's scenario `skin: "worker"` is never rendered.** Worker haulers render
    the player's actor through `use_actor`.
14. **Cargo has three models:** hauler `cargo`/`Assignment`, avatar `ManualJob`, and
    `PileLab` haulers. Each has its own cancel path.
15. **There are two content systems.** `content` movers have sim/view sections and are
    rules-hashed. Settlement scenarios and styles are neither.

Bug found on the way:

16. **`MemoryRecord`, `MemoryDeploy` and `MemoryClear` name no agent**, so
    `CommandKind::avatar()` returns `None` and `apply_commands` skips the owner
    check (`world.rs:117-135, 384`). The memory code acts on the first controlled
    agent, so any peer can trigger these on the host's avatar.

## What is already in good shape

- The sim/view split holds for state. The sim owns position, heading, swing,
  jump phase, dig and health. The view only reads `WorldMirror`.
- `Rig`, with `Motion` chosen from asset metadata, is a reusable "posed body from
  a skin" for every non-player entity.
- `content` already has the target shape for definitions: one file per
  definition, a `sim` section that feeds the rules hash, and a `view` section that
  does not (`docs/design/saves-content.html`). Movers use it today.
- Input is cleanly layered down to `CommandKind`, and is synthetic-testable.

## Directions for a joint character definition

These are for discussion and have not been decided. The open decisions are in
the [character definition discussion](design/character-definition.html).

- **One `CharacterDef` per character in `content`.** Its `sim` section holds body radius,
  speeds, acceleration, health, weapons, attack timings and capabilities
  (jump, dig, jetpack). Its `view` section holds the skin or actor reference,
  scale, `character_motion`, colours and sounds. Scenarios and waves would
  refer to it by id rather than repeating stats, and scenes would stop
  carrying ability flags.
- **Split `AgentState`** into a shared body (position, heading, velocity, radius,
  health) plus role components or tables (avatar, hauler, threat). Threats would then become
  characters rather than a separate struct.
- **A single brain seam**: a shared intent or action vocabulary that player
  commands and AI both produce (move, go to, attack, interact), so a
  hauler or enemy could be possessed or swapped.
- **One rig path.** Express the exosuit as a skin or actor recipe posed by the same
  rig code. Have one function build a `Pose` from mirror state for local, peer and
  Studio use. The view should take timings from sim constants rather than copies.
- **Make style files visual only.** Move walk and run speed out of them and into the
  rules-hashed sim definition.

Status (25 October 2026):

- **Fixed:** 3, 4, 5 and 16.
  - The view's swing windups and ends, combo window, `HOVER`, prediction
    acceleration and radius now come from `sim::settlement::avatar` constants.
  - `SwordStroke::combo` is the one combo rule.
  - Memory commands check ownership.
- **Partly fixed:** 1. `gameplay::WALK_SPEED` is gone, and a test pins the
  style defaults to the sim speeds. The scenario `speed` is documented as unused for avatars.
- **Open:** 2. Haulers still steer around the avatar at its scenario radius.
  Unifying it changes simulation results, so it needs a golden-file update.
