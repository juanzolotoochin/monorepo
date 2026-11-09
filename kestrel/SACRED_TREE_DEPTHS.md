# Kestrel Sacred Tree depths (design)

Status: agreed (2026-11-06), M8.1 to M8.7 done. Tasks in
[`todo/sacred-tree.md`](todo/sacred-tree.md) under M8. It builds on the
verticality MVP in [`VERTICALITY.md`](VERTICALITY.md) (PR #135) and its
line-of-sight prototype, and the rules for the work in
[`SACRED_TREE_HANDOFF.md`](SACRED_TREE_HANDOFF.md) apply here too.

## Goal

The town's mine goes underground. The elder digs tunnels under the
Sacred Tree's map, sprites harvest them, ghouls follow you in, and light
is something you build: sprites only work where lanterns shine.

## Settled

- Verticality in the real game: mining under the town, with sprites and
  ghouls on floors.
- Lanterns underground have gameplay and tie into the line-of-sight view.
- Not now: fog of war, collapse, ghouls that dig, generated mountain
  ranges.
- Each task ends with screenshots; the milestone ends with a demo video
  of a day and a night.

## Decisions

### T1. Today's map as a volume, plus a mountain with a cave

- The generated ground and the hill are converted into a volume at load,
  so the surface looks the same.
- One authored mountain spur near the town (8–10 m high, 40–60 m from the
  tree) has a natural cave mouth, so there is somewhere to tunnel into
  from the first day.

### T2. 8 m deep

- 16 layers below the surface: room for two tunnel levels under each
  other. Today's pits stop at 6 layers.

### T3. The elder shapes, sprites harvest

- The elder digs in 3D (level, down, up), as in the Tunnel Lab.
- Miner sprites mine exposed stone faces they can reach: level digs into
  walls they stand next to, never down or up.

### T4. Stone everywhere, amber deep

- Stone veins as today, richer deeper.
- Amber, a new ore, only below 4 m and in the mountain. It is used for
  lanterns.

### T5. Ghouls chase you in

- All bodies (haulers, sprites, ghouls) get a feet height and 3D paths.
- Ghouls spawn on the surface ring as now and chase the elder and sprites
  into tunnels.

### T6. Nothing is dug under the town

- The hill, the tree, buildings, sites and piles can't be dug at any
  depth (M6's rule, applied to whole columns).

### T7. Line of sight, second pass

- Rock beyond a one-tile rim round what you see goes near black, instead
  of big flat lids.
- Inside, light comes from lanterns and the elder's lamp, not the sun.
- F7 keeps the cutaway for comparison.

### L1. A lantern is a small building

- The elder places a blueprint on any floor, also underground; sprites
  carry the materials and build it, as with other buildings.

### L2. 1 stone and 1 amber, once

- A lantern burns forever.

### L3. Sprites need light underground

- Underground, sprites only walk and work on lit tiles.
- The surface always counts as lit (sun or moon).

### L3a. A lantern errand may walk in the dark (M8.7, for review)

- The gap L3 leaves: the first lantern of a dark tunnel could never be
  built, since sprites can't walk to its site.
- A miner on a lantern errand (fetching for, supplying or installing a
  lantern's site) may walk dark floors, only for that errand. It never
  mines in the dark: a wall is a target only from a lit stand that lit
  floors join to the stone pile.
- A sprite left where it shouldn't be (in the dark, or in a lit pool cut
  off by a dark way, where an errand left it) walks back to the nearest
  lit floor on its level within 12 tiles, or home, through the dark.
- So a deep stretch is mined once lanterns light the way to it from the
  surface: one lantern every 10 m or so of tunnel.
- Only scenes with a lantern to build are ruled by light: the Tunnel Lab
  has none, and its sprites walk anywhere, as before.

### L4. Ghouls avoid light

- Lit tiles cost more in a ghoul's path when another way exists; ghouls
  are never blocked, and a chasing ghoul still comes in.
- M8.7: a lantern-lit node costs a ghoul 30 more in its plan (a half-tile
  step is 10), so a lit tile costs four tiles' walk: a dark way up to about
  three times as long wins. Only lanterns' light counts, not open sky.
- Dawn still burns every ghoul, underground and in the dark too (balance:
  a night ends for all); whether an unlit ghoul underground outlives
  dawn is for M8.8.

### L5. The elder's lamp is for seeing

- A small lamp (today's fill light), view only: you can always see to
  dig, but it doesn't count as lit for sprites.

### L6. A light grid in the sim

- Each lantern lights the tiles in its line of sight on its level within
  6 m. Deterministic, hashed with the world, recomputed only when lanterns
  or the ground change.
- The view draws from the same grid, so no light goes through walls.

## Consequences

- The Sacred Tree's golden lines, soaks and wave tests change: bodies gain
  heights and the ground is a volume. The save format goes up; the input
  log and net protocol only if placing a lantern needs a new command.
- 3D paths must be bounded first: dozens of bodies plan, and one
  unreachable goal took 1.2 s in the Tunnel Lab.
- Haulers carry materials down stairs to sites on any floor.
- M7's night lanterns either become this kind of lantern or stay
  decoration; settled while building.

## Tests

- Paths: bounded time to an unreachable goal; bodies on floors in the
  town; ghouls chase into a tunnel.
- The town as a volume: the surface matches today's heights; nothing is
  dug under the hill, tree, buildings, sites or piles; amber only below
  4 m and in the mountain.
- Miners dig walls they stand next to, never down or up.
- Light: the grid stops at rock and at 6 m, on its level only; sprites
  don't enter or work on dark underground tiles; ghouls prefer dark
  routes but still reach a lit target.
- A lantern blueprint underground gets its stone and amber and is built.
- Soak: several days and nights with digging and lanterns, `validate`
  every tick.
