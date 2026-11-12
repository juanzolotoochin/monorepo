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
  zero to 32 m above (`LAYOUT_LAYERS`).
- Walking off a ledge falls like a jump's way down; falls don't hurt.
  Walls block; paths never step down more than a layer.
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

### K7. Falls hurt

- No damage under 5 m, then 1 health per 2.5 m (the elder has 6), so about
  20 m kills. A dead elder comes back as today. Numbers in content.

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

## Consequences

- A climbing state and stamina on the avatar raise the save format; the
  input log only if climbing needs a command (G to let go may already map
  to one).
- Falls hurt in every scene with drops: dug pits, terraces, the town's
  hill. Golden behaviour lines move only if a golden scene has a fall over
  5 m.
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
