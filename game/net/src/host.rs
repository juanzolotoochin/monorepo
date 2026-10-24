use std::collections::BTreeMap;
use std::time::Duration;

use content::Content;
use sim::{Command, CommandKind, PlayerId, Simulation, World};

use crate::protocol::{decode, encode, encoded_len, Input, Packet, StableId, MAX_PACKET, SAVE_CHUNK};
use crate::transport::Transport;
use crate::{millis, DesyncPolicy, HashLog, NetConfig, NetEvent, NetStats, Pacer, HOST_PLAYER, TICK};

/// Ticks the host runs per update at most. After a longer stall it drops the
/// missed time instead of bursting.
const MAX_CATCH_UP: u64 = 8;
const RESEND_SAVE_CHUNK: Duration = Duration::from_millis(250);
const SAVE_CHUNKS_PER_UPDATE: usize = 48;

struct Upload {
    bytes: Vec<u8>,
    tick: u64,
    /// The client has every chunk below this.
    next: u32,
    /// Which of the 64 chunks after `next` the client has.
    received: u64,
    /// When each chunk was last sent.
    sent_at: Vec<Option<Duration>>,
    welcomed: bool,
    last_welcome: Option<Duration>,
}

impl Upload {
    fn chunks(&self) -> u32 {
        self.bytes.len().div_ceil(SAVE_CHUNK) as u32
    }

    fn has(&self, index: u32) -> bool {
        index < self.next || (index - self.next < 64 && self.received & (1 << (index - self.next)) != 0)
    }
}

struct Remote<A> {
    addr: A,
    player: PlayerId,
    stable_id: StableId,
    /// While the client is loading a save.
    upload: Option<Upload>,
    /// The next turn the client needs.
    turn_ack: u64,
    /// The last input sequence number accepted from the client.
    input_ack: u64,
    /// Hashes for earlier ticks predate the client's current save and are ignored.
    hashes_from: u64,
    last_heard: Duration,
    ping: Option<(u32, Duration)>,
}

pub struct Host<T: Transport> {
    transport: T,
    sim: Simulation,
    content: Content,
    config: NetConfig,
    epoch: u32,
    pacer: Pacer,
    /// Every closed turn, starting at `turns_start`.
    turns: Vec<Vec<Command>>,
    turns_start: u64,
    /// Commands for turns not closed yet, by tick.
    open: BTreeMap<u64, Vec<Command>>,
    remotes: Vec<Remote<T::Addr>>,
    players: BTreeMap<StableId, PlayerId>,
    next_player: u32,
    history: HashLog,
    /// Client hashes for ticks the host hasn't run yet.
    early_reports: Vec<(PlayerId, u64, u64)>,
    halted: Option<u64>,
    last_send: Option<Duration>,
    events: Vec<NetEvent>,
    stats: NetStats,
}

impl<T: Transport> Host<T> {
    /// Hosts `world` (a new game is a world at tick 0). `epoch` tells this
    /// hosting session apart from earlier ones, for example after a re-host.
    pub fn new(transport: T, world: World, content: Content, config: NetConfig, epoch: u32, now: Duration) -> Host<T> {
        let tick = world.tick();
        Host {
            transport,
            sim: Simulation::from_world(world, config.threads),
            content,
            config,
            epoch,
            pacer: Pacer::new(now),
            turns: Vec::new(),
            turns_start: tick,
            open: BTreeMap::new(),
            remotes: Vec::new(),
            players: BTreeMap::new(),
            next_player: HOST_PLAYER.0 + 1,
            history: HashLog::starting_at(tick),
            early_reports: Vec::new(),
            halted: None,
            last_send: None,
            events: Vec::new(),
            stats: NetStats::default(),
        }
    }

    pub fn world(&self) -> &World {
        self.sim.world()
    }

    pub fn history(&self) -> &HashLog {
        &self.history
    }

    /// Every closed turn: the first turn's tick and the turns.
    pub fn turns(&self) -> (u64, &[Vec<Command>]) {
        (self.turns_start, &self.turns)
    }

    pub fn stats(&self) -> &NetStats {
        &self.stats
    }

    pub fn input_delay(&self) -> u64 {
        // The advertised delay still applies to remote clients.
        0
    }

    pub fn players(&self) -> Vec<PlayerId> {
        std::iter::once(HOST_PLAYER).chain(self.remotes.iter().map(|r| r.player)).collect()
    }

    pub fn halted(&self) -> Option<u64> {
        self.halted
    }

    pub fn take_events(&mut self) -> Vec<NetEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn alpha(&self, now: Duration) -> f32 {
        self.pacer.alpha(now)
    }

    /// Schedules a command from the host's own player.
    pub fn queue(&mut self, kind: CommandKind) {
        let tick = self.sim.world().tick();
        self.open.entry(tick).or_default().push(Command { player: HOST_PLAYER, kind });
    }

    /// Commands the host queued that haven't been simulated yet.
    pub fn pending_inputs(&self) -> u64 {
        self.open.values().flatten().filter(|c| c.player == HOST_PLAYER).count() as u64
    }

    /// Handles packets, runs every tick due by `now` (calling `on_step` after
    /// each), and sends turns.
    pub fn update(&mut self, now: Duration, mut on_step: impl FnMut(&World)) {
        self.receive(now);
        self.drop_silent_clients(now);
        if self.halted.is_none() {
            let mut due = self.pacer.due(now);
            if due > MAX_CATCH_UP {
                self.pacer.reset(now);
                due = 1;
            }
            for _ in 0..due {
                self.step();
                on_step(self.sim.world());
            }
            self.pacer.advance(due);
            self.check_early_reports();
        }
        if self.last_send.is_none_or(|at| now >= at + TICK) || self.halted.is_some() {
            self.send(now);
        }
    }

    fn step(&mut self) {
        let tick = self.sim.world().tick();
        let turn = self.open.remove(&tick).unwrap_or_default();
        let hash = self.sim.step(&turn);
        self.history.push(tick, hash.total);
        self.turns.push(turn);
    }

    fn receive(&mut self, now: Duration) {
        while let Some((from, bytes)) = self.transport.recv() {
            self.stats.packets_received += 1;
            self.stats.bytes_received += bytes.len() as u64;
            let Some(packet) = decode(&bytes) else { continue };
            let index = self.remotes.iter().position(|r| r.addr == from);
            match (packet, index) {
                (Packet::Hello { stable_id, rules_hash }, index) => self.hello(from, stable_id, rules_hash, index, now),
                (Packet::Inputs { epoch, turn_ack, inputs, hashes, ping }, Some(i)) if epoch == self.epoch => {
                    self.remotes[i].last_heard = now;
                    self.inputs(i, turn_ack, inputs, hashes, ping, now);
                }
                (Packet::SaveAck { epoch, next, received }, Some(i)) if epoch == self.epoch => {
                    let remote = &mut self.remotes[i];
                    remote.last_heard = now;
                    if let Some(upload) = &mut remote.upload {
                        upload.welcomed = true;
                        if next >= upload.next {
                            upload.next = next;
                            upload.received = received;
                        }
                        if upload.next >= upload.chunks() {
                            remote.upload = None;
                        }
                    }
                }
                (Packet::Bye { epoch }, Some(i)) if epoch == self.epoch => {
                    let remote = self.remotes.remove(i);
                    self.events.push(NetEvent::PlayerLeft { player: remote.player });
                }
                _ => {}
            }
        }
    }

    fn hello(&mut self, from: T::Addr, stable_id: StableId, rules_hash: u64, index: Option<usize>, now: Duration) {
        if rules_hash != self.content.rules_hash {
            let reason = "different game rules (content or version mismatch)".to_string();
            self.send_to(&from, &Packet::Reject { reason });
            return;
        }
        if index.is_some() {
            return; // Already joining or playing; the welcome is resent until acknowledged.
        }
        // A returning player gets their old slot; a stale connection for it is dropped.
        let player = *self.players.entry(stable_id).or_insert_with(|| {
            let player = PlayerId(self.next_player);
            self.next_player += 1;
            player
        });
        self.remotes.retain(|r| r.stable_id != stable_id);
        let tick = self.sim.world().tick();
        self.remotes.push(Remote {
            addr: from,
            player,
            stable_id,
            upload: Some(self.upload()),
            turn_ack: tick,
            input_ack: 0,
            hashes_from: tick,
            last_heard: now,
            ping: None,
        });
        self.events.push(NetEvent::PlayerJoined { player });
    }

    /// A save of the current tick, ready to send.
    fn upload(&self) -> Upload {
        let bytes = save::write(self.sim.world(), &self.content);
        let chunks = bytes.len().div_ceil(SAVE_CHUNK);
        Upload {
            bytes,
            tick: self.sim.world().tick(),
            next: 0,
            received: 0,
            sent_at: vec![None; chunks],
            welcomed: false,
            last_welcome: None,
        }
    }

    fn inputs(&mut self, i: usize, turn_ack: u64, inputs: Vec<Input>, hashes: Vec<(u64, u64)>, ping: u32, now: Duration) {
        let open_tick = self.sim.world().tick();
        let remote = &mut self.remotes[i];
        remote.turn_ack = remote.turn_ack.max(turn_ack);
        remote.ping = Some((ping, now));
        let player = remote.player;
        for input in inputs {
            // Inputs come in sequence; anything else is a repeat or arrived out of order.
            if input.seq != remote.input_ack + 1 {
                continue;
            }
            remote.input_ack = input.seq;
            let tick = if input.tick < open_tick {
                self.stats.late_inputs += 1;
                open_tick
            } else {
                input.tick
            };
            self.open.entry(tick).or_default().push(Command { player, kind: input.kind });
        }
        let hashes_from = remote.hashes_from;
        for (tick, hash) in hashes {
            if tick >= hashes_from {
                self.check_hash(player, tick, hash);
            }
        }
    }

    fn check_hash(&mut self, player: PlayerId, tick: u64, hash: u64) {
        match self.history.get(tick) {
            Some(ours) if ours != hash => self.desync(player, tick),
            Some(_) => {}
            None if tick >= self.history.end() => self.early_reports.push((player, tick, hash)),
            None => {}
        }
    }

    fn check_early_reports(&mut self) {
        let end = self.history.end();
        let (ready, waiting): (Vec<_>, Vec<_>) = self.early_reports.drain(..).partition(|r| r.1 < end);
        self.early_reports = waiting;
        for (player, tick, hash) in ready {
            self.check_hash(player, tick, hash);
        }
    }

    fn desync(&mut self, player: PlayerId, tick: u64) {
        self.events.push(NetEvent::Desync { player: Some(player), tick });
        match self.config.desync {
            DesyncPolicy::Halt => {
                self.halted.get_or_insert(tick);
            }
            DesyncPolicy::Resync => {
                let upload = self.upload();
                let now_tick = upload.tick;
                if let Some(remote) = self.remotes.iter_mut().find(|r| r.player == player) {
                    remote.upload = Some(upload);
                    remote.turn_ack = now_tick;
                    remote.input_ack = 0;
                    remote.hashes_from = now_tick;
                }
            }
        }
    }

    fn drop_silent_clients(&mut self, now: Duration) {
        let timeout = self.config.timeout;
        let (gone, kept): (Vec<_>, Vec<_>) = self.remotes.drain(..).partition(|r| now > r.last_heard + timeout);
        self.remotes = kept;
        for remote in gone {
            self.events.push(NetEvent::PlayerLeft { player: remote.player });
        }
    }

    fn send(&mut self, now: Duration) {
        self.last_send = Some(now);
        let host_tick = self.sim.world().tick();
        for i in 0..self.remotes.len() {
            if let Some(tick) = self.halted {
                let addr = self.remotes[i].addr.clone();
                self.send_to(&addr, &Packet::Desync { epoch: self.epoch, tick });
                continue;
            }
            self.send_upload(i, now);
            let remote = &self.remotes[i];
            let first = remote.turn_ack.max(self.turns_start);
            let (pong, pong_held_ms) = match remote.ping {
                Some((ping, at)) => (ping, millis(now.saturating_sub(at))),
                None => (0, 0),
            };
            let mut packet = Packet::Turns {
                epoch: self.epoch,
                first,
                turns: Vec::new(),
                host_tick,
                input_ack: remote.input_ack,
                pong,
                pong_held_ms,
            };
            // Pack as many unacknowledged turns as fit.
            let mut size = encoded_len(&packet) + 8;
            if let Packet::Turns { turns, .. } = &mut packet {
                for tick in first..host_tick {
                    let turn = &self.turns[(tick - self.turns_start) as usize];
                    let len = encoded_len(turn);
                    if size + len > MAX_PACKET {
                        break;
                    }
                    size += len;
                    turns.push(turn.clone());
                }
            }
            let addr = remote.addr.clone();
            self.send_to(&addr, &packet);
        }
    }

    fn send_upload(&mut self, i: usize, now: Duration) {
        let epoch = self.epoch;
        let input_delay = self.config.input_delay;
        let player = self.remotes[i].player;
        let addr = self.remotes[i].addr.clone();
        let Some(upload) = &mut self.remotes[i].upload else { return };
        let mut packets = Vec::new();
        if !upload.welcomed && upload.last_welcome.is_none_or(|at| now >= at + RESEND_SAVE_CHUNK) {
            upload.last_welcome = Some(now);
            packets.push(Packet::Welcome {
                epoch,
                player,
                input_delay,
                save_tick: upload.tick,
                save_len: upload.bytes.len() as u32,
            });
        }
        if upload.welcomed {
            for index in upload.next..upload.chunks() {
                if packets.len() >= SAVE_CHUNKS_PER_UPDATE {
                    break;
                }
                let recent = upload.sent_at[index as usize].is_some_and(|at| now < at + RESEND_SAVE_CHUNK);
                if upload.has(index) || recent {
                    continue;
                }
                upload.sent_at[index as usize] = Some(now);
                let start = index as usize * SAVE_CHUNK;
                let data = upload.bytes[start..(start + SAVE_CHUNK).min(upload.bytes.len())].to_vec();
                packets.push(Packet::SaveChunk { epoch, index, data });
            }
        }
        for packet in packets {
            self.send_to(&addr, &packet);
        }
    }

    fn send_to(&mut self, to: &T::Addr, packet: &Packet) {
        let bytes = encode(packet);
        self.stats.packets_sent += 1;
        self.stats.bytes_sent += bytes.len() as u64;
        self.transport.send(to, &bytes);
    }

    /// Tells every client the host is leaving.
    pub fn shutdown(&mut self) {
        for i in 0..self.remotes.len() {
            let addr = self.remotes[i].addr.clone();
            self.send_to(&addr, &Packet::Bye { epoch: self.epoch });
        }
    }
}
