//! Packets. Every datagram is `MAGIC`, the protocol version, then a
//! postcard-encoded [`Packet`]. Reliability comes from repetition: each packet
//! repeats everything the receiver hasn't acknowledged, so a lost packet needs
//! no retransmission timer.

use serde::{Deserialize, Serialize};
use sim::{Command, CommandKind, PlayerId};

const MAGIC: &[u8; 4] = b"LSTP";
pub const PROTOCOL_VERSION: u16 = 1;
/// Stay under typical path MTUs (and Steam's unreliable message limit).
pub const MAX_PACKET: usize = 1200;
pub const SAVE_CHUNK: usize = 1000;

/// A player's identity across sessions: a local profile ID now, a Steam or
/// EOS ID later. Maps a returning player back to their slot.
pub type StableId = u64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input {
    /// Per-client sequence number, starting at 1.
    pub seq: u64,
    /// The tick the client asked for. The host may move it later.
    pub tick: u64,
    pub kind: CommandKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Packet {
    /// Client → host: asks to join.
    Hello { stable_id: StableId, rules_hash: u64 },
    /// Host → client: refuses a join.
    Reject { reason: String },
    /// Host → client: accepted. A save of `save_len` bytes at `save_tick`
    /// follows in chunks. Also sent to resync a desynced client.
    Welcome { epoch: u32, player: PlayerId, input_delay: u64, save_tick: u64, save_len: u32 },
    /// Host → client: part of the save.
    SaveChunk { epoch: u32, index: u32, data: Vec<u8> },
    /// Client → host while loading: the first chunk it lacks, and which of
    /// the next 64 it already has.
    SaveAck { epoch: u32, next: u32, received: u64 },
    /// Host → client: official turns from `first` on, the host's latest
    /// closed tick, the last input sequence received from this client, and
    /// the echo of the client's last ping.
    Turns { epoch: u32, first: u64, turns: Vec<Vec<Command>>, host_tick: u64, input_ack: u64, pong: u32, pong_held_ms: u32 },
    /// Client → host: the next turn it needs, every input not yet
    /// acknowledged, recent hashes, and a ping (the client's clock in ms).
    Inputs { epoch: u32, turn_ack: u64, inputs: Vec<Input>, hashes: Vec<(u64, u64)>, ping: u32 },
    /// Host → all: the session halted on a desync at `tick`.
    Desync { epoch: u32, tick: u64 },
    /// Either side: leaving.
    Bye { epoch: u32 },
}

pub fn encode(packet: &Packet) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    postcard::to_extend(packet, out).expect("packets always encode")
}

/// Decodes a datagram, or `None` for anything that isn't ours or is from
/// another protocol version.
pub fn decode(bytes: &[u8]) -> Option<Packet> {
    if bytes.len() < 6 || &bytes[..4] != MAGIC || u16::from_le_bytes([bytes[4], bytes[5]]) != PROTOCOL_VERSION {
        return None;
    }
    postcard::from_bytes(&bytes[6..]).ok()
}

/// The encoded size of a value, for packing packets under [`MAX_PACKET`].
pub fn encoded_len<T: Serialize>(value: &T) -> usize {
    postcard::experimental::serialized_size(value).unwrap_or(usize::MAX)
}
