use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use content::Content;
use sim::{CommandKind, PlayerId, Simulation, World};

use crate::margin::{InputMargin, DEVIATION_GAIN, SAMPLES_TO_ADAPT};
use crate::protocol::{decode, encode, Input, Packet, StableId, SAVE_CHUNK};
use crate::transport::Transport;
use crate::{millis, HashLog, NetConfig, NetDiagnostics, NetEvent, NetStats, Pacer, TICK};

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
    /// The host's world this client is loading or playing; 0 before the first.
    world: u32,
    /// The host app's name for that world.
    label: String,
    player: Option<PlayerId>,
    margin: InputMargin,
    /// The last tick an input asked for. Later inputs never ask for less,
    /// so the host applies them in the order they were sent.
    last_requested: u64,
    /// Official turns received and not simulated yet.
    turns: BTreeMap<u64, Vec<sim::Command>>,
    next_seq: u64,
    /// Inputs queued and, of those, simulated so far.
    queued: u64,
    applied: u64,
    unacked: VecDeque<Input>,
    /// Inputs sent and not simulated here yet, oldest first. Unlike
    /// `unacked`, they stay until they apply.
    pending: VecDeque<Pending>,
    reports: VecDeque<(u64, u64)>,
    history: HashLog,
    pacer: Pacer,
    /// The host's next tick, as of its last packet.
    host_tick: u64,
    rtt: Option<Duration>,
    /// How much longer than `rtt` a sample takes, on average, a shorter one
    /// counting as zero (like TCP's RTTVAR, but only the late side: an early
    /// trip never makes an input late, and a link getting faster shouldn't
    /// grow the margin). And the samples taken.
    rtt_deviation: Duration,
    rtt_samples: u32,
    started: Duration,
    last_heard: Option<Duration>,
    last_hello: Option<Duration>,
    last_send: Option<Duration>,
    events: Vec<NetEvent>,
    stats: NetStats,
    diagnostics: NetDiagnostics,
    last_update: Option<Duration>,
    waiting_for_turn: bool,
    /// Test hook: a command run locally on the next tick only.
    diverge: Option<sim::Command>,
}

/// An input sent and not simulated here yet.
struct Pending {
    /// The tick it asked for.
    tick: u64,
    kind: CommandKind,
    /// How far past the host's estimated present that tick was; `None`
    /// before the round trip was measured, when that estimate is a guess.
    headroom: Option<u64>,
}

impl<T: Transport> Client<T> {
    pub fn new(transport: T, host: T::Addr, content: Content, config: NetConfig, stable_id: StableId, now: Duration) -> Client<T> {
        Client {
            transport,
            host,
            content,
            margin: InputMargin::new(config.input_delay, config.input_margin, now),
            last_requested: 0,
            config,
            stable_id,
            state: State::Connecting,
            epoch: 0,
            world: 0,
            label: String::new(),
            player: None,
            turns: BTreeMap::new(),
            next_seq: 1,
            queued: 0,
            applied: 0,
            unacked: VecDeque::new(),
            pending: VecDeque::new(),
            reports: VecDeque::new(),
            history: HashLog::default(),
            pacer: Pacer::new(now),
            host_tick: 0,
            rtt: None,
            rtt_deviation: Duration::ZERO,
            rtt_samples: 0,
            started: now,
            last_heard: None,
            last_hello: None,
            last_send: None,
            events: Vec::new(),
            stats: NetStats::default(),
            diagnostics: NetDiagnostics::default(),
            last_update: None,
            waiting_for_turn: false,
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

    /// The host's world this client is loading or playing; 0 before the first.
    pub fn world_number(&self) -> u32 {
        self.world
    }

    /// The host app's name for that world, once welcomed.
    pub fn label(&self) -> &str {
        &self.label
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

    /// The input margin in ticks: adaptive (see `margin`), or pinned by
    /// `NetConfig::input_margin`.
    pub fn input_delay(&self) -> u64 {
        self.margin.ticks()
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

    pub fn diagnostics(&self, now: Duration) -> NetDiagnostics {
        let mut diagnostics = self.diagnostics.clone();
        diagnostics.host_tick = (self.world != 0 && self.host_tick != 0).then_some(self.host_tick);
        diagnostics.input_delay = self.margin.ticks();
        diagnostics.input_margin_pinned = self.margin.pinned();
        diagnostics.jitter_buffer = self.config.jitter_buffer;
        diagnostics.last_packet_age = self.last_heard.map(|at| now.saturating_sub(at));
        diagnostics.unacked_inputs = self.unacked.len() as u64;
        diagnostics.inputs_queued = self.queued;
        diagnostics.inputs_applied = self.applied;
        diagnostics
    }

    pub fn take_events(&mut self) -> Vec<NetEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn alpha(&self, now: Duration) -> f32 {
        self.pacer.alpha(now)
    }

    /// The tick a command queued now asks for: the input margin after the
    /// host's current tick, estimated from its last packet and half the
    /// round trip, and never before the last command's tick.
    pub fn input_horizon(&self) -> u64 {
        (self.host_present() + self.margin.ticks()).max(self.last_requested)
    }

    /// The host's current tick, estimated from its last packet and half the
    /// round trip.
    pub(crate) fn host_present(&self) -> u64 {
        let one_way = self.rtt.map_or(0, |rtt| (rtt / 2).as_nanos().div_ceil(TICK.as_nanos()) as u64);
        self.host_tick + one_way
    }

    /// Schedules a command for the input margin after the host's current tick.
    pub fn queue(&mut self, kind: CommandKind) {
        if !matches!(self.state, State::Playing(_)) {
            return;
        }
        // `host_tick` runs ahead of this client's world (the jitter buffer),
        // so this tick says little about when the command applies here;
        // `pending_inputs` does.
        let tick = self.input_horizon();
        self.last_requested = tick;
        self.diagnostics.last_requested_tick = Some(tick);
        let headroom = self.rtt.map(|_| tick.saturating_sub(self.host_present()));
        self.pending.push_back(Pending { tick, kind: kind.clone(), headroom });
        self.unacked.push_back(Input { seq: self.next_seq, tick, kind });
        self.next_seq += 1;
        self.queued += 1;
    }

    /// Commands this client queued that haven't been simulated yet.
    pub fn pending_inputs(&self) -> u64 {
        self.queued.saturating_sub(self.applied)
    }

    /// The commands `pending_inputs` counts, oldest first, each with the
    /// tick it asked for. The host moves a late one to a later tick.
    pub fn pending_commands(&self) -> impl Iterator<Item = (u64, &CommandKind)> + '_ {
        self.pending.iter().map(|p| (p.tick, &p.kind))
    }

    /// Handles packets, runs every tick that is due and available (calling
    /// `on_step` after each), and sends inputs.
    pub fn update(&mut self, now: Duration, mut on_step: impl FnMut(&World)) {
        if self.waiting_for_turn {
            if let Some(previous) = self.last_update {
                self.diagnostics.starved_time += now.saturating_sub(previous);
            }
        }
        self.last_update = Some(now);
        self.waiting_for_turn = false;
        self.diagnostics.steps_last_update = 0;
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
        self.margin.update(now);
        if let State::Playing(sim) = &mut self.state {
            let tick = sim.world().tick();
            let ready = ready_turns(&self.turns, tick);
            let due = self.pacer.due(now);
            // Run what is due, plus extra ticks when far behind the buffer target.
            let extra = ready.saturating_sub(self.config.jitter_buffer + due + 2).min(MAX_CATCH_UP);
            let steps = (due + extra).min(ready);
            self.diagnostics.steps_last_update = steps;
            self.diagnostics.catch_up_ticks += steps.saturating_sub(due);
            for _ in 0..steps {
                let tick = sim.world().tick();
                let mut turn = self.turns.remove(&tick).expect("ready turn");
                let mine = turn.iter().filter(|c| Some(c.player) == self.player).count();
                self.applied += mine as u64;
                // The host keeps a player's commands in the order they were
                // sent. One in a later turn than it asked for came in late.
                for input in self.pending.drain(..mine.min(self.pending.len())) {
                    if tick > input.tick {
                        self.diagnostics.late_inputs += 1;
                        self.diagnostics.shifted_ticks += tick - input.tick;
                        if let Some(headroom) = input.headroom {
                            self.margin.late(headroom, tick - input.tick, now);
                        }
                    }
                }
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
                self.waiting_for_turn = true;
                self.diagnostics.starved_updates += 1;
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
            self.diagnostics.last_receive_interval = self.last_heard.map(|at| now.saturating_sub(at));
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
            Packet::Welcome { epoch, world, label, player, input_delay, save_tick, save_len } => {
                let same_load = matches!(&self.state, State::Loading { tick, len, .. } if *tick == save_tick && *len == save_len as usize)
                    && epoch == self.epoch
                    && world == self.world;
                if same_load || (epoch == self.epoch && world < self.world) {
                    return; // A repeated welcome, or a late one for an earlier world.
                }
                if world == self.world && epoch == self.epoch {
                    // A resync: the same world, later on.
                    self.turns.retain(|&tick, _| tick >= save_tick);
                } else {
                    // A new world: its ticks start over.
                    self.turns.clear();
                    self.host_tick = 0;
                }
                self.epoch = epoch;
                self.world = world;
                self.label = label;
                self.player = Some(player);
                self.margin.welcome(input_delay, now);
                self.last_requested = 0;
                self.unacked.clear();
                self.pending.clear();
                self.next_seq = 1;
                // Inputs not in the new save are lost.
                (self.queued, self.applied) = (0, 0);
                self.diagnostics = NetDiagnostics::default();
                self.waiting_for_turn = false;
                self.reports.clear();
                let chunks = (save_len as usize).div_ceil(SAVE_CHUNK);
                self.state = State::Loading { tick: save_tick, len: save_len as usize, chunks: vec![None; chunks] };
                // Acknowledge the welcome right away so chunks start flowing.
                self.send(now);
            }
            Packet::SaveChunk { epoch, world, index, data } if epoch == self.epoch && world == self.world => {
                if let State::Loading { chunks, .. } = &mut self.state {
                    if let Some(slot) = chunks.get_mut(index as usize) {
                        *slot = Some(data);
                    }
                }
                self.finish_loading(now);
            }
            Packet::Turns { epoch, world, first, turns, host_tick, input_ack, pong, pong_held_ms }
                if epoch == self.epoch && world == self.world =>
            {
                let needed = self.needed_from();
                for (i, turn) in turns.into_iter().enumerate() {
                    let tick = first + i as u64;
                    if tick >= needed {
                        self.turns.entry(tick).or_insert(turn);
                    }
                }
                self.host_tick = self.host_tick.max(host_tick);
                self.diagnostics.last_input_ack = Some(self.diagnostics.last_input_ack.unwrap_or(0).max(input_ack));
                while self.unacked.front().is_some_and(|input| input.seq <= input_ack) {
                    self.unacked.pop_front();
                }
                if pong != 0 {
                    let elapsed = millis(now).wrapping_sub(pong).saturating_sub(pong_held_ms);
                    let sample = Duration::from_millis(elapsed as u64);
                    self.diagnostics.rtt_sample = Some(sample);
                    if let Some(rtt) = self.rtt {
                        self.rtt_deviation = (self.rtt_deviation * (DEVIATION_GAIN - 1) + sample.saturating_sub(rtt)) / DEVIATION_GAIN;
                    }
                    let rtt = match self.rtt {
                        Some(rtt) => (rtt * 7 + sample) / 8,
                        None => sample,
                    };
                    self.rtt = Some(rtt);
                    self.rtt_samples = self.rtt_samples.saturating_add(1);
                    if self.rtt_samples >= SAMPLES_TO_ADAPT {
                        self.margin.measure(rtt, self.rtt_deviation, now);
                    }
                }
            }
            Packet::Desync { epoch, world, tick } if epoch == self.epoch && world == self.world => {
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
                Packet::SaveAck { epoch: self.epoch, world: self.world, next: next as u32, received }
            }
            State::Playing(sim) => {
                let tick = sim.world().tick();
                Packet::Inputs {
                    epoch: self.epoch,
                    world: self.world,
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
