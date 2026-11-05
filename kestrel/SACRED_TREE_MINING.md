# Kestrel Sacred Tree mining (design)

Status: M6 done (2026-11-04). Tasks in
[`todo/sacred-tree.md`](todo/sacred-tree.md) under M6. It builds on the town
MVP in [`SACRED_TREE_HANDOFF.md`](SACRED_TREE_HANDOFF.md) and the ballista
in [`SACRED_TREE_BALLISTA.md`](SACRED_TREE_BALLISTA.md), whose rules for the
work apply here too.

## Goal

Stone stops coming from a building. It lies in the ground, and the elder and
sprites dig it out. The map grows to Dig Lab's 256 × 256 m, and the tree
stands on a hill with two ramps up to it. Pits get sloped edges, so whoever
digs one can always walk back out.

## Settled

- **Digging:** the elder and sprites dig pits. Pit edges become ramps, so no
  pit traps a walker.
- **Stone Gatherer:** removed from the Sacred Tree. All stone comes from
  digging.
- **Miner sprites:** a miner digs the nearest stone it can reach from the
  tree. There's no marking UI.
- **Speed:** the elder digs one layer in about 2 s, as in Dig Lab. Sprites
  are 4 to 5 times slower.
- **Map:** 256 × 256 m of generated ground (Dig Lab's `mine.rs`) around a
  hill set in the level file, with walkable ramps.
- **Waves:** ghouls spawn on a ring about 40 m from the tree, not at the
  map's edge, so paths stay within `nav.rs`'s search window.

## Decisions

### M1. Sloped ground from corner heights

- The state stays one depth per tile (`Terrain::pits`).
- Each tile corner takes the height of the lowest of the four tiles around
  it. The ground inside a tile is interpolated (bilinear) from its four
  corners.
- So a dug tile has a flat floor, and each neighbour slopes down into it on
  the side it touches. This covers every marching-squares case without a
  table of pieces.
- Undug ground uses the same rule, so the generator's 0.25 m terraces become
  gentle slopes.

### M2. Walkable by slope

- A walk fails on ground steeper than one layer (0.5 m) per tile, about 27°.
  This replaces the 0.25 m step rule in sloped scenes.
- So pits are obstacles unless walked into by a ramp, and the hill's sides
  (a meter or more per tile) are walls.

### M3. Dig in front, at most one layer below your feet

- A dig takes the tile in front of the digger (the one it faces), never the
  one it stands on.
- A tile can be dug to at most one layer below the tile the digger stands
  on. So every dug tile is one walkable step from a shallower one, and deep
  digging makes a stair by construction.
- Cutting a closed ring can still leave an island apart from the town. That
  is accepted: it's annoying, not a trap.
- No dig under a building, site, pile, the tree, or the hill (M7).

### M4. Six layers (3 m)

- The deepest pit is six layers in the Sacred Tree. The depth limit becomes
  a per-scene setting: Dig Lab keeps three (M8).

### M5. Generated stone veins

- Stone is a mineral in the layers, generated from the seed like Dig Lab's
  ore. It shows as a tint and is richer deeper down. Earth layers yield
  nothing.
- The generator guarantees a few veins exposed at the surface within about
  15–40 m of the tree, so the first stone needs no earthwork.

### M6. A pile of 3 to 6 beside the pit

- Digging out a stone layer drops 3–6 stone onto a ground pile beside the
  pit, or onto one already there, as Dig Lab does. It's carried from there
  as today.

### M7. The hill

- A plateau about 2 m high, wide enough for today's 26 m town, with cliff
  sides steeper than the slope limit.
- Two ramps on opposite sides, one per wave lane. A 2 m ramp at 0.5 m per
  tile is at least 4 tiles long.
- The hill and the plateau can't be dug.

### M8. A per-scene option; Dig Lab stays as it is

- Sloped ground, digging in front and the depth limit are scene settings.
  Dig Lab keeps its steps, pits, underfoot digging and three layers.
- Dig Lab's tests and golden behaviour lines don't change.

### M9. Miners dig exposed stone only

- A miner digs a tile whose top layer is stone, standing next to it within
  the M3 rule, then carries the drop to the tree's stone pile.
- The elder uncovers buried veins. With nothing to dig, a miner waits at the
  tree's stone pile.
- The plan has the sprites' shape (goal, plan, invalidate, replan).

## Consequences

- **The happy path changes.** The requirements' 10 steps build a Stone
  Gatherer. With it gone, that step becomes the elder digging stone and a
  miner sprite fetching it; the happy-path test follows the new steps.
- **Starting stone:** the first mechanism needs stone, so the supply may need
  a small starting stock; tuned in M6.7.
- **Formats:** the scene settings and stone change the save format. The
  input log, net protocol and command kinds should stay unchanged (a dig
  reuses the existing dig command).

## Tests

- Corner heights: a dug tile's floor is flat, its neighbours slope into it,
  and a tile is too steep where M2 says.
- No sequence of allowed digs (M3) leaves a walker without a walkable way to
  undug ground. A randomized property test over many digs.
- A ghoul's route goes around a pit and up a ramp; the hill's sides block
  walking.
- A stone dig drops 3–6 stone on a pile, and the balances hold.
- A miner fetches exposed stone, and waits at the pile when there's none.
- Dig Lab's golden behaviour lines and tests don't change.
- The soak test runs with digging, with `validate` every tick.
