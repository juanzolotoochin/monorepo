# Climbing

The climbing gym's design is [`CLIMBING.md`](../CLIMBING.md) (decisions
K1–K12). Its tasks CL.1–CL.6 run in order, one agent each, on branch
`kestrel-climbing`.

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
- **Notes from CL.1:** the gym's faces in tiles are in the scene file's
  comments and `CLIMBING.md`'s table; `sim/src/settlement/climbing_gym_tests.rs`
  builds the gym (`gym()`, the elder at the spawn) and reads top floors by
  scene tile, a starting point for climbing tests. The gym has `Digging`
  (the volume is drawn only by the digging service), so F digs there.
  Walking round the mountain on `walk` paths worked with legs of up to
  52 m.

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
- **Notes from CL.1:** the gym's ledges (13.5 m and 27 m on the west
  face), the 20 m shoulder and the pillars give fall heights over and
  under 5 m and 20 m to test on.

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
- **Notes from CL.1:** the gym's zoom (80 m) and turning view are in;
  the view stands four times further back there (`effects::stand_back`).
  The sun is fixed from the south-west (`scene.rs`): east and north faces
  are near black, so a climber there needs the silhouette or a fill
  light. The volume mesher colours stone by layer only up to 8 m
  (`volume_mesh::stone`, `surface`), so a 40 m face is one colour above
  that: banding by height would help read how high the elder is. In the
  chimney (2 m wide) the elder is hidden from every camera turn by the
  slot's walls. The cutaway's peek still notches the ground over the feet
  under the overhang. `sight::lit` (a 5 m roof reads as sky) is the
  view's only: the fog (`fog.rs`, `sight::open`) still counts ground under
  a high overhang as roofed, which matters only if the town gets one.

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
- **Notes from CL.1:** `climbing_gym_demo` is a walk-round with
  `camera_zoom` and `camera_rotate`; a script is capped at 120 s
  (`max_frames <= fps * 120`). The renders took 100 to 195 s locally.

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
- **Notes from CL.1:** height is cheap where the data is spans (the
  volume, its hash, nav floors, the light grid): a 40 m mountain adds a
  span per column. What scales with height: the view's wall meshes (a
  wall is banded per 0.5 m layer, so 80 slabs per tile side on a 40 m
  face) and `VolumeLayout::apply`, which fills cell by cell. What scales
  with map size: the volume hash (all columns, after every edit; 16 641
  columns in the gym), the fog's grid (map-sized per level seen), and the
  view's sight window, fixed at 64 tiles round the eye (`sight::WINDOW`,
  half the gym's width; beyond it inside views are void). The camera's
  near plane matters once ground is taller than the rig's 35 m distance.
  `LAYOUT_EXTENTS` caps a volume at 256 m across.

## Done

### CL.1 The gym and the height cap

- **Status:** done (branch `kestrel-climbing`, PR to come with CL.5)
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
- **Done:** `LAYOUT_LAYERS` is -32..=128 (16 m below to 64 m above);
  `MAX_LAYOUT_CELLS` stays 4 194 304 (the gym's boxes cover about 185 000
  cells). Nothing else in the sim or view sized itself by the cap: columns
  keep spans, the volume mesher bands walls per span, and the fog, light
  grid and nav keep only the floors they reach; only assetc's refusal
  test used a box to layer 100 as out of range (now 140). What a 40 m
  mountain needed: the orthographic camera stands only 35 m from the
  elder, so ground taller than that between them would meet its near
  plane (a scene with its own `zoom_out` now stands the camera four times
  further back, `effects::stand_back`; done ahead, not seen cut); and
  standing under the 7 m overhang put the
  view inside, darkening everything (`sight::lit`: a roof `HIGH_ROOF`, 5 m,
  or more over the floor counts as sky for the view; tunnels and caves are
  3 m). The scene: `app/assets/levels/climbing_gym.ron` (`climbing-gym`,
  the 16th scene: F4 or `--scene climbing-gym`; the digit keys reach only
  the first ten), documented in `CLIMBING.md`. The gym's view zooms out to
  80 m (`zoom_out`, a new scene field; 26 elsewhere) and turns with Z/C
  (`TurningView`, a new scene feature, also the D-pad). A `camera_zoom`
  script command. The layout faces the 40 m face west and the shoulder
  south, into the sun: east and north faces stand in near-black shadow.
  Demo `//kestrel/app:climbing_gym_demo` (and `_video`). Tests:
  `climbing_gym_tests` (sim), `the_climbing_gym_loads_and_validates`
  (assetc), `under_a_high_overhang_the_view_stays_outside` (app).

