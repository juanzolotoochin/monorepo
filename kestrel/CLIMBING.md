# Kestrel climbing gym (design)

Status: agreed (2026-11-11), an exploration. Tasks CL.1–CL.6 in
[`todo/climbing.md`](todo/climbing.md). It builds on the volume
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
  Lab, with none, falls still don't hurt. The view shows a hurting fall as
  a hit from nowhere (the hurt sound, blood, the shake).

### K8. No glider yet

- Climb down, or jump and take the fall. A glider (a leaf or a cloak) is a
  later step.

### K9. Rotate by hand, a silhouette through rock

- Z/C as today, plus a silhouette of the elder drawn through any rock in
  front of them. The gym zooms out further, to see the whole 40 m face.
  No automatic camera moves.

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
stand in shadow, near black:

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
