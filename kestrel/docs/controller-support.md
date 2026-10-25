# Controller support: agreed design

Status: architecture agreed on 25 October 2026; implementation has not started.
The user explicitly requested design agreement before gameplay changes. The
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
this design. The next gameplay implementation requires a separate user request.
