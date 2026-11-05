# Kestrel verticality (design)

Status: MVP done (2026-11-05), V.1–V.8 on the `kestrel-verticality`
branch. Tasks, and the work left (the Sacred Tree on a volume, ghouls and
haulers with a height, collapse, pile reach in 3D, lighting), in
[`todo/verticality.md`](todo/verticality.md).

What changed versus the design below:

- Digging (V5): the direction is a modifier, not where you look (the rig
  is fixed): F level, Ctrl+F down, Alt+F up. A down dig may open into a
  cave below (whoever walks in falls; no stranding check).
- Paths (V8): drops are not nav edges (a path never steps off a ledge
  more than a layer); the walker slides along corners its path's samples
  skipped.
- The cutaway (V7): besides the hole over the head, a narrower cut on the
  camera's side goes down to half a meter over the feet, so a tunnel's
  near wall or a dug stair's side doesn't hide the body; the hole is wide
  only under a ceiling. The section is a light earth colour, and a fill
  light over the head lifts shadowed floors under a ceiling.
- Piles and the walker's body have a height; haulers, threats and chasers
  still stand on the top floor (none in the Tunnel Lab).

The rules for the work in
[`SACRED_TREE_HANDOFF.md`](SACRED_TREE_HANDOFF.md) apply here too (one
subagent per task, tests per task, a `docs/test-time.md` row per
milestone).

## Goal

Tunnels, caves and mines under each other: two or more walkable levels
over the same x–y spot. The MVP is a separate map to iterate on.

## Settled

- Two or more levels in the same x–y spot.
- A new, separate map. Other scenes are unchanged.
- It must later work with the Sacred Tree's sprites and ghouls, but they
  are not needed in the MVP.

## Today

- Ground is a heightfield: one height per x–y (`Terrain::ground(p)`),
  from surfaces, mine terraces and `pits`. Sloped scenes use M6's corner
  rule.
- Bodies have 2D positions (`FxVec2`); their height is read from the
  ground. Nav is A* on a 2D grid. `app/src/digging.rs` meshes the
  heightfield in 16 × 16 chunks. About 105 call sites read `ground` or
  `height`.

## Decisions

### V1. A voxel volume, stored as spans

- Solid or empty per 1 m × 1 m × 0.5 m cell. Each column stores its runs
  of solid cells (spans); a tunnel splits a span in two. Ore is per cell.
- Floors and ceilings are derived: a floor is the top of a span with
  headroom above it.

### V2. Today's cells; 2 m headroom

- 1 m tiles and 0.5 m layers (today's tile and `LAYER`).
- A body needs 4 empty layers (2 m) above its floor to stand there.

### V3. Bodies store their feet's height

- Bodies keep their x–y position and also store the height of their feet.
- `floor(p, z)`: the highest floor at or below `z` plus one step. That is
  the ground a body stands on.
- `ground(p)` stays as "the top floor" for code that doesn't care, so the
  call sites move over gradually.

### V4. Sloped floors underground

- Each floor uses M6's corner rule, taking its neighbours from the floor
  within one layer in the next column, so tunnel floors ramp like pits do.

### V5. Digging in front: level, down or up

- Chosen by where you look or by a modifier:
  - level: clears the 4 cells in front at your height (a tunnel);
  - down: the same, one layer lower (a ramp down);
  - up: one layer higher (a ramp up).
- Repeating "down" makes a stair. You never dig under yourself.

### V6. No collapse

- Ground holds wherever it is. A support rule is deferred.

### V7. A cutaway camera

- Ground above the player's head is not drawn within a radius round them
  (a soft-edged hole). The cut faces are drawn dark so the section reads.

### V8. 3D paths in the sim

- Nav nodes per floor (x, y, floor height), with edges where walkable
  with headroom. Tested headless.
- One test walker in the map follows the player through tunnels. It
  proves the model before sprites and ghouls use it.

### V9. Drawing the volume

- Floors sloped as today's ground meshes; walls and ceilings as blocky
  voxel faces. In 16 × 16 chunks, re-meshed near a dig.

### V10. The Tunnel Lab

- A 64 m map: a mountain about 8 m high with a ready tunnel through it, a
  cave under a natural bridge (two floors over the same spot), stone and
  ore inside, everything diggable.
- Authored as solid and empty boxes in the scene file.

### V11. Beside today's terrain

- An optional volume in `Terrain`, opted into per scene (like sloped
  ground). Other scenes and the golden don't change.
- The Sacred Tree moves over in a later milestone.

## Consequences

- Bodies gain a height, so the save format goes up. The input log and net
  protocol change only if a dig command needs its direction (level, down
  or up).
- Code that reads `ground(p)` keeps working on the top floor; only what
  runs in the Tunnel Lab must move to `floor(p, z)` in the MVP.
- The view needs a second terrain mesher and a cutaway material.

## Tests

- The volume: spans split and merge on dig; floors, ceilings and headroom;
  `floor(p, z)` on two floors over one spot; sloped floors per V4.
- The avatar walks through the tunnel, under the bridge and over it, and
  never stands without headroom.
- Digging level, down and up from each side; never under yourself; a
  stair down and back up.
- 3D paths: through the tunnel, from under the bridge to over it; the
  test walker follows the player.
- Other scenes: golden behaviour lines unchanged.
