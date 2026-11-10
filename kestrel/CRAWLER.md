# Kestrel crawler (design)

Status: done (2026-11-10), a prototype, reviewed by video (agreed
2026-11-09). Tasks CR.1–CR.6 under Done in
[`todo/vehicles.md`](todo/vehicles.md), what's left in its open crawler
entries, and what to judge in play in
[`todo/playtest-review.md`](todo/playtest-review.md) ("Play the crawler").
How it works now: [`docs/vehicles.md`](docs/vehicles.md) ("The crawler").
It builds on the rover and the volume
([`VERTICALITY.md`](VERTICALITY.md)); the rules for the work in
[`SACRED_TREE_HANDOFF.md`](SACRED_TREE_HANDOFF.md) apply here too (one
subagent per task, tests per task, a `docs/test-time.md` row at the end).
Design page: https://claude.ai/artifact/GXbxRoZKNXUvhv92B2SYWq

## Goal

A big vehicle that walks on four legs, planting its feet with inverse
kinematics, and crosses ground wheels and feet can't. A player drives it.
It isn't initial content: you get it later in a game (how is decided after
the prototype).

## Today

- The dig lab's rover is a character with `brain: Vehicle`, its own table
  (`Settlement::vehicles`), boarding with G (`Board`), driving in stage 1
  (`vehicle::drive`), prediction through `AvatarReplay`, saves and hashes.
  Its four wheels sample the heightfield (`Terrain::ground`) and climb up
  to 0.55 m.
- The volume (Tunnel Lab, the town) has floors over each other
  (`floor_near`, `walk_at`); the rover doesn't read it.
- Avatars have a planted-foot gait with a two-bone leg solve
  (`app/src/character.rs`, `solve_leg`), view only.

## Decisions

### C1. A brass-and-timber machine

- A frontier engine with a cab on top, in the Brasswood style.

### C2. About four elders big

- A body 6 m long and 3 m wide, its deck 3 m up, legs reaching about
  4.5 m. It fits a 4 m gap between buildings but not a tunnel.

### C3. A carrier over rough ground

- Its job, later: a cargo deck that holds several piles (wood, stone,
  amber), stepping over terraces, pits and moats.
- The prototype is about the walking; the cargo comes once it walks well.

### C4. How you get it: later

- The prototype only proves the walking.

### C5. Big steps and gaps

- Steps up or down to 1.5 m (three layers) under one foot.
- Gaps up to 2 m between footholds: a moat, a pit, a dug trench.
- The body stays level and rises or sinks with its feet. No cliffs.

### C6. Footholds in the sim, joints in the view

- The sim decides the gait and the footholds, in `Fx` and deterministic:
  which leg lifts when, where each foot lands (a floor in the volume within
  reach), and the body's height and tilt from the planted feet.
- The view solves the joint angles (IK) and draws the swing arcs.

### C7. Walk one leg at a time, trot on boost

- A wave walk when walking: one leg at a time, three always down, heavy
  and steady.
- A diagonal trot (two at a time) when boosting.

### C8. Insect legs, knees high

- Three segments: a high knee over the body, a long shin and a short foot
  that stays upright.
- Solved as a two-bone IK with the foot segment kept vertical.

### C9. A pilot and three riders

- A pilot in the cab plus up to three riders seated on the deck (other
  players' elders). Riders can't steer.
- G boards; G again climbs down beside it.

### C10. Feet shove and stomp

- A foot landing shoves bodies off its spot; a ghoul under a foot takes
  heavy damage.
- The body passes over everyone (it's high).
- Ghouls don't attack it yet.
- A foot never lands on an elder or sprite: it picks another foothold.

### C11. A Crawler Lab

- A new scene on a volume, like the Tunnel Lab: terraces, a 1.5 m step, a
  2 m trench, a moat, a wider pit it can't cross, a cliff, a few ghouls to
  stomp.
- It joins the town later (C4).

### C12. A rig as data, boxes for now

- Like the exosuit's actor file: body, cab, hip, knee and foot parts as
  boxes and cylinders with joints; built in until a style supplies a skin.
- The camera pulls back while piloting.

## What changed versus the design

- No 1.6 m step (C5, C11): layers are 0.5 m, so the lab's too-high case is
  the mesa's 2 m cliff, and its too-wide one the 3 m pit. The 2 m moat is
  a dry ditch, crossed like the trench.
- The body stays level (no pitch or roll) and sinks with its feet: walking
  off the 1.5 m step it drops about 1 m, since a resting foot 1.5 m down is
  past the 4.5 m reach from a body at the upper level. Ground under its box
  higher than its belly (1.2 m under the deck) stops it, which is what
  stops it at the cliff.
- Steering (C7): the stick walks within 60° of the heading, turns in place
  up to 120° and backs up past that. Turning is 20°/s (from 35) so the
  feet keep within 2 m of rest; it speeds up at 1.0 m/s² (from 1.5), so the
  last leg of the wave steps 1.8 s after starting. Stopped at a gap's edge
  and steered away, it backs off at half its reverse speed as it turns
  until its feet have floor to step round on (CR.6).
- Legs (C8): thigh 1.8 m, shin 3.4 m, foot 0.45 m; the knee bends up and a
  little outward, and a foot out of reach eases the leg straight instead
  of snapping. No body bob. The camera pulls back by 1.4 for the pilot and
  riders too.
- Riders (C9): boarding reaches the 6 x 3 m box (0.8 m past it) from any
  side, not a radius; the cab first, then the free deck place nearest. A
  rider doesn't move to the cab when the pilot leaves. Pilot and riders
  stand in the idle pose. Climbing down never goes onto a floor more than
  0.6 m off the highest its feet stand on, so never into a trench.
- Feet (C10): a shove puts a body 0.1 m clear of the foot; a stomp does 4
  damage (a ghoul has 2 or 3), throws off one that lives at 4 m/s and
  staggers it 0.4 s. Planned routes go round the feet; walking by hand
  with the stick, and a wave's shared route, pass through them.
- The rig (C12) is built in (`app/actors/crawler.ron`), not a style's
  skin.
- Demo scripts gained scripted guests (`guest_join`, `guest_walk`,
  `guest_board`), so the demo carries riders (CR.6).

## Consequences

- The rover's framework carries over. The crawler adds four feet and a gait
  clock to its state, so the save format goes up; the input log and net
  protocol only if riders need a new command.
- Footholds read the volume's floors, so the crawler also works in the
  Tunnel Lab and the town as they are; a foot may land on a floor a tunnel
  runs under.
- Turning in place is a gait of its own (legs step sideways); backing up
  uses the same gait reversed.
- Bodies path round its feet, not its body, which is overhead.

## Tests

- Gait: three feet always down at walk, diagonal pairs at boost; the same
  inputs give the same footholds (determinism, prediction replay).
- Footholds: a 1.5 m step and a 2 m gap are crossed; a 1.6 m step, a wider
  pit and a cliff stop it; every planted foot is within reach and on a
  floor; never on an elder or sprite.
- Body: level, at its feet's height, through terraces and the trench.
- Boarding: a pilot and three riders; climbing down beside it.
- Feet: a landing foot shoves bodies and hurts a ghoul under it.
- Saves and hashes round-trip with the feet.
- A soak in the lab and a demo video.
