//! Saves: the world's state at a tick.
//!
//! A new game, a load, a join, a resync and a re-host all go through the same
//! path: a save plus the inputs since it. Layout:
//!
//! ```text
//! "GSAV"  format version (u32, little endian)  zstd(postcard((Header, [Section])))
//! ```
//!
//! The format version sits outside the compressed body so an incompatible
//! save is refused before anything is decoded. Sections are encoded
//! separately (one for the world, one per chunk) so tools can read them one at
//! a time and joins can skip chunks whose hash already matches. The same types
//! print as RON for dumps and diffs.

use std::fmt;
use std::io::Read;

use content::{Content, Names};
use serde::{Deserialize, Serialize};
use sim::{ChunkState, World, WorldConfig, WorldState};

mod inputs;

pub use inputs::{InputLog, INPUT_FORMAT_VERSION, OLDEST_INPUT_FORMAT_VERSION};

/// 3: settlements refer to character definitions by index.
/// 4: every agent has a character, haulers included.
/// 5: the jump and swift sword flags and scene speeds leave the settlement
///    and terrain for character abilities.
/// 6: settlements persist piano positions and each player's held notes.
/// 7: avatars keep the press tick of a queued attack (`queued_at`) in their
///    state and swing.
/// 8: threats keep their windup and cooldown (`Threat::windup`,
///    `ready_at`), and pending messages have the threats' swing queues.
/// 9: avatars keep a threat's push while they swing (`AvatarState::push`).
/// 10: every agent has a `Body` (position, route, velocity, push, health)
///     and a `Carry` (cargo), apart from its hauler and avatar state.
/// 11: an agent's role is a `Role`, a hauler or an avatar; the player's
///     owner, name and weapon cooldown move into its `AvatarState`.
/// 12: material accounts count units players carried off when they left
///     (`Accounting::abandoned`).
/// 13: a depot's stock, reservations and capacity are an `Inventory`.
/// 14: an agent's cargo is an `Inventory` too. Units flying in are its
///     `reserved_in` per item; units flying out are only transfers.
/// 15: a player's depot transfer is a `CarryJob` on its `Carry`, not on its
///     avatar, and holds its units in the cargo too. `Endpoint` is
///     `InventoryRef`, its `Cargo` now `Agent`.
pub const FORMAT_VERSION: u32 = 15;
pub const GAME_VERSION: &str = "0.0.0-dev";
const MAGIC: &[u8; 4] = b"GSAV";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub game_version: String,
    /// The rules hash of the content the save was made with. A save may load
    /// with different content (indices are remapped by name), but peers in a
    /// session must share the hash.
    pub rules_hash: u64,
    /// The next tick to run.
    pub tick: u64,
    pub seed: u64,
    /// Index-to-name tables for the content the save refers to.
    pub names: Names,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub data: Vec<u8>,
}

/// Everything in the world section: the state minus the chunks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct WorldSection {
    config: WorldConfig,
    tick: u64,
    ids: sim::id::IdsState,
    terrain: sim::terrain::Terrain,
    settlement: Option<sim::settlement::Settlement>,
    timers: Vec<Vec<(u64, sim::Id<sim::Mover>)>>,
    pending: sim::messages::Messages,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    NotASave,
    FormatVersion { found: u32, expected: u32 },
    Corrupt(String),
    /// The save uses a definition the current content doesn't have.
    MissingContent { name: String },
    Invalid(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::NotASave => write!(f, "not a save file"),
            LoadError::FormatVersion { found, expected } => write!(
                f,
                "save format version {found} is not supported (this build reads version {expected})"
            ),
            LoadError::Corrupt(message) => write!(f, "save is corrupt: {message}"),
            LoadError::MissingContent { name } => write!(f, "save uses {name}, which no longer exists"),
            LoadError::Invalid(message) => write!(f, "save is invalid: {message}"),
        }
    }
}

impl std::error::Error for LoadError {}

fn corrupt(error: impl fmt::Display) -> LoadError {
    LoadError::Corrupt(error.to_string())
}

fn chunk_section_name(index: usize) -> String {
    format!("chunk/{index}")
}

/// Encodes the world, which must be between ticks.
pub fn write(world: &World, content: &Content) -> Vec<u8> {
    let state = world.to_state();
    let header = Header {
        game_version: GAME_VERSION.to_string(),
        rules_hash: content.rules_hash,
        tick: state.tick,
        seed: state.config.seed,
        names: content.names.clone(),
    };
    let world_section = WorldSection {
        config: state.config,
        tick: state.tick,
        ids: state.ids,
        terrain: state.terrain,
        settlement: state.settlement,
        timers: state.timers,
        pending: state.pending,
    };
    let mut sections = vec![Section { name: "world".into(), data: encode(&world_section) }];
    for (index, chunk) in state.chunks.iter().enumerate() {
        sections.push(Section { name: chunk_section_name(index), data: encode(chunk) });
    }

    let body = encode(&(header, sections));
    let mut out = Vec::with_capacity(body.len() / 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend(ruzstd::encoding::compress_to_vec(body.as_slice(), ruzstd::encoding::CompressionLevel::Fastest));
    out
}

fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    postcard::to_allocvec(value).expect("save types always encode")
}

/// Reads the header and the raw sections.
pub fn read_sections(bytes: &[u8]) -> Result<(Header, Vec<Section>), LoadError> {
    if bytes.len() < 8 || &bytes[..4] != MAGIC {
        return Err(LoadError::NotASave);
    }
    let found = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if found != FORMAT_VERSION {
        return Err(LoadError::FormatVersion { found, expected: FORMAT_VERSION });
    }
    let mut body = Vec::new();
    ruzstd::decoding::StreamingDecoder::new(&bytes[8..])
        .map_err(corrupt)?
        .read_to_end(&mut body)
        .map_err(corrupt)?;
    postcard::from_bytes(&body).map_err(corrupt)
}

pub fn read_header(bytes: &[u8]) -> Result<Header, LoadError> {
    read_sections(bytes).map(|(header, _)| header)
}

/// Decodes the full world state, still indexed by the save's own name tables.
pub fn read_state(bytes: &[u8]) -> Result<(Header, WorldState), LoadError> {
    let (header, sections) = read_sections(bytes)?;
    let mut sections = sections.into_iter();
    let world = sections.next().filter(|s| s.name == "world").ok_or(LoadError::Corrupt("no world section".into()))?;
    let world: WorldSection = postcard::from_bytes(&world.data).map_err(corrupt)?;
    let mut chunks = Vec::with_capacity(world.config.chunk_count());
    for (index, section) in sections.enumerate() {
        if section.name != chunk_section_name(index) {
            return Err(LoadError::Corrupt(format!("unexpected section {}", section.name)));
        }
        chunks.push(postcard::from_bytes::<ChunkState>(&section.data).map_err(corrupt)?);
    }
    let state = WorldState {
        config: world.config,
        tick: world.tick,
        ids: world.ids,
        terrain: world.terrain,
        settlement: world.settlement,
        timers: world.timers,
        pending: world.pending,
        chunks,
    };
    Ok((header, state))
}

/// Loads a save with the current content, remapping content indices by name.
pub fn read(bytes: &[u8], content: &Content) -> Result<World, LoadError> {
    let (header, mut state) = read_state(bytes)?;
    remap(&header.names, content, &mut state)?;
    World::from_state(state, content.rules.clone()).map_err(LoadError::Invalid)
}

/// Rewrites every content index in `state` from the save's tables to `content`'s.
fn remap(saved: &Names, content: &Content, state: &mut WorldState) -> Result<(), LoadError> {
    if *saved == content.names {
        return Ok(());
    }
    let movers = saved
        .movers
        .iter()
        .map(|name| content.names.mover_index(name).ok_or_else(|| LoadError::MissingContent { name: name.clone() }))
        .collect::<Result<Vec<u16>, _>>()?;
    let map = |kind: u16| {
        movers.get(kind as usize).copied().ok_or_else(|| LoadError::Invalid(format!("mover kind {kind} out of range")))
    };
    for chunk in &mut state.chunks {
        for row in &mut chunk.movers {
            row.kind = map(row.kind)?;
        }
    }
    for spawn in &mut state.pending.spawn {
        spawn.kind = map(spawn.kind)?;
    }
    remap_characters(saved, content, state)
}

/// Characters are remapped where the settlement uses them, so content may
/// drop a character no saved entity refers to.
fn remap_characters(saved: &Names, content: &Content, state: &mut WorldState) -> Result<(), LoadError> {
    let Some(settlement) = &mut state.settlement else { return Ok(()) };
    let map = |ix: u16| {
        let name = saved.characters.get(ix as usize).ok_or_else(|| LoadError::Invalid(format!("character {ix} out of range")))?;
        let short = name.strip_prefix("character/").unwrap_or(name);
        content.names.character_index(short).ok_or_else(|| LoadError::MissingContent { name: name.clone() })
    };
    for agent in &mut settlement.agents {
        agent.character = map(agent.character)?;
    }
    for wave in &mut settlement.waves {
        wave.character = map(wave.character)?;
    }
    Ok(())
}

/// Prints a save as RON: the header, then each section.
pub fn dump_ron(bytes: &[u8]) -> Result<String, LoadError> {
    let (header, state) = read_state(bytes)?;
    let pretty = || ron::ser::PrettyConfig::new().struct_names(true);
    let mut out = String::new();
    let mut section = |name: &str, text: Result<String, ron::Error>| {
        out.push_str(&format!("// {name}\n{}\n\n", text.expect("save types always print")));
    };
    section("header", ron::ser::to_string_pretty(&header, pretty()));
    let WorldState { config, tick, ids, terrain, settlement, timers, pending, chunks } = state;
    section("world", ron::ser::to_string_pretty(&WorldSection { config, tick, ids, terrain, settlement, timers, pending }, pretty()));
    for (index, chunk) in chunks.iter().enumerate() {
        section(&chunk_section_name(index), ron::ser::to_string_pretty(chunk, pretty()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
