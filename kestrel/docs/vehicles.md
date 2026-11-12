# Vehicles

A vehicle is a character a player climbs into and drives. The dig lab parks a mining **rover** beside the spawn: walk up to it and press **G** (or click the left stick) to climb in, steer with the movement controls, hold run to boost, and press G again to climb out. The rover rolls up the mine's quarter-meter terraces and out of a one-layer pit, drops off ledges, and stops at a mesa's cliff.

```mermaid
flowchart LR
  Def[character/rover.ron<br/>brain: Vehicle] --> Table[Settlement::vehicles]
  Scene[Scene vehicles list] --> Table
  Cmd[Board command] --> Stage5[Stage 5: board<br/>first in command order]
  Stage5 --> Link[AvatarState::driving]
  Move[MoveAvatar] --> Drive[vehicle::drive<br/>stage 1, the driver's alone]
  Drive --> Step[Stage 2: vehicle::self_update]
  Step --> Seat[Housekeeping: driver to seat]
  Step --> View[app vehicles.rs draws it]
```

## How a vehicle fits the character framework

| Part | What a vehicle uses |
| --- | --- |
| Definition | A character file with `brain: Vehicle`, a `body` radius, `movement` speeds (walk is top speed, run is boost), and a `vehicle` tuning section. See [character definitions](character-definition.md). |
| State | Its own table, `Settlement::vehicles`, as threats have theirs. Each `VehicleState` stores its `CharacterIx`, so definitions stay out of state. An index is stable for the world's life. |
| Who drives | `AvatarState::driving` on the driver is the only link. A vehicle with nobody in it lets go of its controls. Leaving, dying and respawning end the link with no extra bookkeeping. |
| Commands | Movement commands are unchanged: while the link is set, `vehicle::drive` turns them into the vehicle's controls. Only `Board` is new. |
| Ownership | The vehicle is lent to its driver. Only the driver writes its controls, in stage 1, so they need no message. Boarding is a message (`Board`), because two players may reach for one seat on the same tick, and stage 5 seats the first in command order. |
| Prediction | `AvatarReplay` copies the driven vehicle and runs it with the driver, so driving is predicted with the simulation's own code. Climbing in waits for stage 5 and isn't predicted. Climbing out is. |
| Saves and hashes | Vehicles are saved and hashed with the settlement (save format 22), and character indices are remapped by name on load. |

## Driving rules

`sim/src/settlement/vehicle.rs` drives in `Fx`, like everything else in the simulation:

- **Steering:** the stick is a direction on the ground. The rover turns toward it and speeds up toward the stick's length times top speed. If the stick points more than 120° from the heading, it backs up, swinging the rear toward the stick. Turning needs rolling: full rate from a third of top speed.
- **Ground:** four wheels sample `Terrain::ground`. A move or turn that would lift any wheel more than `climb` (0.55 m for the rover), or put one over the arena edge, doesn't happen, and the rover loses that speed. Drops are fine.
- **Height:** the chassis rests on the wheels' mean height. It rises toward it at `lift` and falls at `gravity` when the ground drops away.
- **Collision:** discs (obstacles, cover) and rock columns push it out like an avatar's body, at the rover's `body.radius`.

## The crawler

The Crawler Lab parks a **crawler** (`character/crawler.ron`, design in [`CRAWLER.md`](../CRAWLER.md)): a vehicle with a `crawler` section that walks on four feet instead of rolling. It boards, drives and seats its pilot as the rover does; `sim/src/settlement/crawler.rs` moves it, in `Fx`:

- **State:** `VehicleState::legs` holds four feet (where each stands on which floor, or where it swings to) and the gait's clock (save format 47).
- **Gait:** a wave walk, one leg at a time with three down, and a diagonal trot on boost. Each leg lifts at the start of its slice of the cycle; backing up runs the wave the other way. The stick walks within 60° of the heading, turns in place up to 120° and backs up past that. Turning in place steps the feet sideways round the middle. Stopped with a foot's resting spot over a gap (at the pit's edge) and steered more than about 7° away, it backs off at half its reverse speed as it turns, until its feet have floor to step round on.
- **Footholds:** a lifting foot lands on the floor nearest its resting spot ahead of the body, within reach of its hip, at most 1.5 m up or down, with at most 2 m of gap on the way, and never on an elder, sprite or hauler. With none there (the spot ahead is over a pit or a moat), it looks round its resting spot instead, for a spot that brings it nearer its rest. With none, it stays down, and the body stops where that foot can't reach any further: a planted foot strays at most 2 m from its resting spot, and one that came down further only comes back in.
- **Body:** level, its height easing to its feet's mean floor, sinking when a foot far below needs it. Ground under its box higher than its belly (1.2 m under the deck) stops it, as the mesa's 2 m cliff does.

### Pilot, gunner and riders

A crawler has four seats: the pilot's at the helm on the deck's middle, the gunner's behind the crates (`crawler::GUNNER`, 2.4 m back on the middle), and two riders' places on the cargo deck in front of the crates (`crawler::RIDERS`: 0.75 m back and 0.7 m to each side). A rider is `Seat::Rider { vehicle, place }` (save format 48), the gunner `Seat::Gunner { vehicle }` (format 50, TU.1 of the [turret](../CRAWLER_TURRET.md)); the pilot is `Seat::Vehicle` as a rover's driver. Only a crawler with a `gun` in its definition (`crawler.gun`: its mount on the cab's roof, about 4.8 m up, and a ballista's `defence` section: stone, a store of 20, 7 m range) has the gunner's seat; the gunner stands apart from the gun, which shoots (see [The gun](#the-gun)).

- **Boarding:** G reaches a crawler within 0.8 m (`BOARD_REACH`) past the elder's body of its 6 x 3 m box, not a radius, so from any side or end. It takes the cab if it's free, else the gun if no elder mans it, else the free rider's place nearest the elder. A sprite at the gun gives it up to the elder (see [The crew](#the-crew)). Stage 5 seats players in command order, so on one tick the first gets the cab, the second the gun and the next two the deck; a fifth gets nothing.
- **Riding:** housekeeping keeps the gunner and each rider in their place, facing the crawler's way, at the deck's height. Their movement does nothing: only the pilot steers, and a crawler whose pilot climbed down stands still with them aboard. Nobody moves up to the cab or the gun when it's freed: they climb down and board again. Everyone seated is out of the feet's way (`crawler::standing` skips anyone seated).
- **Climbing down:** G again puts the elder by the box, 0.15 m past it beyond their radius: their seat's own spot first, so four climbing down together land apart (the pilot's beside the middle on the left, a rider's beside the back on their side, the gunner's behind), then their seat's side level with the middle, toward the back and the front, then the other side, then behind and in front. A spot must be in the arena, clear of discs and rock, clear of every foot's spot and of the spot each swinging foot will land on, and on a floor within 0.6 m of the highest floor its feet stand on, so never down a trench or a pit, even with a foot down in it. With none, the elder stays aboard.
- **Prediction:** a rider's or the gunner's client runs the crawler alongside them on its confirmed controls, as if the pilot held their stick: exact until the pilot's next command reaches it. Climbing down is predicted, as the driver's.
- **Checks:** `Settlement::validate` checks every seat holds one elder at most, a rider sits in one of the two places and a gunner on a crawler with a gun.

### The gun

The crawler's gun is the town's ballista on the cab's roof (TU.2 of the [turret](../CRAWLER_TURRET.md), `sim/src/settlement/gun.rs`):

- **State:** `VehicleState::gun` (save format 51) holds its turret (when it may shoot next, where and when it last did) and its ammo store: the gun's ammo item only, up to its cap (20 stones), restocked by hand (see Ammo). A scene that parks a crawler with a gun gives it a full store, counted as supplied, if the scenario has its ammo item (the lab does); without the item there is no store and nothing to fire. Its numbers are resolved from the definition each time, as a ballista kind's are (`buildings::defence`).
- **The mount:** shots and the fireball leave from the gun's mount on the body (`GunDef::mount`), at the deck's height plus the gun's (about 4.8 m, the drawn gun's bow, half a meter over the cab's roof), where the crawler is on that tick. They go through the ballista's helpers: the target pick, the launch (a stone each, spent in the balances) and the fireball (`fire::Mount` names what threw one). The range is on the ground (7 m), and in a volume the gun reaches threats on any floor in sight below the mount, not only the one under it (`combat::below_gun`: feet under the mount's height, on the top floor where they stand, so not in a tunnel): from the plateau onto the ground, or from beside the trench down into it. A ballista keeps to the top floor at its feet.
- **Ammo** (TU.3, T6): an elder carrying stone interacts beside the crawler (`InteractTarget::Gun`, reached from the box, not a point) and delivers as much as fits, the rest staying in the bundle, as at a ballista; nothing comes back out. Only while it stands still (`crawler::walking`: no speed, no fall, the pilot's stick at rest and every foot down): while it walks the target is a readout ("WAIT: THE CRAWLER IS WALKING", `Interaction::Walking`) and the action is refused as `BundleRefusal::Walking`. The elder steps in to 0.4 m past the box, and units move while it is within the boarding reach of the box, on its floor, and the crawler still (`interact::beside_gun`); a delivery under way pauses while the crawler walks and goes on when it stops beside the elder again. The store's reservations are a holder's like any other (`validate_guns`). The prompt is "LOAD N STONE (GUN 17/20)": since TU.5 the app's interact (E, controller A) runs in any settlement with a crawler's gun as in a town, so in the lab E takes a bundle from the stone pile and loads the gun by hand.
- **A sprite at the gun** (`SpriteState::gun`, put there by the crew job, see [The crew](#the-crew), or by `Settlement::man_gun` in tests) stands at the gunner's place, does nothing of its own and shoots the nearest threat in range of the mount on its own, once a period, as a staffed ballista does (`Event::GunShot`).
- **An elder at the gun** (`Seat::Gunner`) aims as at a possessed ballista: the aim is a reticle offset from the mount, cut to the 7 m range, so it moves with the crawler; `Attack` fires there once a period (`GunnerFire`, which carries the offset, so the bolt goes from where the mount is when it fires), and the Burst key throws the fireball on the elder's own fireball cooldown, for no stone. An empty store says `NoAmmo`, a shot within the period `Reloading`; the pilot and riders get `NotAtGun` and their Burst is refused.
- **Shelter:** nothing hits anyone aboard, pilot, rider or gunner, elder or sprite, and ghouls don't chase them or the crawler.
- **Prediction:** a gunner's aim is predicted with the rest of their avatar; their shots and fireballs show when the world confirms them, as a possessing elder's do.
- **View** (TU.5, bigger in TU.6): the gun is a swivel ballista on the cab's roof (the rig's `gun` pieces: an iron turntable, a dark timber stock and bow about 1.5 m across, a pale canvas mantlet trimmed in brass), its bow at the mount, turning, eased, to its aim: the local gunner's reticle while they aim, another elder gunner's confirmed reticle, else the last shot's (a sprite's or an elder's), else where it last pointed. Bolts and the fireball are drawn from the sim's projectiles and fireballs, so they leave the mount.
- **Controls** (TU.5, T5): an elder at the gun gets a possessed ballista's reticle (`input::town`, `Aimer::Gun`): a ring of the gun's 7 m round the mount on the crawler's floor and a cross, under the mouse or moved by the right stick, an offset from the mount, so it rides with the crawler; it starts half the range ahead. Left mouse or RT, held, fires at the period through `gun_fire_check` ("No ammo: bring stone" over the elder on a press with the store empty); R or RB throws the fireball. The pilot and riders get no reticle, and a seated elder's click never swings a weapon.
- **Readout:** the lab's HUD line reads the store and who mans the gun ("CRAWLER LAB / GUN 17/20 STONE (ASH) / 3 GHOULS STOPPED, 2 OUT / PACK 0/5"), with the fireball's cooldown while the local elder mans it; the gun's prompt says the count too.

### The crew

A sprite crews a crawler's gun on the job `SpriteJob::Crew` (TU.4 of the [turret](../CRAWLER_TURRET.md), T3, T4; `sim/src/settlement/crew.rs` and `sprites.rs`), set by `SetSpriteJob` as a town's jobs are:

- **Climbing up:** the sprite picks the nearest crawler (by its box) with a gun nobody else mans and, while it stands still (`crawler::walking`), walks to the free spot beside its box nearest it (`crawler::board_spot`: the eight climbing-down spots, by their rules) and plans `[GoTo, Crew]`. Beside the box (`interact::beside_gun`: within the boarding reach, on its floor, the crawler still) stage 3 asks (`Messages::crew_gun`) and stage 5, last, after the elders' boarding, seats the first to ask in agent order (`crew_guns`, through `man_gun`). So an elder pressing G on the same tick wins the gun.
- **A walking crawler waits for nobody:** while it walks, or while someone else mans its gun, a crew sprite on its way stops (its route dropped) and waits, planning again every retry (`Step::Wait`); it never climbs aboard a walking crawler.
- **Bumped (T3):** an elder boarding takes the gun from a sprite there. The sprite moves to the free rider's place with the lowest index (`SpriteState::rider`, save format 52), or, with both taken by elders, climbs down by the gunner's climbing-down rules and waits beside the crawler. With neither (no free place, no spot to climb down to), the elder isn't offered the gun. A riding sprite holds its place against elders boarding, does nothing and is sheltered.
- **Taking it back:** when the elder leaves the gun, a sprite riding on the crew job takes it in housekeeping that tick (`crew_housekeeping`); one waiting beside the crawler climbs up again once it stands still.
- **Climbing down:** any other job climbs a sprite aboard down in housekeeping, from its place by the climbing-down rules, and it goes on with its job (idle: it walks home). With nowhere to climb down to it stays aboard until there is.
- **Death:** nothing hits a sprite aboard. One killed on its way comes back at home with its job and goes again.
- **Checks:** `validate_guns` checks a riding sprite is alive, holds no slot, isn't at a gun too, and rides a crawler in one of its places alone, no elder there.
- **App:** E on a sprite opens the job menu (since TU.5 in the lab too), which offers "CREW THE CRAWLER" wherever a crawler has a gun; the debug key **J** still gives the sprite nearest the elder the crew job, or takes it off (back to idle). The control protocol's `settlement_sprite_job` takes the job `"crew"`, so a demo script can send a sprite to the gun (`{ "op": "settlement_sprite_job", "sprite": "sprite_ash", "job": "crew" }`); the view reports the job `"crew"` and the activities `"gunning"` and `"riding"`. A sprite aboard is drawn at its place on the deck, from its position and feet, as any sprite is.

### Feet and bodies

A foot coming down matters to whoever stands there (C10, `crawler.rs`):

- **Shove:** stage 2 notes the feet that came down (`Settlement::landings`), stage 3 lists the agents and threats under each, within the foot's 0.4 m (`FOOT_RADIUS`) plus their radius and on its floor (within 1 m), as a `FootLands` message (save format 49 for its queue), and stage 5 applies it after the threats' swings. An elder, sprite or hauler still under it is put just clear of the foot, 0.1 m past touching (`SHOVE_GAP`), away from it (or from the crawler's middle if it stood on the spot itself), else turned by eighths of a turn: on a floor it can walk to, clear of discs, rock and every foot. An elder shoved stops digging. With nowhere to go it stays.
- **Stomp:** a threat under it loses `STOMP_DAMAGE` (4) hit points, armor or not, more than a ghoul has; at 0 it dies as a strike's kill (`Event::Hit` with `Impact::Stomp`, a kill counted). One that lives is thrown off at 4 m/s and staggered 0.4 s.
- **Skipped:** the pilot and riders, seated on the deck, are never under a foot, and a sprite inside a building isn't there. A foot never comes down on an elder, sprite or hauler it saw standing when it lifted (`foothold`); only one that walked under it since is shoved. Ghouls don't attack the crawler.
- **Paths:** bodies path round its feet, not its body, which is overhead: an elder's click-to-move route, a ghoul's chase and its route home when it plans again, and haulers' and sprites' detours round traffic take a disc at every foot's spot (planted, or where a swing lands) level with them, except one they already touch. A wave's route, planned once for all its enemies, doesn't.
- **View:** each footfall is an `Event::Footfall` (shoved, stomped). The view raises a puff of dust under the foot with a step's sound; a shove rings it with a little more dust, and a stomp with a ring of six bursts and a heavy landing (`vehicles::footfalls`). The stomped ghoul bleeds and falls as any kill.

`Settlement::validate` checks the planted feet are in reach and the gait's feet in the air; `validate_footholds` checks they stand on floors. The soak (`crawler_lab_tests`, four minutes in CI, about a second of test time, and an ignored half hour) drives a pilot with a rider aboard at random, walking, trotting, turning and backing among ghouls from both corners and an elder put under its feet. A sprite on the crew job and an elder share the gun: the elder shoots the nearest ghoul in range and throws the fireball; every 15 to 25 s the pilot stops, the elder climbs down (the sprite takes the gun and shoots), loads stone by hand beside the box and boards again, bumping the sprite to a rider's place. Every tick it checks both, everyone aboard in their place, and that every kill is a stomp, a gun's bolt or fire. Half an hour: 270 bolts and 270 stones spent, 254 loaded by hand in 63 restocks, 20 fireballs, 74 bumps; kills 719 by stomp, 60 by bolt, 865 by fire.

### The demo

The lab (`crawler_lab_supply.ron`) sends waves of ghouls on a timer from two corners, the south-east and the north-east, whose diagonals cross by the parked crawler: three, four and five from each side, about every 25 s, from 2 s to 137 s, 24 in all; every one passes within the gun's 7 m of where the crawler is parked. Two sprites stand by a stone pile (60 stones) north-west of the spawn, for the gun's ammo.

The Crawler Lab's help line follows the elder's seat: on foot G boards, E loads or takes, Q drops, J sets the crew job; piloting, WASD walks and Shift trots; at the gun, the mouse aims, LMB fires and R throws the fireball; riding, G climbs down. B builds nothing there ("Nothing to build here").

The gun's demos (`_video` for the mp4, local GPU targets):

- `//kestrel/app:crawler_gunner_demo` (TU.6, T10): a guest pilots the crawler (`guest_drive`) south-east to the first wave while the elder at the gun shoots its three ghouls and throws the fireball, then north to the north-east wave's line, which the elder clears too: 9 bolts, 2 fireballs, 6 kills. The elder's shots (`settlement_fire`, offsets from the mount) were aimed from the demo's frame trace, each ghoul led by its walk over the bolt's flight; retune them if the route, the waves or the gun change.
- `//kestrel/app:crawler_sprite_gunner_demo` (TU.6): the elder pilots and `sprite_ash`, on the crew job, climbs to the parked gun, then shoots both waves from the walking and stopped crawler by itself: 12 bolts, 6 kills. It parks on the north-east wave's line because a sprite aims where a ghoul is, not where it will be, and misses ghouls crossing its line of fire.
- `//kestrel/app:crawler_turret_demo` (TU.5): a guest in the cab, the elder at the gun aims round, fires and throws the fireball at the first wave, climbs down, and a sprite crews the gun and shoots the next.

Demo scripts drive guests with `guest_join`, `guest_walk`, `guest_board` and `guest_drive` (a guest's stick, as `drive` is the host's, so a guest pilots while the local elder mans the gun).

`//kestrel/app:crawler_lab_demo` (`_video` for the mp4, a local GPU target) boards a pilot and three guests (since TU.1 the second aboard takes the gun, the others ride), scripted guests (`guest_join`, `guest_walk` and `guest_board` in demo scripts, which a host sends as players 1 to 8), and walks the lab: south over the moat onto the island, east off it, north-north-east and up the plateau's 1.5 m south face, then trots west down the terraces (stomping a ghoul of the wave) and across the trench to the pit, where it stops. It turns north beside the pit, trots north and walks east into the mesa's cliff, where it stops again, and the pilot climbs down. The route is open-loop (sticks held for set frames): retune it if speeds or the gait change.

## Drawing

`app/src/vehicles.rs` draws each vehicle as a chassis, hood, roll bar and four wheels. The local player's rover is drawn from the player's own drawn transform, so the predicted driver and their rover never drift apart. Other rovers are interpolated between ticks. Each wheel eases onto the ground under it and the chassis tilts to them, so a terrace step lifts the front, then the back.

A crawler is drawn from its rig, `app/actors/crawler.ron` (`app/src/crawler.rs`): the body's boxes and cylinders (timber hull, brass cab and helm, a railed cargo deck), the gun's (a swivel ballista standing on the cab's roof with its bow at the mount, turned about it, see [The gun](#the-gun); the stack and dome stand on the deck in front of the cab, clear of it) and one leg's hip, thigh, shin and foot, mirrored to the four. Each leg is a two-bone solve from its hip on the deck to the ankle over the sim's foot, the foot upright under it and the knee bending up and outward; a foot out of reach eases the leg straight instead of snapping. A swinging foot rises over the higher of its two floors, crosses at the gait's `lift` and comes down onto its landing spot. Feet and body are interpolated between ticks; a foot the sim aims again glides to its new spot. While the local player pilots, rides or mans the gun of a crawler the camera's view widens by `PILOT_PULL` (1.4) and eases back after they climb down. Riders are drawn in their places on the cargo deck, which the rig keeps clear: the crates sit across its middle, between the front two places and the back one. A local rider's or gunner's crawler is drawn from their own drawn transform, less their place (`crawler::deck_spot`), so the two never drift apart.

## Current limits

- Rovers don't collide with avatars, haulers, threats or each other. A crawler's feet shove and stomp, but bodies walking by hand (the stick) pass through a planted foot; only planned routes go round it.
- Clicking a point to drive to stops the rover instead of routing there.
- The driver and a crawler's riders stand at their seats in their idle pose, and there's no on-screen "press G" prompt.
- The rover is a built-in mesh and the crawler a built-in rig, not style skins.

`kestrel/todo/vehicles.md` lists these as tasks.
