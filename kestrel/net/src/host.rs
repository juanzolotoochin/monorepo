use std::collections::BTreeMap;
use std::time::Duration;

use content::Content;
use sim::{Command, CommandKind, PlayerId, Simulation, World};

use crate::protocol::{decode, encode, encoded_len, Input, Packet, StableId, MAX_PACKET, SAVE_CHUNK};
use crate::transport::Transport;
use crate::{millis, DesyncPolicy, HashLog, NetConfig, NetDiagnostics, NetEvent, NetStats, Pacer, PeerDiagnostics, HOST_PLAYER, TICK};

/// Ticks the host runs per update at most. After a longer stall it drops the
/// missed time instead of bursting.
const MAX_CATCH_UP: u64 = 8;
const RESEND_SAVE_CHUNK: Duration = Duration::from_millis(250);
const SAVE_CHUNKS_PER_UPDATE: usize = 48;
/// The most a turn may take encoded, so it always fits in a `Turns` packet
/// with that packet's own fields.
const TURN_BUDGET: usize = MAX_PACKET - 64;
/// Packets in a row each turn goes in, so a lost packet or two loses nothing.
const TURN_REPEATS: u8 = 3;
/// A turn still unacknowledged this long after it was last sent goes again.
/// Longer than most round trips, so turns that arrived are rarely resent.
const TURN_RESEND: Duration = Duration::from_millis(300);
/// A connection heard from this recently is alive: a join with its stable
/// ID from elsewhere is refused rather than replacing it.
const DUPLICATE_WINDOW: Duration = Duration::from_secs(1);

#[derive(Clone)]
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
    /// When it was given the save it is loading or last loaded.
    load_started: Duration,
    /// The next turn the client needs.
    turn_ack: u64,
    /// Turns from `turn_ack` on that went out: how many times, and when last.
    turn_sends: BTreeMap<u64, (u8, Duration)>,
    /// The last input sequence number accepted from the client.
    input_ack: u64,
    /// Hashes for earlier ticks predate the client's current save and are ignored.
    hashes_from: u64,
    /// The client is playing the current world.
    loaded: bool,
    last_heard: Duration,
    ping: Option<(u32, Duration)>,
    late_inputs: u64,
    shifted_ticks: u64,
    last_requested_tick: Option<u64>,
    last_assigned_tick: Option<u64>,
}

pub struct Host<T: Transport> {
    transport: T,
    sim: Simulation,
    content: Content,
    config: NetConfig,
    epoch: u32,
    /// Counts the worlds loaded in this session, from 1.
    world: u32,
    /// The app's name for the current world, passed on to clients.
    label: String,
    /// A newly loaded world waits at its first tick until every client has it.
    starting: bool,
    pacer: Pacer,
    /// Every closed turn, starting at `turns_start`.
    turns: Vec<Vec<Command>>,
    turns_start: u64,
    /// Commands for turns not closed yet, by tick.
    open: BTreeMap<u64, Vec<Command>>,
    /// The last save written for joiners, and the world it is of (`upload`).
    saved: Option<(u32, Upload)>,
    /// The tick the host's own last command went in, so a full turn
    /// doesn't put its commands out of order.
    host_filed: u64,
    remotes: Vec<Remote<T::Addr>>,
    /// Every player's slot: those playing and those who left, whose slot is
    /// kept for when they return.
    players: BTreeMap<StableId, PlayerId>,
    /// Players who left and still hold a slot, oldest departure first.
    departures: Vec<StableId>,
    history: HashLog,
    /// Client hashes for ticks the host hasn't run yet.
    early_reports: Vec<(PlayerId, u64, u64)>,
    halted: Option<u64>,
    last_send: Option<Duration>,
    events: Vec<NetEvent>,
    stats: NetStats,
    steps_last_update: u64,
}

impl<T: Transport> Host<T> {
    /// Hosts `world` (a new game is a world at tick 0), which the app calls
    /// `label`. `epoch` tells this hosting session apart from earlier ones,
    /// for example after a re-host.
    pub fn new(
        transport: T,
        world: World,
        label: String,
        content: Content,
        config: NetConfig,
        epoch: u32,
        now: Duration,
    ) -> Host<T> {
        let tick = world.tick();
        Host {
            transport,
            sim: Simulation::from_world(world, config.threads),
            content,
            config,
            epoch,
            world: 1,
            label,
            starting: false,
            pacer: Pacer::new(now),
            turns: Vec::new(),
            turns_start: tick,
            open: BTreeMap::new(),
            host_filed: tick,
            saved: None,
            remotes: Vec::new(),
            players: BTreeMap::new(),
            departures: Vec::new(),
            history: HashLog::starting_at(tick),
            early_reports: Vec::new(),
            halted: None,
            last_send: None,
            events: Vec::new(),
            stats: NetStats::default(),
            steps_last_update: 0,
        }
    }

    pub fn world(&self) -> &World {
        self.sim.world()
    }

    /// The current world's number in this session, from 1.
    pub fn world_number(&self) -> u32 {
        self.world
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// Whether the current world is waiting for clients to load it.
    pub fn starting(&self) -> bool {
        self.starting
    }

    /// Replaces the world with a new one (a new map, at tick 0). Every client
    /// loads it from a save, and its first tick waits until they all have.
    /// Commands queued for the old world are dropped.
    pub fn load_world(&mut self, world: World, label: String, now: Duration) {
        let tick = world.tick();
        self.world += 1;
        self.label = label;
        self.sim = Simulation::from_world(world, self.config.threads);
        self.turns.clear();
        self.turns_start = tick;
        self.open.clear();
        self.host_filed = tick;
        self.history = HashLog::starting_at(tick);
        self.early_reports.clear();
        self.halted = None;
        self.pacer.reset(now);
        if self.remotes.is_empty() {
            return;
        }
        let upload = self.upload();
        for remote in &mut self.remotes {
            remote.upload = Some(upload.clone());
            remote.load_started = now;
            remote.turn_ack = tick;
            remote.turn_sends.clear();
            remote.input_ack = 0;
            remote.hashes_from = tick;
            remote.loaded = false;
            remote.late_inputs = 0;
            remote.shifted_ticks = 0;
            remote.last_requested_tick = None;
            remote.last_assigned_tick = None;
        }
        self.starting = true;
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

    pub fn diagnostics(&self, now: Duration) -> NetDiagnostics {
        NetDiagnostics {
            host_tick: Some(self.sim.world().tick()),
            remote_input_delay: self.config.input_delay,
            jitter_buffer: self.config.jitter_buffer,
            steps_last_update: self.steps_last_update,
            peers: self.remotes.iter().map(|remote| PeerDiagnostics {
                player: remote.player,
                loading: !remote.loaded,
                last_packet_age: now.saturating_sub(remote.last_heard),
                turn_ack: remote.turn_ack,
                input_ack: remote.input_ack,
                late_inputs: remote.late_inputs,
                shifted_ticks: remote.shifted_ticks,
                last_requested_tick: remote.last_requested_tick,
                last_assigned_tick: remote.last_assigned_tick,
            }).collect(),
            ..NetDiagnostics::default()
        }
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
        let tick = self.sim.world().tick().max(self.host_filed);
        self.host_filed = file(&mut self.open, tick, Command { player: HOST_PLAYER, kind });
    }

    /// Commands the host queued that haven't been simulated yet.
    pub fn pending_inputs(&self) -> u64 {
        self.open.values().flatten().filter(|c| c.player == HOST_PLAYER).count() as u64
    }

    /// Handles packets, runs every tick due by `now` (calling `on_step` after
    /// each), and sends turns.
    pub fn update(&mut self, now: Duration, mut on_step: impl FnMut(&World)) {
        self.steps_last_update = 0;
        self.receive(now);
        self.drop_silent_clients(now);
        if self.starting {
            self.drop_slow_loaders(now);
        }
        if self.starting && self.remotes.iter().all(|r| r.loaded) {
            // Everyone has the new world (or left): start it together.
            self.starting = false;
            self.pacer.reset(now);
        }
        if self.halted.is_none() && !self.starting {
            let mut due = self.pacer.due(now);
            if due > MAX_CATCH_UP {
                self.pacer.reset(now);
                due = 1;
            }
            for _ in 0..due {
                self.step();
                on_step(self.sim.world());
            }
            self.steps_last_update = due;
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
                // A packet about an earlier world still shows the client is there.
                (Packet::Inputs { epoch, world, turn_ack, inputs, hashes, ping }, Some(i)) if epoch == self.epoch => {
                    self.remotes[i].last_heard = now;
                    if world == self.world {
                        self.inputs(i, turn_ack, inputs, hashes, ping, now);
                    }
                }
                (Packet::SaveAck { epoch, world, next, received }, Some(i)) if epoch == self.epoch => {
                    let remote = &mut self.remotes[i];
                    remote.last_heard = now;
                    if world != self.world {
                        continue;
                    }
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
                    self.depart(remote);
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
        // The same player from a second address while the first is still
        // talking is a second copy of the game, not a reconnect.
        if self.remotes.iter().any(|r| r.stable_id == stable_id && now < r.last_heard + DUPLICATE_WINDOW) {
            let reason = "this player is already in the game (start a second copy with --profile <name>)".to_string();
            self.send_to(&from, &Packet::Reject { reason });
            return;
        }
        // A returning player gets their old slot; a stale connection for it is dropped.
        let player = match self.players.get(&stable_id) {
            Some(&player) => player,
            None => match self.new_slot() {
                Some(player) => player,
                None => {
                    let reason = format!("the game is full ({} players)", self.config.max_players);
                    self.send_to(&from, &Packet::Reject { reason });
                    return;
                }
            },
        };
        self.players.insert(stable_id, player);
        self.departures.retain(|&id| id != stable_id);
        self.remotes.retain(|r| r.stable_id != stable_id);
        let tick = self.sim.world().tick();
        let upload = self.upload();
        self.remotes.push(Remote {
            addr: from,
            player,
            stable_id,
            upload: Some(upload),
            load_started: now,
            turn_ack: tick,
            turn_sends: BTreeMap::new(),
            input_ack: 0,
            hashes_from: tick,
            loaded: false,
            last_heard: now,
            ping: None,
            late_inputs: 0,
            shifted_ticks: 0,
            last_requested_tick: None,
            last_assigned_tick: None,
        });
        self.events.push(NetEvent::PlayerJoined { player });
    }

    /// A slot for a player who has none, if the game isn't full: the lowest
    /// one nobody holds, or else the slot of the player who left longest ago.
    fn new_slot(&mut self) -> Option<PlayerId> {
        let playing = 1 + self.remotes.len() as u32;
        if playing >= self.config.max_players {
            return None;
        }
        let held = |player: PlayerId| self.players.values().any(|&p| p == player);
        if let Some(free) = (HOST_PLAYER.0 + 1..self.config.max_players).map(PlayerId).find(|&p| !held(p)) {
            return Some(free);
        }
        let oldest = self.departures.first().copied()?;
        self.departures.remove(0);
        self.players.remove(&oldest)
    }

    /// A remote is gone: its avatar goes on the next tick, for everyone, and
    /// its player keeps their slot until it is needed.
    fn depart(&mut self, remote: Remote<T::Addr>) {
        let tick = self.sim.world().tick();
        file(&mut self.open, tick, Command { player: remote.player, kind: CommandKind::Leave });
        self.departures.push(remote.stable_id);
        self.events.push(NetEvent::PlayerLeft { player: remote.player });
    }

    /// A save of the current tick, ready to send.
    /// Players joining on the same tick share it, so a save is written once
    /// however many ask.
    fn upload(&mut self) -> Upload {
        let tick = self.sim.world().tick();
        if let Some((world, upload)) = &self.saved {
            if *world == self.world && upload.tick == tick {
                return upload.clone();
            }
        }
        let bytes = save::write(self.sim.world(), &self.content);
        let chunks = bytes.len().div_ceil(SAVE_CHUNK);
        let upload = Upload {
            bytes,
            tick,
            next: 0,
            received: 0,
            sent_at: vec![None; chunks],
            welcomed: false,
            last_welcome: None,
        };
        self.saved = Some((self.world, upload.clone()));
        upload
    }

    fn inputs(&mut self, i: usize, turn_ack: u64, inputs: Vec<Input>, hashes: Vec<(u64, u64)>, ping: u32, now: Duration) {
        let open_tick = self.sim.world().tick();
        let remote = &mut self.remotes[i];
        // Clients send inputs only once they are playing.
        remote.loaded = true;
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
                remote.late_inputs += 1;
                remote.shifted_ticks += open_tick - input.tick;
                open_tick
            } else {
                input.tick
            };
            // Never before an earlier input from the same client, which a full
            // turn may have moved later.
            let tick = tick.max(remote.last_assigned_tick.unwrap_or(0));
            remote.last_requested_tick = Some(input.tick);
            remote.last_assigned_tick = Some(file(&mut self.open, tick, Command { player, kind: input.kind }));
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
                    remote.turn_sends.clear();
                    remote.input_ack = 0;
                    remote.hashes_from = now_tick;
                }
            }
        }
    }

    /// A new world waits at its first tick for every client, but not for
    /// longer than `NetConfig::load_timeout`: a client still loading then is
    /// told why and dropped, and can join again mid-game.
    fn drop_slow_loaders(&mut self, now: Duration) {
        let limit = self.config.load_timeout;
        let (slow, kept): (Vec<_>, Vec<_>) = self.remotes.drain(..).partition(|r| !r.loaded && now > r.load_started + limit);
        self.remotes = kept;
        for remote in slow {
            let reason = format!("loading the new world took over {} s", limit.as_secs());
            self.send_to(&remote.addr, &Packet::Reject { reason });
            self.depart(remote);
        }
    }

    fn drop_silent_clients(&mut self, now: Duration) {
        let timeout = self.config.timeout;
        let (gone, kept): (Vec<_>, Vec<_>) = self.remotes.drain(..).partition(|r| now > r.last_heard + timeout);
        self.remotes = kept;
        for remote in gone {
            self.depart(remote);
        }
    }

    fn send(&mut self, now: Duration) {
        self.last_send = Some(now);
        let host_tick = self.sim.world().tick();
        for i in 0..self.remotes.len() {
            if let Some(tick) = self.halted {
                let addr = self.remotes[i].addr.clone();
                self.send_to(&addr, &Packet::Desync { epoch: self.epoch, world: self.world, tick });
                continue;
            }
            self.send_upload(i, now);
            let remote = &mut self.remotes[i];
            // Each turn goes in a few packets in a row, then again only if it
            // is still unacknowledged a while later. Repeating every
            // unacknowledged turn in every packet would send each about a
            // round trip's worth of times. Packets carry consecutive turns,
            // so they start at the oldest one due.
            let needed = remote.turn_ack.max(self.turns_start);
            remote.turn_sends = remote.turn_sends.split_off(&needed);
            let sends = &remote.turn_sends;
            let due = |tick: u64| sends.get(&tick).is_none_or(|&(count, at)| count < TURN_REPEATS || now >= at + TURN_RESEND);
            let first = (needed..host_tick).find(|&tick| due(tick)).unwrap_or(host_tick);
            let (pong, pong_held_ms) = match remote.ping {
                Some((ping, at)) => (ping, millis(now.saturating_sub(at))),
                None => (0, 0),
            };
            let mut packet = Packet::Turns {
                epoch: self.epoch,
                world: self.world,
                first,
                turns: Vec::new(),
                host_tick,
                input_ack: remote.input_ack,
                pong,
                pong_held_ms,
            };
            // Pack as many turns from there as fit.
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
                    let sent = remote.turn_sends.entry(tick).or_insert((0, now));
                    *sent = (sent.0.saturating_add(1), now);
                }
            }
            let addr = remote.addr.clone();
            self.send_to(&addr, &packet);
        }
    }

    fn send_upload(&mut self, i: usize, now: Duration) {
        let (epoch, world) = (self.epoch, self.world);
        let label = self.label.clone();
        let input_delay = self.config.input_delay;
        let player = self.remotes[i].player;
        let addr = self.remotes[i].addr.clone();
        let Some(upload) = &mut self.remotes[i].upload else { return };
        let mut packets = Vec::new();
        if !upload.welcomed && upload.last_welcome.is_none_or(|at| now >= at + RESEND_SAVE_CHUNK) {
            upload.last_welcome = Some(now);
            packets.push(Packet::Welcome {
                epoch,
                world,
                label,
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
                packets.push(Packet::SaveChunk { epoch, world, index, data });
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

/// Puts `command` in the open turn for `tick`, or the first later one with
/// room: a turn must fit in one packet, or it could never be sent and every
/// client would wait for it forever. Returns the tick it went in.
fn file(open: &mut BTreeMap<u64, Vec<Command>>, mut tick: u64, command: Command) -> u64 {
    let len = encoded_len(&command);
    loop {
        let turn = open.entry(tick).or_default();
        if turn.is_empty() || encoded_len(turn) + len <= TURN_BUDGET {
            turn.push(command);
            return tick;
        }
        tick += 1;
    }
}
