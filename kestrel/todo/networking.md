# Networking

## Tint each player's suit with their slot color

- **Status:** open
- **Why:** name tags have slot colors, but every avatar wears the same suit,
  so in a crowd you still read tags to tell players apart.
- **What to do:** give an accent part of the suit (for example the chest or
  shoulder pieces) the player's `slot_color`, for peers and your own avatar,
  in a way that fits each scene's style profile.
- **Where:** `slot_color` in `kestrel/app/src/nametags.rs`; `dress_body` in
  `kestrel/app/src/style.rs`; `kestrel/app/src/peers.rs`.
- **Found in:** eight-player stage 4 (D5 in `kestrel/EIGHT_PLAYER_HANDOFF.md`).

## Let a LAN host allow 16 players

- **Status:** open
- **Why:** the cap is `NetConfig::max_players` (8) with no way to change it
  from the game. 16 players on a LAN measured 2.1 Mbit/s up from the host.
- **What to do:** a `--max-players <n>` flag (and a menu field) for the
  host, limited to 16; check the network lab with `-Clients 15`.
- **Where:** `kestrel/app/src/main.rs`, `kestrel/app/src/lockstep.rs`
  (`open`), `kestrel/app/src/menu.rs`.
- **Found in:** eight-player design session.

## Find what stalls clients around a leave and rejoin at 75 ms

- **Status:** open
- **Why:** in the eight-player lab at 75 ± 20 ms, each client's time spent
  waiting for turns (`starved_ms`) went from 8–90 ms to 270–515 ms between
  one client closing and it rejoining 3 minutes later, then grew only
  15–50 ms in the next 54 s. So every client stalled for 0.2–0.4 s in total
  around the leave, the rejoin, or both. The rejoining client itself didn't.
- **What to do:** rerun with `-Clients 7 -Rejoin -ControlPortBase 7800`,
  read `telemetry.network.starved_ms` from every window just before and
  after the close, and just before and after the rejoin, to tell which one
  stalls them. Then look at what the host sends then: the save upload to
  the joiner sharing the relay, or the turns that carry `Leave` and `Join`.
- **Where:** `Host::send_upload` and `depart` in `kestrel/net/src/host.rs`;
  the client's starved time in `kestrel/net/src/client.rs`.
- **Found in:** the eight-player lab run (runs `20261026-182404`).

## Spread name tags that overlap

- **Status:** open (low priority)
- **Why:** spawn spots are 1.2 m apart, so at spawn in a full game the eight
  tags sit on top of each other and few names can be read. They separate
  once players move. Client 5's orange tag also reads dimmer than the rest
  on the dark backing.
- **What to do:** nudge overlapping tags apart on screen (or stack them),
  and check the orange slot color's contrast.
- **Where:** `kestrel/app/src/nametags.rs` (`update`, `SLOT_COLORS`).
- **Found in:** the eight-player lab run.

## The network lab can't start Node through a mise shim

- **Status:** open (low priority)
- **Why:** on this machine `node` on PATH is a mise shim, and the repo has
  no mise config that picks a version, so the shim fails ("cannot find
  binary path") and the lab can't start its relay. The lab ran with an
  installed Node put first on PATH.
- **What to do:** pin Node for the repo (a `mise.toml` or `.tool-versions`
  entry), or let `network-lab.ps1` take a `-Node` path and say so when the
  `node` it finds can't run.
- **Where:** `kestrel/tools/network-lab.ps1` (the `Get-Command node` check).
- **Found in:** the eight-player lab run.

## Keep a client's movement prediction when its avatar's index moves

- **Status:** open (low priority)
- **Why:** when a player before you leaves, your avatar's index drops by
  one, and the prediction, which notes the index, starts over for a frame.
- **What to do:** check in the network lab whether it shows; if it does,
  key the prediction by player rather than index.
- **Where:** `kestrel/app/src/prediction.rs` (`Confirmed::agent`),
  `kestrel/app/src/lockstep.rs`.
- **Found in:** eight-player stage 2.

## Adapt the turn resend time to the round trip

- **Status:** open (low priority)
- **Why:** a turn still unacknowledged 300 ms after it was last sent goes
  again (`TURN_RESEND`). On round trips over 300 ms, turns that arrived are
  sent once more, which costs upload for nothing.
- **What to do:** the host measures each client's round trip from its pings
  and resends after that plus a margin.
- **Where:** `TURN_RESEND` and `Host::send` in `kestrel/net/src/host.rs`.
- **Found in:** eight-player stage 3.

## Cap how far ahead a client may request a tick

- **Status:** open
- **Why:** each input carries the tick the client wants it applied on, and
  the host files it under any tick that hasn't run yet, with no upper bound
  (`Host::inputs`, `net/src/host.rs`). A broken or hostile client could
  request ticks far in the future: the host keeps those commands in `open`
  until then, which grows memory without limit and lets a client schedule
  actions long in advance. Harmless with our own client, but worth closing
  before playing with strangers.
- **What to do:** reject or clamp inputs whose requested tick is more than
  a limit past the host's open tick (for example 2 s, well above any sane
  input margin), and count them in the remote's diagnostics next to
  `late_inputs`. Decide whether to clamp (keep the input) or drop it. Add a
  `net` test for both an over-limit input and one just under the limit.
- **Where:** `Host::inputs` in `kestrel/net/src/host.rs`; diagnostics in
  `kestrel/net/src/lib.rs`; Network window rows in
  `kestrel/app/src/telemetry/network.rs`.
- **Found in:** review of the adaptive input margin (PR #111).

## Let the input margin shrink back faster after a lag spike

- **Status:** open
- **Why:** the adaptive margin grows at once on a late input but shrinks by
  only 1 tick after each 3 s quiet period. After a 250 ms lag spike it takes
  about 40 s to return to normal, and confirmed results (hits, damage) arrive
  later than needed the whole time.
- **What to do:** shrink faster when the measured one-way time plus safety
  is far below the current margin (for example several ticks per quiet
  period, or straight to the measured value plus a tick), while keeping
  requested ticks monotonic. Check it doesn't oscillate (grow, shrink, late,
  grow) at 100 and 150 ± 20 ms.
- **Where:** `InputMargin` in `kestrel/net/src/margin.rs`; its `net` tests.
- **Found in:** stage 3 of PR #111 (notes in `kestrel/LASER_PREDICTION_HANDOFF.md`).

## Bring the input margin closer to the old fixed value at 75–100 ms

- **Status:** open
- **Why:** at 75–100 ms one way the adaptive margin settles 1–4 ticks above
  the old fixed 8, so confirmed results arrive 17–67 ms later there.
  Prediction hides it for the player's own movement and shots, not for hits
  on others.
- **What to do:** measure how much safety is actually needed (it is 1 tick
  plus 4× the late-side deviation; 3× was also clean in tests) on
  network-lab runs, and tune it down only if late inputs stay at zero.
- **Where:** `kestrel/net/src/margin.rs`; network-lab runs with
  `-DelayMs 75/100 -JitterMs 20`.
- **Found in:** stage 3 of PR #111.

## Re-measure the 67 ms jitter buffer

- **Status:** open
- **Why:** decision C4 left the client's jitter buffer alone until the
  adaptive margin landed. Now that it has, the buffer may be larger than
  needed, and it delays everything the client draws as confirmed (other
  players, enemies, hits).
- **What to do:** measure buffer underruns on network-lab runs (LAN, 75,
  100, 150 ± 20 ms) and decide whether to shrink it or make it adaptive
  like the margin.
- **Where:** the client's turn buffering in `kestrel/net/src/client.rs`;
  `kestrel/docs/network-diagnostics.md`.
- **Found in:** decision C4 in `kestrel/LASER_PREDICTION_HANDOFF.md`.

## Test laser prediction against moving targets and broken cover

- **Status:** open
- **Why:** the laser tests run in the laser pit, which has no enemies, so
  decision B1 (the predicted beam stops at targets as drawn) and cover
  broken just before a shot are untested. Those are the cases where the
  predicted and confirmed beams can disagree.
- **What to do:** add scenarios with a moving enemy or a second player
  crossing the line of fire, and with cover broken a few ticks before the
  shot. Measure how often a laser mispredict is logged and how far apart the
  ends are; decide whether that is acceptable.
- **Where:** laser scenarios in the `kestrel/app/src/lockstep.rs` tests;
  `kestrel/app/src/event_prediction.rs`.
- **Found in:** stage 2 of PR #111.

## The `Join` command is always late on a delayed link

- **Status:** open (low priority)
- **Why:** a joining client sends `Join` before it has a round-trip
  measurement, so on a delayed link the host always moves it to a later
  tick. Harmless today, but it counts as a late input in diagnostics and
  can hide real ones.
- **What to do:** either don't count `Join` as late, or send it only once
  the first round trip is measured.
- **Where:** `kestrel/net/src/client.rs` (queue, late-input detection);
  `Host::inputs` in `kestrel/net/src/host.rs`.
- **Found in:** stage 3 of PR #111.

## Done

### Play an eight-player network lab run

- **Status:** done (branch `kestrel-eight-player-lab`). Ran
  `network-lab.ps1 -Clients 7` at LAN and at 75 ± 20 ms, checking every
  window over its control port and with screenshots:
  - All 8 joined (slots 0–7, "hosting, 8 players"). Each spawned on its own
    spot of the 1.2 m grid, and every window showed the same 7 others at
    the same positions, also after each player walked to its own point.
  - Name tags show for all 8 in slot colors, and each window borders its
    own.
  - A client closing its window left at once (the host went to 7 players
    and its avatar disappeared everywhere), and it rejoined in its old
    slot at a free spawn spot, at LAN and at 75 ms.
  - A 9th player was turned away: "the game is full (8 players)".
  - Relay at LAN: nothing dropped or reordered, late p99 0.1 ms. At 75 ms:
    nothing dropped, reordering from the jitter, late p99 19 ms. Clients
    measured 150–166 ms round trips with 8–10 tick margins and no late
    inputs.
  The lab needed three changes for this: `-ControlPortBase` (a control port
  per window), `-Rejoin` (restart a client window that closes), and a relay
  that forgets clients silent for 3 s so a rejoin's new address gets a
  slot. Follow-ups: the stall around a leave and rejoin, overlapping tags,
  and the Node shim, above.
- **Why:** the multi-client relay was checked with fake peers and
  `network-lab.ps1 -Clients` was only parse-checked; no run had 8 real game
  windows.
- **Where:** `kestrel/tools/network-lab.ps1`, `kestrel/tools/network_lab/relay.mjs`.
- **Found in:** eight-player stage 3.

### Eight players: cap, slots, leaving, spawn spots, colors, upload, loading

- **Status:** done (https://github.com/juanzolotoochin/monorepo/pull/117; stages 1–5 of
  `kestrel/EIGHT_PLAYER_HANDOFF.md`). A host allows 8 players with
  reusable slots and saved player IDs (`--profile`), leaving removes the
  avatar for everyone (`CommandKind::Leave`, protocol 4, save format 12),
  joiners spawn at their own clear spots, name tags have slot colors, the
  host sends each turn 3 times instead of every packet (8 players online:
  3.3 → 1.4 Mbit/s), a turn never outgrows a packet (which could stall every
  client), joiners share a save, and a slow loader no longer holds a new
  world. The network lab relays several clients (`-Clients N`).
