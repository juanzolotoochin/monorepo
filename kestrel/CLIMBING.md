# Kestrel climbing gym (design)

Status: done (2026-11-15), an exploration reviewed by video (CL.5); the
open-world study (CL.6) is still to write. Agreed 2026-11-11. Tasks
CL.1–CL.6 in [`todo/climbing.md`](todo/climbing.md); what changed from
the decisions below is under [What changed versus the
design](#what-changed-versus-the-design), and what to judge in play is
"Climb the gym" in [`todo/playtest-review.md`](todo/playtest-review.md). It builds on the volume
([`VERTICALITY.md`](VERTICALITY.md)); the rules for the work in
[`SACRED_TREE_HANDOFF.md`](SACRED_TREE_HANDOFF.md) apply here too (one
subagent per task, tests per task, a `docs/test-time.md` row at the end).
Design page: https://claude.ai/artifact/5FMnnt9hvPPC4eoUbTEeRL

## Goal

How vertical can the game be? A gym with a mountain about 40 m tall,
where the elder climbs any wall with a stamina wheel, as in Breath of the
Wild. An exploration, not content. Keep an open world in mind: huge maps,
regions reached by hash, streaming in and out.

## Settled

- A gym scene to explore in.
- Elders climb, for now.
- A stamina wheel.
- The camera rotates by hand (Z/C).

## Today

- The volume: 1 × 1 × 0.5 m cells, each column stored as its runs of
  solid cells (`volume.rs`), so air costs nothing. Scenes reach 16 m below
  zero to 32 m above (`LAYOUT_LAYERS`; 64 m since CL.1).
- Walking off a ledge falls like a jump's way down; falls don't hurt
  (they do since CL.3, K7). Walls block; paths never step down more than
  a layer.
- The camera is orthographic at a fixed pitch, zoom 3–26, and rotates with
  Z/C. Underground, the ground over the head fades.
- What doesn't scale to an open world: the volume is one sorted list of
  columns hashed whole after each edit; the fog keeps a map-sized grid per
  level seen; the sim runs everything everywhere. Saves (edits only),
  ground meshes (16 × 16 chunks) and paths (searched area) already scale.

## Decisions

### K1. A Climbing Gym

- A new scene, 128 m across, flat ground round a 40 m mountain in the
  middle with faces of 5, 10, 20 and the full 40 m, ledges to rest on
  partway, an overhang, an inside corner and a chimney.
- A few freestanding pillars (5, 10 and 15 m) to learn on.

### K2. Every wall is climbable

- Every face of solid ground at least 1 m (two layers) tall, anywhere.
- An overhang (rock over the climber's head reaching out) can't be
  climbed past.

### K3. Walk into a wall to grab on

- Walking into a wall more than a step high and pushing grabs on.
- On the wall the stick moves up, down and sideways (about 1 m/s up).
  Outside corners wrap; at the top the elder pulls over automatically.
- Jump (Space) on the ground at a wall grabs higher. G or B lets go.
- Changed in CL.2: G lets go (it is Board, which has nothing to board on
  a wall; on a gamepad the left stick's click). The pad's B stays Drop, so
  no new command. On a face the stick is read against the face: into the
  rock climbs up, out of it climbs down, along it moves sideways. Only the
  stick (`Walk`) grabs, after 0.15 s of pushing within 60 degrees of
  straight in; a click-to-move route never does.

### K4. A wheel climbs about 15 m

- Climbing drains stamina, hanging still drains it slowly, and it refills
  quickly on the ground or a ledge. The 40 m face needs two rests on
  ledges. The numbers live in the elder's content.

### K5. A climbing jump

- Jump on the wall lunges about 2 m in the stick's direction for a fifth
  of the wheel.

### K6. Out of stamina, you fall

- The elder lets go. The wheel flashes red near empty and refills only
  once they stand on ground.
- Added in CL.2: once empty the elder can't grab again until the wheel is
  full, and after letting go not until they stand on a floor. A climber
  with no jump of its own (the elder) now falls under the climb's gravity
  (20 m/s²) off a face and off any ledge in a volume, where it used to
  drop to the floor at once.

### K7. Falls hurt

- No damage under 5 m, then 1 health per 2.5 m (the elder has 6), so about
  20 m kills. A dead elder comes back as today. Numbers in content.
- Settled in CL.3: the numbers are on the elder's `climb` section
  (`safe_fall: 5.0, fall_step: 2.5`, beside the fall's gravity), so a
  character falls hurt exactly when it climbs. A fall is measured from its
  highest point (a let-go's height, a lunge's top, a jump's apex) to the
  floor it lands on: 5 m takes 1, each 2.5 m more another (7.5 m 2, 10 m 3,
  15 m 5), so 17.5 m takes the elder's 6 and 20 m kills with one to spare.
  Any fall counts: letting go, an empty wheel, walking off a ledge.
  Jetpacks don't fall that way, so they never hurt. It is applied in
  housekeeping on the landing tick through the same path as a killing
  blow, so a dead elder comes back at home 2 s later. Only where players
  have health: the town (6) and the gym, which now gives its elder 6 (a
  scene's `player_health`, for a scene without a scenario); in the Tunnel
  Lab, with none, falls still don't hurt. The view shows a hurting fall
  with its own thud since CL.4: dust round the feet, the landing sound and
  a shake by the damage, and in the air a red "!" in the wheel.

### K8. No glider yet

- Climb down, or jump and take the fall. A glider (a leaf or a cloak) is a
  later step.

### K9. Rotate by hand, a silhouette through rock

- Z/C as today, plus a silhouette of the elder drawn through any rock in
  front of them. The gym zooms out further, to see the whole 40 m face.
  No automatic camera moves.
- Settled in CL.4: the silhouette is flat cyan, drawn where rock stands
  more than 1.3 m nearer the camera than the body, in volume scenes. The
  wheel is a ring right of the body, in screen space (`climb_view.rs`).

### K10. Scenes up to 64 m

- The cap goes to 64 m above zero (128 layers), keeping 16 m below. Check
  the view and the fog handle it.

### K11. In the sim, with movement

- The avatar's movement gets a climbing state (the face's tile and side,
  the feet's height) and a stamina value, stepped in stage 1 with the rest
  of movement, predicted like walking.
- Grabbing, the lunge and letting go come from the movement input and the
  existing jump, so no new command if possible. The save format goes up.

### K12. The gym first, then an open-world study

- The gym is built on the volume as it is: height is cheap.
- Then a written study of an open-world map: regions keyed by coordinates,
  a hash per region combined in a tree, streaming regions in and out, fog
  in regions, and what the sim runs far from players. The study becomes
  its own design page.

## The Climbing Gym (CL.1)

Scene `climbing-gym` (`app/assets/levels/climbing_gym.ron`, the 16th in
the scene list: F4 steps to it, or `--scene climbing-gym`), 128 m across
(extent 64). The scene file's comments name each feature in tiles (x
east, z south) and layers. The sun shines from the south-west, so the faces
that matter most face south or west, in the light; east and north faces
stand in shadow (near black until CL.5 gave the gym its own `ambient`,
800, the town's at noon):

| Feature | Where (tiles) | Height |
| --- | --- | --- |
| The summit; its north face, and its south face from x -10 to -5 | x -10..10, z -20..0 | 40 m |
| The west face, the full 40 m: three pitches of about 13.5 m, a 2 m ledge between each | ledges at x -14..-13 (13.5 m) and x -12..-11 (27 m) | 40 m |
| The south shoulder's face | x -4..10, z 1..8 | 20 m |
| The inside corner: the shoulder's west face meets the summit's south face | x -5..-4, z 0..1 | 20 m |
| The chimney: a slot 2 m wide, 6 m deep, open to the sky | x 3..4, z 3..8 | 20 m |
| The east wing's east and south faces (over it the summit's east face, 30 m, no ledge) | x 11..22, z -20..6 | 10 m |
| The low step's south and east faces | x 11..22, z 7..14 | 5 m |
| The overhang: a roof 4 m out from the wing's east face, a 7 m ceiling under it | x 23..26, z -14..-6 | 7 to 10 m |
| Pillars, 2 x 2 tiles | x 24, 30, 36; z 20..21 | 5, 10, 15 m |

The elder spawns on the flat at (28, 26), by the pillars; no ghouls or
sprites. The scene's `zoom_out: Some(80.0)` lets the view zoom out to 80 m
(the usual is 26), and the camera stands four times further back so the
mountain never comes in front of it; its `TurningView` feature turns the
view with Z and C (the D-pad's left and right) as in a town. Demo:
`//kestrel/app:climbing_gym_demo` (and `_video`), walking round the
mountain while the view turns and zooms.

## The review (CL.5)

The exploration was reviewed by video, two clips of at most 120 s (a
script's cap), rendered locally (`bazel build` of the `_video` targets):

- `//kestrel/app:climbing_face_demo` (71 s): the 40 m west face in three
  pitches with a rest on each ledge, the silhouette from the far side on
  the second, a lunge on the third, the summit.
- `//kestrel/app:climbing_tour_demo` (85 s): the 5 m pillar round its
  corner and a step off it (1 health), the chimney's three walls and out
  of its mouth, the inside corner, a wheel run out with lunges and the
  fall from 11 m (3 health), the overhang from below and on to the wing.

`//kestrel/app:climbing_view_demo` (CL.4) is the shorter look at the pose
and wheel. The soak (`climbing_soak_tests`) climbs at random round the
mountain and pillars, checking every tick that the climber hangs off a
face it holds, clear of rock, that the wheel stays in range and is never
grabbed exhausted, and that deaths come back.

## What changed versus the design

Gathered from the tasks' Done paragraphs in
[`todo/climbing.md`](todo/climbing.md):

- **G lets go, not "G or B"** (K3, CL.2): G is Board, which has nothing to
  board on a wall (a gamepad's left-stick click). The pad's B stays Drop,
  so climbing needed no new command. Only the stick grabs (0.15 s pushing
  within 60 degrees of straight in), never a click-to-move route; on a
  face the stick reads against the face (into it up, out of it down,
  along it sideways).
- **The exhaustion rule** (K6, CL.2): an empty wheel lets go, and the
  elder can't grab again until the wheel is full on a floor; after any
  let-go, not until they stand on a floor. So nothing catches a fall
  part way, and nothing bridges a chimney's two walls (CL.5).
- **Ledge falls are real** (CL.2): a climber with no jump of its own (the
  elder) falls under the climb's 20 m/s² off a face and off any ledge in a
  volume, where it used to drop to the floor at once.
- **17.5 m kills** (K7, CL.3): 5 m takes 1, each 2.5 m more another, so
  17.5 m takes the elder's 6; "about 20 m" in the design. A full wheel
  climbs about 15 m (5 health), so running out from the ground straight
  up is survivable, just.
- **The gym's `player_health`** (CL.3): a new scene field (1–100, not
  beside a scenario) gives the scenario-less gym's elder 6 health, so its
  falls hurt; the Tunnel Lab, without it, stays unhurt.
- **The high-roof sight rule** (CL.1): a roof 5 m or more over the floor
  counts as sky for the view (`sight::lit`), so standing under the 7 m
  overhang doesn't darken the screen as a tunnel does. The fog still
  counts it as roofed.
- **`zoom_out`** (K9, CL.1): a scene field (the gym's 80 m; 26 elsewhere),
  with the camera standing four times further back there
  (`effects::stand_back`) so a 40 m mountain never meets its near plane;
  and `TurningView`, a scene feature for Z/C outside a town.
- **The silhouette and the wheel** (K9, CL.4): flat cyan, drawn where rock
  stands over 1.3 m nearer the camera than the body, volume scenes only;
  the wheel is a ring right of the body in screen space, with a red "!"
  in the air when a fall will hurt. A hurting fall has its own thud (dust,
  the landing sound, a shake by the damage), not a ghoul's hit.
- **The pull-over** (CL.5): the feet reach the top before the body moves
  over the edge (it used to start moving over a third of the way in and
  cut through the edge's corner for a few ticks).
- **Inside corners** (CL.5): the climber turns into an inside corner only
  when the stick is more along the face than up or down it. Pushed
  diagonally into a corner it climbs straight up it; before, each face's
  sideways part pointed into the other, and it turned between the two
  every tick.
- **The gym's own light** (CL.5): a scene `ambient` field (the ambient
  light's brightness in place of the style's), the gym's 800, so the east
  and north faces read dark slate instead of black; and the HUD says
  "FALLING" for a climber's fall (it said "JETPACK / SAFE LANDING").
- **The numbers stood** (CL.5): the elder's `climb` section is as CL.2
  and CL.3 set it; the soak and the video found nothing to tune.

## Consequences

- A climbing state and stamina on the avatar raise the save format; the
  input log only if climbing needs a command (G to let go may already map
  to one).
- Falls hurt in every scene with drops: dug pits, terraces, the town's
  hill. Golden behaviour lines move only if a golden scene has a fall over
  5 m. (CL.3: in the town only a dug shaft is that deep, its hill's cliffs
  and terraces are about 2 m; the golden's scenes have no volume, so only
  its state-hash lines moved.)
- Sprites, ghouls and haulers keep walking round cliffs: a ghoul can't
  follow you up, so height is safety for now.
- With the fog, a peak sees most of a map: climbing becomes scouting.

## Tests

- Grabbing a wall by pushing into it; not a step; jump grabs higher.
- Climbing up, down and sideways; wrapping an outside corner; an inside
  corner; pulling over the top; an overhang stops the climb.
- Stamina: about 15 m from full; slow drain hanging; refills on ground and
  ledges; empty lets go; the lunge costs a fifth.
- Falls: none under 5 m, the damage steps, about 20 m kills.
- Determinism, saves and prediction replay while climbing.
- A soak in the gym and a demo video.
