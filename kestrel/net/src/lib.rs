//! Deterministic lockstep networking.
//!
//! The host's game is the server and simulates like any client. Every tick
//! the host closes a turn (the commands for that tick) and sends it to every
//! client; a client simulates tick T only once it has turn T. Only inputs and
//! turns cross the network. Nothing here reads a clock: callers pass `now`,
//! so tests run on the virtual clock of [`netsim`].
//!
//! - The host's own input enters its next open turn. Clients schedule inputs
//!   an input margin ahead of the host, sized from the measured network
//!   (`margin`). The host never waits: an input that arrives after its tick
//!   closed moves to the next open turn.
//! - Clients keep a small buffer of turns and catch up by running extra ticks.
//! - Clients report hashes; the host compares them with its own. On a
//!   mismatch the session halts (dev) or the client is resynced through the
//!   join path (release).
//! - Joining: the host sends a save of the current tick, then turns from
//!   that tick on.

mod client;
mod host;
mod margin;
pub mod netsim;
pub mod protocol;
pub mod transport;

use std::time::Duration;

pub use client::{Client, ClientPhase, CommandId, PendingCommand, APPLIED_HISTORY};
pub use host::Host;
pub use protocol::StableId;
pub use transport::{NullTransport, Transport, UdpTransport};

use sim::PlayerId;

/// The simulation rate.
pub const TICK: Duration = Duration::from_nanos(1_000_000_000 / content::TICKS_PER_SECOND as u64);
/// The host's own player.
pub const HOST_PLAYER: PlayerId = PlayerId(0);
/// `NetConfig::max_players` unless set.
pub const DEFAULT_MAX_PLAYERS: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesyncPolicy {
    /// Stop every peer and keep the evidence. For development.
    Halt,
    /// Resend the save to the desynced client. For release.
    Resync,
}

#[derive(Clone, Debug)]
pub struct NetConfig {
    /// Ticks between an input and the tick it applies on. A host tells
    /// clients this as their starting input margin.
    pub input_delay: u64,
    /// Pins a client's input margin (ticks) instead of adapting it to the
    /// measured network.
    pub input_margin: Option<u64>,
    pub desync: DesyncPolicy,
    /// Turns a client tries to keep in hand to absorb jitter.
    pub jitter_buffer: u64,
    /// Silence after which a peer counts as gone.
    pub timeout: Duration,
    /// How long a new world waits at its first tick for a client to load it
    /// before dropping that client.
    pub load_timeout: Duration,
    /// Simulation threads.
    pub threads: usize,
    /// Players in a game at most, the host included. A host refuses anyone
    /// past it. 8 for now; LAN games may allow 16 later.
    pub max_players: u32,
}

impl Default for NetConfig {
    fn default() -> Self {
        NetConfig {
            input_delay: content::millis_to_ticks(133),
            input_margin: None,
            desync: DesyncPolicy::Halt,
            jitter_buffer: content::millis_to_ticks(67),
            timeout: Duration::from_secs(5),
            load_timeout: Duration::from_secs(20),
            threads: 4,
            max_players: DEFAULT_MAX_PLAYERS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetEvent {
    PlayerJoined { player: PlayerId },
    PlayerLeft { player: PlayerId },
    /// The client finished loading the host's save.
    Loaded { tick: u64 },
    /// A client's hash differed from the host's.
    Desync { player: Option<PlayerId>, tick: u64 },
    /// The host refused to let this client join.
    Rejected { reason: String },
    /// The host went silent or left. The session is over.
    HostLost,
}

/// Numbers for the net graph overlay.
#[derive(Clone, Debug, Default)]
pub struct NetStats {
    /// Round-trip time to the host (clients only).
    pub rtt: Option<Duration>,
    /// Turns received but not simulated yet.
    pub turns_buffered: u64,
    /// How far this peer's simulation is behind the host's.
    pub ticks_behind_host: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    /// Inputs that arrived after their tick and moved to a later turn (host
    /// only; a client counts its own in `NetDiagnostics::late_inputs`).
    pub late_inputs: u64,
}

/// Observations only: never serialized or included in deterministic state.
/// Tick counters name the next tick to simulate, not the last completed tick.
#[derive(Clone, Debug, Default)]
pub struct NetDiagnostics {
    pub host_tick: Option<u64>,
    /// A client's input margin: adaptive, or pinned by `input_margin_pinned`.
    pub input_delay: u64,
    pub input_margin_pinned: bool,
    /// A client's inputs that applied later than they asked for, as it saw
    /// them in the host's turns, and the ticks they moved in all.
    pub late_inputs: u64,
    pub shifted_ticks: u64,
    pub remote_input_delay: u64,
    pub jitter_buffer: u64,
    pub last_packet_age: Option<Duration>,
    pub rtt_sample: Option<Duration>,
    pub last_receive_interval: Option<Duration>,
    pub unacked_inputs: u64,
    pub inputs_queued: u64,
    pub inputs_applied: u64,
    pub last_requested_tick: Option<u64>,
    pub last_input_ack: Option<u64>,
    pub steps_last_update: u64,
    pub catch_up_ticks: u64,
    /// Updates when ticks were due but their official turns were unavailable.
    pub starved_updates: u64,
    /// Time observed between updates while the client was waiting for a turn.
    pub starved_time: Duration,
    pub peers: Vec<PeerDiagnostics>,
}

#[derive(Clone, Debug)]
pub struct PeerDiagnostics {
    pub player: PlayerId,
    pub loading: bool,
    pub last_packet_age: Duration,
    /// Receipt acknowledgement, not the remote simulation's current tick.
    pub turn_ack: u64,
    pub input_ack: u64,
    pub late_inputs: u64,
    pub shifted_ticks: u64,
    pub last_requested_tick: Option<u64>,
    pub last_assigned_tick: Option<u64>,
}

/// Every tick's total hash since the session started here.
#[derive(Clone, Debug, Default)]
pub struct HashLog {
    start: u64,
    hashes: Vec<u64>,
}

impl HashLog {
    fn starting_at(tick: u64) -> HashLog {
        HashLog { start: tick, hashes: Vec::new() }
    }

    fn push(&mut self, tick: u64, hash: u64) {
        debug_assert_eq!(tick, self.start + self.hashes.len() as u64);
        self.hashes.push(hash);
    }

    pub fn get(&self, tick: u64) -> Option<u64> {
        tick.checked_sub(self.start).and_then(|i| self.hashes.get(i as usize)).copied()
    }

    pub fn start(&self) -> u64 {
        self.start
    }

    pub fn end(&self) -> u64 {
        self.start + self.hashes.len() as u64
    }
}

/// Paces ticks against the caller's clock.
#[derive(Clone, Debug)]
struct Pacer {
    /// When the next tick is due.
    next: Duration,
}

impl Pacer {
    fn new(now: Duration) -> Pacer {
        Pacer { next: now }
    }

    /// Ticks due by `now`.
    fn due(&self, now: Duration) -> u64 {
        if now < self.next {
            0
        } else {
            ((now - self.next).as_nanos() / TICK.as_nanos()) as u64 + 1
        }
    }

    fn advance(&mut self, ticks: u64) {
        self.next += TICK * ticks as u32;
    }

    /// Forgets missed ticks, so a stall doesn't turn into a burst later.
    fn reset(&mut self, now: Duration) {
        self.next = now;
    }

    /// How far render time is between the last two ticks.
    fn alpha(&self, now: Duration) -> f32 {
        if now >= self.next {
            return 1.0;
        }
        1.0 - ((self.next - now).as_secs_f32() / TICK.as_secs_f32()).min(1.0)
    }
}

fn millis(now: Duration) -> u32 {
    now.as_millis() as u32
}

#[cfg(test)]
mod tests;
