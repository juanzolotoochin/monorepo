//! The deterministic game simulation.
//!
//! The world is split into chunks, and each tick runs six stages (see
//! `game/docs/architecture.md`):
//!
//! 1. Apply inputs (single thread)
//! 2. Self update (parallel per chunk): an entity changes only itself
//! 3. Read phase (parallel per chunk): read the world, write messages
//! 4. Merge (single thread): concatenate outboxes in shuffled chunk order
//! 5. Apply phase (single thread): validate and apply messages per type
//! 6. Housekeeping (single thread, with parallel index rebuilds and hashing)
//!
//! The only entity type so far is [`Mover`], a throwaway test entity that
//! exercises every stage: movement with soft separation, sleeping, damage,
//! a contested shared resource (chunk food) and spawning.

pub mod hash;
pub mod id;
pub mod messages;
pub mod mine;
pub mod mover;
pub mod nav;
pub mod rng;
pub mod scenario;
pub mod settlement;
mod soa;
pub mod spatial;
pub mod terrain;
pub mod timer;
mod world;

pub use hash::TickHash;
pub use id::Id;
pub use mover::{Mover, MoverRow};
pub use rng::Rng;
pub use world::{
    Chunk, ChunkState, Command, CommandKind, PlayerId, Simulation, World, WorldConfig, WorldState, CHUNK_TILES,
};

#[cfg(test)]
mod tests;
