# Legacy demo and deterministic-engine boundary

`tools/gameplay/core` and `examples/rust/bevy_isometric` remain the playable
prototype. The target architecture is [architecture.md](architecture.md): a
30 Hz fixed-point simulation under `game/`, with Bevy consuming extracts and
sending tick-stamped commands. The prototype is **not** a lockstep peer yet.

The prototype now has exact-tick stepping, a same-build replay fingerprint,
counter-keyed combat variation, integer-tick attack contact, swept projectile
collision, and a Bazel test
that replays the same command script twice. These catch accidental state drift
while combat and content are still being explored. A replay fingerprint is not
a rules hash or a cross-platform desync hash: the prototype still uses `f32`,
frame-duration accumulation, and player movement authored in Bevy.

Scripted runs feed the simulation an exact duration per script frame. The local
`game_demo` action also runs the same script headlessly and compares gameplay
fingerprints at every completed command before encoding the video. This checks
rendered/headless parity for the prototype's settlement scenes.

The migration must preserve these boundaries:

1. Move player movement, attack windup input, transfers, and AI decisions
   behind a tick-stamped command API in `game/sim`. Remove
   `set_controlled_pose` from the view path. Interactive input and demo scripts
   must produce the same command type and run through the same turn path.
2. Port simulation math to `game/fx`, including collision, navigation and
   projectile flight. Use integer ticks for timers and counter-based random
   streams keyed by world seed, tick, entity and purpose.
3. Author rules separately from visual content. Validate the rules hash at
   session start and in save/replay metadata. Use stable scene/content names in
   files, remapping to dense runtime IDs at load.
4. Produce a canonical per-tick hash over the authoritative state, including
   IDs, queues, reservations, RNG-relevant counters, and sleep state. Exclude
   rendering, audio, telemetry and wall-clock data. Compare golden hashes on
   Linux, Windows and Steam Deck, plus save-and-continue and thread-count runs.
5. Rebuild `game_demo` on a tick-indexed input log. Keep the world-state dump
   as a test artifact; video and audio are optional views of that replay. Add
   assertion checks for collision outcomes and presentation attachments.

The existing `combat_replay_test` checks same-build reproducibility. It cannot
prove cross-platform lockstep or rendered/headless parity. Those become release
gates when the authoritative `game/sim` path replaces the prototype.
