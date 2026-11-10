# Vehicles

The crawler prototype's design is [`CRAWLER.md`](../CRAWLER.md)
(decisions C1–C12). Its tasks CR.1–CR.6 run in order, one agent each.

## CR.1 The Crawler Lab and the crawler

- **Status:** open
- **Why:** the crawler needs a scene with every case and a definition.
- **What to do:** a `crawler_lab` scene on a volume (C11): terraces, a
  1.5 m step, a 2 m trench, a moat, a pit wider than 2 m, a cliff, a few
  chasing ghouls. A `character/crawler.ron` with `brain: Vehicle`, its body
  (6 m by 3 m, deck 3 m up, C2), speeds and a crawler tuning section (leg
  hips, reach about 4.5 m, step 1.5 m, gap 2 m, gait timings). A pilot
  boards with G as with the rover. Until CR.2 it may move like the rover on
  the volume's floors; say how.
- **Where:** `app/assets/levels/`, `content/data/character/`,
  `content/src/character.rs`, `sim/src/settlement/vehicle.rs`, the scene
  list and keys.
- **Found in:** crawler design.
- **Tests:** the scene loads; the crawler is in the table; boarding.

## CR.2 Gait and footholds in the sim

- **Status:** open
- **Why:** C5–C7: the crawler walks on footholds the sim chooses.
- **What to do:** in `Fx`, deterministic: four feet (planted spot and
  floor, swing from and to, progress) and a gait clock; a wave walk at
  walk and a diagonal trot at boost (C7); a foot lifts when it falls
  behind and lands on the floor nearest its ideal spot that is within
  reach, at most 1.5 m up or down and across at most 2 m of gap
  (`Volume::floor_near`); never on an elder or sprite (C10). The body's
  height and tilt follow the planted feet (level, C5). Turning in place
  steps sideways; backing up reverses. With no foothold it stops. Saved
  and hashed (save format up); `AvatarReplay` copies it.
- **Where:** `sim/src/settlement/vehicle.rs` (or a new `crawler.rs`),
  `sim/src/volume.rs`, `save/`.
- **Found in:** crawler design.
- **Tests:** the gait's feet down; crossing the step and the trench;
  stopped by 1.6 m, the wide pit and the cliff; reach; determinism and
  prediction replay; save round trip.

## CR.3 Legs, IK and the camera

- **Status:** open
- **Why:** C8, C12: the legs must read.
- **What to do:** a rig as data (`app/actors/crawler.ron`): body, cab,
  hips, knees, shins and feet as boxes and cylinders. The view solves each
  leg as a two-bone IK with the foot kept vertical and the knee high (C8),
  swings feet along arcs between the sim's spots, eases the body to its
  height and tilt, and interpolates other players' crawlers. The camera
  pulls back while piloting.
- **Where:** `app/src/vehicles.rs` (or `app/src/crawler.rs`),
  `app/actors/`, the camera.
- **Found in:** crawler design.
- **Tests:** the IK reaches every planted foot with the knee over the hip
  line; screenshots and a short video walking the lab.

## CR.4 Riders

- **Status:** open
- **Why:** C9: a pilot and three riders.
- **What to do:** four seats (cab and deck); G boards the nearest free one
  (the cab first); riders ride seated and can't steer; G climbs down to a
  free floor spot beside it. Prediction and saves as the rover's driver.
- **Where:** `sim/src/settlement/vehicle.rs`, `avatar.rs`, the view.
- **Found in:** crawler design.
- **Tests:** four players board in order; a fifth can't; climbing down
  beside it, never into a pit.

## CR.5 Feet and bodies

- **Status:** open
- **Why:** C10: footfalls matter.
- **What to do:** a landing foot shoves bodies off its spot (a message,
  applied in stage 5 like knockback) and a ghoul under it takes heavy
  damage. Bodies path round planted feet. Ghouls don't attack it yet.
- **Where:** `sim/src/settlement/vehicle.rs`, `combat.rs`, `messages.rs`.
- **Found in:** crawler design.
- **Tests:** a shove; a stomp; never a foot on an elder or sprite.

## CR.6 Soak, tuning and demo

- **Status:** open
- **Why:** the prototype is reviewed by video.
- **What to do:** a soak in the lab (driving randomly for minutes,
  `validate` every tick); tune speeds and the gait; a demo video crossing
  the terraces, the step, the trench and the moat, stopping at the wide
  pit and the cliff, stomping a ghoul and carrying riders; the full suite;
  a `docs/test-time.md` row; `docs/vehicles.md` and `CRAWLER.md`'s status.
- **Where:** sim tests, `app/demos/`, docs.
- **Found in:** crawler design.
- **Tests:** the soak; the full suite.

## Collide rovers with bodies

- **Status:** open
- **Why:** a rover drives through avatars, haulers, threats and other rovers. Pushing them would be an effect on other entities, so it needs messages rather than a direct write.
- **What to do:** in stage 3, a moving rover emits a push message for each body it overlaps, and a stop for itself on another rover. Stage 5 applies them in a fixed order, as a ghoul's knockback is. Decide whether a rover hurts threats it hits.
- **Where:** `kestrel/sim/src/settlement/vehicle.rs`, `combat.rs` (knockback), `messages.rs`.
- **Found in:** the vehicle prototype (`docs/vehicles.md`).

## Drive to a clicked point

- **Status:** open
- **Why:** `Intent::GoTo` stops a driven rover (`vehicle::drive`), so mouse players can't drive.
- **What to do:** plan a route with the rover's radius and its own step rule (wheels climb up to `climb`), and steer along it like a hauler's cart.
- **Where:** `vehicle.rs` (`drive`), `nav.rs` (`path_with`).
- **Found in:** the vehicle prototype.

## Show a driver seated, and a boarding prompt

- **Status:** open
- **Why:** the driver stands at the seat in the idle pose, and nothing tells a player that G climbs in.
- **What to do:** add a seated pose to the rig while `AvatarState::driving` is set. Show "G: DRIVE" in reach of a free rover, as the repair prompt does.
- **Where:** `kestrel/app/src/rig.rs`, `scene.rs` (HUD prompts), `vehicles.rs`.
- **Found in:** the vehicle prototype.

## Draw rovers from a style skin

- **Status:** open
- **Why:** `app/src/vehicles.rs` builds a fixed mesh, ignoring the character's `view.skin` and the scene's style.
- **What to do:** let a style supply the `rover` skin (chassis and wheel parts), with the built-in mesh as a fallback.
- **Where:** `kestrel/app/src/vehicles.rs`, `style.rs`.
- **Found in:** the vehicle prototype.

## Done
