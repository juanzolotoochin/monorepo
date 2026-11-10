# Vehicles

The crawler prototype's design is [`CRAWLER.md`](../CRAWLER.md)
(decisions C1–C12). Its tasks CR.1–CR.6 run in order, one agent each.

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
- **Notes from CR.1:** the tuning is `CrawlerDef` (`content::character`,
  the `crawler:` section of `character/crawler.ron`): `hip` and `stance`
  are the front right leg's offsets (`along` the heading, `across` to the
  right), the others mirror them in `VehicleState::wheels` order (front
  left, front right, rear left, rear right); `walk`/`trot` hold `cycle`,
  `swing` (ticks) and `lift`. Today the crawler is a plain
  `VehicleState` rolling on four points under its hips (the `crawler`
  section sets `vehicle`'s wheelbase, track, climb = step and seat =
  deck; the file may not write them), each reading the floor it rolls on
  in a volume (`ground_under`: `Terrain::floor(p, wheel + climb)`). It
  rolls up the plateau's 1.5 m face and stops at the mesa's 2 m cliff,
  but a hip point drops into the 2 m-deep trench, the pit or the moat and
  can't climb out, and it can't turn standing still (turning needs
  rolling). Replace that model, keeping `drive`, boarding and the seat.
  Layers are 0.5 m, so "1.6 m" can't be built: the lab's next height up
  is the mesa's 2 m. A resting foot (`stance`) is 3.36 m from its hip, so
  stepping down 1.5 m under a level body is 4.65 m, past the 4.5 m reach:
  the body must sink with its feet, or tune `stance`/`reach`. Content
  tests check the ranges; `CrawlerDef` joins `rules_hash` like
  `vehicle`.

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
- **Notes from CR.1:** `app/src/vehicles.rs` draws a crawler (a vehicle
  whose character has a `crawler`) as a PLACEHOLDER: a timber box its
  length and width hanging under the deck, a brass cab and rail on it,
  and brass posts under the hips stretched to the ground
  (`spawn_crawler`). Replace it; the wheel ground there now reads the
  volume's floor as the sim does. The demo is `crawler_lab_demo`
  (`app/demos/crawler_lab.json`).

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
- **Notes from CR.1:** the one seat today is the rover's: boarding reaches
  `body.radius` (1.5 m, half the width) + the elder's + 0.8 m from the
  middle, so only beside the long sides, not at the ends; the lab parks
  it 2.5 m from the spawn for that. `exit_spot` puts the pilot
  `radius + 0.47` m out: just clear of the sides, but its behind and in
  front fallbacks land under the 6 m body. Make the seats and the exit
  spots follow the body's box.

## CR.5 Feet and bodies

- **Status:** open
- **Why:** C10: footfalls matter.
- **What to do:** a landing foot shoves bodies off its spot (a message,
  applied in stage 5 like knockback) and a ghoul under it takes heavy
  damage. Bodies path round planted feet. Ghouls don't attack it yet.
- **Where:** `sim/src/settlement/vehicle.rs`, `combat.rs`, `messages.rs`.
- **Found in:** crawler design.
- **Tests:** a shove; a stomp; never a foot on an elder or sprite.
- **Notes from CR.1:** the lab's ghouls are a timed wave of three
  `ghoul_sacred_tree` (2 hit points, chase within 3 m, leash 5 m) from the
  south-east corner to the north-west one (`crawler_lab_supply.ron`);
  each breaches there and is gone, so a stomp test should catch one on
  its way, or add a wave.

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

## CR.1 The Crawler Lab and the crawler

- **Status:** done (branch `kestrel-crawler`, PR to come with CR.6)
- **Why:** the crawler needs a scene with every case and a definition.
- **What to do:** a `crawler_lab` scene on a volume (C11), a
  `character/crawler.ron` with its body, speeds and a crawler tuning
  section, boarding with G, and a demo.
- **Found in:** crawler design.
- **Done:** scene `crawler-lab` (CRAWLER LAB / ROUGH GROUND, last in the
  scene list: F4 steps to it), `app/assets/levels/crawler_lab.ron`: 64 m
  of stone with three half-meter terraces up to a plateau at 1.5 m whose
  north face is a single 1.5 m step, a mesa with a 2 m cliff, a trench
  2 m wide and deep, a pit 3 m wide and deep, a dry moat 2 m wide round a
  13 x 11 m island, and 9 m of ground between trench and pit. Its supply
  (`crawler_lab_supply.ron`) has the elder and a timed wave of three
  chasing `ghoul_sacred_tree` across the lab; the forest (Brasswood)
  style draws them. `character/crawler.ron`: `brain: Vehicle`, radius
  1.5, walk 1.6 m/s, run 3.2 m/s, and `crawler:` (`CrawlerDef`): 6 x 3 m,
  deck 3 m, hip (2.4, 1.2), stance (3.0, 2.6), reach 4.5, step 1.5, gap
  2, walk (144 frames a cycle, 33 a swing, 0.9 m lift), trot (60, 27,
  0.6 m), validated (ranges, a hip on the body, a resting foot in reach,
  four or two swings per cycle) and in `rules_hash`. Until CR.2 it rolls
  like the rover on four points under its hips, the `crawler` section
  setting `vehicle`'s wheelbase, track, climb (the step) and seat (the
  deck); every vehicle's wheels now read the floor they roll on in a
  volume scene (`ground_under`, so the rover is unchanged elsewhere). G
  boards the one seat, on the deck, and climbs down beside it. The view
  draws it as a placeholder box with a cab and posts
  (`vehicles::spawn_crawler`). A volume is now drawn, and the built-in
  deck hidden, without the Digging feature (`Digging::init`,
  `SceneView`); the lab follows the elder with `CameraFollow` and can't
  be dug. Demo `//kestrel/app:crawler_lab_demo` (`_video`). Tests:
  `crawler_lab_tests` (every feature's height, the crawler parked in the
  table with its legs and the ghouls coming, boarding, riding on the
  deck and climbing down, rolling up the step but not the cliff),
  content `the_crawler_has_its_legs_and_its_wheels_follow_them` and
  `crawlers_are_validated`, the app's scene list and obstacles. Save
  format unchanged; the character indices shift (`crawler` sorts first),
  so the rules hash changed and the sim golden's state hashes moved
  (behaviour lines unchanged, regenerated); headless hashes unchanged.
  The HUD shows the settlement's base, wave and pack lines and a stock
  help line that don't apply to the lab (CR.6 may hide them).
