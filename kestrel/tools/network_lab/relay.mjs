// A localhost UDP relay that delays each datagram, for feeling Kestrel over a
// worse network on one machine. Started by ../network-lab.ps1; see there.
//
//   clients <-> 127.0.0.1:<listen> (this relay) <-> 127.0.0.1:<host>
//
// Each client sends to the listen socket and hears replies from it, so its
// host-address check holds. The relay forwards each client to the host from
// a socket of its own, so the host sees every client as its own peer and
// replies to that socket. Up to --max-clients addresses are served; anything
// past them is dropped and counted.
//
// Each datagram is held for max(0, delay + uniform(-jitter, +jitter)) ms, one
// seeded random stream per client and direction. Datagrams may overtake one
// another.
//
// Windows timers wake in ~15.6 ms steps, which alone would add ~7 ms of
// delay. The relay sleeps on a timer until a delivery is close, then polls
// until it is due (about one CPU core while packets are in flight), and
// reports how late deliveries actually were. `--spin-ms 0` turns polling off.
//
// Node built-ins only. Status lives in the run directory: `ready.json` once
// both sockets are bound, `stats.json` every second and on exit. Creating
// `stop` in the run directory, or the parent process exiting, stops it.

import dgram from "node:dgram";
import fs from "node:fs";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { parseArgs } from "node:util";

const LOOPBACK = "127.0.0.1";

const { values: args } = parseArgs({
  options: {
    listen: { type: "string" },
    host: { type: "string" },
    "delay-ms": { type: "string" },
    "jitter-ms": { type: "string" },
    seed: { type: "string" },
    // Wake this long before a delivery and poll the rest: one Windows timer step.
    "spin-ms": { type: "string", default: "17" },
    "max-packets": { type: "string", default: "4096" },
    "max-bytes": { type: "string", default: String(8 * 1024 * 1024) },
    "max-clients": { type: "string", default: "16" },
    "run-dir": { type: "string" },
    "parent-pid": { type: "string" },
  },
  strict: true,
});

function integer(name, min, max) {
  const text = args[name];
  if (text === undefined || !/^\d+$/.test(text) || Number(text) < min || Number(text) > max) {
    fail(`--${name} must be an integer from ${min} to ${max}, got ${text}`);
  }
  return Number(text);
}

function fail(message) {
  console.error(`relay: ${message}`);
  process.exit(2);
}

const listenPort = integer("listen", 1, 65535);
const hostPort = integer("host", 1, 65535);
const delayMs = integer("delay-ms", 0, 10000);
const jitterMs = integer("jitter-ms", 0, 10000);
const seed = integer("seed", 0, 0xffffffff);
const spinMs = integer("spin-ms", 0, 100);
const maxPackets = integer("max-packets", 1, 1_000_000);
const maxBytes = integer("max-bytes", 1, 1 << 30);
const maxClients = integer("max-clients", 1, 64);
const parentPid = args["parent-pid"] === undefined ? null : integer("parent-pid", 1, 0xffffffff);
const runDir = args["run-dir"] ?? fail("--run-dir is required");
if (listenPort === hostPort) fail("--listen and --host must differ");

// mulberry32: small, seedable, good enough for delay samples.
function random(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// One direction of the relay: its random stream, its queue and its counters.
function direction(name, streamSeed, send) {
  return {
    name,
    random: random(streamSeed),
    send,
    received: 0,
    receivedBytes: 0,
    delivered: 0,
    deliveredBytes: 0,
    queueDrops: 0,
    queueDropBytes: 0,
    sendErrors: 0,
    reordered: 0,
    lastDeliveredSequence: -1,
    sequence: 0,
    queuedPackets: 0,
    queuedBytes: 0,
    peakPackets: 0,
    peakBytes: 0,
    lateness: [],
  };
}

const started = performance.now();
const startedAt = new Date().toISOString();
// Pending deliveries in both directions, sorted by due time.
const queue = [];
let timer = null;
let polling = false;
let strangerDrops = 0;
let stopping = false;

const listen = dgram.createSocket("udp4");
// By "address:port", in the order they first sent.
const clients = new Map();

// A client's first datagram gives it a socket toward the host and a delay
// stream each way. Null once --max-clients are served.
function clientFor(from) {
  const key = `${from.address}:${from.port}`;
  const known = clients.get(key);
  if (known || clients.size >= maxClients) return known ?? null;
  const n = clients.size;
  const upstream = dgram.createSocket("udp4");
  const client = {
    address: from.address,
    port: from.port,
    upstream,
    toHost: direction("client_to_host", (seed ^ 0x5eed0001 ^ Math.imul(n, 0x9e3779b9)) >>> 0, (bytes, done) =>
      upstream.send(bytes, hostPort, LOOPBACK, done),
    ),
    toClient: direction("host_to_client", (seed ^ 0x5eed0002 ^ Math.imul(n, 0x9e3779b9)) >>> 0, (bytes, done) =>
      listen.send(bytes, from.port, from.address, done),
    ),
  };
  upstream.on("message", (bytes, source) => {
    if (source.address !== LOOPBACK || source.port !== hostPort) {
      strangerDrops += 1;
      return;
    }
    enqueue(client.toClient, bytes);
  });
  watchErrors(upstream);
  upstream.bind({ port: 0, address: LOOPBACK });
  clients.set(key, client);
  return client;
}

function enqueue(dir, bytes) {
  dir.received += 1;
  dir.receivedBytes += bytes.length;
  if (dir.queuedPackets + 1 > maxPackets || dir.queuedBytes + bytes.length > maxBytes) {
    dir.queueDrops += 1;
    dir.queueDropBytes += bytes.length;
    return;
  }
  const hold = Math.max(0, delayMs + (dir.random() * 2 - 1) * jitterMs);
  const entry = { dir, bytes, due: performance.now() + hold, sequence: dir.sequence++ };
  let low = 0;
  let high = queue.length;
  while (low < high) {
    const mid = (low + high) >> 1;
    if (queue[mid].due <= entry.due) low = mid + 1;
    else high = mid;
  }
  queue.splice(low, 0, entry);
  dir.queuedPackets += 1;
  dir.queuedBytes += bytes.length;
  dir.peakPackets = Math.max(dir.peakPackets, dir.queuedPackets);
  dir.peakBytes = Math.max(dir.peakBytes, dir.queuedBytes);
  schedule();
}

function deliverDue() {
  const now = performance.now();
  let count = 0;
  while (count < queue.length && queue[count].due <= now) count += 1;
  for (const { dir, bytes, due, sequence } of queue.splice(0, count)) {
    dir.queuedPackets -= 1;
    dir.queuedBytes -= bytes.length;
    if (sequence < dir.lastDeliveredSequence) dir.reordered += 1;
    dir.lastDeliveredSequence = Math.max(dir.lastDeliveredSequence, sequence);
    recordLateness(dir, now - due);
    dir.send(bytes, (error) => {
      if (error) dir.sendErrors += 1;
      else {
        dir.delivered += 1;
        dir.deliveredBytes += bytes.length;
      }
    });
  }
}

// Keeps a bounded sample of how late deliveries were, for the stats.
function recordLateness(dir, ms) {
  if (dir.lateness.length < 4096) dir.lateness.push(ms);
  else dir.lateness[Math.floor(dir.random() * 4096)] = ms;
}

function schedule() {
  if (timer !== null) {
    clearTimeout(timer);
    timer = null;
  }
  if (queue.length === 0 || polling) return;
  const wait = queue[0].due - performance.now();
  if (wait <= spinMs) {
    polling = true;
    setImmediate(poll);
  } else {
    timer = setTimeout(() => {
      timer = null;
      schedule();
    }, wait - spinMs);
  }
}

function poll() {
  polling = false;
  deliverDue();
  schedule();
}

listen.on("message", (bytes, from) => {
  const client = clientFor(from);
  if (client === null) {
    strangerDrops += 1;
    return;
  }
  enqueue(client.toHost, bytes);
});

// Windows reports an earlier send's ICMP "port unreachable" (the host not
// up yet) as a socket error; it is packet loss, not a reason to stop.
function watchErrors(socket) {
  socket.on("error", (error) => {
    if (error.code === "ECONNRESET") return;
    console.error(`relay: socket error: ${error.message}`);
    stop(1);
  });
}
watchErrors(listen);

function percentile(sorted, p) {
  if (sorted.length === 0) return null;
  return round(sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))]);
}

function round(ms) {
  return Math.round(ms * 100) / 100;
}

function summary(dir) {
  const lateness = [...dir.lateness].sort((a, b) => a - b);
  return {
    received: dir.received,
    received_bytes: dir.receivedBytes,
    delivered: dir.delivered,
    delivered_bytes: dir.deliveredBytes,
    queue_cap_drops: dir.queueDrops,
    queue_cap_drop_bytes: dir.queueDropBytes,
    send_errors: dir.sendErrors,
    reordered: dir.reordered,
    queued_packets: dir.queuedPackets,
    queued_bytes: dir.queuedBytes,
    peak_queued_packets: dir.peakPackets,
    peak_queued_bytes: dir.peakBytes,
    lateness_ms: { samples: lateness.length, p50: percentile(lateness, 0.5), p99: percentile(lateness, 0.99), max: percentile(lateness, 1) },
  };
}

function writeJson(file, value) {
  const target = path.join(runDir, file);
  const temp = `${target}.tmp`;
  fs.writeFileSync(temp, JSON.stringify(value, null, 2));
  fs.renameSync(temp, target);
}

// Every client's direction `name` as one, for the totals.
function combined(name) {
  const dirs = [...clients.values()].map((c) => (name === "client_to_host" ? c.toHost : c.toClient));
  const total = direction(name, 0, null);
  for (const dir of dirs) {
    for (const key of Object.keys(total)) {
      const summed = !["sequence", "lastDeliveredSequence", "peakPackets", "peakBytes"].includes(key);
      if (typeof total[key] === "number" && summed) total[key] += dir[key];
    }
    total.lateness.push(...dir.lateness);
    total.peakPackets = Math.max(total.peakPackets, dir.peakPackets);
    total.peakBytes = Math.max(total.peakBytes, dir.peakBytes);
  }
  return summary(total);
}

function writeStats(state) {
  writeJson("stats.json", {
    state,
    started_at: startedAt,
    uptime_s: Math.round((performance.now() - started) / 100) / 10,
    stranger_drops: strangerDrops,
    // Totals over every client; peaks are the largest single client's.
    client_to_host: combined("client_to_host"),
    host_to_client: combined("host_to_client"),
    clients: [...clients.values()].map((c) => ({
      address: c.address,
      port: c.port,
      client_to_host: summary(c.toHost),
      host_to_client: summary(c.toClient),
    })),
  });
}

function stop(code) {
  if (stopping) return;
  stopping = true;
  try {
    writeStats("stopped");
  } catch (error) {
    console.error(`relay: cannot write stats: ${error.message}`);
  }
  listen.close();
  for (const client of clients.values()) client.upstream.close();
  process.exit(code);
}

function bind(socket, port) {
  return new Promise((resolve, reject) => {
    socket.once("error", reject);
    socket.bind({ port, address: LOOPBACK, exclusive: true }, () => {
      socket.off("error", reject);
      resolve();
    });
  });
}

try {
  await bind(listen, listenPort);
} catch (error) {
  fail(`cannot bind: ${error.message}`);
}

writeJson("ready.json", {
  pid: process.pid,
  listen: `${LOOPBACK}:${listenPort}`,
  host: `${LOOPBACK}:${hostPort}`,
  max_clients: maxClients,
  delay_ms: delayMs,
  jitter_ms: jitterMs,
  seed,
  spin_ms: spinMs,
  max_packets: maxPackets,
  max_bytes: maxBytes,
});
console.log(`relay: ready on ${LOOPBACK}:${listenPort} -> ${LOOPBACK}:${hostPort}`);

setInterval(() => {
  if (fs.existsSync(path.join(runDir, "stop"))) return stop(0);
  if (parentPid !== null) {
    try {
      process.kill(parentPid, 0);
    } catch {
      return stop(0);
    }
  }
  writeStats("running");
}, 1000);

process.on("SIGINT", () => stop(0));
process.on("SIGTERM", () => stop(0));
