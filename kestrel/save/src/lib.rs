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
/// 16: a hauler's haul is two carry jobs, its load in `Carry::job` and its
///     unload in `Carry::next`; its `Assignment` keeps only the link.
///     Pending messages have `ReserveHaul` and `CarryUnit` queues for
///     `Reserve`, `Load`, `Unload` and `ManualUnit`.
/// 17: a pile scene's minerals are settlement items, after the scenario's.
///     Piles and the collector hold an `Inventory`, and a player's minerals
///     are in its `Carry` (the lab's `Hauler` has no cargo). `InventoryRef`
///     gains `Pile` and `Collector`; pending messages have a `pile_carry`
///     queue of `CarryUnit` for `PileStep`.
/// 18: a player's cargo has one capacity, the larger of the scenario's and
///     the pile lab's, for any mix; the lab keeps no capacity of its own.
/// 19: a pile haul's units are a `CarryJob` on the avatar's `Carry`, held on
///     the pile or collector too; the lab's `PileJob` keeps the walk, with
///     `started` for `remaining`, and no `next_toss` or `destination`.
/// 20: threats keep their character (`Threat::character`); waves no longer
///     copy its speed, health, armor or breach damage.
/// 21: an agent's `Body` has no `speed` (haulers read their character's
///     walk) and no `travel_heading`, which moved to `HaulerState`. A
///     `PileJob`'s action can be `PileAction::DropMineral`, appended.
/// 22: the settlement keeps its `vehicles`, and an avatar the vehicle it
///     drives (`AvatarState::driving`, now `seat`).
/// 23: the settlement keeps its `building_kinds` and `buildings`, and an
///     occupied building's production.
/// 24: an agent's role can be a sprite (`Role::Sprite`, its job, plan and
///     the slot it holds), and pending messages have `claim_slot` and
///     `enter_slot` queues.
/// 25: a holder can be a building's output (`InventoryRef::Output`, in
///     transfers and carry jobs), and an inventory can take one item only
///     (`Kinds::Only`, the Sacred Tree's piles).
/// 26: a holder can be a site's materials (`InventoryRef::Site`, in
///     transfers and carry jobs), which deliveries fill.
/// 27: a town keeps a pile lab (`Settlement::piles`, `PileLab::town`) for
///     the bundles dropped on the ground, whose piles hold any item; a 26
///     town has none.
/// 28: an avatar's vehicle (`AvatarState::driving`, 22) is a seat
///     (`AvatarState::seat`): a vehicle it drives or a building it
///     possesses (`Seat`).
/// 29: an avatar keeps the tick its next Burst is ready on
///     (`AvatarState::burst_ready`).
/// 30: a building kind may be a ballista (`BuildingKind::product` is
///     optional, `BuildingKind::defence`), a building keeps its turret's
///     aim and next shot (`BuildingState::turret`), and pending messages
///     have a `building_fire` queue.
/// 31: a ballista kind may have a fireball (`Defence::fireball`), an avatar
///     keeps the tick its next fireball is ready on
///     (`AvatarState::fireball_ready`), a threat keeps its burn
///     (`Threat::burn`), and the settlement keeps fireballs in flight and
///     fire patches (`Settlement::fireballs`, `Settlement::fires`).
/// 32: a threat keeps what it is after (`Threat::goal`: the tree or a
///     quarry it chases), and pending messages have a `threat_goal` queue.
/// 33: a threat's windup and swing messages name a target, an agent or a
///     building (`combat::SwingTarget`), and a chaser may attack a building
///     (`Quarry::Building`); a building kind and a building keep health
///     (`BuildingKind::health`, `BuildingState::health`), and a sprite the
///     tick it comes back on after dying (`SpriteState::respawn_at`).
/// 34: the terrain keeps whether it slopes (`Terrain::sloped`) and how deep
///     its pits go (`Terrain::max_layers`), and a mine its layers
///     (`MineSpec::layers`).
/// 35: a mine keeps what its veins hold (`MineSpec::veins`: the dig lab's
///     ore or a town's stone), and the terrain its undiggable tiles
///     (`Terrain::undiggable`).
/// 36: a mine keeps its authored hill (`MineSpec::hill`: the Sacred Tree's
///     plateau and ramps), and a blueprint can be refused over a pit or
///     steep ground (`PlacementRefusal::Uneven`).
/// 37: miner sprites (M6.5): a sprite job to mine (`SpriteJob::Mine`), its
///     steps (`ClaimTile`, `Dig`, `Pickup`, `Deposit`), a sprite's dig claim,
///     dig and given-up goals (`SpriteState::dig_claim`, `dig`, `asks`,
///     `unreachable`, `unreachable_at`), and the queues that claim tiles and
///     start its bundles (`Messages::claim_dig_tile`, `sprite_bundle`).
/// 38: a threat cut off from the tree waits as close as it can walk
///     (`Threat::stranded`, M6.7).
/// 39: night waves and dawn (M7.2): a wave's nights (`WaveState::nights`),
///     the settlement's day cycle and the threats the sunrise burned away
///     (`Settlement::clock`, `dispelled`), and a burn's `dawn`.
/// 40: the terrain keeps its volume, if the scene has one
///     (`Terrain::volume`: solid spans per column and ore per cell, V.1 of
///     the verticality design).
/// 41: a dig keeps the first layer it clears in a volume scene
///     (`avatar::Dig::layer`, V.4 of the verticality design).
/// 42: a sprite keeps its feet's height (`SpriteState::height`) and a pile
///     the floor it lies on (`Pile::height`), V.7 of the verticality
///     design; a sprite's job can be `SpriteJob::Follow`, appended.
/// 43: every body on a floor (M8.2 of the Sacred Tree depths): a hauler and
///     a threat keep their feet's height (`HaulerState::height`,
///     `Threat::height`), haulers, sprites and threats a replan backoff
///     (`Backoff`), a threat its chase's 3D route (`Threat::chase`), and a
///     player's arrow the floor it flies over (`Projectile::floor`).
/// 44: miners mine walls (M8.4 of the Sacred Tree depths): a sprite's plan
///     can hold a level wall dig (`sprites::Step::DigWall`, appended).
/// 45: lanterns and the light grid (M8.6 of the Sacred Tree depths): a
///     building kind keeps its light (`BuildingKind::light`), a building
///     the floor it stands on (`BuildingState::height`), a miner's plan can
///     fetch for, supply and install a lantern's site (`sprites::Step::Fetch`,
///     `Supply`, `Install`, appended), and the pending messages a queue of
///     miners' installs (`Messages::sprite_install`). The light grid itself
///     is derived and recomputed on load, never saved.
/// 46: a volume made from the mine (a town's) keeps the layout laid over
///     it (`Terrain::mine_volume`) and is saved as its edits over the
///     generated one (`WorldSection::volume_edits`, M8.8 of the Sacred
///     Tree depths), remade at load.
/// 47: a vehicle keeps a crawler's legs (`VehicleState::legs`: its feet,
///     where they stand or swing to, and its gait's clock, CR.2 of the
///     crawler).
pub const FORMAT_VERSION: u32 = 47;
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
    /// A volume made from the mine (a town's) is saved as its edits over
    /// the generated one (`Terrain::generated_volume`), remade at load
    /// (format 46): the town's save went from about 1.5 MB to a few KB.
    volume_edits: Option<sim::volume::VolumeEdits>,
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
    let mut state = world.to_state();
    let base = state.terrain.volume.as_ref().and_then(|_| state.terrain.generated_volume());
    let volume_edits = base.map(|base| {
        let volume = state.terrain.volume.take().expect("checked above");
        volume.edits_over(&base)
    });
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
        volume_edits,
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
    let mut world: WorldSection = postcard::from_bytes(&world.data).map_err(corrupt)?;
    if let Some(edits) = &world.volume_edits {
        let base = world.terrain.generated_volume().ok_or(LoadError::Invalid("volume edits without a generated volume".into()))?;
        world.terrain.volume = Some(sim::volume::Volume::with_edits(base, edits));
    }
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
    for threat in &mut settlement.threats {
        threat.character = map(threat.character)?;
    }
    for vehicle in &mut settlement.vehicles {
        vehicle.character = map(vehicle.character)?;
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
    section("world", ron::ser::to_string_pretty(&WorldSection { config, tick, ids, terrain, settlement, timers, pending, volume_edits: None }, pretty()));
    for (index, chunk) in chunks.iter().enumerate() {
        section(&chunk_section_name(index), ron::ser::to_string_pretty(chunk, pretty()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
