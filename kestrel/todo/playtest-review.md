# Playtest review

Decisions taken while the user was travelling (2026-11-05 to 2026-11-08)
and reviewed only by video and screenshots. All were accepted "for now";
each needs a look in a real play session before it is settled. Work on a
decision only after that review: until then the current behaviour stands.

The work is on three stacked branches, none merged:

1. `kestrel-verticality`: tunnels MVP and the Tunnel Lab (PR #135, open).
2. `kestrel-verticality-sight`: seeing inside by line of sight (not pushed).
3. `kestrel-sacred-tree-depths`: M8, the town on a volume with lanterns
   (not pushed, last commit 823d4a4).

Designs: [`VERTICALITY.md`](../VERTICALITY.md),
[`SACRED_TREE_DEPTHS.md`](../SACRED_TREE_DEPTHS.md). Demo targets:
`//kestrel/app:tunnel_lab_demo_video`,
`//kestrel/app:sacred_tree_depths_demo_video`.

## Merge and push the stack

- **Status:** open
- **Why:** the three branches wait on the user; nothing past #135 is pushed.
- **What to do:** after the playtest, merge #135, then push the sight and
  depths branches and open their PRs (stacked, with `merge -s ours`
  rather than a force-push).
- **Where:** the branches above.
- **Found in:** M8.8 report.

## Play the tunnels and digging

- **Status:** open
- **Why:** the digging controls and paths were settled without playing.
- **What to do:** check each, in the Tunnel Lab (scene key in the HUD)
  and the town:
  - F digs level; in volume scenes it digs a step down when nothing is
    left level in front (M8.8). Ctrl+F digs down, Alt+F digs up. The
    direction is a modifier, not where you look, since the camera rig is
    fixed. Is a modifier comfortable, or should looking down dig down?
  - A down dig may open into a cave below; whoever walks in falls, with no
    check that they can climb out.
  - Paths never step off a ledge more than a layer (0.5 m) high: a body
    walks round rather than drops.
  - Walkers on stairs: the stair-corner hesitation is fixed (M8.8); watch
    for any walker sliding along a step.
- **Where:** `sim/src/volume.rs`, `sim/src/nav.rs`,
  `sim/src/settlement/haulers.rs` (`move_worker`).
- **Found in:** V.4, V.7, M8.8.

## Play the inside view

- **Status:** open
- **Why:** the line-of-sight view was the user's idea from a devlog ("you
  only see the inside when you're inside") but "not exactly how it feels";
  it is a prototype.
- **What to do:** decide whether it is the view to keep, against the V7
  cutaway (F7 switches). Things to judge:
  - Under a ceiling, the ground over you fades over the whole view and
    what's out of your sight is dimmed and hidden (sprites, ghouls,
    piles, buildings).
  - Rock beyond a one-tile rim of what you see goes near black.
  - The sight wedge steps a tile at a time as you move.
  - Brightness underground by day and night: ambient 0.07, the elder's
    lamp 1.6, a lantern 2.0 in warm orange.
  - Fog of war was left out on purpose.
- **Where:** `app/src/sight.rs`, `app/src/cutaway.rs`,
  `app/shaders/cutaway.wesl`.
- **Found in:** the sight prototype, M8.5.

## Play light as a resource

- **Status:** open
- **Why:** the light rules are new gameplay and were tuned only by tests
  and a soak.
- **What to do:** play a few days in the town and judge:
  - **L3a, the lantern errand.** A sprite carrying parts for a lantern
    may walk dark tunnels, but never mines there; afterwards a sprite left
    in the dark walks back to light. Without it the first lantern of a
    dark tunnel can't be built. Accepted for now; alternatives are the
    elder carrying the first lantern, or a lit "work light" around the
    elder.
  - **Sprites need light.** Underground, sprites only walk and work on lit
    floors; the surface always counts as lit. A tunnel needs a lantern
    about every 8–10 m, and a stair one every few steps (a lantern lights
    only floors within one layer of its own). Too fiddly?
  - **The elder's lamp** is for seeing only and doesn't count as light.
  - **Ghouls avoid light.** A lit tile costs a ghoul four tiles' walk, so
    a dark way up to about three times as long wins; ghouls are never
    blocked. Wave routes aren't replanned when a lantern goes up.
  - **Dawn burns every ghoul**, underground and in the dark too. Should an
    unlit ghoul underground outlive dawn?
  - **Light only in scenes with lanterns.** The Tunnel Lab has no lantern,
    so its sprites ignore light.
  - **M7's night lanterns** stay decoration and give no light to sprites.
- **Where:** `sim/src/settlement/light.rs`, `miners.rs`, `chase.rs`.
- **Found in:** M8.6, M8.7.

## Play the amber economy

- **Status:** open
- **Why:** amber income was set from one soak, not from play.
- **What to do:** check that it feels right:
  - A lantern costs 1 stone and 1 amber, once, and burns forever.
  - Amber only below 4 m and in the mountain spur; one amber per cell, in
    small frequent pockets: about one lantern a day of digging 5–6 m
    down. A 40 m tunnel at 6 m misses amber 29% of the time.
  - Some ore is lost once the pile cap is reached (the soak met 14 amber
    cells and got 8).
- **Where:** `sim/src/mine.rs` (`AMBER_YIELD`), `sim/src/settlement/piles.rs`.
- **Found in:** M8.8.

## Look at the town's ground

- **Status:** open
- **Why:** the town is now a volume of 0.5 m cells, not the old heightfield.
- **What to do:** check it looks right:
  - Terraces step 0.25 m but cells are 0.5 m, so a tile an odd number of
    steps up rounds down a quarter meter, and ramps fall half a meter
    every other tile. Left as is; the fix would be 0.25 m cells or
    terraces in half meters.
  - Nothing can be dug under the hill, the tree, buildings, sites or
    piles, at any depth.
  - The mountain spur with its cave hall west of the tree, and the 8 m
    (16 layers) of diggable ground.
- **Where:** `app/assets/levels/sacred_tree.ron`, `sim/src/terrain.rs`.
- **Found in:** M8.3.

## The demo camera underground

- **Status:** open
- **Why:** in the M8 demo the tunnel shots are dark and small at the town
  camera's distance; the lantern's light barely reads.
- **What to do:** a closer camera (or a zoom op in demo scripts) when the
  player is underground; reshoot the depths demo.
- **Where:** `app/src/` camera rig, `app/demos/sacred_tree_depths.json`.
- **Found in:** M8.8.

## Other follow-ups

Smaller open work from these milestones, not decisions to review:
"Depths follow-ups" in [`sacred-tree.md`](sacred-tree.md); "Line of
sight follow-ups", "Collapse and support", "Pile reach in 3D", "The
jetpack into a 2 m tunnel" and "Lighting underground" in
[`verticality.md`](verticality.md).
