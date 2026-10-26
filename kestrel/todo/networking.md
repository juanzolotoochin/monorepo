# Networking

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

## Done
