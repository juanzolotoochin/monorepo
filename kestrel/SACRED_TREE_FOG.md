# Kestrel Sacred Tree fog of war (design)

Status: agreed (2026-11-09), a prototype. Tasks in
[`todo/sacred-tree.md`](todo/sacred-tree.md) under M9. It stacks on the
depths milestone ([`SACRED_TREE_DEPTHS.md`](SACRED_TREE_DEPTHS.md), branch
`kestrel-sacred-tree-depths`, not merged) and the rules for the work in
[`SACRED_TREE_HANDOFF.md`](SACRED_TREE_HANDOFF.md) apply here too. Design
page: https://claude.ai/artifact/JsBhNPLfnPrzZpBLb2UDmT

## Goal

The player sees only what their side's units see. The rest of the map is
dark, or remembered as it was. Enemies may use the fog or not: the aim is
what feels fun, not a full simulation.

## Settled

- The player sees only what their units can see.
- Built on the inside view's line of sight (`app/src/sight.rs`) and M8's
  light grid (`sim/src/settlement/light.rs`).
- A prototype stacked on the depths branch, reviewed by screenshots and a
  demo video until the user can play it.

## Today

- The whole 256 m town is visible, day and night. Inside, a line of sight
  from the elder's head dims and hides what's out of sight (M8.5,
  `SightView::shows`).
- That line of sight is view only, from one eye, on one level, in a
  64-tile window, recomputed when the eye changes tile.
- Ghouls already hunt by sight: they turn on an elder or sprite they see
  within 3 m (`chase: (detect: 3.0, ...)`) and walk to the tree knowing
  where it is.
- Up to 8 players in co-op, each with an elder; G possesses a sprite.

## Decisions

### F1. Whose eyes reveal the map

- The elder (or the sprite a player possesses) sees far by line of sight,
  about 14 m.
- Every sprite sees a short radius round itself, about 6 m.
- The Sacred Tree (about 8 m), buildings (about 4 m) and lit lanterns
  (what they light) reveal round them.

### F2. The ground is remembered, not who's on it

- Ground you've seen stays drawn, dimmed and desaturated, with its
  buildings and piles as you last saw them.
- Ghouls and other moving things show only while in sight.
- Never-seen ground is black.

### F3. Fog everywhere

- On the surface and underground, day and night.
- Underground, each level is seen on its own.

### F4. Night shortens sight; light reveals

- Night roughly halves every unit's sight (elder about 8 m, a sprite about
  3 m).
- Light doesn't change: lanterns and the tree reveal what they light at
  any hour.
- Underground it is always night.

### F5. The ground blocks sight

- Ground over eye height blocks (the hill, terraces, the mountain); on the
  hill you see over the town.
- Buildings and pines don't block.

### F6. You hear ghouls you can't see

- A ghoul within about 12 m of an elder shows only its glowing eyes
  through the fog (and through rock underground), with its growl: you know
  something is there, not what.

### F7. Ghouls stay as they are

- They turn on an elder or sprite they see within 3 m and walk to the tree
  knowing where it is. No sim change for the prototype; revisit after
  playing it. Candidates then: light gives you away (ghouls spot whoever
  stands in light from further), or breaking line of sight to escape.

### F8. The team shares the fog

- The team sees what any player's elder, any sprite and the lights see;
  each screen draws the same fog.

### F9. The fog lives in the view

- A pure function of the sim's state (positions, volume, light grid), like
  the inside view.
- The memory of seen ground isn't saved: a loaded game starts with what's
  in sight now, plus the town round the tree.

### F10. One system with the inside view

- The fog replaces the inside view's dimming. Inside, the ground over you
  still fades, and what's out of sight is fog like anywhere else.
- F7 cycles three modes: fog, the inside view as it is now, and the old
  cutaway.

### F11. The town only

- Only the Sacred Tree town gets fog; F7 turns it off for comparison.
  Other scenes, the Tunnel Lab included, are unchanged.

## Consequences

- Many eyes: the elder(s), a dozen or so sprites, the tree, buildings and
  lanterns at once. Each eye's sight is recomputed only when it changes
  tile; the fog grid covers the whole 256 m map, per level.
- Hiding things is mostly built: `SightView::shows` already hides sprites,
  ghouls, piles, buildings and their labels out of sight inside; the fog
  widens it to the surface and many eyes.
- No save format, net protocol, input log or golden change: nothing in the
  sim changes.
- Demos need a script that walks the elder out, so the fog shows, with a
  night and the glowing eyes.

## Tests

- The fog grid: an eye's sight stops at ground over eye height and at its
  range; night halves it; a lantern reveals what it lights; levels are
  separate; seen ground stays remembered; the team's eyes add up.
- Cost: the town's fog with every eye stays within a frame budget, and an
  eye that doesn't change tile costs nothing.
- Hiding: a ghoul out of sight isn't drawn; one within 12 m of an elder
  shows its eyes only; buildings and piles in remembered ground show as
  last seen.
- Modes: F7 cycles fog, inside view and cutaway; other scenes are
  unchanged.
