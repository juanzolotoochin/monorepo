//! Deterministic lockstep networking.
//!
//! The host's game is the server and simulates like any client. Every tick
//! the host closes a turn (the commands for that tick) and sends it to every
//! client; a client simulates tick T only once it has turn T. Only inputs and
//! turns cross the network. Nothing here reads a clock: callers pass `now`,
//! so tests run on the virtual clock of [`netsim`].
//!
//! - An input made now is scheduled `input_delay` ticks ahead of the host. The
//!   host never waits: an input that arrives after its tick closed moves to
//!   the next open turn.
//! - Clients keep a small buffer of turns and catch up by running extra ticks.
//! - Clients report hashes; the host compares them with its own. On a
//!   mismatch the session halts (dev) or the client is resynced through the
//!   join path (release).
//! - Joining: the host sends a save of the current tick, then turns from
//!   that tick on.

mod client;
mod host;
pub mod netsim;
pub mod protocol;
pub mod transport;

use std::time::Duration;

pub use client::{Client, ClientPhase};
pub use host::Host;
pub use protocol::StableId;
pub use transport::{NullTransport, Transport, UdpTransport};

use sim::PlayerId;

/// The simulation rate.
pub const TICK: Duration = Duration::from_nanos(1_000_000_000 / content::TICKS_PER_SECOND as u64);
/// The host's own player.
pub const HOST_PLAYER: PlayerId = PlayerId(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesyncPolicy {
    /// Stop every peer and keep the evidence. For development.
    Halt,
    /// Resend the save to the desynced client. For release.
    Resync,
}

#[derive(Clone, Debug)]
pub struct NetConfig {
    /// Ticks between an input and the tick it applies on.
    pub input_delay: u64,
    pub desync: DesyncPolicy,
    /// Turns a client tries to keep in hand to absorb jitter.
    pub jitter_buffer: u64,
    /// Silence after which a peer counts as gone.
    pub timeout: Duration,
    /// Simulation threads.
    pub threads: usize,
}

impl Default for NetConfig {
    fn default() -> Self {
        NetConfig {
            input_delay: 4,
            desync: DesyncPolicy::Halt,
            jitter_buffer: 2,
            timeout: Duration::from_secs(5),
            threads: 4,
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
    /// Inputs that arrived after their tick and moved to a later turn (host only).
    pub late_inputs: u64,
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
