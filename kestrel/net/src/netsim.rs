//! A loopback network simulator on a virtual clock. Latency, jitter, loss and
//! spikes are drawn from a seeded generator, so a network test is fast and
//! reproduces exactly. Every delivered or dropped packet can be traced.

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, VecDeque};
use std::rc::Rc;
use std::time::Duration;

use crate::transport::Transport;

pub type SimAddr = u32;

#[derive(Clone, Debug)]
pub struct LinkConditions {
    /// One-way base latency.
    pub latency: Duration,
    /// Extra delay drawn uniformly from `[0, jitter]` per packet. Reorders packets.
    pub jitter: Duration,
    /// Packets lost per thousand.
    pub loss_permille: u32,
    /// Extra delay for packets sent in `[start, end)`.
    pub spikes: Vec<(Duration, Duration, Duration)>,
}

impl LinkConditions {
    pub fn perfect() -> LinkConditions {
        LinkConditions { latency: Duration::ZERO, jitter: Duration::ZERO, loss_permille: 0, spikes: Vec::new() }
    }

    /// 100 ms one way, up to 40 ms jitter, 3% loss.
    pub fn internet() -> LinkConditions {
        LinkConditions {
            latency: Duration::from_millis(100),
            jitter: Duration::from_millis(40),
            loss_permille: 30,
            spikes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceEntry {
    pub sent: Duration,
    /// `None` if the packet was dropped.
    pub delivered: Option<Duration>,
    pub from: SimAddr,
    pub to: SimAddr,
    pub len: usize,
}

struct InFlight {
    from: SimAddr,
    to: SimAddr,
    bytes: Vec<u8>,
}

pub struct NetSim {
    now: Duration,
    rng: u64,
    default_link: LinkConditions,
    links: BTreeMap<(SimAddr, SimAddr), LinkConditions>,
    /// (delivery time, send order) → packet, earliest first.
    queue: BinaryHeap<Reverse<(Duration, u64)>>,
    in_flight: BTreeMap<u64, InFlight>,
    sent: u64,
    inboxes: BTreeMap<SimAddr, VecDeque<(SimAddr, Vec<u8>)>>,
    pub trace: Vec<TraceEntry>,
}

impl NetSim {
    pub fn new(seed: u64, default_link: LinkConditions) -> Rc<RefCell<NetSim>> {
        Rc::new(RefCell::new(NetSim {
            now: Duration::ZERO,
            rng: seed,
            default_link,
            links: BTreeMap::new(),
            queue: BinaryHeap::new(),
            in_flight: BTreeMap::new(),
            sent: 0,
            inboxes: BTreeMap::new(),
            trace: Vec::new(),
        }))
    }

    /// Overrides the conditions for packets from `from` to `to`.
    pub fn set_link(&mut self, from: SimAddr, to: SimAddr, link: LinkConditions) {
        self.links.insert((from, to), link);
    }

    /// Delivers every packet due by `now`.
    pub fn advance_to(&mut self, now: Duration) {
        self.now = now;
        while let Some(Reverse((at, id))) = self.queue.peek().copied() {
            if at > now {
                break;
            }
            self.queue.pop();
            let packet = self.in_flight.remove(&id).expect("queued packet");
            self.inboxes.entry(packet.to).or_default().push_back((packet.from, packet.bytes));
        }
    }

    fn random(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn send(&mut self, from: SimAddr, to: SimAddr, bytes: &[u8]) {
        let link = self.links.get(&(from, to)).unwrap_or(&self.default_link).clone();
        let mut entry = TraceEntry { sent: self.now, delivered: None, from, to, len: bytes.len() };
        if self.random() % 1000 < link.loss_permille as u64 {
            self.trace.push(entry);
            return;
        }
        let jitter_ns = link.jitter.as_nanos() as u64;
        let jitter = if jitter_ns == 0 { 0 } else { self.random() % (jitter_ns + 1) };
        let spike: Duration = link
            .spikes
            .iter()
            .filter(|(start, end, _)| self.now >= *start && self.now < *end)
            .map(|s| s.2)
            .sum();
        let at = self.now + link.latency + Duration::from_nanos(jitter) + spike;
        entry.delivered = Some(at);
        self.trace.push(entry);
        let id = self.sent;
        self.sent += 1;
        self.in_flight.insert(id, InFlight { from, to, bytes: bytes.to_vec() });
        self.queue.push(Reverse((at, id)));
    }
}

/// One endpoint on a [`NetSim`].
pub struct SimSocket {
    net: Rc<RefCell<NetSim>>,
    addr: SimAddr,
}

impl SimSocket {
    pub fn new(net: &Rc<RefCell<NetSim>>, addr: SimAddr) -> SimSocket {
        SimSocket { net: net.clone(), addr }
    }
}

impl Transport for SimSocket {
    type Addr = SimAddr;

    fn send(&mut self, to: &SimAddr, bytes: &[u8]) {
        self.net.borrow_mut().send(self.addr, *to, bytes);
    }

    fn recv(&mut self) -> Option<(SimAddr, Vec<u8>)> {
        self.net.borrow_mut().inboxes.get_mut(&self.addr)?.pop_front()
    }
}
