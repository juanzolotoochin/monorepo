# Climbing

The climbing gym's design is [`CLIMBING.md`](../CLIMBING.md) (decisions
K1–K12). Its tasks CL.1–CL.6 run in order, one agent each, on branch
`kestrel-climbing`.

## CL.1 The gym and the height cap

- **Status:** open
- **Why:** K1, K10: a place to climb, taller than scenes may be today.
- **What to do:** raise `LAYOUT_LAYERS` to 64 m above zero (128 layers),
  keeping 16 m below, and check what assumes the old cap (the view's
  meshes, the fog's levels, the cutaway, `MAX_LAYOUT_CELLS`). A Climbing
  Gym scene on a volume, 128 m across: a 40 m mountain with 5, 10, 20 and
  40 m faces, ledges partway, an overhang, an inside corner and a chimney;
  pillars of 5, 10 and 15 m; the elder spawning on the flat. The gym
  zooms out further (enough for the 40 m face). A demo target like the
  Crawler Lab's.
- **Where:** `sim/src/volume.rs`, `app/assets/levels/`, scene list,
  `app/src/effects.rs` (zoom), `app/BUILD.bazel`, `app/demos/`.
- **Found in:** climbing design.
- **Tests:** the gym loads and validates; its faces have the authored
  heights; a 60 m box loads and a 70 m one is refused.

## CL.2 Climbing and stamina in the sim

- **Status:** open
- **Why:** K2–K6, K11.
- **What to do:** a climbing state on the avatar (face tile and side,
  feet height) and stamina, stepped in stage 1 with movement and predicted
  like walking. Pushing into a face two or more layers tall grabs on; the
  stick climbs (about 1 m/s up); outside corners wrap; inside corners
  turn; at the top it pulls over; an overhang stops it. Jump at a wall
  grabs higher; jump on the wall lunges about 2 m for a fifth of the
  wheel; G or B lets go. Stamina climbs about 15 m from full, drains
  slowly hanging, refills on the ground or a ledge, and empty lets go.
  Numbers in the elder's content. No new command if possible. Save format
  up.
- **Where:** `sim/src/settlement/avatar.rs`, `content/src/character.rs`,
  `content/data/character/elder*.ron`, `sim/src/volume.rs`.
- **Found in:** climbing design.
- **Tests:** the cases in `CLIMBING.md`'s Tests (grab, move, corners,
  top, overhang, stamina, lunge, let go); determinism, saves and
  prediction replay while climbing.

## CL.3 Falls hurt

- **Status:** open
- **Why:** K7.
- **What to do:** a fall's height (from the highest point of the fall)
  hurts on landing: nothing under 5 m, then 1 health per 2.5 m, so about
  20 m kills; the dead elder comes back as today. Numbers in content.
  Check every scene with drops; the golden's behaviour lines move only if
  a golden scene falls over 5 m.
- **Where:** `sim/src/settlement/avatar.rs`, health, content.
- **Found in:** climbing design.
- **Tests:** the damage steps; a 20 m fall kills; a climb let go at 4 m
  doesn't hurt; jumping down a terrace doesn't.

## CL.4 Climbing in the view

- **Status:** open
- **Why:** K3, K4, K9.
- **What to do:** a climbing pose (hands and feet on the face, a reach
  cycle), the pull-over and lunge; the stamina wheel by the elder
  (draining, flashing red near empty, hidden when full); the elder's
  silhouette drawn through rock in front of them; the gym's zoom.
- **Where:** `app/src/character.rs`, the HUD, the render passes.
- **Found in:** climbing design.
- **Tests:** the pose follows the sim's climbing state; the wheel's
  states; screenshots from the far side of the mountain.

## CL.5 Soak, tuning and demo

- **Status:** open
- **Why:** the exploration is reviewed by video.
- **What to do:** a soak in the gym (random climbing, `validate` every
  tick); tune speeds, stamina and falls; a demo video climbing a pillar,
  the 40 m face with two rests, a corner, an overhang, a lunge, running
  out and falling; the full suite; a `docs/test-time.md` row; docs and
  `CLIMBING.md`'s status; a "Climb the gym" section in
  `todo/playtest-review.md`.
- **Where:** sim tests, `app/demos/`, docs.
- **Found in:** climbing design.
- **Tests:** the soak; the full suite.

## CL.6 An open-world study

- **Status:** open
- **Why:** K12: huge maps reached by hash and streamed in and out don't
  fit one list of columns hashed whole, map-sized fog grids, or a sim that
  runs everything everywhere.
- **What to do:** a written study, read-only on the code: regions keyed
  by coordinates, a hash per region combined in a tree, streaming regions
  in and out (sim and view), fog in regions, what the sim runs far from
  players, saves and the net protocol; measured numbers from today's
  volume (edit and hash costs by map size); options with trade-offs. It
  becomes its own design page.
- **Where:** `sim/src/volume.rs`, `app/src/fog.rs`, the save and net
  code; a new design document.
- **Found in:** climbing design.

## Done
