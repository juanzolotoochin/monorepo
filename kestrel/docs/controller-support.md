# Controller support: agreed design

Status: architecture agreed on 25 October 2026. Implemented on 25 October 2026
through step 4 of the sequence below, with synthetic (hardware-free) tests;
physical USB validation (step 5) has not been done. See
[Implementation](#implementation) for what was built, the routine decisions
taken, and how it was validated. The
[interactive review](design/controller-input.html) contains the source review
and editable design alternatives. This document records the selected behavior.

## Scope and constraints

- Small typed action/context layer above the existing deterministic command
  boundary; devices and floating point stay in the front end.
- Directional stick movement at existing walk/run speeds, with explicit run.
  Proportional analog speed is deferred.
- Independent right-stick aim, retaining the last valid aim while neutral.
- Nearby target selection for controller logistics; no virtual cursor.
- Windows native first, one controller for the local network player,
  keyboard/mouse coexistence, fixed bindings, gameplay and main-menu/pause
  navigation. Browser controller support, local split-screen, rebinding UI,
  and controller operation of Character Studio are outside this milestone.
- Xbox-compatible controller over USB is the first hardware validation target.
  Exact model is unspecified. Initial development and automated acceptance
  must work on a machine without a physical controller.

## First milestone: synthetic controller input

Implement the synthetic device path before the native controller path. Tests
must inject physical-style device state, not call PlayerControl or simulation
commands directly: those would bypass the behavior this feature is meant to
verify.

```text
Native Bevy device reader ──┐
                           ├─> GamepadSnapshot -> bindings/dead zones/edges
Synthetic device source ────┘                          |
                                                     v
Keyboard/mouse adapters ----------------> context + device arbitration
                                                     |
                                                     v
                                  resolved gameplay/UI action frame
                                                     |
                                  targets + gameplay command helpers
                                                     |
                                  SessionService -> Session -> turns -> sim
```

`GamepadSnapshot` is a proposed front-end type containing a source-local device
identity, connection state, raw stick/trigger values, and button state.
The native reader and synthetic source feed the same snapshot consumer, so
dead zones, hysteresis, edge extraction, controller claiming, ownership, and
disconnect handling are shared. These identities never enter simulation
commands or saves.

Provide a reusable test harness that advances virtual elapsed time and supplies
snapshots, including connection/disconnection and multiple attached devices.
Timed input scenarios should run at different render rates; their actions must
not be expressed only as already-resolved gameplay commands. Provide a
developer-only way to replay the same synthetic scenarios in a rendered run
for visual QA. Keep this injection explicitly enabled for tests/development;
normal player runs read actual devices.

Synthetic scenarios need press, hold, release, diagonal and neutral sticks,
small noisy axis changes, threshold crossings, reconnects, and simultaneous
keyboard/mouse input. They must work without creating an OS-level virtual
controller, installing a virtual-device driver, or connecting hardware.

This validates our snapshot-to-action-to-command path. It does not establish
that Bevy or Windows discovers a USB controller correctly, or that native
axis/button mapping matches the physical device. Verify the thin native reader
and physical feel separately when hardware is available.

## Responsibilities and frame ordering

An App-layer input service samples devices once per frame and owns connection
state, the selected controller seat, bindings, and input history. It exists
before a session so menus can use it. Evaluate bindings for the effective
context and track consumption so one physical press cannot activate both UI
and gameplay actions.

App UI handlers resolve text/menu/utility contexts before Session pause handling
and World gameplay dispatch. Context transitions take effect for consumption
and cancellation in the same Input phase, even when visual state changes are
applied later. UI confirm/back/open/close must never fall through to gameplay.
Text messages remain separate from action edges. Only residual, eligible
gameplay actions reach the World dispatcher before Step.

Keep the existing Input -> Step -> Mirror -> Present phases. Move memory action
reading out of Mirror into the shared Input path. Replace independent raw
keyboard/mouse gameplay readers with adapters and shared action handlers.
Camera zoom is a view action; it does not enter the simulation.

PlayerControl remains World-scoped and provides command-producing avatar
helpers. Feature helpers retain logistics, inventory, machine, and memory
validation. Presentation consumes resolved actions for immediate feedback and
confirmed simulation state for authoritative outcomes. Existing high-level
automation remains available, but it is not evidence that device bindings work.

## Fixed bindings

| Control | Gameplay | UI context |
|---|---|---|
| Left stick | Camera-relative directional movement | Navigate focus |
| Right stick | Camera-relative independent aim | No gameplay effect |
| LB | Hold run | No gameplay effect |
| RB | Press jump in Jump scenes; hold jetpack otherwise | No gameplay effect |
| RT | One attack per fresh press | Never confirms UI |
| A | Primary interaction | Confirm |
| X | Collect more compatible material while carrying | No gameplay effect |
| B | Request validated nearby cargo drop | Back/cancel chooser |
| Y | Cycle available weapons | No gameplay effect |
| D-pad left/right | Cycle nearby targets | Navigate focus |
| D-pad up/down | Zoom | Navigate focus |
| View | Open utility panel | Back/close utility as appropriate |
| Menu/Start | Claim controller seat, or open pause once claimed | Pause/resume where applicable |
| LT / stick clicks | Unbound | Unbound |

The utility panel offers available Dig, Record/Deploy, Clear Recording,
Respawn, and explicit Cancel Logistics actions. It captures gameplay and
suspends direct control under the same policy as pause while the world and
accepted jobs continue. A confirms, B returns. Item/action choosers use the
same UI navigation and consumption rules.

Main menu, pause, utility, and item/action choosers support D-pad or left-stick
focus navigation. Navigation can repeat while held; confirm never repeats.
Names, host ports, and join addresses use keyboard text editing and remembered
settings. New connection details therefore still require typing. Studio editing
and debug scene switching remain keyboard/mouse tools.

## Device claiming and exclusive takeover

Menu/Start claims the single local controller seat. Consume the claim press;
it must not also open pause or perform gameplay. Ignore other controllers
until the seat is released. A selected-controller disconnect releases the seat
and requires a fresh claim or explicit keyboard/mouse resume.

One device group owns the complete gameplay input set: keyboard/mouse or the
selected controller. Last deliberate gameplay input takes ownership. Idle
devices, passive mouse drift, and stick noise do nothing. Use thresholded
deliberate movement rather than every raw mouse or axis change as takeover.
Resolve simultaneous contributions once in the shared arbiter; make its
tie-break stable and test it. Do not let service execution order pick winners.

On takeover, release outgoing direct movement/thrust before applying eligible
incoming actions. Preserve the current valid world aim until the new owner
provides explicit aim. Neutral input from the non-owner must never send Stop.
Run/thrust held by the outgoing device cannot remain latched after takeover.

## Movement, aim, and attack

Left stick uses radial dead-zone hysteresis, then a unit direction in the
same camera-relative horizontal basis as keyboard movement. Run remains a
separate held action. Keep tuning constants named and centralized; validate
noise behavior using synthetic traces before tuning physical feel over USB.

Initialize aim from confirmed avatar facing on world entry or respawn. Retain
the last explicit aim in world space when the aim stick becomes neutral;
walking sideways/backward must not replace it. Resolve an aim change before
an attack from the same sample. Derive gameplay targeting from confirmed
state, never the interpolated/predicted avatar pose.

RT generates one attack on crossing its press threshold. Require release below
a distinct release threshold before another attack. Preserve existing deliberate
combo buffering for melee and ranged weapons. Holding RT does not repeat fire
or fill a queue. RB jump is a press edge; RB jetpack is held state with release
and cancel. Scene features determine which action is available.

Quantize unit directions into Fx at the gameplay command boundary and deduplicate
against the last sent command. Filter/coalesce continuous movement and aim
without losing ordered press/release/cancel edges. Sample edges once per frame;
do not re-emit an attack for every tick in a multi-tick catch-up Step. Keep the
joined-client preview consistent with resolved actions.

## Nearby interaction

Choose the nearest eligible interactable within a tunable local search radius;
the initial design value is 3 m beyond its interaction surface. Retain the
target while valid instead of continuously switching on small distance changes.
D-pad left/right cycles candidates, with stable target ID ordering/ties.
Exclude other floors and targets without a locally valid approach. Simulation
validation remains authoritative; local eligibility is not a success guarantee.

Show an outline, target name, current verb, and unavailable reason. Combat aim
does not automatically change interaction selection. Never silently substitute
a different target after an action fails.

Primary interaction takes when empty or deposits compatible carried goods at a
pile/depot, delivers at a collector, dispenses at a dispenser, and toggles a
machine. X explicitly collects more compatible material while carrying. Preserve
existing quantities, capacity limits, timings, and feature validation. For
multiple depot item types, present an item/action chooser before sending a
transfer.

Interacting with a selected nearby logistics target may start an accepted
approach/job that walks into reach and completes the action. New deliberate
manual movement cancels that job; context loss does not. Machine/dispenser
actions retain existing range validation rather than gaining new travel
automation. B proposes a cargo drop just ahead of confirmed facing, within
local reach, with a visible placement preview. Invalid placement reports a
reason instead of searching for a replacement location.

## Context cancellation and accepted jobs

Pause, focus loss, or selected-controller disconnect stops manual movement,
disables thrust, clears unsent edges and the local follow-up attack buffer,
and cancels click routes. Accepted logistics jobs continue under normal
simulation rules. Focus loss/disconnect opens pause in a live session; returning
focus or reconnecting never auto-resumes. Require buttons to be released and
sticks to be neutral before contributing again after suspension.

Add a device-independent semantic `ReleaseDirectControl { agent }` command.
Validate ownership like other avatar commands. It always disables thrust and
stops manual/click travel only when no accepted logistics job owns the route.
Do not implement it by unconditionally calling `move_avatar(Stop)`, which
currently cancels pile jobs. Evaluate route ownership against authoritative
state when the command applies, including success/failure of preceding queued
logistics commands. Reset local command deduplication/preview state so a stale
cached direction cannot suppress the next intentional movement.

Cancellation does not recall accepted attacks/jumps or skip network delay.
Accepted actions finish normally; remote release takes effect in its turn.
Already airborne item transfers finish under existing conservation rules.
Explicit Cancel Logistics and new deliberate movement/attack are separate
gameplay actions, not effects of simply entering a UI context.

Extending CommandKind changes the serialized command vocabulary. Preserve
existing variant ordering and check both network protocol and input-log version
handling; deliberately version new output and reject unsupported formats or
provide an explicitly tested compatibility reader. Do not claim compatibility
merely because the content rules hash is unchanged. If authoritative ownership
requires new persistent fields, include saves/hashes in the compatibility work.

## Implementation sequence and acceptance

1. Build the snapshot abstraction and synthetic source/harness. Extract shared
   keyboard/mouse action handling and context consumption without requiring a
   physical gamepad. Add meaningful tests for arbitration and edge behavior.
2. Implement semantic release and shared direct-control/job ownership handling;
   validate accepted/rejected/delayed logistics, replay, and command compatibility.
3. Exercise the complete controller path using synthetic snapshots: movement,
   aim, attacks, run, jump/flight, equip, logistics targets and choosers, drop,
   utility, and main-menu/pause navigation. Include rendered synthetic scenarios.
4. Enable/verify the pinned native Bevy backend for Windows and add the thin
   native snapshot reader. Validate native mappings independently of the
   hardware-free tests. Keep existing browser/keyboard builds working.
5. On an Xbox-compatible USB controller, verify discovery, axes/button mapping,
   disconnect/reconnect, focus loss, neutral rearming, UI prompts, and feel.
   Until then, report synthetic tests as passing and physical integration as
   unverified; do not describe the hardware milestone as complete.

Required automated scenarios include:

- Controller movement with idle keyboard, and deliberate takeover in both
  directions; outgoing run/thrust releases and passive drift never steals control.
- Claim/reclaim, multiple connected devices, disconnect while flying, and no
  surprise action after pause/resume or focus return.
- Unit diagonal movement, dead-zone/trigger threshold crossings, noisy sticks,
  remembered aim, and aim-plus-attack in the same sample.
- One attack per press at low/high render rates and during multi-tick catch-up;
  UI open/close/confirm never leaks gameplay actions.
- Sticky/cycled targets, invalid/out-of-reach actions, incompatible cargo,
  multi-item chooser, and preserved accepted jobs during release.
- Jobs accepted or rejected before release applies, explicit job cancellation,
  and airborne item conservation.
- Exact accepted-turn replay, host/client avatar ownership, and joined-client
  preview convergence after movement and cancellation.

No gameplay changes, dependency updates, or Bazel runs were made while recording
this design. Implementation was then requested separately (below).

## Implementation

Built on 25 October 2026 on `controller_support_branch`, following steps 1–4.
Step 5 (an Xbox-compatible controller over USB) is still to do. Until it is,
the synthetic tests pass and physical integration is unverified.

### Where things live

| Piece | Code |
|---|---|
| Semantic release | `sim/src/settlement/avatar.rs` (`release_direct_control`); `CommandKind::ReleaseDirectControl` in `sim/src/world.rs` |
| Snapshot, filters, edges, re-arming | `app/src/input/pad.rs` |
| Tuning constants | `app/src/input/tuning.rs` |
| Seat, ownership, contexts, bindings, history (App layer) | `app/src/input/mod.rs` (`InputService`) |
| Synthetic source and timed scenarios | `app/src/input/synthetic.rs` |
| Native reader (Bevy gamepads → snapshots) | `app/src/input/native.rs` |
| Nearby targets | `app/src/input/targets.rs` |
| Shared dispatcher (World layer) | `app/src/input/gameplay.rs` |
| Prompts, target outline, drop preview, panels | `app/src/input/hud.rs` |
| Pause navigation and suspension | `app/src/pause.rs` |
| Main-menu navigation | `app/src/menu.rs` (`controller`) |
| Tests | `app/src/input/tests.rs`, plus `pad.rs`, `synthetic.rs` and `native.rs`; release in `sim/src/settlement/scene_tests.rs` and `combat_tests.rs`; versions in `save/src/inputs.rs` and `net/src/tests.rs` |

Order inside the Input phase: debug-UI routing (`GameInput`), then
`InputService` (the first App service: it samples every device once, claims
the seat, resolves ownership, and runs the utility panel and choosers), then
`MainMenu`, then `Pause` (Session), then the feature pointer adapters (pile
and depot clicks), then `PlayerControl`'s slot. That slot runs
`input::gameplay::prepare` (suspension release, takeover release, weapon
choice), then the cursor route and cursor attack adapters, then
`input::gameplay::dispatch` (aim, attacks, lift, utility, logistics,
movement). Keyboard and controller share `prepare` and `dispatch`. The old raw
readers are gone: `keyboard_input`, the settlement's E and transfer-cancel
reader, and the memory trial's E/Q reader, which ran in Mirror.

### Routine decisions

- **Release ownership.** A pile haul (`Hauler::job`) owns the route. So does an
  accepted depot transfer (`AvatarState::job`) while the avatar's intent is
  `GoTo`, because the app sends the dock walk together with the transfer. This
  uses existing state only, so the save format and state hash are unchanged.
  Release leaves a dig in progress alone: it stops travel and thrust only.
- **Versions.** The new variant is appended last. Input logs are now version 2.
  Version-1 logs still load (same bytes, checked against frozen v1 bytes), and
  a version-1 log that holds the new command is refused. The network protocol
  is version 3, and packets from version-2 peers are ignored like any other
  version mismatch. The save format did not change.
- **Deliberate input.** Keyboard: a fresh press of a gameplay key (movement,
  Shift, Space, F, R, E, Q, Z/X/C/V, Ctrl). M, digits, F3 and `/` do not take
  over. Mouse: a button press, or at least 8 px of movement in one frame.
  Controller: a fresh press, or a stick leaving its dead zone. If both groups
  are deliberate in the same frame, the current owner keeps gameplay. If
  several controllers press Menu in the same frame, the lowest device identity
  gets the seat.
- **Keyboard bindings on the shared path.** R, F, E and Q produce the same
  actions as the utility panel's Respawn, Dig, Record/Deploy and Clear
  Recording. In scenes with a supply chain, E stays the nearest-machine toggle
  within 2.5 m (it does not use the controller's selected target) and Q does
  nothing, as before. Arrow keys now cancel a depot transfer, like WASD.
- **Keyboard pause and focus loss.** Escape and focus loss now send the
  release instead of relying on held keys' release edges. This is the agreed
  behavior change: pausing no longer cancels an accepted haul.
- **Re-arming.** A device already at rest when suspended has nothing to
  re-arm. Otherwise everything reads as released until every button is up and
  both sticks are centered.
- **Panels.** Item choosers suspend direct control, like the utility panel.
  Menu or View closes the utility panel. Keyboard Escape closes a panel without
  also pausing. The utility panel lists Dig where digging is enabled, Record
  and Clear Recording where there is no supply chain, Respawn, and Cancel
  Logistics while a haul or transfer runs.
- **Logistics.** X collects at a pile whether or not the player carries (when
  carrying, it must be the same mineral). Cancel Logistics sends a deliberate
  Stop, which ends a haul, plus `CancelTransfer`. B proposes a drop 1.4 m ahead
  of the confirmed facing; the simulation validates it and reports a refusal.
  Dispensers keep the click's lack of a range limit. Machines keep E's 2.5 m
  and show "Move within 2.5 m" when farther.
- **Targets.** Candidates are within 3 m of their interaction surface, within
  0.75 m of the ground height under the avatar, and, for piles, the collector
  and depots, reachable on foot by `sim::nav::path` (rechecked every 0.25 s).
  Cycling follows the target key: piles, dispensers, the collector, depots,
  machines, each by ID.
- **Aim.** Starts from the confirmed avatar's aim if it has one, else its
  heading. After a respawn it is taken again once a newer tick is mirrored.
- **Menus.** Once seated, the controller navigates the launch menu: up/down
  within a column, left/right between the scene list and the session column,
  A acts like a click (on a text field it gives that field the keyboard), and
  B leaves a field. B never quits. Pause: up/down, A confirms, B or Menu
  resumes.
- **Native backend.** `bevy_gilrs` 0.20.0 is a Windows-only dependency
  (`[target.'cfg(windows)'.dependencies]` in `Cargo.toml`, and a `select` in
  `kestrel/app/BUILD.bazel`). Bevy's `gamepad` feature provides the types on
  every platform. Linux gets no backend: gilrs needs libudev there, which the
  hermetic toolchain does not provide. The browser gets none because it is out
  of scope. `Cargo.lock` gained only the new crates. While resolving, Cargo
  also moved arboard and cpal to newer Windows crates; those two edges were
  restored by hand, and `cargo metadata --locked` accepts the result.
- **Tests and development only.** `Options::headless_menus` runs the menu and
  pause overlay in simulation tests. `--gamepad-script <file.json>` replaces
  native controllers with a synthetic scenario in any run, for visual QA.
  `kestrel/app/demos/controller_field.gamepad.json` is a sample.

### Validation

Automated and hardware-free (`bazel test --config=full_local -c opt //kestrel/...`):

- Simulation: release stops walking and click routes and switches thrust off.
  It keeps accepted hauls and depot dock walks, with their routes. It judges a
  haul accepted or rejected earlier in the same turn, conserves airborne
  units, and is refused for another player's avatar. Deliberate movement still
  cancels a haul.
- Formats: input-log v2 round trip, frozen v1 bytes, a v1 log with the new
  command refused, unknown versions refused; protocol 3 round trip, v2 refused.
- Input service: seat claiming (the claim press is consumed, other devices are
  ignored, simultaneous claims resolve stably), takeover in both directions,
  the tie-break, dead-zone noise and mouse drift not taking over, neutral
  non-owners contributing nothing, unit diagonals and direction coalescing,
  trigger thresholds, disconnect and reconnect, focus loss and re-arming, the
  utility panel, the chooser, UI consumption, navigation repeat,
  scene-dependent keyboard bindings, and history.
- Whole game, headless, with synthetic controllers: walking with the keyboard
  idle and a single Stop; run; takeover releasing thrust and movement in both
  directions; one attack per press at 15, 60 and 144 fps; aim from facing, aim
  plus attack in one sample, and remembered aim; pause and focus loss
  releasing once, with no surprise actions after resuming; disconnecting while
  flying; several controllers; the utility panel and zoom not leaking into
  gameplay; sticky and cycled pile targets with take, deposit and collect
  more; incompatible cargo with no substitution; the drop proposal; an
  accepted haul surviving pause; explicit Cancel Logistics; the multi-item
  depot chooser and machine reach; arrow-key transfer cancellation; exact
  replay of a controller session from its written input log; the same timed
  scenario at 24, 60 and 120 fps; the menu and pause driven by a controller;
  and a joined client's controller moving only its own avatar, with the
  preview settling after a disconnect release.
- Native mapping: a Bevy `Gamepad` with set buttons and axes maps to the
  expected snapshot. This checks the mapping code, not a device.

Not yet validated (step 5, needs an Xbox-compatible controller over USB):
discovery on Windows through gilrs, whether physical buttons and axes match
the mapping, disconnect and reconnect, focus loss, neutral re-arming, the
prompts, and the physical feel of the dead zones and thresholds. Browser and
Linux builds have no controller backend by design.
