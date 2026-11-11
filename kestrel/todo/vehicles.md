# Vehicles

The crawler prototype's design is [`CRAWLER.md`](../CRAWLER.md)
(decisions C1–C12). Its tasks CR.1–CR.6 are done (see Done); what's left
of it is in the open entries below.
The crawler's turret is [`CRAWLER_TURRET.md`](../CRAWLER_TURRET.md)
(decisions T1–T10), tasks TU.1–TU.6 run in order, one agent each, on
branch `kestrel-crawler-turret` stacked on `kestrel-crawler`.

## TU.2 The turret shoots

- **Status:** open
- **Why:** T5, T8, T9.
- **What to do:** `VehicleState` gets a turret (ready, aim, fired) and an
  ammo store; the ballista's helpers (pick a target, launch a bolt, the
  fireball) take a mount position instead of a building. A sprite gunner
  (seated directly by tests until TU.4) shoots the nearest threat in range
  from the moving mount; an elder gunner's reticle, `Attack` and Burst
  key fire bolts and the fireball as at the ballista. Nobody aboard can
  be hit; ghouls ignore the crawler. Prediction of a gunner's shots.
- **Where:** `sim/src/settlement/buildings.rs`, `combat.rs`, `fire.rs`,
  `avatar.rs`, `vehicle.rs`.
- **Found in:** crawler turret design.
- **Tests:** a sprite gunner hits the nearest threat while walking; an
  elder fires at the reticle, clamped to 7 m, at the period, and the
  fireball on its cooldown; nothing fires empty; the town's and the
  forest's golden behaviour lines unchanged; prediction replay matches.
- **Notes from TU.1:** the gun is `CrawlerDef::gun` (`GunDef`: `mount`,
  `height` over the deck, `defence: scenario::Defence` with the ammo as
  an item id). Resolve it to `buildings::Defence` (ticks, `ItemIx`) the
  way `building_kinds` does (`buildings.rs`, the `defence:` closure:
  worth making a function both use). Its `muzzle_height` is the default
  1.6 m and means nothing on a gun: shots leave from `deck + height` at
  `mount`. A gunner today goes through `avatar::act`'s seated branch with
  riders: `Aim` passes through to the normal handling, `Attack` does
  nothing and `Burst` is refused as `NotPossessing`; give `Seat::Gunner`
  its own arm there. `Settlement::gunner_of(vehicle)` finds the gunner.
  No turret or store state was added to `VehicleState` yet; the save
  format is 50, so bump to 51.

## TU.3 Ammo

- **Status:** open
- **Why:** T6.
- **What to do:** an elder carrying stone interacts beside a stopped
  crawler and delivers as much as fits (cap 20), as at the ballista; a
  walking crawler takes none. Bolts spend stone (balances); the fireball
  doesn't. The lab starts it full.
- **Where:** `sim/src/settlement/interact.rs`, `vehicle.rs`.
- **Found in:** crawler turret design.
- **Tests:** a delivery while stopped; none while walking; the cap;
  conservation holds.
- **Notes from TU.1:** the lab's stone pile is the depot `lab_stone`
  (60 stones, only stone) at (-3, -4.5), north-west of the spawn, about
  5.5 m from the parked crawler's west side; the gun's ammo is `stone`,
  an item id resolved per scenario (unchecked: a scene without stone
  parks a gun with nothing to load).

## TU.4 Crew the crawler

- **Status:** open
- **Why:** T3, T4.
- **What to do:** a `SpriteJob` to crew a crawler, set through
  `SetSpriteJob` like `Staff`: the sprite walks to it, climbs to the gun
  when it stands still within reach, waits for a walking one; changing
  the job climbs it down. An elder boarding takes the gun from a sprite,
  which moves to a free rider place or climbs down. The lab sets the job
  the way the town does if that's cheap, else from a key.
- **Where:** `sim/src/settlement/` (sprites), `avatar.rs`, `vehicle.rs`,
  the app's job controls.
- **Found in:** crawler turret design.
- **Tests:** a sprite boards a stopped crawler and shoots; waits for a
  walking one; climbs down on a job change; an elder bumps it; saves and
  hashes.
- **Notes from TU.1:** the lab's sprites are `sprite_ash` and
  `sprite_fern` by the stone pile, at (-4.6, -3.6) and (-4.6, -5.4), near
  the south-east waves' path (ghouls turn on them within 3 m). `Seat`
  lives on `AvatarState`, so a sprite at the gun needs its own link (a
  sprite field, or a `VehicleState` crew slot); `free_seat` returns the
  gun only while `gunner_of` is `None`, so an elder bumping a sprite needs
  it to see the sprite too. `crawler::GUNNER` is where it stands.

## TU.5 The gun in the view

- **Status:** open
- **Why:** T1, T5.
- **What to do:** the gun on the cab's roof in the rig
  (`actors/crawler.ron`), swivelling to its aim; bolts and the fireball
  leave from the mount; a gunner sees the reticle and gets the camera
  pull; a sprite gunner drawn at its seat.
- **Where:** `app/src/crawler.rs`, `vehicles.rs`, the ballista's view.
- **Found in:** crawler turret design.
- **Tests:** the gun turns to its aim; the reticle shows for an elder
  gunner only; screenshots.
- **Notes from TU.1:** the mount is (along 1.55, across 0), 1.3 m over
  the deck: the rig's cab top (z 0.8 to 2.3, 1.3 m up). The gunner stands
  at `crawler::GUNNER` behind the crates, drawn like a rider
  (`vehicles::deck_offset`); they already get `PILOT_PULL`. The control
  protocol's `seat` is `{"vehicle": 0, "gunner": true}` for a gunner.

## TU.6 Soak, demos and docs

- **Status:** open
- **Why:** the prototype is reviewed by video (T10).
- **What to do:** a soak in the lab with waves, a pilot, a gunner and
  sprites, `validate` every tick; two demo videos (a pilot and a guest
  gunner clearing a wave; a sprite gunner); the full suite; a
  `docs/test-time.md` row; `docs/vehicles.md`, `CRAWLER_TURRET.md`'s
  status, a "Play the crawler's turret" section in
  `todo/playtest-review.md`.
- **Where:** sim tests, `app/demos/`, docs.
- **Found in:** crawler turret design.
- **Tests:** the soak; the full suite.
- **Notes from TU.1:** the soak (`crawler_lab_tests::soak`) now boards a
  pilot, a gunner and two riders and checks the gunner's place every
  tick; it still widens only `waves[0]` (the old south-east wave, 1 every
  4 s), the other five waves run as written. The demo
  (`demos/crawler_lab.json`) still boards three guests; since TU.1 the
  first guest takes the gun, the others ride. The north-east waves set
  off at 25 s and reach the crawler's start about a minute later, after
  the demo has left it; its stomps are of the south-east wave as before
  (not re-rendered in TU.1). The lab's waves end at 137 s (24 ghouls):
  a longer soak or demo needs more, or widen a wave as the soak does.

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
- **Why:** the driver (and a crawler's riders, CR.4) stands at the seat in the idle pose, and nothing tells a player that G climbs in.
- **What to do:** add a seated pose to the rig while `AvatarState::driving` is set. Show "G: DRIVE" in reach of a free rover, as the repair prompt does.
- **Where:** `kestrel/app/src/rig.rs`, `scene.rs` (HUD prompts), `vehicles.rs`.
- **Found in:** the vehicle prototype.

## Draw rovers from a style skin

- **Status:** open
- **Why:** `app/src/vehicles.rs` builds a fixed mesh, ignoring the character's `view.skin` and the scene's style.
- **What to do:** let a style supply the `rover` skin (chassis and wheel parts), with the built-in mesh as a fallback. The crawler's rig (`app/actors/crawler.ron`) is built in the same way: a style's `crawler` skin could re-dress its pieces by finish.
- **Where:** `kestrel/app/src/vehicles.rs`, `style.rs`.
- **Found in:** the vehicle prototype.

## Crawler feet on dug or filled floors

- **Status:** open
- **Why:** a planted foot keeps the floor it landed on; digging or
  filling under it (the Tunnel Lab, the town) leaves it standing on air
  or in stone, and `validate_footholds` would fail.
- **What to do:** in `crawler::self_update`, re-read each planted foot's
  floor; a foot that lost it lifts (or the crawler stops), and one whose
  floor rose steps up if within `step`.
- **Where:** `kestrel/sim/src/settlement/crawler.rs`.
- **Found in:** CR.2 of the crawler.

## Predict a joined pilot's crawler feet

- **Status:** open
- **Why:** on a joined client the pilot's body is drawn predicted (ahead
  of the confirmed world) but its feet come from the confirmed ticks, so
  a swing starts a round trip late under a body already moving.
- **What to do:** keep the replay's vehicle (`AvatarReplay::vehicle`)
  per predicted tick in `prediction::replay` and `DrawnState`, and draw
  the local pilot's feet from it.
- **Where:** `kestrel/app/src/prediction.rs`, `vehicles.rs`.
- **Found in:** CR.3 of the crawler.

## Predicted climb-out reads the confirmed vehicle

- **Status:** open
- **Why:** `AvatarReplay::apply` builds its view from the confirmed
  settlement, so a predicted G off a moving vehicle picks its exit spot
  beside where the vehicle was confirmed, not beside the replay's copy;
  the sim then puts the elder beside where it is, and the view snaps by
  the distance it moved in a round trip.
- **What to do:** let the replay's view read its own vehicle copy (a
  `Cow` of `vehicles` with the copy in place) for the seated branch of
  `avatar::act`.
- **Where:** `kestrel/sim/src/settlement/prediction.rs`, `action.rs`
  (`View::vehicles`).
- **Found in:** CR.4 of the crawler.

## Riders climbing down together share a spot

- **Status:** open
- **Why:** climbing down reads no agents (`action::View` has none), so
  two elders pressing G on one tick land on the same spot when one's own
  spot is blocked and they fall back to the same one, overlapping until
  bodies separate. Each seat has its own first spot, so it's rare.
- **What to do:** pick the spot in stage 5 with the other elders known,
  or skip spots taken this tick (a message, as boarding is).
- **Where:** `kestrel/sim/src/settlement/crawler.rs` (`exit_spot`),
  `avatar.rs`.
- **Found in:** CR.4 of the crawler.

## Walking into a crawler's foot by hand

- **Status:** open
- **Why:** planned routes go round a crawler's planted feet (CR.5), but an
  elder walking with the stick, a ghoul walking its wave's route or
  steering straight at a quarry outside a volume walks through a foot's
  post.
- **What to do:** treat planted feet as discs in the avatar's step
  (`avatar::step`, as `Terrain::clear` blocks obstacles) and in a threat's
  `walk_route_at`/`steer`; or push bodies out of them in stage 2 from last
  tick's feet. A wave's route, planned once for all its enemies, could
  replan when it meets one.
- **Where:** `kestrel/sim/src/settlement/avatar.rs`, `chase.rs`,
  `combat.rs` (`follow_route`).
- **Found in:** CR.5 of the crawler.

## Predicted routes near a crawler's feet

- **Status:** open
- **Why:** an elder's click-to-move route goes round the crawlers' feet
  (CR.5) as `action::View::vehicles` has them. A joined client's replay
  plans with the confirmed vehicles, the sim with the tick's own, so a
  route planned near a walking crawler can differ by a foot's step and the
  drawn elder snaps when the confirmed route arrives. A shove isn't
  predicted either: the local elder is drawn on the spot until the
  confirmed shove reaches them.
- **What to do:** plan with the replay's own vehicle copy where it has one
  (as "Predicted climb-out reads the confirmed vehicle"), and let the
  replay apply a confirmed `FootLands` it can see coming (a swing whose
  landing spot is under the elder).
- **Where:** `kestrel/sim/src/settlement/prediction.rs`, `avatar.rs`
  (`set_intent`).
- **Found in:** CR.5 of the crawler.

## Crawler locks beside a gap's end

- **Status:** open
- **Why:** the rules that free a crawler held at a gap (CR.6) move it along
  its heading only. A path that turned it beside the pit's south end, with
  a foot stepped round the end onto the far rim and its resting spot over
  the pit, stalled at 167° of a 180° turn (seen before the last of those
  rules; not checked since). A foot held sideways of the heading may still
  lock it.
- **What to do:** a soak that drives at the gaps' ends and edges on
  purpose; if it locks, let the body step sideways toward the held feet
  (or lift the held foot back across when it can't reach its rest).
- **Where:** `kestrel/sim/src/settlement/crawler.rs` (`over_a_gap`,
  `fits`, `foothold`), `crawler_lab_tests.rs`.
- **Found in:** CR.6 of the crawler.

## Vehicles in the demo trace

- **Status:** open
- **Why:** a demo's frame trace (`settlement.simulation`) has agents and
  threats but no vehicles, so routing the crawler demo read the pilot's
  position for the crawler's and couldn't see its feet.
- **What to do:** add each vehicle's position, heading, speed and (for a
  crawler) feet to `settlement::snapshot`.
- **Where:** `kestrel/app/src/settlement.rs` (`snapshot`).
- **Found in:** CR.6 of the crawler.

## The Crawler Lab's HUD

- **Status:** open
- **Why:** the lab shows the settlement's base, wave and pack lines and a
  stock help line that don't apply to it (CR.1).
- **What to do:** hide them in scenes without a base, or give the lab its
  own help line (G boards, run trots).
- **Where:** `kestrel/app/src/settlement.rs`, `scene.rs`.
- **Found in:** CR.1 of the crawler.

## A crawler's gun without its ammo

- **Status:** open
- **Why:** a crawler's gun names its ammo by item id in the character
  file (`crawler.gun.defence.ammo`, TU.1 of the turret), which can't see
  a scenario's items, so nothing checks it: a scene with no `stone` item
  parks a crawler whose gun can never be loaded, silently.
- **What to do:** once the gun shoots (TU.2), warn when a scene parks a
  crawler whose ammo isn't one of its items, or let a scene's vehicle
  entry name the ammo.
- **Where:** `kestrel/sim/src/settlement/mod.rs` (vehicle setup),
  `content/src/character.rs` (`GunDef`).
- **Found in:** TU.1 of the crawler turret.

## Done

## TU.1 The gunner's seat and the lab

- **Status:** done (branch `kestrel-crawler-turret`, PR to come with TU.6)
- **Why:** T2, T3, T7, T10: somewhere to stand and something to shoot.
- **What to do:** the crawler's own `defence` section in content (copied
  from the ballista's: stone, cap 20, range 7 m, its period, damage and
  fireball) and its gun mount on the cab's roof; a gunner seat in `Seat`
  (the back rider place becomes it: pilot, gunner, two riders); G boards
  the cab, then the gun, then the riders. The lab: waves of ghouls on a
  timer from two sides, two sprites, a stone pile. Save format up.
- **Where:** `content/src/character.rs` (`CrawlerDef`),
  `content/data/character/crawler.ron`, `sim/src/settlement/avatar.rs`,
  `crawler.rs`, `vehicle.rs`, `app/assets/levels/crawler_lab*.ron`.
- **Found in:** crawler turret design.
- **Tests:** boarding order; a fifth can't; the gunner climbs down like a
  rider; saves round-trip; the lab loads with its waves and sprites.
- **Done:** content: `CrawlerDef::gun: Option<GunDef>` (`content/src/character.rs`),
  `GunDef { mount: Offset, height, defence: scenario::Defence }`, the
  scenario's own `Defence` reused (it gained `PartialEq, Eq` and a
  `Defence::valid` for its numbers, shared with building kinds; the
  ammo, an item id, isn't checked: a character can't know a scenario's
  items, and the save test parks a crawler in a scene whose
  only item is `none`, so a first cut that refused it at setup broke
  that; a gun without its item has nothing to load). `CrawlerDef` is no longer `Copy` (the defence holds
  its ammo's id), so callers clone or borrow it. `crawler.ron`'s `gun`:
  mount (along 1.55, across 0) on the cab's roof, 1.3 m over the deck
  (4.3 m over its feet); stone, cap 20, range 7 m, the ballista's
  period 0.7 s, damage 1, bolt speed 8 and fireball. Documented in
  `docs/character-definition.md`. Seats: `Seat::Gunner { vehicle }`
  appended (save format 50); `crawler::RIDERS` is two places (in front
  of the crates) and `crawler::GUNNER` the old back place (2.4 m back on
  the middle), the gunner apart from the gun as the design page drew it;
  `crawler::deck_spot(seat)` gives a seat's spot on the deck, and
  `VehicleState::seat_at`, `vehicle::seat` and both `exit_spot`s take
  that `Option<Offset>` instead of a place index. `free_seat`: the cab,
  then the gun (on a crawler with a `gun`, whichever side the elder
  reaches from), then the nearest rider's place; `gunner_of(vehicle)`.
  The gunner rides, can't steer and climbs down as a rider (their own
  first spot behind the box), and gets the camera pull and the
  rider's drawn offset (`vehicles::deck_offset`); the control protocol's
  `seat` reports `{"vehicle", "gunner": true}`. `validate_seats`: one
  elder a seat, a rider in one of two places, a gunner only on a crawler
  with a gun. Nothing shoots yet. No turret or store state in
  `VehicleState` (TU.2 adds it with its save bump). The lab
  (`crawler_lab_supply.ron`): six timed waves, three from the south-east
  corner (the old wave first, then four at 50 s and five at 100 s) and
  three from the north-east (three at 25 s, four at 75 s, five at 125 s),
  24 ghouls whose diagonals cross by the parked crawler; two sprites
  (`sprite_ash`, `sprite_fern`, at (-4.6, -3.6) and (-4.6, -5.4)) beside
  a stone pile (`lab_stone`, 60 stones, at (-3, -4.5)). Tests:
  `crawler_lab_tests` `a_pilot_a_gunner_and_two_riders_board_and_a_fifth_cannot`
  (five on one tick: cab, gun, two riders, nobody; places held every tick
  of a 5 s walk; gunner and riders steer nothing, with or without the
  pilot), `the_gun_is_taken_after_the_cab_and_again_when_freed` (the
  second aboard gets the gun from either side, freed it goes to the next;
  no gun, no gunner; `validate` refuses two at the gun, a rider in place
  2 and a gunner on a gunless crawler), `riders_climb_down_beside_it_on_the_ground`
  (the gunner behind), `riding_is_predicted_and_saved` (gunner and rider
  replays both match the sim for 300 ticks; both seats round-trip),
  `the_lab_has_waves_from_two_sides_sprites_and_stone` (six timed waves
  from two corners, at least 20 s apart, two sprites, 60 stones of the
  gun's ammo; over 240 s all 24 ghouls are sent and every one comes
  within 7 m of the parked crawler, `validate` every tick); the soak
  boards pilot, gunner and two riders; save
  `a_crawler_saved_with_riders_aboard_continues` with a gunner; content
  `the_crawler_has_its_legs_and_its_wheels_follow_them` (the gun's
  numbers against the town ballista's) and `crawlers_are_validated`;
  app `a_riders_offset_matches_their_seat` (riders and the gunner) and
  `the_rig_keeps_the_riders_places_clear` (the gunner's place too).
  Results: `content_test`, `save_test` and the whole `sim_test` (402
  passed, the four-minute soak included) pass; sim golden unchanged (no
  golden scene has a vehicle, and the crawler's rules bytes are added
  only when a character has one, as before). App `movement_test`
  filtered to `vehicles`, `crawler` and `control` (45 tests) passes. The
  existing crawler_lab tests needed no re-routing or re-timing: the new
  waves start at 25 s, after `the_crawler_is_parked_beside_the_spawn`
  counts three ghouls at 15 s. The demo wasn't re-rendered (TU.6).

## CR.6 Soak, tuning and demo

- **Status:** done (branch `kestrel-crawler`, PR to come)
- **Why:** the prototype is reviewed by video.
- **What to do:** a soak in the lab (driving randomly for minutes,
  `validate` every tick); tune speeds and the gait; a demo video crossing
  the terraces, the step, the trench and the moat, stopping at the wide
  pit and the cliff, stomping a ghoul and carrying riders; the full suite;
  a `docs/test-time.md` row; `docs/vehicles.md` and `CRAWLER.md`'s status.
- **Where:** sim tests, `app/demos/`, docs.
- **Found in:** crawler design.
- **Tests:** the soak; the full suite.
- **Done:** soak: `crawler_lab_tests::four_minutes_driving_with_riders_among_ghouls_validates`
  (and `half_an_hour_..._validates`, `#[ignore]`d, run once) on
  `crawler_lab_with_guests`: the pilot and three riders board, a fifth
  elder stays on the ground; the wave sends a ghoul every 4 s; the pilot
  walks, trots, turns in place either way, backs, heads for the lab's
  middle or steers at random, 1 to 5 s each; every 1.5 s a ghoul (or,
  every third time, the grounded elder) is put on a foot's landing spot a
  tick before it lands. Every tick: `validate`, `validate_footholds`, the
  pilot aboard, the riders in their places, every `Event::Hit` a stomp and
  `combat.kills` equal to the stomp kills. Four minutes: 461 lifts, 60 of
  60 ghouls put under a foot stomped, 29 shoves; half an hour: 3574 lifts,
  450 of 450, 221 shoves (about 2 s of test time). Tuning: no speed or gait
  change; the turn beside a pit and a lock astride the moat's edge, found
  routing the demo, are fixed (see "Crawler turns in place beside a pit",
  Done). Demo `crawler_lab.json` (3405 frames, `max_frames` 3600): the pilot and three guest
  riders (see "Guests in demo scripts", Done) board, walk south over the
  moat onto the island, turn and walk east off it, north-north-east and up
  the plateau's 1.5 m south face, trot west down the terraces (stomping the
  third ghoul about frame 1800 near (2.9, 4.2)) and across the trench,
  stop at the pit, turn north beside it, trot north along it and walk east
  into the mesa's cliff, where it stops; the pilot climbs down. Video and
  frames reviewed (`cr6_demo_small.mp4`, `cr6_*.png`). Docs:
  `docs/vehicles.md` (steering, footholds, the soak, the demo),
  `CRAWLER.md` (done; "What changed versus the design"),
  `todo/playtest-review.md` ("Play the crawler").
- **Notes from CR.5:** feet shove and stomp (`crawler::read_footfalls`,
  `Settlement::foot_lands`, a `FootLands` message, save format 49).
  Tuning knobs: `STOMP_DAMAGE` (4), `STOMP_KNOCKBACK` (4 m/s),
  `STOMP_STAGGER` (0.4 s) and `SHOVE_GAP` (0.1 m) in `crawler.rs`; the
  view's dust in `vehicles::footfalls` (`STOMP_DUST`, `SHOVE_DUST`,
  `DUST_RING`). The lab's ghouls walk the diagonal from the south-east
  corner past the parked crawler at 0.6 m/s; the demo as it stands
  stomps two of them on its north-east leg (frames 674 and 818, about
  (16.8, 11.9) and (20.1, 14.7)), and the third walks on to the corner.
  Retuning the gait or the route can lose them. A soak with ghouls under the feet now exercises the stomp: validate
  every tick then, and check `combat.kills`. `crawler_feet_tests` has the
  shove, stomp, among-bodies, seated and path tests; its
  `put_under_a_foot` puts anything on a landing spot a tick before.
- **Notes from CR.4:** riders need other players: a demo script drives
  only the local player (`control::protocol::Script`), so carrying riders
  in the demo needs "Guests in demo scripts" first, or a video from a
  hosted session with joined clients. The soak could seat riders with
  `CommandKind::Join` and `Board` in the sim (`crawler_lab_tests`'s
  `crawler_lab_with_guests` puts four guests beside it). Save format is
  48. `docs/test-time.md` has no CR.4 row; add it with CR.6's.
- **Notes from CR.3:** turning in place right after stopping at the pit
  stalls (the body won't turn: a front foot's turned spot is over the pit
  edge); the demo backs off 5 s first, and a test path that backed 3 s
  still stalled at 41°. See "Crawler turns in place beside a pit". The
  demo now climbs the plateau's south face (toward the camera; the north
  face can't be seen from the follow camera) and ends turning south and
  trotting, about 100 s. Tuning the gait or `lift` changes the view's
  arcs too; the
  rig's lengths (`actors/crawler.ron`) are checked against `reach` in
  `crawler::tests`.
- **Notes from CR.2:** `crawler_lab_tests::a_minute_driving_round_the_lab_validates`
  is the short soak (random sticks, `validate` and `validate_footholds`
  every tick); lengthen it. Tuning knobs: `WORKSPACE` (2 m) and `SETTLE`
  in `crawler.rs`, the `CrawlerDef` gaits, and `vehicle.turn`,
  `acceleration` and `lift` in `crawler.ron`. At a 25°/s turn or faster
  the walk can't keep its feet within 2 m of rest and stutters. From
  rest the body may stall a moment until the last leg of the wave steps.
  The moat (2 m) is crossable like the trench; the lab's too-wide cases
  are the 3 m pit and the 2 m cliff (no 1.6 m step can be built from
  0.5 m layers). The demo's route is open-loop (held sticks for set
  frames); retune it if speeds change.

## Crawler turns in place beside a pit

- **Status:** done (with CR.6, branch `kestrel-crawler`, PR to come)
- **Why:** stopped with its front feet at the 3 m pit's edge, the crawler
  turned in place (stick 90° off) doesn't turn at all, or turns a little
  and stops, and a boost doesn't free it: the feet's turned resting spots
  fall over the pit, no foothold is found, and the body can't turn on the
  feet it has.
- **What to do:** when the ideal spot has no foothold, try spots nearer
  the body (or the body's side away from the gap) before staying down;
  or let the turn walk the body back a little.
- **Where:** `kestrel/sim/src/settlement/crawler.rs` (`foothold`, `fits`).
- **Found in:** CR.3 of the crawler (the demo's turn at the pit).
- **Done:** two rules in `crawler.rs`. Footholds: with no foothold round
  the ideal spot, `foothold` looks round the foot's resting spot (at
  landing, then now) for one that brings it at least `SETTLE` nearer its
  rest, so a foot beside a pit steps in toward the body instead of staying
  put (and doesn't shuffle along the edge). Steering: stopped (speed not
  positive) with a planted foot's resting spot over a gap
  (`over_a_gap`: no floor within `step`) and the stick more than about 7°
  off the heading (`TURN_NEAR`), it moves at half its reverse speed toward
  the held feet as it turns: back off a pit ahead, on from a moat behind.
  And `fits` lets a foot that came down past `WORKSPACE` come back in (it
  only refused the body moving it further), which had locked the body.
  Tests: `the_crawler_turns_in_place_beside_the_pit` (stopped at the pit,
  it turns south or north within 12 s, backing off under 3 m, then walks
  away along the edge; no foot ever down the pit) and
  `the_crawler_walks_on_after_turning_by_the_moat` (CR.6's demo route,
  which had locked it astride the moat's edge for good).

## Guests in demo scripts

- **Status:** done (with CR.6, branch `kestrel-crawler`, PR to come)
- **Why:** a demo script (`app/demos/*.json`) drives only the local
  player, so a demo can't show a crawler carrying riders (C9) or
  anything else with several elders.
- **What to do:** a script op that joins a scripted guest (a
  `CommandKind::Join` under a new `PlayerId`) and ops that send that
  guest's commands (walk, board), applied through the session like the
  local player's.
- **Where:** `kestrel/app/src/control/protocol.rs`, `control/mod.rs`,
  `lockstep.rs`.
- **Found in:** CR.4 of the crawler.
- **Done:** three script ops (`control::protocol::Command`):
  `guest_join { guest }` joins player `guest` (1 to 8), `guest_walk
  { guest, position }` sends their elder to a point along a planned route,
  `guest_board { guest }` presses G for them. A host files them through
  `net::Host::queue_as` (`session::Session::queue_as`,
  `SessionService::command_as`) in order with its own commands; a client
  refuses them ("Guests need a host"). The crawler demo seats three riders
  with them. No other script verbs for guests yet (strike, dig, carry).

## CR.5 Feet and bodies

- **Status:** done (branch `kestrel-crawler`, PR to come with CR.6)
- **Why:** C10: footfalls matter.
- **What to do:** a landing foot shoves bodies off its spot (a message,
  applied in stage 5 like knockback) and a ghoul under it takes heavy
  damage. Bodies path round planted feet. Ghouls don't attack it yet.
- **Where:** `sim/src/settlement/vehicle.rs`, `combat.rs`, `messages.rs`.
- **Found in:** crawler design.
- **Tests:** a shove; a stomp; never a foot on an elder or sprite.
- **Done:** a foot coming down shoves and stomps (C10).
  `crawler::self_update` reports which feet came down this tick;
  `Settlement::vehicles_self_update` keeps them in `Settlement::landings`
  (not saved, emptied in housekeeping); stage 3
  (`Settlement::read_footfalls`) lists under each foot the agents (alive,
  not seated, not inside a building) and threats within the foot's 0.4 m
  (`FOOT_RADIUS`) plus their radius whose feet are within 1 m of its floor
  (any height outside a volume), as a `FootLands` message, a new late
  queue (hashed only when not empty), so the save format is 49 (the
  pending messages' queues). Stage 5 (`Settlement::foot_lands`, after the
  threats' swings, before breaches) checks each again: an agent is put
  just clear of the foot, 0.1 m past touching (`SHOVE_GAP`), away from it
  (from the crawler's middle if it stood on the spot itself), or turned by
  eighths of a turn, in the arena, clear of discs, rock and every foot, on
  a floor a walk from where it stood reaches (`Terrain::walk_at`), with
  its feet's height set there; an elder shoved stops digging; with
  nowhere to go it stays. A threat loses `STOMP_DAMAGE` (4, armor or not:
  more than a ghoul's 2 or 3), dying at 0 as a strike's kill
  (`Event::Hit` with the new `Impact::Stomp`, `combat.kills`), else thrown
  off at 4 m/s and staggered 0.4 s. Then `Event::Footfall { shoved,
  stomped }` for the view. Seated elders are skipped in stage 3 and stage
  5; a foot still never picks a foothold on a standing elder, sprite or
  hauler (`foothold`, unchanged). Paths: `crawler::feet_in_the_way` gives
  a disc at every foot's spot (planted, or a swing's landing) level with
  the walker, but one it already touches; an elder's click-to-move route
  (`set_intent`, on foot; not a foot on the target), a ghoul's chase
  (straight check and plan) and its route home when it replans
  (`route_home_at` takes the discs; a wave's shared route doesn't), and
  haulers' and sprites' traffic detours (`move_worker`'s nearby discs) go
  round them. The body is overhead and blocks nothing. Ghouls don't
  attack it. View (`vehicles::footfalls`): a stone-dust burst under every
  foot that comes down and a step's sound; a shove adds a ring of three
  bursts, a stomp a ring of six and the landing cue; the ghoul bleeds and
  falls as any kill (`Impact::Stomp` in `legacy`). Tests
  (`crawler_feet_tests`): a guest elder put on a landing spot a tick
  before is shoved just clear, away, on the floor, `Footfall` (1, 0); a
  2-hit-point ghoul there dies (`Impact::Stomp`, a kill), one with 10
  lives at 6, staggered and thrown away from the foot; both the same on 1
  and 4 threads and across a save made the tick before; walking south
  among four elders and a sprite standing where a free walk's feet came
  down, no foot lands on one and none is shoved, and it walks on; a foot
  forced under the pilot and each rider lists none of them and a message
  naming them moves nobody; an elder sent straight across a parked foot
  walks round it (never nearer than touching) and gets there. Existing
  crawler, prediction and determinism tests pass unchanged; the whole
  `sim_test` passed with the golden unchanged (no golden scene has a
  crawler), and `movement_test`, `save_test` and `determinism_test`
  passed. The demo is unchanged: as it stands it stomps two of the wave's
  ghouls on its north-east leg (frames 674 and 818); screenshots
  `cr5_stomp_*.png` from it. See "Walking into a crawler's foot by hand"
  and "Predicted routes near a crawler's feet".
- **Notes from CR.4:** riders and the pilot are seated
  (`Seat::Vehicle`, `Seat::Rider`) and so out of `crawler::standing`: a
  stomp or shove must skip them too. An elder climbing down stands clear
  of every foot's spot and swing landing (`crawler::exit_spot`) but not
  of a foot that lifts later; the foothold then avoids them as a standing
  body, as before.
- **Notes from CR.3:** a landing's view is `crawler::swing_point` at
  `t = 1`: the sole comes down the last 40% of the swing fast, so a stomp
  effect (dust, a thud) fits the tick the `Swing` becomes planted. The
  drawn foot's pad is 0.7 m across (the rig's foot pieces).
- **Notes from CR.2:** `foothold` already refuses spots on standing
  agents (`Standing`, radius + 0.4 m, within 1 m of the floor) and leaves
  threats out of that list, so a stomp is a landing (a `Swing` becoming
  planted in `crawler::self_update`) on a threat's spot. Stage 2 can't
  write threats: emit a message there or read the landings in stage 3.
  The bodies list is every agent, scanned per candidate spot; a town
  with many agents may want a grid.
- **Notes from CR.1:** the lab's ghouls are a timed wave of three
  `ghoul_sacred_tree` (2 hit points, chase within 3 m, leash 5 m) from the
  south-east corner to the north-west one (`crawler_lab_supply.ron`);
  each breaches there and is gone, so a stomp test should catch one on
  its way, or add a wave.

## CR.4 Riders

- **Status:** done (branch `kestrel-crawler`, PR to come with CR.6)
- **Why:** C9: a pilot and three riders.
- **What to do:** four seats (cab and deck); G boards the nearest free one
  (the cab first); riders ride seated and can't steer; G climbs down to a
  free floor spot beside it. Prediction and saves as the rover's driver.
- **Where:** `sim/src/settlement/vehicle.rs`, `avatar.rs`, the view.
- **Found in:** crawler design.
- **Tests:** four players board in order; a fifth can't; climbing down
  beside it, never into a pit.
- **Notes from CR.3:** the deck's top is the rig's y = 0
  (`actors/crawler.ron`): the cab covers z 0.8 to 2.3 m and stands 1.3 m
  over it, the helm is at z 0.6, the cargo deck (crates, rail) z -0.4 to
  -2.9. Seats for riders want clear deck: around (±0.7, -0.3) or on the
  crates; move pieces in the rig if needed. The camera pull keys on the
  drawn avatar's seat being a crawler's (`vehicles::draw`); decide whether
  riders get it too.
- **Notes from CR.2:** riders seated on the deck stay out of
  `crawler::standing` (it skips any avatar with a seat). Climbing down
  must not put an elder under a foot about to land: the feet avoid
  standing bodies only when they pick a foothold, at lift.
- **Notes from CR.1:** the one seat today is the rover's: boarding reaches
  `body.radius` (1.5 m, half the width) + the elder's + 0.8 m from the
  middle, so only beside the long sides, not at the ends; the lab parks
  it 2.5 m from the spawn for that. `exit_spot` puts the pilot
  `radius + 0.47` m out: just clear of the sides, but its behind and in
  front fallbacks land under the 6 m body. Make the seats and the exit
  spots follow the body's box.
- **Done:** four seats: the pilot's (`Seat::Vehicle`, at the helm on
  the deck's middle, as before) and three riders' places
  (`Seat::Rider { vehicle, place }`, appended to `Seat`, save format 48),
  `crawler::RIDERS` from the middle: (-0.75, -0.7) and (-0.75, 0.7)
  (along, across) in front of the crates, and (-2.4, 0) behind them.
  `Settlement::board` reaches a crawler within `BOARD_REACH` (0.8 m) past
  the elder's radius of its 6 x 3 m box (`crawler::box_distance_squared`),
  from any side or end (the rover keeps its radius), and takes its free
  seat (`free_seat`): the cab first, else the free place nearest the
  elder (lower place on a tie); stage 5 seats in command order, so five
  on one tick give a pilot, three riders and nobody. Housekeeping keeps
  riders in place (`VehicleState::seat_at`), facing the crawler's way at
  the deck's height; a rider's commands go through `avatar::act`'s seated
  branch, so movement does nothing (only `Seat::vehicle`, the pilot's,
  drives), and a crawler with riders but no pilot stands still. A rider
  doesn't move to the cab when the pilot leaves: they climb down and
  board again. Climbing down (`crawler::exit_spot`, for pilot and riders;
  the rover's `exit_spot` is unchanged): around the box, 0.15 m past it
  beyond the elder's radius; each seat's own spot first, so four climbing
  down on one tick land apart (the pilot's beside the middle on the left,
  a front rider's 1.5 m back on their side, the back rider's behind),
  then the seat's side at the middle, 1.5 m back and 1.5 m forward, the
  other side, behind and in front; in the arena and `Terrain::clear`,
  at least foot radius + elder radius + 0.2 m from every foot's spot and every swing's
  landing spot, and its floor (`footprint_near`) within 0.6 m of the
  highest floor its feet stand on, so never into the trench even with a
  foot down in it (a first cut used the nearest foot's floor and let a
  pilot down into the trench beside a foot in it). None: the elder stays
  aboard. `Settlement::validate` checks every seat holds one elder and a
  rider sits on a crawler in a real place. Prediction: `AvatarReplay`
  copies the vehicle the avatar is aboard (`Seat::aboard`), pilot or
  rider; a rider's runs on its confirmed controls, as if the pilot held
  them (exact until the pilot's next command arrives), and `vehicle()`
  returns it for riders too. View: the rig's crates moved to the cargo
  deck's middle (z -1.2 to -2.0, a small one at the back right) to clear
  the places; a local rider's crawler is drawn from the rider's drawn
  transform less their place (`vehicles::rider_offset`), as the pilot's
  is; the control protocol's `seat` reports `{"vehicle", "rider"}` for a
  rider. Camera: riders get the pull-back too (`PILOT_PULL` while aboard
  a crawler, pilot or rider): they see what it steps over. Riders stand in
  the idle pose, like the pilot (see "Show a driver seated, and a
  boarding prompt"). Tests: `crawler_lab_tests`
  `a_pilot_and_three_riders_board_and_a_fifth_cannot` (five on one tick;
  riders in place every tick of a 5 s walk; their sticks steer nothing,
  with the pilot aboard or gone), `riders_climb_down_beside_it_on_the_ground`
  (outside the box, on the ground, clear of the feet, the right-hand rider
  on the right, the back one behind, all four apart), `climbing_down_never_goes_into_a_pit_or_under_a_foot`
  (parked astride the trench's edge, a foot in it or past it: pilot and
  left rider down on the right; in the sim too; a swing's landing spot
  passed over), `riding_is_predicted_and_saved` (a rider's replay equals
  the sim over 300 ticks while the pilot holds the stick; a postcard
  round trip with riders continues with equal hashes); save
  `a_crawler_saved_with_riders_aboard_continues` (`write`/`read`, same
  bytes again, same steps after); app
  `crawler::tests::the_rig_keeps_the_riders_places_clear` and
  `vehicles::tests::a_riders_offset_matches_their_seat`. Sim golden
  unchanged (no golden scene has a vehicle; the whole `sim_test` passed). No
  screenshots: demo scripts drive only the local player, so a demo can't
  seat riders (see "Guests in demo scripts").

## CR.3 Legs, IK and the camera

- **Status:** done (branch `kestrel-crawler`, PR to come with CR.6)
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
- **Done:** the rig is data, `app/actors/crawler.ron`, read by
  `app/src/crawler.rs` (`Rig`, built in, compiled with the app): the
  body's pieces (boxes and cylinders, each `turn`ed by degrees, in the
  deck's frame) and one leg's `hip` pieces and `thigh`, `shin` and `foot`
  segments, each a length and pieces in its own frame (y along it, z to
  the knee's side); the other legs mirror it, at the sim's `hip` offsets.
  Finishes are the Brasswood palette's (timber, dark timber, brass, iron
  darkened, leather, canvas, cloth, a lamp's glow). The body: a timber
  hull hanging no lower than `BELLY`, a brass-edged plank deck, a prow
  lamp, a timber cab with a window band, brass roof and iron stack on the
  front half, the helm on the middle where the pilot stands, and a railed
  cargo deck of crates under canvas behind. Legs: thigh 1.8 m, shin 3.4
  m, foot 0.45 m (thigh and shin reach 5.2 m, past `reach` 4.5 m by the
  soft zone; the shin nearly twice the thigh keeps the knee high).
  `crawler::solve`: hip (on the deck) to the ankle `foot` over the sole,
  the knee in the plane through hip and ankle on the side of up and a
  little outward (`KNEE_OUT`), so it never flips; out of reach the leg
  eases straight over `SOFT_REACH` (0.25 m) instead of snapping, and a too
  near ankle is drawn as far as the folded leg goes. `swing_point`: the
  sole rises in the first 40% of the swing to `lift` over the higher of
  its two floors, crosses between 20% and 80% (smoothstep) and comes down
  in the last 40%. `vehicles::draw_crawler` replaces the placeholder: the
  body level at the sim's height (interpolated, or the pilot's drawn
  transform for the local pilot as before; no bob, it read fine without),
  each foot's sole interpolated between the last two ticks' states
  (`foot_point` on each), and a foot whose spot jumps (faster than
  `FOOT_JUMP`, 30 m/s: the sim aimed a waiting swing again) glides there,
  the gap halving every 0.06 s. Camera: `Vehicles::pull` eases (1.6/s)
  toward `PILOT_PULL` (1.4) while the drawn local avatar's seat is a
  crawler and back to 1 after; `effects::camera_feedback` scales the
  orthographic view's height by it. The pilot reads on the deck behind
  the cab. Demo `crawler_lab.json`: it now climbs the plateau's south
  face (the north face is hidden from the follow camera), and after
  stopping at the pit it backs off 5 s, turns in place south and trots
  south along the strip between trench and pit. Tests: `crawler::tests` (the rig loads, validates,
  reaches past `reach`, hangs no lower than the belly, and rejects bad
  lengths and pieces; the solve's sole on its target, upright, the knee
  0.5 m over the hip and outward at rest, easing past reach without a
  jump; in the lab, up and down the step, west across the trench,
  trotting, and turning in place by the spawn, every planted foot reached
  within 1 mm with the knee off the hip–ankle line toward up and out, the
  knee over the hip itself for 98% of planted feet (not for a far rear
  foot under a body raised by its front feet on the step), and each
  swing's arc above the volume's top floor at 99 samples), `vehicles::tests`
  (the pull's easing; in the Crawler Lab app, the camera pulls back to
  `PILOT_PULL` piloting and eases in after climbing down).
- **Notes from CR.2:** the sim's feet are `VehicleState::legs`
  (`sim::settlement::crawler`): each foot's `spot` and `floor` (a tile's
  level floor height), and while it swings its `Swing` (`to`, `floor`,
  `progress` of the gait's `swing` ticks; at `progress == swing` it waits
  for reach). Draw a swing from `spot` to `to` with `gait.lift` at the top
  (the placeholder `draw_crawler` hops along a sine). The body is level at
  `VehicleState::height` (its feet's level; the deck `deck` over it, the
  hips at the deck); the sim eases it at `lift` (2.5 m/s), so ease it in
  the view too. Feet aren't interpolated for other players' crawlers yet.
  Ground under the body's box stays under its belly, 1.2 m under the deck
  (`crawler::BELLY`); a rig hanging lower would clip the mesa's edge.
- **Notes from CR.1:** `app/src/vehicles.rs` draws a crawler (a vehicle
  whose character has a `crawler`) as a PLACEHOLDER: a timber box its
  length and width hanging under the deck, a brass cab and rail on it,
  and brass posts under the hips stretched to the ground
  (`spawn_crawler`). Replace it; the wheel ground there now reads the
  volume's floor as the sim does. The demo is `crawler_lab_demo`
  (`app/demos/crawler_lab.json`).

## CR.2 Gait and footholds in the sim

- **Status:** done (branch `kestrel-crawler`, PR to come with CR.6)
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
- **Done:** `sim/src/settlement/crawler.rs`: `VehicleState::legs`
  (`Legs`: four `Foot`s, each its spot and floor and a `Swing` to a spot
  and floor with its progress, and the gait, its direction and clock),
  set for a vehicle whose character has a `crawler` (`VehicleState::new`
  parks its feet at `stance`, the clock a tick before the walk's first
  lift); `vehicle::self_update_among` hands a crawler to
  `crawler::self_update`, so `drive`, boarding and the seat are the
  rover's. The stick walks within 60° of the heading, turns in place to
  120° and backs up past it. Gait: the walk's wave (rear left, front
  left, rear right, front right; four slices of `cycle`) and the trot's
  diagonal pairs (two), each foot lifting as its slice starts (as it
  ends, backing up: the wave reversed) for `swing` ticks; gait and
  direction change only with every foot down (the clock maps by phase);
  the clock runs while moving or a foot is out of place (more than 0.25 m
  from where it would step) and holds while a swung foot waits for reach.
  Footholds (`foothold`): the grid spot (0.25 m) nearest the ideal (its
  `stance` spot as the body will be half a stance after landing, at the
  speed it's making for) within 2 m (`WORKSPACE`) of its resting spot both
  now and at landing, in horizontal reach of its hip at landing and with
  a body height the other feet allow, on its tile's floor within `step` of
  the floor it lifted from (nearest, no ceiling under the hip), with at
  most `gap` of the way over tiles with no floor within `step` (measured
  tile by tile, exactly) and no ground over the swing's top (`lift`), and
  clear of every standing agent (`crawler::standing`: alive, not seated,
  not inside; threats aren't, for CR.5's stomp). None: the foot stays
  down. The body moves (and turns) only to poses where each planted foot
  is in 3D reach of its hip and at least 1 m (`MIN_RISE`) under it at the
  body's height, within `WORKSPACE` of its resting spot, and no ground
  under the body's box is over its belly (1.2 m under the deck,
  `BELLY`); otherwise it loses that speed. So a pit too wide holds a front
  foot back until the body stops over the edge, the mesa's cliff stops
  the body at its face, and backing off walks away. Body: level, no pitch
  or roll (the placeholder draws it flat); its height eases at `lift`
  (2.5 m/s now) toward the mean of its feet's floors (swinging feet's
  landing floors), clamped low enough for feet coming down to reach and
  high enough for its belly: walking off the 1.5 m step it sinks about
  1 m with its front feet down. Tuning moved: `acceleration` 1.0 (from
  1.5: the last foot of the wave steps 1.8 s after starting), `turn`
  20°/s (from 35: turning in place keeps the feet within 2 m of rest),
  `lift` 2.5. Prediction: `AvatarReplay` runs the driven crawler with the
  confirmed settlement's standing bodies (exact while no body moves near
  a foot). Save format 47. `Settlement::validate` checks every planted
  foot in reach and under its hip and the feet up (one at a walk, a
  diagonal pair at a trot); `Settlement::validate_footholds(terrain)`
  checks they stand on floors (validate has no terrain). The view
  (`vehicles::draw_crawler`) stands a post on each foot up to the deck, a
  swinging foot hopping along its swing; the body box is 1.2 m deep, level
  at the sim's height. Demo `crawler_lab.json` boards, turns, walks
  north-east, up the plateau's north face, west down the terraces and
  across the trench to the pit. Tests (`crawler_lab_tests`): the walk's
  wave with one foot up and the trot's pairs, at full speed; up and down
  the step (front feet up with rear down; sinking 0.5 m or more); across
  the trench and stopped at the pit, then backing off; stopped at the
  cliff; never on a body; turning in place (the body still, 150°+ in
  8 s); backing on the reversed wave; prediction replay; determinism
  across threads and a save made mid-step; a minute's random driving
  validating every tick. Sim golden and headless hashes unchanged (no
  golden scene has a vehicle).
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
