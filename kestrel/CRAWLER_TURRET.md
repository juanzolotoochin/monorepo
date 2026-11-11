# Kestrel crawler turret (design)

Status: agreed (2026-11-10), a prototype. Tasks TU.1–TU.6 in
[`todo/vehicles.md`](todo/vehicles.md). It builds on the crawler
([`CRAWLER.md`](CRAWLER.md), branch `kestrel-crawler`) and the town's
ballista ([`SACRED_TREE_BALLISTA.md`](SACRED_TREE_BALLISTA.md)); the rules
for the work in [`SACRED_TREE_HANDOFF.md`](SACRED_TREE_HANDOFF.md) apply
here too (one subagent per task, tests per task, a `docs/test-time.md` row
at the end). Design page: https://claude.ai/artifact/5JHfr45ygrTWHRUCaSjs6e

## Goal

A gun on the crawler, so one player drives while another shoots. It is the
town's ballista with its fireball (the "fire turret") for now: a sprite can
man it, an elder mans it better.

## Today

- The ballista is a building kind with a `defence` section
  (`buildings.rs`): stone ammo (cap 10), range 3.8 m, a bolt every 0.7 s,
  1 damage, and a fireball every 25 s that leaves 4 s of fire (`fire.rs`).
- A sprite staffs it with `SpriteJob::Staff(kind)` and it shoots the
  nearest threat on its own (`ready_to_shoot`). An elder possesses it with
  G: the aim is a reticle within range, `Attack` fires, the Burst key
  throws the fireball (`fire_plan`, `burst_plan`). Occupants are sheltered.
- The crawler (CR.1–CR.6) seats a pilot (`Seat::Vehicle`) and three riders
  (`Seat::Rider`); G takes the cab first, then the nearest free place.
  Ghouls ignore it; its feet stomp them. The Crawler Lab has one timed wave
  of three ghouls, no sprites and no stone.

## Decisions

### T1. On the cab's roof, all round

- The gun sits on the cab's roof, the crawler's highest point, and swivels
  all the way round.

### T2. Four seats: pilot, gunner, two riders

- The back rider place (behind the crates) becomes the gunner's.

### T3. Boarding: the cab, then the gun, then the riders

- G takes the cab if free, then the gun, then the nearest free rider place.
- An elder boarding while a sprite mans the gun takes the gun; the sprite
  moves to a free rider place, or climbs down if there is none.

### T4. A sprite job: Crew the crawler

- Set like `Staff`: the sprite walks to the crawler and climbs to the gun
  when the crawler stands still within reach. A walking crawler waits for
  nobody. Changing the job climbs it down.

### T5. As at the ballista

- A sprite shoots the nearest threat in range on its own.
- An elder aims with the reticle (mouse or right stick; the reticle moves
  with the crawler), fires with the left button and throws the fireball
  with R.

### T6. A stone store, restocked by hand

- Stone, cap 20, one a bolt. An elder carrying stone delivers it by
  interacting beside the crawler while it stands still, as at the ballista.
- The lab starts it full and has a stone pile. The fireball costs no stone.

### T7. 7 m, otherwise the ballista

- Range 7 m (the gun sits about 4 m up); the ballista's period, damage and
  fireball otherwise. The numbers live in content: the crawler's own
  defence section, copied from the ballista's.

### T8. Ghouls don't fight back yet

- Ghouls still ignore the crawler, and everyone aboard is sheltered (can't
  be hit), as an occupant of a building.

### T9. Part of the vehicle, through shared helpers

- `VehicleState` gets a turret (ready, aim, fired) and an ammo store; the
  crawler's definition names its defence; a gunner seat joins `Seat`.
- The ballista's shared helpers (picking a target, launching a bolt, the
  fireball) take a mount position instead of a building, as B1 did for the
  forest's ballista. The town's and the forest's golden behaviour lines
  stay unchanged. The save format goes up.

### T10. The Crawler Lab, with waves

- Waves of ghouls on a timer from two sides, two sprites and a stone pile.
- Demo videos: a pilot and a gunner (a guest, through the demo script's
  guest ops) clearing a wave; and one with a sprite gunner.

## Consequences

- No new command: aiming, firing and the fireball are the ballista's
  commands, read from a gunner's seat; the crew job is a new
  `SpriteJob` value through the existing `SetSpriteJob`.
- A gunner gets the camera pull-back and the reticle; the pilot's view is
  unchanged.
- A gunner's shots are predicted as a possessing elder's are.
- Deliveries go to a thing that moves: only while it stands still.

## Tests

- Boarding: the cab, then the gun, then the riders; an elder takes the gun
  from a sprite, which moves to a rider place or climbs down.
- A sprite gunner shoots the nearest threat in range, from the moving
  mount; an elder gunner fires at the reticle, clamped to 7 m, at the
  period, and throws the fireball on its cooldown; nothing fires empty.
- Deliveries: stone only while the crawler stands still, up to 20.
- The crew job: a sprite boards a stopped crawler, waits for a walking one,
  and climbs down when its job changes.
- Nobody aboard is hit; ghouls ignore the crawler.
- The town's and the forest's golden behaviour lines don't change.
- Saves, hashes and prediction replay with a gunner aboard.
- A soak in the lab with waves, and the demo videos.
