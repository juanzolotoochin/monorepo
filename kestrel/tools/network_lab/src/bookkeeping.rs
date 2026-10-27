//! The relay's bookkeeping without sockets or clocks: who the clients are,
//! when each held datagram is due, and the counters. Times are milliseconds
//! since the relay started; `main.rs` reads the clock and moves the bytes.

use serde::Serialize;
use std::net::SocketAddr;

/// What the command line sets, besides where to write files.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub delay_ms: u32,
    pub jitter_ms: u32,
    pub seed: u32,
    /// Wake this long before a delivery and poll the rest: one Windows timer step.
    pub spin_ms: u32,
    pub max_packets: usize,
    pub max_bytes: usize,
    pub max_clients: usize,
    /// Clients send every tick, so a few seconds of silence means it closed.
    /// 0 never forgets one.
    pub idle_ms: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            delay_ms: 0,
            jitter_ms: 0,
            seed: 0,
            spin_ms: 17,
            max_packets: 4096,
            max_bytes: 8 * 1024 * 1024,
            max_clients: 16,
            idle_ms: 3000,
        }
    }
}

/// mulberry32: small, seedable, good enough for delay samples. Gives the
/// same stream as the Node relay it replaced for the same seed.
#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(seed)
    }

    /// Uniform in [0, 1).
    pub fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

/// The seed of client `n`'s stream in direction `way`, as the Node relay had it.
pub fn stream_seed(seed: u32, way: Way, n: u32) -> u32 {
    let salt = match way {
        Way::ToHost => 0x5eed_0001,
        Way::ToClient => 0x5eed_0002,
    };
    seed ^ salt ^ n.wrapping_mul(0x9e37_79b9)
}

/// How long to hold a datagram: max(0, delay + uniform(-jitter, +jitter)).
pub fn hold_ms(delay_ms: u32, jitter_ms: u32, rng: &mut Rng) -> f64 {
    (f64::from(delay_ms) + (rng.next() * 2.0 - 1.0) * f64::from(jitter_ms)).max(0.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    ToHost,
    ToClient,
}

/// How many lateness samples each direction keeps.
const LATENESS_SAMPLES: usize = 4096;

/// One direction of one client: its random stream and its counters.
#[derive(Clone, Debug)]
pub struct Direction {
    rng: Rng,
    pub received: u64,
    pub received_bytes: u64,
    pub delivered: u64,
    pub delivered_bytes: u64,
    pub queue_drops: u64,
    pub queue_drop_bytes: u64,
    pub send_errors: u64,
    pub reordered: u64,
    last_delivered_sequence: Option<u64>,
    sequence: u64,
    pub queued_packets: usize,
    pub queued_bytes: usize,
    pub peak_packets: usize,
    pub peak_bytes: usize,
    /// A bounded sample of how late deliveries were, in ms.
    pub lateness: Vec<f64>,
}

impl Direction {
    fn new(seed: u32) -> Self {
        Direction {
            rng: Rng::new(seed),
            received: 0,
            received_bytes: 0,
            delivered: 0,
            delivered_bytes: 0,
            queue_drops: 0,
            queue_drop_bytes: 0,
            send_errors: 0,
            reordered: 0,
            last_delivered_sequence: None,
            sequence: 0,
            queued_packets: 0,
            queued_bytes: 0,
            peak_packets: 0,
            peak_bytes: 0,
            lateness: Vec::new(),
        }
    }

    fn record_lateness(&mut self, ms: f64) {
        if self.lateness.len() < LATENESS_SAMPLES {
            self.lateness.push(ms);
        } else {
            let i = (self.rng.next() * LATENESS_SAMPLES as f64) as usize;
            self.lateness[i] = ms;
        }
    }

    /// Adds `other` into these totals; peaks are the larger of the two.
    fn absorb(&mut self, other: &Direction) {
        self.received += other.received;
        self.received_bytes += other.received_bytes;
        self.delivered += other.delivered;
        self.delivered_bytes += other.delivered_bytes;
        self.queue_drops += other.queue_drops;
        self.queue_drop_bytes += other.queue_drop_bytes;
        self.send_errors += other.send_errors;
        self.reordered += other.reordered;
        self.queued_packets += other.queued_packets;
        self.queued_bytes += other.queued_bytes;
        self.peak_packets = self.peak_packets.max(other.peak_packets);
        self.peak_bytes = self.peak_bytes.max(other.peak_bytes);
        self.lateness.extend_from_slice(&other.lateness);
    }

    pub fn summary(&self) -> DirectionStats {
        let mut lateness = self.lateness.clone();
        lateness.sort_by(f64::total_cmp);
        DirectionStats {
            received: self.received,
            received_bytes: self.received_bytes,
            delivered: self.delivered,
            delivered_bytes: self.delivered_bytes,
            queue_cap_drops: self.queue_drops,
            queue_cap_drop_bytes: self.queue_drop_bytes,
            send_errors: self.send_errors,
            reordered: self.reordered,
            queued_packets: self.queued_packets,
            queued_bytes: self.queued_bytes,
            peak_queued_packets: self.peak_packets,
            peak_queued_bytes: self.peak_bytes,
            lateness_ms: Lateness {
                samples: lateness.len(),
                p50: percentile(&lateness, 0.5),
                p99: percentile(&lateness, 0.99),
                max: percentile(&lateness, 1.0),
            },
        }
    }
}

/// The value at fraction `p` of `sorted`, rounded to 0.01 ms.
pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let i = ((p * sorted.len() as f64).floor() as usize).min(sorted.len() - 1);
    Some(round2(sorted[i]))
}

fn round2(ms: f64) -> f64 {
    (ms * 100.0).round() / 100.0
}

/// A client being served, or one forgotten.
#[derive(Clone, Debug)]
pub struct Client {
    /// Clients served before this one; numbers its delay streams and names
    /// its socket toward the host in `main.rs`.
    pub id: u32,
    pub addr: SocketAddr,
    pub last_heard: f64,
    pub to_host: Direction,
    pub to_client: Direction,
    pub retired_at: Option<String>,
}

impl Client {
    fn way_mut(&mut self, way: Way) -> &mut Direction {
        match way {
            Way::ToHost => &mut self.to_host,
            Way::ToClient => &mut self.to_client,
        }
    }
}

/// What `Relay::admit` made of a datagram's sender.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admit {
    Known(u32),
    /// A new client: give it a socket toward the host.
    New(u32),
    /// --max-clients are being served: drop it.
    Full,
}

#[derive(Clone, Debug)]
struct Pending {
    client: u32,
    way: Way,
    bytes: Vec<u8>,
    due: f64,
    sequence: u64,
}

/// A datagram whose time has come.
#[derive(Clone, Debug, PartialEq)]
pub struct Delivery {
    pub client: u32,
    /// Where the client is, for `Way::ToClient`.
    pub addr: SocketAddr,
    pub way: Way,
    pub bytes: Vec<u8>,
}

pub struct Relay {
    pub config: Config,
    /// Pending deliveries in both directions, sorted by due time.
    queue: Vec<Pending>,
    /// Clients being served, in the order they first sent.
    clients: Vec<Client>,
    /// Forgotten clients, kept for the totals.
    retired: Vec<Client>,
    served: u32,
    pub stranger_drops: u64,
}

impl Relay {
    pub fn new(config: Config) -> Self {
        Relay { config, queue: Vec::new(), clients: Vec::new(), retired: Vec::new(), served: 0, stranger_drops: 0 }
    }

    /// A client's first datagram gives it a delay stream each way.
    pub fn admit(&mut self, addr: SocketAddr, now: f64) -> Admit {
        if let Some(client) = self.clients.iter().find(|c| c.addr == addr) {
            return Admit::Known(client.id);
        }
        if self.clients.len() >= self.config.max_clients {
            return Admit::Full;
        }
        let id = self.served;
        self.served += 1;
        let seed = self.config.seed;
        self.clients.push(Client {
            id,
            addr,
            last_heard: now,
            to_host: Direction::new(stream_seed(seed, Way::ToHost, id)),
            to_client: Direction::new(stream_seed(seed, Way::ToClient, id)),
            retired_at: None,
        });
        Admit::New(id)
    }

    /// A datagram from a client: admits the sender and holds the datagram,
    /// or counts a stranger when --max-clients are being served.
    pub fn from_client(&mut self, addr: SocketAddr, bytes: &[u8], now: f64) -> Admit {
        let admit = self.admit(addr, now);
        match admit {
            Admit::Full => self.stranger_drops += 1,
            Admit::Known(id) | Admit::New(id) => {
                if let Some(client) = self.client_mut(id) {
                    client.last_heard = now;
                }
                self.enqueue(id, Way::ToHost, bytes, now);
            }
        }
        admit
    }

    /// A datagram the host sent to client `id`'s socket.
    pub fn from_host(&mut self, id: u32, bytes: &[u8], now: f64) {
        self.enqueue(id, Way::ToClient, bytes, now);
    }

    pub fn stranger(&mut self) {
        self.stranger_drops += 1;
    }

    fn client_mut(&mut self, id: u32) -> Option<&mut Client> {
        self.clients.iter_mut().find(|c| c.id == id)
    }

    fn enqueue(&mut self, id: u32, way: Way, bytes: &[u8], now: f64) {
        let Config { delay_ms, jitter_ms, max_packets, max_bytes, .. } = self.config;
        let Some(client) = self.clients.iter_mut().find(|c| c.id == id) else { return };
        let dir = client.way_mut(way);
        dir.received += 1;
        dir.received_bytes += bytes.len() as u64;
        if dir.queued_packets + 1 > max_packets || dir.queued_bytes + bytes.len() > max_bytes {
            dir.queue_drops += 1;
            dir.queue_drop_bytes += bytes.len() as u64;
            return;
        }
        let due = now + hold_ms(delay_ms, jitter_ms, &mut dir.rng);
        let sequence = dir.sequence;
        dir.sequence += 1;
        dir.queued_packets += 1;
        dir.queued_bytes += bytes.len();
        dir.peak_packets = dir.peak_packets.max(dir.queued_packets);
        dir.peak_bytes = dir.peak_bytes.max(dir.queued_bytes);
        // After any entry due at the same time, so those keep their order.
        let at = self.queue.partition_point(|p| p.due <= due);
        self.queue.insert(at, Pending { client: id, way, bytes: bytes.to_vec(), due, sequence });
    }

    /// When the next delivery is due, if any is pending.
    pub fn next_due(&self) -> Option<f64> {
        self.queue.first().map(|p| p.due)
    }

    /// Takes every delivery due by `now`, recording how late each one is.
    pub fn take_due(&mut self, now: f64) -> Vec<Delivery> {
        let count = self.queue.partition_point(|p| p.due <= now);
        let due: Vec<Pending> = self.queue.drain(..count).collect();
        let mut out = Vec::with_capacity(due.len());
        for p in due {
            let Some(client) = self.find_any_mut(p.client) else { continue };
            let addr = client.addr;
            let dir = client.way_mut(p.way);
            dir.queued_packets -= 1;
            dir.queued_bytes -= p.bytes.len();
            if dir.last_delivered_sequence.is_some_and(|last| p.sequence < last) {
                dir.reordered += 1;
            }
            dir.last_delivered_sequence = Some(dir.last_delivered_sequence.map_or(p.sequence, |l| l.max(p.sequence)));
            dir.record_lateness(now - p.due);
            out.push(Delivery { client: p.client, addr, way: p.way, bytes: p.bytes });
        }
        out
    }

    /// Records whether sending a delivery worked.
    pub fn sent(&mut self, id: u32, way: Way, len: usize, ok: bool) {
        let Some(client) = self.find_any_mut(id) else { return };
        let dir = client.way_mut(way);
        if ok {
            dir.delivered += 1;
            dir.delivered_bytes += len as u64;
        } else {
            dir.send_errors += 1;
        }
    }

    /// A client being served or forgotten: a forgotten client's replies
    /// still in the queue go out.
    fn find_any_mut(&mut self, id: u32) -> Option<&mut Client> {
        self.clients.iter_mut().chain(self.retired.iter_mut()).find(|c| c.id == id)
    }

    /// Forgets clients silent for --idle-ms with nothing left to send to the
    /// host, so their socket toward the host can close. Returns their ids.
    pub fn retire_idle(&mut self, now: f64, wall_time: &str) -> Vec<u32> {
        let idle = f64::from(self.config.idle_ms);
        if self.config.idle_ms == 0 {
            return Vec::new();
        }
        let mut gone = Vec::new();
        let mut i = 0;
        while i < self.clients.len() {
            let c = &self.clients[i];
            if now - c.last_heard < idle || c.to_host.queued_packets > 0 {
                i += 1;
                continue;
            }
            let mut client = self.clients.remove(i);
            client.retired_at = Some(wall_time.to_string());
            gone.push(client.id);
            self.retired.push(client);
        }
        gone
    }

    #[cfg(test)]
    pub fn active_clients(&self) -> usize {
        self.clients.len()
    }

    /// Forgotten clients first, then those being served.
    fn all_clients(&self) -> impl Iterator<Item = &Client> {
        self.retired.iter().chain(self.clients.iter())
    }

    fn combined(&self, way: Way) -> DirectionStats {
        let mut total = Direction::new(0);
        for c in self.all_clients() {
            total.absorb(match way {
                Way::ToHost => &c.to_host,
                Way::ToClient => &c.to_client,
            });
        }
        total.summary()
    }

    pub fn stats(&self, state: &str, started_at: &str, uptime_ms: f64) -> Stats {
        Stats {
            state: state.to_string(),
            started_at: started_at.to_string(),
            uptime_s: (uptime_ms / 100.0).round() / 10.0,
            stranger_drops: self.stranger_drops,
            client_to_host: self.combined(Way::ToHost),
            host_to_client: self.combined(Way::ToClient),
            active_clients: self.clients.len(),
            clients: self
                .all_clients()
                .map(|c| ClientStats {
                    address: c.addr.ip().to_string(),
                    port: c.addr.port(),
                    retired_at: c.retired_at.clone(),
                    client_to_host: c.to_host.summary(),
                    host_to_client: c.to_client.summary(),
                })
                .collect(),
        }
    }
}

/// `stats.json`, which `network-lab.ps1` reads.
#[derive(Serialize, Debug)]
pub struct Stats {
    pub state: String,
    pub started_at: String,
    pub uptime_s: f64,
    pub stranger_drops: u64,
    /// Totals over every client; peaks are the largest single client's.
    pub client_to_host: DirectionStats,
    pub host_to_client: DirectionStats,
    pub active_clients: usize,
    /// Forgotten clients first, with when they were forgotten.
    pub clients: Vec<ClientStats>,
}

#[derive(Serialize, Debug)]
pub struct ClientStats {
    pub address: String,
    pub port: u16,
    pub retired_at: Option<String>,
    pub client_to_host: DirectionStats,
    pub host_to_client: DirectionStats,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct DirectionStats {
    pub received: u64,
    pub received_bytes: u64,
    pub delivered: u64,
    pub delivered_bytes: u64,
    pub queue_cap_drops: u64,
    pub queue_cap_drop_bytes: u64,
    pub send_errors: u64,
    pub reordered: u64,
    pub queued_packets: usize,
    pub queued_bytes: usize,
    pub peak_queued_packets: usize,
    pub peak_queued_bytes: usize,
    pub lateness_ms: Lateness,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Lateness {
    pub samples: usize,
    pub p50: Option<f64>,
    pub p99: Option<f64>,
    pub max: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn config(delay_ms: u32, jitter_ms: u32) -> Config {
        Config { delay_ms, jitter_ms, seed: 42, ..Config::default() }
    }

    #[test]
    fn rng_matches_the_node_relay() {
        // Outputs of the Node relay's mulberry32 for seed 42, and the seed
        // and first output of the second client's to-client stream.
        let mut rng = Rng::new(42);
        let first: Vec<f64> = (0..3).map(|_| rng.next()).collect();
        assert_eq!(first, vec![0.6011037519201636, 0.44829055899754167, 0.8524657934904099]);
        assert_eq!(stream_seed(42, Way::ToHost, 0), 42 ^ 0x5eed_0001);
        assert_eq!(stream_seed(42, Way::ToClient, 1), 3_235_543_441);
        assert_eq!(Rng::new(3_235_543_441).next(), 0.5170173910446465);
    }

    #[test]
    fn same_seed_same_delays() {
        let holds = |seed| {
            let mut rng = Rng::new(seed);
            (0..100).map(|_| hold_ms(75, 20, &mut rng)).collect::<Vec<_>>()
        };
        assert_eq!(holds(7), holds(7));
        assert_ne!(holds(7), holds(8));
        for hold in holds(7) {
            assert!((55.0..=95.0).contains(&hold), "{hold}");
        }
    }

    #[test]
    fn hold_never_goes_negative() {
        let mut rng = Rng::new(1);
        let holds: Vec<f64> = (0..1000).map(|_| hold_ms(5, 50, &mut rng)).collect();
        assert!(holds.iter().all(|&h| h >= 0.0));
        assert!(holds.iter().any(|&h| h == 0.0));
        assert!(holds.iter().any(|&h| h > 40.0));
    }

    #[test]
    fn delivers_after_the_delay_in_due_order() {
        let mut relay = Relay::new(config(50, 0));
        assert_eq!(relay.from_client(addr(1000), b"a", 0.0), Admit::New(0));
        assert_eq!(relay.from_client(addr(1000), b"b", 10.0), Admit::Known(0));
        relay.from_host(0, b"reply", 20.0);
        assert_eq!(relay.next_due(), Some(50.0));
        assert!(relay.take_due(49.9).is_empty());
        let out = relay.take_due(61.0);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].way, out[0].bytes.as_slice()), (Way::ToHost, &b"a"[..]));
        assert_eq!((out[1].way, out[1].bytes.as_slice()), (Way::ToHost, &b"b"[..]));
        let out = relay.take_due(70.0);
        assert_eq!(out, vec![Delivery { client: 0, addr: addr(1000), way: Way::ToClient, bytes: b"reply".to_vec() }]);
        relay.sent(out[0].client, out[0].way, out[0].bytes.len(), true);
        let stats = relay.stats("running", "t", 0.0);
        assert_eq!(stats.client_to_host.received, 2);
        assert_eq!(stats.client_to_host.peak_queued_packets, 2);
        assert_eq!(stats.client_to_host.queued_packets, 0);
        assert_eq!(stats.host_to_client.delivered, 1);
        assert_eq!(stats.host_to_client.delivered_bytes, 5);
        // 61 - 50, 61 - 60 and 70 - 70 ms late.
        assert_eq!(stats.client_to_host.lateness_ms.max, Some(11.0));
        assert_eq!(stats.host_to_client.lateness_ms.p99, Some(0.0));
    }

    #[test]
    fn counts_datagrams_that_overtake() {
        let mut relay = Relay::new(config(100, 100));
        for i in 0..200 {
            relay.from_client(addr(1000), &[i as u8], f64::from(i));
        }
        let out = relay.take_due(1000.0);
        assert_eq!(out.len(), 200);
        let reordered = relay.stats("running", "t", 0.0).client_to_host.reordered;
        assert!(reordered > 10, "{reordered}");
    }

    #[test]
    fn same_seed_same_schedule() {
        let schedule = || {
            let mut relay = Relay::new(config(75, 20));
            let mut dues = Vec::new();
            for i in 0..50 {
                relay.from_client(addr(1000 + (i % 2)), b"x", f64::from(i));
                relay.from_host((i % 2) as u32, b"y", f64::from(i));
            }
            while let Some(due) = relay.next_due() {
                let n = relay.take_due(due).len();
                dues.push((due, n));
            }
            dues
        };
        assert_eq!(schedule(), schedule());
    }

    #[test]
    fn caps_the_queue_and_the_clients() {
        let mut relay = Relay::new(Config { max_packets: 2, max_bytes: 10, max_clients: 1, ..config(50, 0) });
        relay.from_client(addr(1000), b"1234", 0.0);
        relay.from_client(addr(1000), b"1234", 0.0);
        relay.from_client(addr(1000), b"1", 0.0); // over 2 packets
        assert_eq!(relay.from_client(addr(2000), b"1", 0.0), Admit::Full);
        let stats = relay.stats("running", "t", 0.0);
        assert_eq!(stats.client_to_host.queue_cap_drops, 1);
        assert_eq!(stats.client_to_host.queue_cap_drop_bytes, 1);
        assert_eq!(stats.stranger_drops, 1);
        relay.take_due(100.0);
        relay.from_client(addr(1000), b"12345678901", 100.0); // over 10 bytes
        assert_eq!(relay.stats("running", "t", 0.0).client_to_host.queue_cap_drops, 2);
    }

    #[test]
    fn forgets_a_silent_client_and_frees_its_slot() {
        let mut relay = Relay::new(Config { max_clients: 1, ..config(50, 0) });
        relay.from_client(addr(1000), b"x", 0.0);
        // Still something to send to the host: kept.
        assert!(relay.retire_idle(3000.0, "w").is_empty());
        relay.take_due(3000.0);
        assert!(relay.retire_idle(2999.0, "w").is_empty());
        assert_eq!(relay.retire_idle(3000.0, "then"), vec![0]);
        assert_eq!(relay.active_clients(), 0);
        // The rejoin's new address gets the slot and new streams.
        assert_eq!(relay.from_client(addr(1001), b"x", 3100.0), Admit::New(1));
        let stats = relay.stats("running", "t", 0.0);
        assert_eq!(stats.clients.len(), 2);
        assert_eq!(stats.clients[0].retired_at.as_deref(), Some("then"));
        assert_eq!(stats.clients[0].port, 1000);
        assert_eq!(stats.clients[1].retired_at, None);
        assert_eq!(stats.active_clients, 1);
        assert_eq!(stats.client_to_host.received, 2);
    }

    #[test]
    fn idle_zero_never_forgets() {
        let mut relay = Relay::new(Config { idle_ms: 0, ..config(0, 0) });
        relay.from_client(addr(1000), b"x", 0.0);
        relay.take_due(0.0);
        assert!(relay.retire_idle(1e9, "w").is_empty());
    }

    #[test]
    fn percentiles_index_like_the_node_relay() {
        assert_eq!(percentile(&[], 0.99), None);
        let sorted: Vec<f64> = (0..100).map(f64::from).collect();
        assert_eq!(percentile(&sorted, 0.5), Some(50.0));
        assert_eq!(percentile(&sorted, 0.99), Some(99.0));
        assert_eq!(percentile(&sorted, 1.0), Some(99.0));
        assert_eq!(percentile(&[1.23456], 0.5), Some(1.23));
    }

    #[test]
    fn totals_take_the_largest_peak_and_every_sample() {
        let mut relay = Relay::new(config(10, 0));
        relay.from_client(addr(1000), b"a", 0.0);
        relay.from_client(addr(1000), b"a", 0.0);
        relay.from_client(addr(1000), b"a", 0.0);
        relay.from_client(addr(2000), b"b", 0.0);
        relay.take_due(15.0);
        let stats = relay.stats("stopped", "t", 1234.0);
        assert_eq!(stats.client_to_host.peak_queued_packets, 3);
        assert_eq!(stats.client_to_host.lateness_ms.samples, 4);
        assert_eq!(stats.client_to_host.lateness_ms.p99, Some(5.0));
        assert_eq!(stats.uptime_s, 1.2);
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["state"], "stopped");
        assert!(json["host_to_client"]["lateness_ms"]["p99"].is_null());
    }

    #[test]
    fn keeps_a_bounded_lateness_sample() {
        let mut relay = Relay::new(config(0, 0));
        for i in 0..5000 {
            relay.from_client(addr(1000), b"x", f64::from(i));
            relay.take_due(f64::from(i));
        }
        assert_eq!(relay.stats("running", "t", 0.0).client_to_host.lateness_ms.samples, 4096);
    }
}
