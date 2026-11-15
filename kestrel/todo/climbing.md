# Climbing

The climbing gym's design is [`CLIMBING.md`](../CLIMBING.md) (decisions
K1–K12). Its tasks CL.1–CL.6 run in order, one agent each, on branch
`kestrel-climbing`.

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
- **Notes from CL.2:** climbing reads only the held tile and the body's
  columns (`Volume::spans`), so it is local; nothing in it scales with map
  size.
- **Notes from CL.3:** fall damage is local too (the avatar's own
  `fall_top`, applied in housekeeping on landing).
- **Notes from CL.4:** nothing in the view scales with map size: the pose,
  wheel and silhouette read only the local avatar; the silhouette is one
  extra blended draw per elder mesh.
- **Notes from CL.5:** the gym soak plays 10 minutes of climbing in about a
  second and two hours in about 15 s: with one agent nothing in climbing
  is costly, so far regions' climbers could run in the sim as they are.
  The demo's headless runs (`--simulation`) render nothing and took a few
  seconds for 2 minutes of play.

## A route while climbing

- **Status:** open
- **Why:** found in CL.5: a click-to-move route (`Intent::GoTo`) given to a
  climber on a face does nothing: only the stick moves a climber, so the
  elder hangs there, draining the wheel slowly, until it runs out (90 s)
  and lets go. In the review demo a `walk` op after a climb waited forever
  (the script hit `max_frames`). In play a click on the ground while on a
  wall seems ignored.
- **What to do:** decide what a route does on a face: climb down to the
  floor and then walk it, let go (and fall), or be refused with a notice
  ("Let go first (G)"); make the demo `walk` op fail fast on a face.
- **Where:** `sim/src/settlement/climb.rs` (`stick`, `step`),
  `app/src/control/mod.rs` (`Command::Walk`).
- **Found in:** CL.5.

## The gym's controls line

- **Status:** open
- **Why:** found in CL.5's video: the controls line at the screen's foot
  in the gym is the style's generic one ("SPACE JETPACK ... WASD STEER"),
  but there Space jumps at a face and lunges on it, G lets go and Z/C turn
  the view; nothing tells a player how to climb.
- **What to do:** a controls line for a climber (WASD climb, SPACE lunge,
  G let go, Z/C turn), as the town's ballista has its own
  (`TOWN_BALLISTA_HELP`).
- **Where:** `app/src/style.rs` (the help line), `app/src/scene.rs`.
- **Found in:** CL.5.

## Climbing while carrying

- **Status:** open
- **Why:** found in CL.2: an elder with a bundle or on a pile haul climbs
  as freely as an empty-handed one; a haul's walk sets a route the climb
  ignores, and a bundle's dock may be out of reach up a wall.
- **What to do:** decide whether carrying stops grabbing (or drains the
  wheel faster), and end or keep a haul when its walker grabs a face.
- **Where:** `sim/src/settlement/climb.rs`, `piles.rs`.
- **Found in:** CL.2.

## The climbing pose on styled rigs

- **Status:** open
- **Why:** found in CL.4: the climbing IK (`character::climb_limb_targets`)
  solves articulated arm and leg chains; a style whose legs are baked
  whole (`ActorFootContacts`, no shin or foot joint) gets only the
  fallback leg angles, and the targets are sized for the exosuit's 1.9 m.
- **What to do:** solve baked legs as `actor_leg_targets` does, and size
  the hand and foot targets by the rig's shoulder and hip heights.
- **Where:** `app/src/character.rs`, `app/src/climb_view.rs`.
- **Found in:** CL.4.

## Done

### CL.5 Soak, tuning and demo

- **Status:** done (branch `kestrel-climbing`, PR to come)
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
- **Notes from CL.2:** numbers in `elder.ron`'s `climb`. Untested: the
  chimney (two walls 2 m apart: inside-corner turns from its back wall),
  climbing in the town (any 1 m wall pushed into for 0.15 s is grabbed;
  in a 2 m tunnel there is no room to rise, so none), climbing with a
  bundle or a pile haul. A demo script needs `jump` and `board` (G) to
  lunge and let go.
- **Notes from CL.3:** the demo's "running out and falling" now hurts
  (about 15 m: 4 or 5 of 6) and a fall from 17.5 m or more kills, with a
  2 s respawn at the spawn by the pillars; the soak should expect deaths
  and respawns. Tuning: `safe_fall` and `fall_step` on the elder's
  `climb`. The town's falls hurt only into dug shafts 5 m or deeper.
- **Notes from CL.4:** `//kestrel/app:climbing_view_demo` already climbs
  the west face (round the corner, over the 13.5 m ledge, a lunge, a fall
  to the ledge) in 61 s: a start for the review video; it uses the new
  `jump` op. Stick timings: the grab at 0.15 s, 1 m/s up, the mantle at
  12.5 m on the 13.5 m pitch with about 17% of the wheel left. The pose and
  wheel are tuned on the built-in exosuit rig; a styled rig with baked legs
  (`ActorFootContacts`) is solved by the same IK but not looked at. Look at
  the chimney (the silhouette should carry it) and the overhang in the
  video. Step sounds play on sideways climbing (the gait clock counts the
  body's flat move on a face).
- **Done:** the soak: `climbing_soak_tests` (sim). `gym_soak` plays the
  gym from a seed: on the ground it walks a route to one of 18 spots 0.4 m
  off a face (the pillars, the west face's pitches, the north face, the
  shoulder, the inside corner, the chimney's three walls, the wing beside
  and under the overhang, the low step) and pushes into it or jumps at
  it, pushes or jumps a random way, stops, or presses G; on a face it
  mostly climbs up (2–10 s at a time), else sideways, diagonally, down,
  hangs, lunges (11%) or lets go (5%); falling, it reaches for a face now
  and then. Every tick it validates the settlement and checks: on a face
  the body is clear of rock, holds rock at its tile, hangs exactly
  radius + `GAP` off the face's plane, in the tile in front of it, and no
  climb lasts over 92 s; the wheel stays in [0, 1] and never refills on a
  face; a grab is never made exhausted, nor in the air after a let-go;
  off a face the body stands free (`soak_tests::standing_free`) or falls
  clear of rock; it stays on the map, its feet from -16 to 64 m; a death
  (a `Fell` that killed) comes back at home, whole, within 3 s. The CI
  test plays 10 minutes in about a second (the gym has one agent and no
  threats) and checks the first minute hashes alike twice; the `#[ignore]`d
  sibling plays four seeds for 30 minutes each (about 15 s), run once.
  Counts, 10 min: 22 grabs, 4 leaps, 4 pull-overs, 8 lunges, 2 G let-gos,
  18 empty wheels, 5 corner wraps, 2 inside turns, 22 falls that hurt,
  10 deaths, highest 15 m. Over the four half hours: 310 grabs, 56 leaps,
  102 pull-overs, 124 lunges, 47 G let-gos, 164 empty wheels, 70 wraps,
  25 turns, 212 falls that hurt, 82 deaths, up to the summit (40 m). Two
  bugs, fixed: the pull-over moved the body over the edge from a third of
  the way, before the feet were up, so for a few ticks it cut through the
  edge's corner (now the feet reach the top at two thirds and only then
  does it move over); and pushed diagonally into an inside corner the
  climber turned between the two faces every tick (each face's sideways
  part points into the other), its heading swapping 90 degrees a tick
  (now it turns only when the stick is more along the face than up or
  down it, so a diagonal climbs straight up the corner). The untested
  cases, in `climbing_tests`: `round_the_chimney_s_three_walls_and_out`
  (up the east wall, into the back wall, across onto the west wall, out
  of the mouth round onto the shoulder's face, and a let-go with the
  stick at the other wall grabs nothing until the floor: bridging is not
  special, the exhaustion and let-go rules already say no),
  `the_overhang_from_below` (nothing to grab under the roof; up the
  wing's face the climb stops at the roof, a lunge there stays under it,
  out from under it the elder climbs on to the wing's top, walks out over
  the roof and off its edge, 10 m for 3), `climbing_out_of_a_pit` (a pit
  3 m deep, as dug, climbed out of). Tuning: the elder's `climb` numbers
  are unchanged; nothing in the traces or the video looked off (13 s
  pitches, 2.5 s rests on the ledges, the run-out at about 11 m with
  lunges). The view: the HUD said "JETPACK / SAFE LANDING" for a climber's
  fall (it took any airborne body without a jump for a jetpack's
  descent); it now says "FALLING". The gym's black faces: a scene
  `ambient` field (assetc, 0–2000) in place of the style's, the gym's 800
  (the town's at noon; the style's is 180), so the faces in shade read
  dark slate instead of black (see "The gym's black faces" below). The
  demo, two clips of at most 120 s, 1280 x 720:
  `//kestrel/app:climbing_face_demo` (and `_video`, 71 s: the 40 m west
  face in three pitches with a rest on each ledge, the silhouette from the
  far side on the second pitch, a lunge on the third, the summit and the
  view turning round it) and `//kestrel/app:climbing_tour_demo` (85 s: the
  5 m pillar round its corner and a step off it, 1 health; the chimney's
  three walls and out of its mouth; the inside corner; a wheel run out
  with lunges at 11 m and the fall, 3 health; up the wing's face under the
  overhang, out from under the roof and onto the wing). Their waits were
  set against headless runs of the scripts (`demo --simulation
  --demo-script ... --trace ...`): the traced feet show each mantle to the
  frame. Found: a click-to-move route on a face does nothing (see "A route
  while climbing"). The docs: `CLIMBING.md` done, with "What changed
  versus the design"; "Climb the gym" in `todo/playtest-review.md`.

### The gym's black faces

- **Status:** done in CL.5 (branch `kestrel-climbing`)
- **Why:** found in CL.4: with the sun fixed from the south-west, the
  summit's east face (and the north faces) draw near black. The silhouette
  only shows a climber hidden behind rock, so one climbing in plain sight
  on such a face is a dark body on a black wall, readable by its ivory
  blocks only.
- **What to do:** a modest fill or ambient in the gym (or a sky light from
  the opposite side), or height banding past 8 m (`volume_mesh::stone`);
  check against the video in CL.5.
- **Where:** `app/src/scene.rs` (lights), `app/src/volume_mesh.rs`.
- **Found in:** CL.4.
- **Done:** in CL.5: a scene `ambient` (the ambient light's brightness in
  place of the style's), the gym's 800, the town's at noon (the style's
  is 180). The east and north faces now read dark slate, a climber on
  them by the body as well as its ivory blocks. Height banding past 8 m
  is not done: in the video the climber's height reads from the ledges,
  the wheel and the zoom.

### CL.4 Climbing in the view

- **Status:** done (branch `kestrel-climbing`, PR to come with CL.5)
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
- **Notes from CL.2:** read `AvatarState::climbing`: `climb` (tile,
  `Face`, `ClimbPhase::Hold`/`Lunge`/`Mantle` with their ticks), `stamina()`
  for the wheel, `exhausted` for the red. While on a face `jump` is
  `Grounded` and `flight.airborne` false, so today the elder is drawn
  standing on air against the wall; heading faces the rock. Positions
  jump at corners (an outside wrap moves the body about 0.7 m round the
  corner in one tick, an inside turn none) and the mantle lifts first,
  then moves over: the view should smooth them.
- **Notes from CL.3:** the gym's elder now has 6 health (scene
  `player_health`); check how the HUD shows it there and that it reads
  well beside the wheel. A hurting fall arrives as `Event::Fell` (height,
  damage, killed) and shows as a ghoul hit from nowhere (legacy
  `ThreatSwing`, `from: None`): a landing pose or thud of its own would
  read better (see "A fall's own feedback"). `AvatarState::fall_top` is
  the fall's top while in the air, if the view wants a "this will hurt"
  cue.
- **Done:** `app/src/climb_view.rs`, read from the drawn avatar's
  `climbing` (so a joined client's predicted climb poses at once). The
  pose: `Pose::climb` (`ClimbPose`: `Hold`, `Lunge`, `Mantle`, `Falling`)
  takes over the whole body (`Pose::climbing`, upright, head up the face,
  the weapon put away), and `character::climb_limb_targets` solves both
  arms and legs onto the face by two-bone IK (`two_bone`, elbows out and
  down, knees out, feet flat), blended at 32/s from last frame. The
  hands hold 1.3–1.7 m and the feet 0.2–0.5 m over the feet, on the face's
  plane (`face` = (radius + `GAP`) over the actor scale, 0.34 m); a reach
  cycle (`ClimbClock`, 0.32 m a hand, 60% of it gripping) moves them in
  diagonal pairs and advances only with the drawn body's move on the face
  while `Hold` (hanging, lunging and pulling over leave it), so a held
  hand stays put on the rock as the body climbs past it, and the reaching
  hand leaves the rock only while moving. The lunge stretches both hands
  its way (from `ClimbPhase::Lunge`'s up/side) with the feet trailing;
  the pull-over puts the hands on the top past the edge, a knee up, then
  the other foot, the chest leaning over; falling off a face (`Airborne`
  with no climb, for a climber with no jump of its own) flails, harder
  when it will hurt. The drawn position on a face eases toward the sim's
  (`ClimbFollow`, 16/s, snapping over 2.5 m, finishing quickly once off)
  with the heading, so a corner's 0.7 m wrap and quarter turn and the
  mantle's lift-then-over read smoothly; the ground-step easing is off on
  a face. Remote players get the pose too (no position easing). The wheel
  (`Wheel`, `wheel`): a ring of 32 segments right of the body in screen
  space, green; a spent part (a lunge's fifth) shows pale for 0.35 s and
  eases away; under 30% it flashes red at 3 Hz; exhausted it is red until
  full; full and off a face it hides after 0.8 s; it sits right of the
  torso, clear of the health bar over the head (the gym's 6). In the air,
  a fall that will hurt (`fall_top` - `flight.support` ≥ `safe_fall`) shows
  a pulsing red "!" in it. The silhouette (`SilhouetteMaterial`,
  `shaders/silhouette.wesl`): each mesh under the local player gets a twin
  child (`silhouettes`) drawn blended after the opaque pass with the depth
  test reversed (`Less`, no depth write), discarded unless the depth
  prepass there is over 1.3 m nearer the camera than the body (so the
  elder's own limbs don't show it), flat cyan; volume scenes only. A
  hurting fall now has its own feedback ("A fall's own feedback", done
  here): `legacy::Event::Fell`, dust puffs round the feet (more the harder,
  up to 4), the land and stone sounds and a shake scaled by the damage;
  no blood. The dark faces: not changed. The faces in shade toward the
  camera read dark brown, but the summit's east face is near black; a climber
  hidden behind rock shows by the silhouette whatever the light, and one in
  plain sight on a black face keeps the exosuit's ivory blocks, so no fill
  light was added (see "The gym's black faces"). A `jump` demo op (Space, `CommandKind::Jump`) and
  `//kestrel/app:climbing_view_demo` (up the west face, round the corner,
  over the 13.5 m ledge, a lunge, the silhouette from the far side, a
  hurting fall). Tests: `climb_view` (the reach cycle advancing only while
  moving; hands and feet in diagonal pairs on the face; the lunge and the
  pull-over; the pose following the sim's climb state, including whether a
  fall will hurt; the wheel's states; the corner easing; the silhouette
  twins for the player only), `climbing_hands_and_feet_reach_the_face`
  (the exosuit rig's hands and feet reach their targets on the face, no
  joint or the chest in the rock), `a_fall_that_hurts_lands_with_a_thud_of_its_own`.

### A fall's own feedback

- **Status:** done in CL.4 (branch `kestrel-climbing`)
- **Why:** found in CL.3: a hurting fall reuses a ghoul's hit (its
  sound, blood and shake, `legacy` maps `Event::Fell` to `ThreatSwing`),
  so it sounds like a blow and the blood sprays from nowhere.
- **What to do:** a landing thud and dust scaled by the damage, and a
  heavier landing pose; maybe a cue in the air when the fall will hurt
  (`AvatarState::fall_top`).
- **Where:** `app/src/legacy.rs`, `app/src/settlement.rs`, `app/src/sound`.
- **Found in:** CL.3.
- **Done:** in CL.4: `legacy::Event::Fell`; dust, a thud and a shake by the damage, no blood; the falling pose flails, harder when it will hurt, and a red "!" in the wheel warns in the air.


### CL.3 Falls hurt

- **Status:** done (branch `kestrel-climbing`, PR to come with CL.5)
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
- **Notes from CL.2:** the elder now falls (`JumpPhase::Airborne` under
  `ClimbDef::gravity`, 20 m/s², landing 8 frames) off a face (`climb::let_go`)
  and off any ledge in a volume; the fall's top is the feet when it
  starts, the landing is `jump_step`'s `Airborne` to `Landing`. A dead
  climber drops to the floor under it at once (`hurt_player`). The full
  wheel lets go at about 15 m on the summit's north face, a fall to test.
- **Done:** a climber's fall hurts on landing (K7). Numbers on the
  elder's `climb` section: `safe_fall: 5.0, fall_step: 2.5` (`ClimbDef`,
  checked: 1–80 m and 0.1–80 m), so a character falls hurt exactly when it
  climbs; `climb::fall_damage`: under 5 m nothing, then 1 and 1 more per
  2.5 m (5 m 1, 7.5 m 2, 10 m 3, 15 m 5, 17.5 m 6, 20 m 7). The fall's top
  is `AvatarState::fall_top` (save format 48), the highest the feet have
  been while `jump` is `Airborne` (`climb::track_fall`, last in a
  climber's stage 2; a let-go sets it, since a let-go by command falls a
  tick before stage 2 sees it), kept on the landing tick and then cleared;
  housekeeping (`Settlement::fall_housekeeping`, after
  `avatar_housekeeping`) hurts through `hurt_player` on the tick it lands,
  so a kill ends what a killing blow does and the elder comes back at home
  2 s later, and pushes `Event::Fell { agent, height, damage, killed }`
  (only falls that hurt). Jetpack descents never set `Airborne`, so never
  hurt. Only where bodies have health (`max_health > 0`): the town (6)
  and the gym, which gets 6 from a new scene field `player_health`
  (`assetc` `Scene`, 1–100, refused beside a scenario; `SceneSetup::player_health`
  for the scenario-less settlement); the Tunnel Lab and other scenes
  without health stay unhurt. The view: `legacy` maps `Fell` to a
  `ThreatSwing` hit with no threat (`threat: 0`, `from: None`), so the
  ghoul-hit sound, blood and, for the local player, the shake play; no
  other view work. Scenes: the gym's drops (pillars, faces, ledges) now
  hurt; in the town only a dug shaft 5 m or deeper does (its hill's cliffs
  and terraces are about 2 m); the golden's scenes (forest, arena) have no
  volume, so behaviour lines stayed and only state-hash lines moved
  (the settlement's encoding gained `fall_top`), regenerated.
  The prediction replay runs `track_fall` with the rest of stage 2 and
  leaves damage to the confirmed world. Tests: `fall_tests` (sim, 11: the
  steps, drops of 4.5–15 m, off the 10 m pillar, a let-go at 4 m, a 1.5 m
  terrace, the 20 m shoulder killing and the respawn, the wheel running
  out at about 15 m, the top from a lunge and from rising, no health no
  hurt, determinism on 1 and 4 threads, the replay),
  `a_fall_in_the_air_hurts_alike_after_loading` (save),
  `a_fall_that_hurts_shows_as_a_hit_from_nowhere` (app), the climb
  section's and the gym's health in `content` and `assetc`.


### CL.2 Climbing and stamina in the sim

- **Status:** done (branch `kestrel-climbing`, PR to come with CL.5)
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
- **Done:** `sim/src/settlement/climb.rs`: `AvatarState::climbing`
  (`Climbing`: the `Climb` held, its tile, `Face` and `ClimbPhase` (Hold,
  Lunge, Mantle); the wheel as `spent`, 0 full to 1 empty; `exhausted`,
  `released` and the `pushing` count), stepped in `avatar::self_update`
  in place of walking while on a face, so `AvatarReplay` predicts it.
  Grab: the stick (`Intent::Walk`, never a route) within 60 degrees of
  straight into a face whose tile is solid in the feet's layer and the
  next (1 m or more; a one-layer step is walked up), the face within
  0.15 m of the body, room to rise, 9 ticks (0.15 s) in a row; on the
  ground or falling. On a face the stick is read against it: into the
  rock up (1 m/s), out down (1.5), along it sideways (1.5). Along past
  the face's end it wraps round the outside corner; into rock beside the
  body it turns onto it (inside corner); feet a meter under the held
  span's top it pulls over (30 frames) if that top is a floor with room
  to stand, else stops; rock over the body (an overhang) stops it going
  up; down to the floor it stands. Space (`Action::Jump`) at a face from
  the ground grabs 1.5 m up (a free lunge); on a face it lunges 2 m in 20
  frames in the stick's direction (up without one) for a fifth of the
  wheel. G (`Action::Board`) lets go; interacting waits; attacks and digs
  are refused on a face. The wheel drains 1/15 a second moving (15 m
  up), 1/90 hanging, refills in 2 s on any floor; empty lets go, and an
  exhausted elder grabs again only once it is full; after letting go,
  only once on a floor. The elder (no jump) now falls under the climb's
  20 m/s² off a face and off any ledge in a volume (it used to drop at
  once). Content: a `climb` section (`ClimbDef`, `elder.ron`), checked,
  and in the rules hash only when a character has one. The app's Space
  sends `Jump` for a climber. Save format 47. The behaviour digest adds
  avatars' climbing only once any is off the default, so behaviour lines
  are unchanged; the sim golden moved in its state-hash lines only
  (the settlement's encoding gained the field), regenerated. Tests:
  `climbing_tests` (sim, 19), `a_climb_section_parses_and_only_the_elder_has_one`
  (content), `a_climb_in_progress_continues` (save).

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

