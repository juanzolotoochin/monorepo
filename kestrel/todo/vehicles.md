# Vehicles

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
