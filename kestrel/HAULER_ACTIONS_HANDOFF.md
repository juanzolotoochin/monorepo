# Kestrel hauler actions (design)

Status: planned (branch `kestrel-hauler-actions`). Part of character step 5 ([character definitions](docs/character-definition.md)).

## Goal

A hauler's `Phase` state machine sets its route directly today, in three
places in stage 2 and two message handlers in stage 5. Make it pick
actions instead, from the same vocabulary players use (`Action`,
`sim/src/settlement/action.rs`), and apply them through one `act` that
dispatches by `Role`. The result: every route a brain or a command gives
any agent goes through `act`, and a later AI brain (or possession) plugs
into the same path.

**Behaviour stays identical.** No rule change and no state change: the
golden file and `testdata/*.hashes` stay byte for byte, totals included,
and the save `FORMAT_VERSION` stays 19.

## What exists today

| Where | Stage | Writes the route |
|---|---|---|
| `haulers::self_update`, `Idle` away from home | 2 | `hauler.plan(.., home)`, then walks the same tick |
| `haulers::self_update`, `Loading` done | 2 | `hauler.plan(.., drop dock)`, no walk that tick |
| `approach`, route empty, replan due | 2 | `hauler.plan(.., dock)`, sets `retries`, then walks |
| `approach` queue back-out | 2 | a one-waypoint route for one step, then restores the old one |
| `move_worker` traffic detour | 2 | `hauler.plan(.., goal, nearby)` |
| `Settlement::equip` (`economy.rs`) | 5 | `message.route`, planned in stage 3 by `decide` |
| `Settlement::reserve_haul` | 5 | `message.route`, planned in stage 3 by `decide` |

`decide` (stage 3) plans the route to check the job is reachable, then
sends it in `Equip` or `ReserveHaul`. Stage 5 can still refuse (another
hauler took the stock), so the route is only set on acceptance.

Avatars: `Settlement::act` (stage 1) runs `avatar::act`, which plans a
`GoTo` with `nav::path_with` (`set_intent`). It returns early for anything
but an avatar.

## Decisions

### D1. The brain is the phase machine in stage 2; it picks a `Move`

- `haulers::self_update` splits into three parts, run in this order:
  1. `think(a, me, ctx) -> Option<Action>`: the phase changes that come
     before walking, and the action to take. It writes the hauler's own
     brain state (`phase`, `departure_dock`, `retries`, `assignment`,
     `carry.start_next`, capacity on attach), never `body.route`.
  2. `action::act(a, &action, &view, def, &mut scratch)` if there is one.
  3. `walk(a, start, me, ctx)`: `travel`/`move_worker`, the queue at a busy
     dock, and the arrival checks, keyed on the phase the tick **started**
     in (`start`).
- **Why stage 2:** the decisions that only read the hauler and last tick's
  world run there now, and they must stay on the same tick (planning and
  the first step on one tick). Moving them to stage 3 would set the route
  a tick later.
- **Why not one action per phase change:** `Loading → ToDrop` and the like
  are the brain's own memory, as `AvatarState::intent` is the avatar's.
  The vocabulary is for what an agent *does*; inventing `StartUnload` or
  `FinishAttach` actions adds names no player or other brain would use.

Per phase (keep every check and counter where it is today; only the route
write moves into `act`):

| Start phase | `think` | `walk` |
|---|---|---|
| `Idle`, more than 0.1 m from home | `→ Returning`, `GoTo(home)` | travel at `speed_now`; route empty `→ Idle` |
| `Idle` at home | nothing | nothing |
| `Returning` | nothing | as above |
| `ToPickup`, `ToDrop`, `ToEquip` | route empty, more than 0.1 m off, `tick >= retries`: `GoTo(dock)`, `retries = tick + APPROACH_REPLAN_TICKS` | `approach`'s rest: queue (`dock_wait_ticks`, back-out), else travel; at the dock and owning it `→ Loading`/`Unloading`/`Attaching` |
| `Loading`, load landed | `start_next`, `departure_dock`, `GoTo(drop dock)`, `→ ToDrop` | nothing (no step on this tick, as today) |
| `Unloading`, done | `departure_dock`, clear `assignment`, `→ Idle` | nothing |
| `Attaching`, attached | `departure_dock`, capacity, `→ Idle` | nothing |

The departure-dock check stays first and the trailer follow stays last.
`walk` keys on `start` so `Loading → ToDrop` doesn't step on its tick
while `Idle → Returning` does, as today.

### D2. Haulers use `Move(GoTo)` and a new `Follow(route)`; no `CommandKind` change

- `Action::Move(Intent::GoTo { target, run: false })`: the brain's three
  plans. `run` is ignored for a hauler.
- `Action::Follow(Vec<FxVec2>)`, new: walk a route already planned. Stage 5
  uses it for an accepted job (D4). `Action::of` never produces it, so no
  command can send a route. Name it so it isn't confused with the
  `FollowRoute` brain.
- **Why no interact/haul/equip action:** accepting a job writes the world
  (depot stock, both carry jobs), and the request comes from stage 3,
  which can't write the agent. It stays a message (`Equip`, `ReserveHaul`).
  "Interact" lands with player pile hauls (todo below).
- `Action` isn't saved, hashed or sent, so a variant is free.
  `CommandKind` and `Intent` don't change.
- Steering stays in `walk`: the traffic detour and the queue back-out
  rewrite the route they follow, as `avatar::walk` slides around terrain.
  They are how to walk, not where to go.

### D3. One `act`, dispatching by `Role`

- `action::act(a: &mut AgentState, action, view, def, out)` matches the
  role: `Avatar` → `avatar::act` (unchanged body); `Hauler` →
  `haulers::act(body, hauler, action, view)`.
- `haulers::act`: `Move(GoTo { target, .. })` sets
  `body.route = hauler.plan(body, view.terrain, target, &[])` (the trailer
  path when towing); `Follow(route)` sets `body.route = route.clone()`.
  Anything else does nothing, with a `debug_assert!`, since only the
  hauler brain and stage 5 drive haulers. `avatar::act` gets a
  `Follow(_) => {}` arm.
- Hauler actions emit no messages. Stage 2 and stage 5 pass a scratch
  `ActionMessages` and `debug_assert!(out.is_empty())`.
- `Settlement::act` (commands) keeps its avatar-only guard: commands never
  drive haulers. Its body moves into `Settlement::act_agent(tick, me,
  action, terrain, out)` (no guard, the `mem::take(agents)` trick), which
  stage 5 also calls. `AvatarReplay::apply` calls `action::act`.
- **Stage 2 needs a `View`.** `Settlement::self_update` builds one per
  tick from the destructured fields (compute `depot_items()` first) and
  puts it in `Ctx`; each agent uses `View { me, ..ctx.view }`. Derive
  `Clone, Copy` on `View`. Leave `Ctx`'s overlapping fields unless the
  dedupe is trivial.

### D4. Stage 5 accepts the job, then the hauler follows the stage 3 route

- `equip` and `reserve_haul` keep sizing, reserving and writing the
  hauler's job (`phase`, `assignment`, `carry.job`, `carry.next`) as now.
  Only `body.route = message.route` becomes
  `self.act_agent(tick, agent, &Action::Follow(message.route), terrain, &mut scratch)`.
  Both handlers take `terrain` (`world.rs`, as `attach` does).
- **Why keep the route in the message:** `decide` must plan it anyway to
  check the job is reachable. Planning again later costs a second path
  search per job and can disagree: stage 5 and 6 change terrain between
  (cover damage in the arena, a memory door, digging).
- **Why not plan on the next stage 2 (`GoTo(dock)` from `think`):** that is
  `approach`'s replan, which sets `retries`, so later replans would wait
  longer, and the route could differ (above). Both move the golden
  behaviour lines. Not worth a gameplay commit for no player-visible gain.
- **Why stage 5 may write the job:** stage 5 already writes agents where a
  contested decision lands (`carry_unit`, `attach`, `retry`, a refused
  `ReserveStock` rolling back a player's job). The job and its depot holds
  are one atomic grant. The move itself goes through `act`.
- `attach` keeps snapping `travel_heading` and placing the trailer: that
  is the hitch, not a route.

### D5. Golden file, digest and format

- **Digest:** no extension. `behaviour_digest` already hashes position,
  heading, travelled, phase, route, cargo and trailer, and the tick totals
  hash all saved state (`retries`, `departure_dock`, counters). Any slip
  shows in the golden file.
- **Format:** no saved or hashed state changes (`Action` and
  `ActionMessages` aren't saved; `Equip`/`ReserveHaul` keep their route).
  `FORMAT_VERSION` stays 19.
- **Golden:** `golden.txt` and `testdata/*.hashes` unchanged, totals
  included. Both golden scenes (forest, arena) have haulers, equipment and
  carts, so they cover every path above.
- **No gameplay commit.**

## Todos

- **Let a hauler's haul end early:** stays a todo. It is a rule change
  with no trigger yet (haulers can't be hurt; links switched off drain
  committed jobs by design). Add to it: ending a haul is an action with
  world effects (`drop_carry`'s releases), and stage 2 has no outbox, so
  it needs per-agent `ActionMessages` from stage 2 merged in agent order
  (and a place in stage 5), or the brain asks for it in stage 3.
- **Move hauler equipment onto the carry path:** stays. It depends on
  equipment joining `Inventory` and changes the format; actions don't
  touch it.
- **Move pile hauls and piano keys onto actions:** stays. It is the
  player side ("interact"), with its own prediction work.
- **Trim body fields one role ignores:** its trigger ("once actions drive
  haulers") is met by this work. Keep it separate (format change); note
  it's unblocked.

## Commit order

Step 5's order: digest (not needed, D5), format (not needed, D5), refactor
with the golden file unchanged, gameplay (none). Each step is one commit
on `kestrel-hauler-actions`.

1. **One `act` for every role.** Add `Action::Follow`, `action::act`
   (dispatch), `haulers::act`, the `Follow` arm in `avatar::act`, and
   `Settlement::act_agent` with `Settlement::act` as the guarded entry.
   `AvatarReplay::apply` calls `action::act`. Nothing calls the hauler
   arm yet. `bazel build` only.
   - Where: `settlement/action.rs`, `avatar.rs` (`Settlement::act`, `act`),
     `haulers.rs`, `prediction.rs` (`apply`).
   - Done in step 1: as designed. `action::act` lives in `action.rs`;
     `haulers::act` takes `&HaulerState` (it only plans), the body `&mut`.
2. **Stage 2 brain.** Split `haulers::self_update` into `think`, `act`,
   `walk` per D1's table; `approach` splits into its replan (`think`) and
   the rest (`walk`). Put a `View` in `Ctx` (D3). The three
   `hauler.plan` route writes in `self_update`/`approach` become
   `GoTo` actions. **Run `bazel test --config=remote //kestrel/sim:sim_test`
   here**: it is the risky step and step 3 builds on it; the golden test
   must pass unchanged.
   - Where: `haulers.rs` (`self_update`, `approach`, `travel`, `Ctx`),
     `economy.rs` (`Settlement::self_update`), `action.rs` (`View`).
   - Done in step 2: as designed; `sim_test` passed first time with the
     golden file unchanged. `Ctx` keeps its overlapping fields beside `view`.
3. **Stage 5 follows through `act`.** `equip` and `reserve_haul` set the
   route with `Follow` (D4) and take `terrain`. `bazel build` only.
   - Where: `economy.rs` (`equip`, `reserve_haul`, `hauler_parts`),
     `world.rs` (`apply_messages`).
4. **Tests and docs.** The tests below; update the `action.rs` module doc
   ("Only player avatars act for now"), the `haulers.rs` module doc, the
   "Haulers" bullet in `docs/character-definition.md` (done, link here),
   the todos above, and "Done in step N" notes here. Run the full suite
   once. Add the `docs/test-time.md` row.

## Tests

- `think` picks without moving: a hauler whose load has landed gets
  `Some(Move(GoTo(drop dock)))`, phase `ToDrop`, and its `body.route`
  unchanged by `think`; an `Idle` hauler away from home gets `GoTo(home)`;
  one at home gets `None`.
- `haulers::act`: `GoTo` sets the route to `hauler.plan(..)`, for a hauler
  with and without a trailer; `Follow` sets it verbatim; an avatar ignores
  `Follow`.
- Commands never drive haulers: a `MoveAvatar` naming a hauler leaves the
  state hash as a run without it.
- Bit equality: `golden_report_matches` unchanged, and
  `settlements_are_deterministic_across_thread_counts`.
- Put them in `settlement/tests.rs` beside the hauler tests.

## Open questions

- None blocking. Possession (a player driving a hauler) stays out of
  scope; `Settlement::act`'s guard is where it would change.

## Rules for the work

- Each numbered step is its own commit, in order. Tests run once at the
  end, plus the one run in step 2 (`AGENTS.md`, "When to run tests").
- Bazel: `bazel test --config=remote ...` (`AGENTS.md`).
- The golden file and hash files must not change. If one does, the
  refactor slipped: find it, don't regenerate.
- Things found but out of scope go in `kestrel/todo/`.
- When a step is done, add a "Done in step N" note under it here.
