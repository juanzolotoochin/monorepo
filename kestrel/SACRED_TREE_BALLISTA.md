# Kestrel Sacred Tree ballista (design)

Status: design agreed (2026-11-01), M5.1 to M5.5 done. Tasks in
[`todo/sacred-tree.md`](todo/sacred-tree.md) under M5. It builds on the town
MVP in [`SACRED_TREE_HANDOFF.md`](SACRED_TREE_HANDOFF.md) (D1–D6), whose
rules for the work apply here too.

## Goal

A defence building for the Sacred Tree. It fires like the forest's Woodland
Ballista, but only while it has an occupant. A staffed sprite shoots the
nearest enemy. A possessing elder aims where they point, and gets a fireball
that sets the ground alight; burning enemies take damage over time.

Enemies walk to the tree, but turn on an elder or sprite who comes close,
and give up to return to the tree when the target gets away. They also
break buildings that stand in their way.

It tests the requirements' prototype question "Is the elder's intervention
exciting, or does it feel like mandatory production optimization?" with an
intervention that isn't production.

## Settled

- **Targets:** enemies attack the elder and sprites. Sprites get health and
  can die.
- **Ammo:** stone, carried to the ballista by the player.
- **Elder fire:** left mouse fires at the cursor while possessing.
- **Waves:** a calm start, then waves on a timer.
- **Possession reward:** aiming and the fireball. A possessed fire-rate
  boost is deferred (todo).
- **Numbers:** from the Woodland Ballista (range 3.8 m, 0.7 s period,
  1 damage); ghouls (`ghoul_forest`) as the enemies. Every number lives in
  content.

## Decisions

### B1. A building that fires on its own, through shared helpers

- A ballista is a building kind with a `defence` section (range, period,
  damage, projectile speed, ammo cap). It is built from two parts like a
  gatherer. A built ballista fires only while its slot is occupied and its
  ammo store isn't empty; it produces nothing.
- It fires from the buildings table (`buildings.rs`), not through a
  `DefenderState` row: defenders stay fixed at scene load.
- **Shared helpers:** what both ballistas do moves out of `aim_defender`
  and `fire` (`combat.rs`) into helpers both call:
  - picking a target: the nearest threat in range that the projectiles
    already in flight won't kill, lowest id on a tie;
  - launching a bolt: spending one unit of ammo, the projectile, the shot
    count, the event and the sound.
- The Woodland Ballista must behave the same after the extraction: the
  golden file's behaviour lines stay unchanged.

### B2. An ammo store on the ballista

- `Only(stone)`, capped (about 10). Interact while carrying stone delivers
  as much as fits, like a site delivery; nothing is taken back out.
- Each bolt spends one stone (counted as spent in the balances). An empty
  ballista doesn't fire, even when staffed, and says so on its card.

### B3. Possession reuses the avatar's aim and attack

- While seated in a ballista, the avatar's aim point (`AimAvatar`) is the
  reticle, clamped to the ballista's range. `Attack` fires a bolt at the
  reticle, at most once per period, so holding fire repeats at the
  ballista's period.
- R throws the fireball at the reticle (the Burst key; a ballista has no
  Burst). Controller: right stick moves the reticle, RT fires, RB throws.
- No new aim commands. The fireball reuses `Burst` by building kind
  (decided in M5.3): in a ballista the Burst key throws it, with a
  cooldown of its own on the elder.

### B4. The fireball and burning ground

- The fireball flies to the reticle, deals impact damage, and leaves a fire
  patch: a new table of patches (position, radius about 1.5 m, lasting
  about 6 s).
- A threat inside a patch has its burn refreshed each tick (burns don't
  stack). A burning threat takes 1 damage per second, until about 3 s after
  it leaves the fire.
- Fire hurts only enemies.
- Its cooldown is on the elder, like Burst's (about 20 s), and costs no
  stone.

### B5. Enemies chase, then return

- About every 0.5 s, an enemy looks for the nearest elder or sprite within
  a detect radius (about 3 m) that it can see.
- It steers straight at its target and swings when in reach (the existing
  swing).
- If the target dies, gets past a leash radius (about 5 m) or out of
  sight, the enemy plans a fresh route to the tree from where it stands.
- **Buildings come second:** with no elder or sprite to chase, an enemy
  attacks a building that blocks it or is within reach (B8).
- The shape is the sprites' (goal, plan, invalidate, replan), so a real
  planner can replace it later.

### B6. Sprite health and death

- Sprites get health. A dead sprite fades and returns at the tree after
  about 15 s, keeping its job; meanwhile its slot is free.
- An occupant of a building can't be hit, whether sprite or seated elder:
  the building shelters it.

### B7. The tree's health

- The tree has health (about 20). An enemy that reaches it breaches it, as
  at the forest base.
- At 0 the run ends with a short "The tree has fallen" screen and a
  restart.

### B8. Buildings can be broken

- Buildings (and sites) get health (about 10). An enemy hits them with its
  usual swing.
- At 0 a building becomes a site again: its parts are lost. Its occupant is
  put out beside it (a sprite replans), and its output and ammo spill onto
  a ground pile, so conservation still holds.
- Losing every Wood Gatherer with less than 4 wood left is accepted: the
  tree will fall and the run restarts (B7).
- No repair for now (todo).

## Tests

- An empty ballista never fires; a staffed one with ammo fires at the
  nearest enemy; one with no ammo doesn't.
- A possessed ballista fires at the reticle, clamped to range, at its
  period.
- A burn deals the right number of hits, refreshes rather than stacks, and
  ends 3 s after leaving the fire.
- An enemy leaves its route to chase a sprite, gives up past the leash and
  reaches the tree.
- A dead sprite comes back at the tree with its job.
- A broken building becomes a site, its contents land on a ground pile and
  the balances hold.
- The tree at 0 ends the run.
- The soak runs with waves, with `validate` every tick.
- The Woodland Ballista's golden behaviour lines don't change.
