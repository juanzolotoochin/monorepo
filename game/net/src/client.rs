use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use content::Content;
use sim::{CommandKind, PlayerId, Simulation, World};

use crate::protocol::{decode, encode, Input, Packet, StableId, SAVE_CHUNK};
use crate::transport::Transport;
use crate::{millis, HashLog, NetConfig, NetEvent, NetStats, Pacer, TICK};

const HELLO_INTERVAL: Duration = Duration::from_millis(250);
/// Extra ticks a client runs per update at most while catching up.
const MAX_CATCH_UP: u64 = 8;
/// Hashes repeated in every packet, so a lost packet doesn't lose a report.
const REPORTED_HASHES: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientPhase {
    Connecting,
    Loading { received: usize, total: usize },
    Playing,
    Stopped { reason: String },
}

enum State {
    Connecting,
    Loading { tick: u64, len: usize, chunks: Vec<Option<Vec<u8>>> },
    Playing(Box<Simulation>),
    /// Keeps the world, if there was one, so it can still be saved.
    Stopped(String, Option<Box<Simulation>>),
}

pub struct Client<T: Transport> {
    transport: T,
    host: T::Addr,
    content: Content,
    config: NetConfig,
    stable_id: StableId,
    state: State,
    epoch: u32,
    player: Option<PlayerId>,
    input_delay: u64,
    /// Official turns received and not simulated yet.
    turns: BTreeMap<u64, Vec<sim::Command>>,
    next_seq: u64,
    /// Inputs queued and, of those, simulated so far.
    queued: u64,
    applied: u64,
    unacked: VecDeque<Input>,
    reports: VecDeque<(u64, u64)>,
    history: HashLog,
    pacer: Pacer,
    /// The host's next tick, as of its last packet.
    host_tick: u64,
    rtt: Option<Duration>,
    started: Duration,
    last_heard: Option<Duration>,
    last_hello: Option<Duration>,
    last_send: Option<Duration>,
    events: Vec<NetEvent>,
    stats: NetStats,
    /// Test hook: a command run locally on the next tick only.
    diverge: Option<sim::Command>,
}

impl<T: Transport> Client<T> {
    pub fn new(transport: T, host: T::Addr, content: Content, config: NetConfig, stable_id: StableId, now: Duration) -> Client<T> {
        Client {
            transport,
            host,
            content,
            input_delay: config.input_delay,
            config,
            stable_id,
            state: State::Connecting,
            epoch: 0,
            player: None,
            turns: BTreeMap::new(),
            next_seq: 1,
            queued: 0,
            applied: 0,
            unacked: VecDeque::new(),
            reports: VecDeque::new(),
            history: HashLog::default(),
            pacer: Pacer::new(now),
            host_tick: 0,
            rtt: None,
            started: now,
            last_heard: None,
            last_hello: None,
            last_send: None,
            events: Vec::new(),
            stats: NetStats::default(),
            diverge: None,
        }
    }

    pub fn world(&self) -> Option<&World> {
        match &self.state {
            State::Playing(sim) | State::Stopped(_, Some(sim)) => Some(sim.world()),
            _ => None,
        }
    }

    pub fn player(&self) -> Option<PlayerId> {
        self.player
    }

    pub fn phase(&self) -> ClientPhase {
        match &self.state {
            State::Connecting => ClientPhase::Connecting,
            State::Loading { chunks, .. } => ClientPhase::Loading {
                received: chunks.iter().filter(|c| c.is_some()).count(),
                total: chunks.len(),
            },
            State::Playing(_) => ClientPhase::Playing,
            State::Stopped(reason, _) => ClientPhase::Stopped { reason: reason.clone() },
        }
    }

    pub fn history(&self) -> &HashLog {
        &self.history
    }

    pub fn input_delay(&self) -> u64 {
        self.input_delay
    }

    pub fn stats(&self) -> NetStats {
        let mut stats = self.stats.clone();
        stats.rtt = self.rtt;
        if let State::Playing(sim) = &self.state {
            let tick = sim.world().tick();
            stats.turns_buffered = self.ready_turns(tick);
            stats.ticks_behind_host = self.host_tick.saturating_sub(tick);
        }
        stats
    }

    pub fn take_events(&mut self) -> Vec<NetEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn alpha(&self, now: Duration) -> f32 {
        self.pacer.alpha(now)
    }

    /// Schedules a command for `input_delay` ticks after the host's current tick.
    pub fn queue(&mut self, kind: CommandKind) {
        if !matches!(self.state, State::Playing(_)) {
            return;
        }
        let one_way = self.rtt.map_or(0, |rtt| (rtt / 2).as_nanos().div_ceil(TICK.as_nanos()) as u64);
        // `host_tick` runs ahead of this client's world (the jitter buffer),
        // so this tick says little about when the command applies here;
        // `pending_inputs` does.
        let tick = self.host_tick + one_way + self.input_delay;
        self.unacked.push_back(Input { seq: self.next_seq, tick, kind });
        self.next_seq += 1;
        self.queued += 1;
    }

    /// Commands this client queued that haven't been simulated yet.
    pub fn pending_inputs(&self) -> u64 {
        self.queued.saturating_sub(self.applied)
    }

    /// Handles packets, runs every tick that is due and available (calling
    /// `on_step` after each), and sends inputs.
    pub fn update(&mut self, now: Duration, mut on_step: impl FnMut(&World)) {
        self.receive(now);
        if matches!(self.state, State::Stopped(..)) {
            return;
        }
        let heard = self.last_heard.unwrap_or(self.started);
        if now > heard + self.config.timeout {
            self.stop("lost the host".into());
            self.events.push(NetEvent::HostLost);
            return;
        }
        if let State::Playing(sim) = &mut self.state {
            let tick = sim.world().tick();
            let ready = ready_turns(&self.turns, tick);
            let due = self.pacer.due(now);
            // Run what is due, plus extra ticks when far behind the buffer target.
            let extra = ready.saturating_sub(self.config.jitter_buffer + due + 2).min(MAX_CATCH_UP);
            let steps = (due + extra).min(ready);
            for _ in 0..steps {
                let tick = sim.world().tick();
                let mut turn = self.turns.remove(&tick).expect("ready turn");
                self.applied += turn.iter().filter(|c| Some(c.player) == self.player).count() as u64;
                turn.extend(self.diverge.take());
                let hash = sim.step(&turn);
                self.history.push(tick, hash.total);
                self.reports.push_back((tick, hash.total));
                if self.reports.len() > REPORTED_HASHES {
                    self.reports.pop_front();
                }
                on_step(sim.world());
            }
            if steps < due {
                // Out of turns: wait for the host rather than owing ticks.
                self.pacer.reset(now);
            } else {
                self.pacer.advance(due);
            }
        }
        if self.last_send.is_none_or(|at| now >= at + TICK) {
            self.send(now);
        }
    }

    fn ready_turns(&self, tick: u64) -> u64 {
        ready_turns(&self.turns, tick)
    }

    fn stop(&mut self, reason: String) {
        let sim = match std::mem::replace(&mut self.state, State::Connecting) {
            State::Playing(sim) => Some(sim),
            State::Stopped(_, sim) => sim,
            _ => None,
        };
        self.state = State::Stopped(reason, sim);
    }

    fn receive(&mut self, now: Duration) {
        while let Some((from, bytes)) = self.transport.recv() {
            if from != self.host {
                continue;
            }
            self.stats.packets_received += 1;
            self.stats.bytes_received += bytes.len() as u64;
            let Some(packet) = decode(&bytes) else { continue };
            self.last_heard = Some(now);
            self.handle(packet, now);
        }
    }

    fn handle(&mut self, packet: Packet, now: Duration) {
        match packet {
            Packet::Reject { reason } => {
                self.events.push(NetEvent::Rejected { reason: reason.clone() });
                self.stop(reason);
            }
            Packet::Welcome { epoch, player, input_delay, save_tick, save_len } => {
                let same_load = matches!(&self.state, State::Loading { tick, len, .. } if *tick == save_tick && *len == save_len as usize)
                    && epoch == self.epoch;
                if same_load {
                    return; // A repeated welcome.
                }
                self.epoch = epoch;
                self.player = Some(player);
                self.input_delay = input_delay;
                self.turns.retain(|&tick, _| tick >= save_tick);
                self.unacked.clear();
                self.next_seq = 1;
                // Inputs not in the new save are lost.
                (self.queued, self.applied) = (0, 0);
                self.reports.clear();
                let chunks = (save_len as usize).div_ceil(SAVE_CHUNK);
                self.state = State::Loading { tick: save_tick, len: save_len as usize, chunks: vec![None; chunks] };
                // Acknowledge the welcome right away so chunks start flowing.
                self.send(now);
            }
            Packet::SaveChunk { epoch, index, data } if epoch == self.epoch => {
                if let State::Loading { chunks, .. } = &mut self.state {
                    if let Some(slot) = chunks.get_mut(index as usize) {
                        *slot = Some(data);
                    }
                }
                self.finish_loading(now);
            }
            Packet::Turns { epoch, first, turns, host_tick, input_ack, pong, pong_held_ms } if epoch == self.epoch => {
                let needed = self.needed_from();
                for (i, turn) in turns.into_iter().enumerate() {
                    let tick = first + i as u64;
                    if tick >= needed {
                        self.turns.entry(tick).or_insert(turn);
                    }
                }
                self.host_tick = self.host_tick.max(host_tick);
                while self.unacked.front().is_some_and(|input| input.seq <= input_ack) {
                    self.unacked.pop_front();
                }
                if pong != 0 {
                    let elapsed = millis(now).wrapping_sub(pong).saturating_sub(pong_held_ms);
                    let sample = Duration::from_millis(elapsed as u64);
                    self.rtt = Some(match self.rtt {
                        Some(rtt) => (rtt * 7 + sample) / 8,
                        None => sample,
                    });
                }
            }
            Packet::Desync { epoch, tick } if epoch == self.epoch => {
                self.events.push(NetEvent::Desync { player: None, tick });
                self.stop(format!("desync at tick {tick}"));
            }
            Packet::Bye { epoch } if epoch == self.epoch => {
                self.events.push(NetEvent::HostLost);
                self.stop("the host left".into());
            }
            _ => {}
        }
    }

    /// The first tick this client still needs a turn for.
    fn needed_from(&self) -> u64 {
        match &self.state {
            State::Loading { tick, .. } => *tick,
            State::Playing(sim) => sim.world().tick(),
            _ => u64::MAX,
        }
    }

    fn finish_loading(&mut self, now: Duration) {
        let State::Loading { tick, chunks, .. } = &self.state else { return };
        if chunks.iter().any(Option::is_none) {
            return;
        }
        let tick = *tick;
        let bytes: Vec<u8> = chunks.iter().flatten().flatten().copied().collect();
        match save::read(&bytes, &self.content) {
            Ok(world) => {
                self.history = HashLog::starting_at(tick);
                self.state = State::Playing(Box::new(Simulation::from_world(world, self.config.threads)));
                self.pacer.reset(now);
                self.events.push(NetEvent::Loaded { tick });
            }
            Err(error) => self.stop(format!("could not load the host's save: {error}")),
        }
    }

    fn send(&mut self, now: Duration) {
        self.last_send = Some(now);
        let packet = match &self.state {
            State::Connecting => {
                if self.last_hello.is_some_and(|at| now < at + HELLO_INTERVAL) {
                    return;
                }
                self.last_hello = Some(now);
                Packet::Hello { stable_id: self.stable_id, rules_hash: self.content.rules_hash }
            }
            State::Loading { chunks, .. } => {
                let next = chunks.iter().position(Option::is_none).unwrap_or(chunks.len());
                let received = (0..64)
                    .filter(|i| chunks.get(next + i).is_some_and(Option::is_some))
                    .fold(0u64, |bits, i| bits | 1 << i);
                Packet::SaveAck { epoch: self.epoch, next: next as u32, received }
            }
            State::Playing(sim) => {
                let tick = sim.world().tick();
                Packet::Inputs {
                    epoch: self.epoch,
                    turn_ack: tick + ready_turns(&self.turns, tick),
                    inputs: self.unacked.iter().cloned().collect(),
                    hashes: self.reports.iter().copied().collect(),
                    // Zero means "no ping"; the clock starts at an arbitrary point.
                    ping: millis(now).max(1),
                }
            }
            State::Stopped(..) => return,
        };
        let bytes = encode(&packet);
        self.stats.packets_sent += 1;
        self.stats.bytes_sent += bytes.len() as u64;
        self.transport.send(&self.host, &bytes);
    }

    /// Tells the host this client is leaving.
    pub fn shutdown(&mut self) {
        let bytes = encode(&Packet::Bye { epoch: self.epoch });
        self.transport.send(&self.host, &bytes);
    }

    /// Test hook: runs one extra local command on the next tick without telling
    /// the host, which diverges this client's state.
    #[doc(hidden)]
    pub fn diverge_for_test(&mut self, kind: CommandKind) {
        let player = self.player.unwrap_or(PlayerId(u32::MAX));
        self.diverge = Some(sim::Command { player, kind });
    }
}

/// How many consecutive turns from `tick` are in hand.
fn ready_turns(turns: &BTreeMap<u64, Vec<sim::Command>>, tick: u64) -> u64 {
    turns.range(tick..).enumerate().take_while(|(i, (t, _))| **t == tick + *i as u64).count() as u64
}
