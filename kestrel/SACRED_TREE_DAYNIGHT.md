# Kestrel Sacred Tree day and night (design)

Status: M7 done (2026-11-04). Tasks in
[`todo/sacred-tree.md`](todo/sacred-tree.md) under M7. It builds on mining
in [`SACRED_TREE_MINING.md`](SACRED_TREE_MINING.md) (PR #133), and the
rules for the work in [`SACRED_TREE_HANDOFF.md`](SACRED_TREE_HANDOFF.md)
apply here too.

## Goal

Days are for digging and building; ghouls come at night. The sun crosses
the sky, so shadows swing and stretch, and the light's color tells the
hour.

## Settled

- Ghouls attack at night.
- The sun moves, so shadows change direction and length through the day.
- Day and night lengths are set in content, and they can differ.
- The sunlight and the scene's color hint at the hour: warm at dawn and
  dusk, cool and dark at night.

## Decisions

### N1. The sim owns the clock, as a function of the tick

- The time of day is `(tick + start) mod (day + night)`, from the
  scenario's cycle. No new state: nothing to hash or save for the clock,
  and every peer agrees.
- The sim exposes it as pure functions (the phase, the fraction through
  it, the night's number, the time to the next dusk). The view reads the
  same functions.
- Skipping or pausing time is out of scope; it would need state then.

### N2. One wave per night

- The supply lists nights. Each night says how many ghouls each lane
  sends. A lane is today's wave entry without its timing: id, position,
  target, character.
- A night's ghouls spawn spread over its first third, so the columns
  still form (a lane's ghouls a few seconds apart).
- This replaces `first`/`every`/`count` for a scenario with a day cycle.
  Scenarios without one keep today's waves unchanged.

### N3. Ghouls burn away at dawn

- At sunrise every ghoul still alive catches fire (the fire effect) and
  falls within a few seconds. It counts as dispelled, not killed (no
  kill reward, its own event).
- Stalling ghouls (moats, stranded ghouls) until dawn now wins a night
  outright. The open moat question in the todo gets sharper.

### N4. Day 150 s, night 75 s, starting at dawn

- A 3:45 cycle. The first day is the calm start: the first night falls at
  2:30, near today's 2:45. Four nights fit in a 15-minute playtest.
- Both lengths live in the supply.

### N5. One light: the sun by day, the moon by night

- One `DirectionalLight` follows an east-to-west arc tilted south. Its
  elevation peaks near noon and is clamped low (about 10°) at the horizon,
  so shadows stay a sane length.
- At dusk it dims and turns blue, then rises in the east again as the
  moon, on the same arc. Shadows stay on throughout.
- **The moon is dim.** Night is dark enough that the lights of N7 matter.
- The shadow cascades may need tuning for the 256 m map and long low-sun
  shadows.

### N6. Keyframes in the scene file

- A `day_cycle` section in the scene file (`sacred_tree.ron`) gives four
  keys, dawn, noon, dusk and midnight. Each sets the sunlight color and
  illuminance, the ambient color and brightness, the sky (clear) color and
  the fog. Values between keys are blended.
- Scenes without one keep their fixed light, as today.

### N7. Lights at night: the tree, ghouls' eyes and lanterns

- The tree glows: a warm point light that rises at dusk and fades at dawn.
- Ghouls' eyes glow (two small emissive dots), so they read in the dark.
- Lanterns: a few fixed lanterns placed in the scene file, lit at dusk:
  at the top of each ramp and round the tree's plaza. Not on every
  building.

### N8. The HUD shows the time

- A small sun-and-moon dial with the night's number.
- A "Night falls in 0:20" warning near dusk.

### N9. Sprites at night: as today

- They keep their jobs and flee to the tree from nearby danger.

### N10. The last night repeats

- After the last listed night, it repeats forever. The HUD counts nights
  survived.

## Consequences

- The Sacred Tree's waves and their tests (waves, lanes, soak, happy path)
  are retuned for nights. Other scenes keep their waves, and their golden
  behaviour lines don't change.
- The save format goes up for the new wave schema. The input log, net
  protocol and command kinds stay unchanged.
- The demo video shows a full cycle: a day of building, dusk, a night
  attack and dawn.

## Tests

- The clock: phases, fractions and night numbers at the boundaries, for
  unequal lengths and a start offset.
- No ghoul spawns by day; each night sends the listed counts per lane,
  within its first third; the last night repeats.
- At dawn every ghoul burns away and counts as dispelled; the balances
  hold.
- Left alone, the town falls on a known night; one ballista per ramp
  holds four nights.
- The soak test runs through several cycles with `validate` every tick.
- Scenes without a day cycle: waves and golden behaviour lines unchanged.
