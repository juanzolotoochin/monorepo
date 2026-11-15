# Playtest review

Decisions taken while the user was travelling (2026-11-05 to 2026-11-09)
and reviewed only by video and screenshots. All were accepted "for now";
each needs a look in a real play session before it is settled. Work on a
decision only after that review: until then the current behaviour stands.

The work was on four stacked branches, all merged on master since
(#135–#138, see Done). The crawler prototype ([`CRAWLER.md`](../CRAWLER.md))
is on branch `kestrel-crawler`, its PR to come.

Designs: [`VERTICALITY.md`](../VERTICALITY.md),
[`SACRED_TREE_DEPTHS.md`](../SACRED_TREE_DEPTHS.md),
[`SACRED_TREE_FOG.md`](../SACRED_TREE_FOG.md). Demo targets:
`//kestrel/app:tunnel_lab_demo_video`,
`//kestrel/app:sacred_tree_depths_demo_video`,
`//kestrel/app:sacred_tree_fog_demo_video`.

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

## Play the fog of war

- **Status:** open
- **Why:** the fog ([`SACRED_TREE_FOG.md`](../SACRED_TREE_FOG.md)) is a
  prototype reviewed only by video (M9.4); the aim is what feels fun, not a
  full simulation.
- **What to do:** play a day and a night in the town and judge:
  - **Ranges.** By day the elder sees 14 m, a sprite 6 m, the tree 8 m, a
    built building 4 m, a lantern what it lights (6 m). By night and under
    a roof the elder's and sprites' halve (7 m, 3 m); the tree and
    buildings keep theirs, as lights. Too short, too long?
  - **The start.** The town round the tree starts remembered out to 30 m
    (the clearing and the ballista ring). Enough, or too much?
  - **Remembered ground at night.** Seen ground is dimmed and greyed, and
    by night keeps a floor of 0.12 so it reads apart from never-seen
    black. Readable without being bright?
  - **The eyes and the growl.** A ghoul out of sight within 12 m of an
    elder (through rock too) shows two glowing eyes, a fixed-size mark
    over everything, and growls once per pack. Does it warn enough, and is
    12 m right?
  - **Last-seen buildings and piles.** Out of sight, sites and piles show
    as last seen (frozen), at full colour on dimmed ground.
  - **F7's modes.** Fog, the inside view, the cutaway: which to keep, and
    whether the fog replaces the inside view underground.
  - **Ghouls ignore the fog for now** (F7 of the design): they turn on
    whoever they see within 3 m and know where the tree is. The two
    candidates after playing: light gives you away (ghouls spot whoever
    stands in light from further), or breaking line of sight lets you
    escape.
- **Where:** `app/src/fog.rs`, `app/src/heard.rs`, `app/src/cutaway.rs`,
  `app/shaders/cutaway.wesl`, `app/src/settlement.rs` (`remember`),
  `sim/src/settlement/chase.rs` (for the ghouls).
- **Found in:** M9.1–M9.4.

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

## Play the crawler

- **Status:** open
- **Why:** the crawler ([`CRAWLER.md`](../CRAWLER.md)) is a prototype
  tuned by tests, a soak and one video (CR.6); nobody has driven it.
- **What to do:** in the Crawler Lab (F4 steps to it), board with G and
  judge:
  - **Speed and weight.** It walks at 1.6 m/s on a wave of one leg at a
    time and trots at 3.2 m/s on boost; it takes about 2 s to get going
    (the last leg of the wave steps 1.8 s in) and turns at 20°/s. Too
    slow, too heavy, or right for something four elders big?
  - **Steering.** The stick walks within 60° of the heading, turns in
    place up to 120° and backs up past that. Is turning in place as a
    separate gait clear, or does it feel unresponsive?
  - **Pits and cliffs.** It stops at the 3 m pit and the 2 m cliff with no
    message; turned in place at the pit's edge it backs off slowly as it
    turns (CR.6). Does a stop read as "too wide", or as stuck?
  - **The legs.** Knees high and a little outward, feet upright, swing
    arcs of 0.9 m walking and 0.6 m trotting. Do they read at the play
    camera, and does the 1.4 pull-back while aboard help or lose the
    elder?
  - **Riders.** Three riders on the cargo deck, the pilot at the helm,
    all standing in the idle pose; G boards the cab first, then the free
    place nearest. Climbing down lands beside the box, never into a
    trench.
  - **Stomps and shoves.** A foot coming down kills a ghoul under it
    (4 damage) and shoves an elder or sprite 0.1 m clear; feet never land
    on someone already standing there. Walking into a foot with the stick
    passes through it. Is a stomp satisfying, too easy, or too rare?
- **Where:** `sim/src/settlement/crawler.rs`,
  `content/data/character/crawler.ron`, `app/src/crawler.rs`,
  `app/actors/crawler.ron`, `app/src/vehicles.rs`.
- **Found in:** CR.1–CR.6.

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
"Depths follow-ups" and "Fog of war follow-ups" in
[`sacred-tree.md`](sacred-tree.md); "Line of
sight follow-ups", "Collapse and support", "Pile reach in 3D", "The
jetpack into a 2 m tunnel" and "Lighting underground" in
[`verticality.md`](verticality.md).

## Done

## Merge and push the stack

- **Status:** done (#135, #136, #137 and #138, merged on master)
- **Why:** the four branches wait on the user; nothing past #135 is pushed.
- **What to do:** after the playtest, merge #135, then push the sight,
  depths and fog branches and open their PRs (stacked, with `merge -s ours`
  rather than a force-push).
- **Where:** the branches above.
- **Found in:** M8.8 report.
