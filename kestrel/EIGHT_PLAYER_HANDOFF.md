# Kestrel eight-player handoff

## Goal

A Kestrel game holds up to 8 players: the host and 7 clients. Every player
gets their own avatar at a free spot, can leave and come back, and can tell
the other seven apart. The host refuses a ninth player with a clear reason.

The limit may go up to 16 later, for LAN games. So nothing hard-codes 8: the
cap is a setting (`NetConfig::max_players`), and spawn spots and slot colors
come from lists that already hold more than 16.

## Status

All five stages are built on branch `kestrel-eight-players`, with tests.
What follows is what was built and why; open follow-ups are in
`todo/networking.md`.

## Where things stood

The lockstep core had no fixed player count: `Host` kept a `Vec` of remotes,
`Settlement::join` gave each `PlayerId` an avatar, and the sim treated all
controlled agents alike. What broke or went untested past 2 players:

| Gap | Fixed by |
|---|---|
| No player cap | D1 |
| Player IDs only grew, and the app's stable ID was random per run, so every rejoin was a new player | D2 |
| Leaving kept the avatar forever (standing, targetable, holding its job) | D3 |
| Joiners stood in a row from the spawn, by count: into cover at 8, or on top of each other after a leave | D4 |
| Every other player's tag looked the same | D5 |
| Host upload not measured; it turned out to be 3.3 Mbit/s at 8 players over a 200 ms round trip | D6 |
| A turn with more commands than fit in a packet could never be sent, and every client waited for it forever | D6 |
| A save per joiner; one slow loader held a new world at tick 0 for everyone | D7 |
| Tests stopped at 2 players; the network lab relayed 1 client | D9 |

## Decisions, as built

- **D1. Cap.** `NetConfig::max_players`, default 8 (`DEFAULT_MAX_PLAYERS`),
  counting the host. A new player past it gets
  `Reject { reason: "the game is full (8 players)" }` (`Host::hello`).
- **D2. Slots.** Clients are `PlayerId` 1 to `max_players - 1`. A new player
  takes the lowest slot nobody holds, else the slot of whoever left longest
  ago (`Host::new_slot`). A player who leaves keeps their slot for their
  stable ID until it is needed. The app keeps its stable ID in
  `kestrel-profile.id` in the temp folder (`stable_id`,
  `app/src/lockstep.rs`), or `kestrel-profile-<name>.id` with
  `--profile <name>`. A join with a stable ID that is still playing from
  another address (heard within 1 s) is refused with a hint to use
  `--profile`, since it is a second copy of the game, not a reconnect.
- **D3. Leaving removes the avatar.** The host queues `CommandKind::Leave`
  for a player it drops (`Host::depart`), so every peer removes the avatar on
  the same tick. `Leave` is the last `CommandKind` variant, so older input
  logs still decode; protocol version 4.
  - `Leave` commands apply first in their tick, so the tick's other commands
    and events see the agents without the leaver.
  - `Settlement::leave` (`sim/src/settlement/players.rs`) releases the
    avatar's manual job, lands units flying to or from it at once, writes off
    what it carried in `Accounting::abandoned` (save format 12), drops its
    piano notes, threat windups, dock ownership and pile-lab hauler, removes
    it, and shifts every stored index above it (`remap_agents`: dock owners,
    transfer endpoints, windups, piano notes, pile haulers).
  - The scene's first avatar (the host's) never leaves: every joiner is a
    copy of it.
  - Joins append and nothing else adds agents, so only joined avatars after
    the leaver move.
  - A command that drives an avatar now drives its sender's own avatar,
    whatever index it names (`World::apply_commands`): a command sent just
    before someone left names an old index. Nobody can drive another
    player's avatar either way. `players_equip_their_own_weapons` changed to
    match.
  - The app keys other players' bodies by `PlayerId` (`app/src/peers.rs`),
    and finds the previous tick's state of an agent by owner
    (`Settlement::same_agent_in`), for its own avatar, peers and the legacy
    view, so nothing jumps between two players' states.
- **D4. Spawn spots by slot.** Spots are a grid at 1.2 m around the first
  avatar's home (where a scene puts its spawn), nearest first: 48 in three
  rings. Slot n starts at the n-th spot and takes the first one that is clear
  of cover, reachable on foot, and nobody else's home. The spot is the
  avatar's home, so respawns use it. Scenes can't list their own spots yet;
  nothing needed it.
- **D5. Slot colors on name tags.** 16 colors (`SLOT_COLORS`,
  `app/src/nametags.rs`), based on Okabe-Ito, tint each player's name tag;
  your own tag also has a border. Default names are "PLAYER n" by slot. The
  suit accent is a follow-up.
- **D6. Bandwidth, measured.** The `net` test
  `the_host_upload_fits_eight_players_online_and_sixteen_on_a_lan` has every
  player aiming every tick and moving every half second:
  - Before: 3.31 Mbit/s up from the host for 8 players over
    `LinkConditions::internet()` (100 ms ± 40 each way, 3% loss), because
    each packet repeated about 15 unacknowledged turns and filled to 1200 B.
  - Now each turn goes in 3 packets in a row, then again only if still
    unacknowledged 300 ms after it was last sent (`TURN_REPEATS`,
    `TURN_RESEND`). The client already handled gaps, so the protocol didn't
    change. 8 players online: 1.40 Mbit/s. 16 players on a 1 ms LAN:
    2.11 Mbit/s.
  - Measuring 16 found the stuck-turn bug: inputs queued while 15 clients
    loaded all landed in one turn bigger than a packet. The host now never
    lets a turn outgrow one (`file`, `TURN_BUDGET`): commands that don't fit
    go to the next tick, and each player's commands stay in order.
    `a_burst_of_inputs_spills_into_later_turns_in_order` covers it.
- **D7. Loading.** Joiners on the same tick share one save
  (`Host::upload` caches it by world and tick). A new world waits at its
  first tick for at most `NetConfig::load_timeout` (20 s); a client still
  loading then gets a `Reject` that says why and is dropped, and can join
  again mid-game.
- **D8. Unchanged.** Desync policy, the memory trial (host's avatar only).
  The sim's cost is not a concern: 8 players fighting 12 ghouls take about
  0.03 ms a tick (`eight_players_fight_and_come_and_go_deterministically`).
- **D9. Tests and tools.**
  - `net`: full game, slot reuse (free slot first, then oldest departure),
    second copy refused, leave in the turns for everyone, upload, burst, slow
    loader, protocol 4.
  - `sim`: spawn spots clear of cover and apart; leave removes the avatar and
    later avatars keep their players and commands; leave remaps references
    and balances material; 8 players coming and going, deterministic on 1
    and 4 threads.
  - App: slot colors distinct.
  - Network lab: the relay gives each client its own socket toward the host
    (`--max-clients`, default 16), and `network-lab.ps1 -Clients N` starts N
    client windows, each with its own `--profile`.

## Known limits

- When a player before you leaves, your own avatar's index changes, so the
  client's movement prediction starts over for a frame
  (`app/src/prediction.rs`). Not visible in tests; worth a look in the
  network lab.
- `TURN_RESEND` is fixed at 300 ms. On round trips longer than that, turns
  that did arrive are sent once more. Harmless, but it could adapt to the
  measured round trip.
- The game has no `--max-players` flag yet, so a 16-player LAN game needs a
  code change (`NetConfig::max_players`).
- The network lab with several game windows was not run in this session:
  the relay was checked with fake peers and the launcher only parse-checked.
