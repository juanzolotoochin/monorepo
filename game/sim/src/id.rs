//! Generational IDs, one allocator per entity type.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use crate::hash::{StateHash, StateHasher};

/// A handle to an entity of type `T`. Stale handles (to a freed entity whose
/// index was reused) fail to resolve because the generation differs.
pub struct Id<T> {
    index: u32,
    generation: u32,
    _type: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    pub const fn index(self) -> u32 {
        self.index
    }

    pub const fn generation(self) -> u32 {
        self.generation
    }

    /// A unique 64-bit key, for RNG streams and hashing.
    pub const fn to_bits(self) -> u64 {
        (self.generation as u64) << 32 | self.index as u64
    }
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.to_bits() == other.to_bits()
    }
}

impl<T> Eq for Id<T> {}

impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Id<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.index, self.generation).cmp(&(other.index, other.generation))
    }
}

impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_bits().hash(state);
    }
}

impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({}v{})", self.index, self.generation)
    }
}

impl<T> StateHash for Id<T> {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write_u64(self.to_bits());
    }
}

/// Where an entity's row lives: its chunk and its slot in that chunk's table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location {
    pub chunk: u32,
    pub slot: u32,
}

struct Slot {
    generation: u32,
    /// `None` while free, and also between allocation in the apply phase and
    /// insertion in housekeeping.
    location: Option<Location>,
    allocated: bool,
}

/// Allocates IDs for one entity type and maps them to locations. Only the
/// single-threaded stages allocate or free.
pub struct Ids<T> {
    slots: Vec<Slot>,
    /// Freed indices, reused last in, first out.
    free: Vec<u32>,
    allocated: u32,
    _type: PhantomData<fn() -> T>,
}

impl<T> Default for Ids<T> {
    fn default() -> Self {
        Ids { slots: Vec::new(), free: Vec::new(), allocated: 0, _type: PhantomData }
    }
}

impl<T> Ids<T> {
    /// Allocates an ID with no location yet.
    pub fn alloc(&mut self) -> Id<T> {
        self.allocated += 1;
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                self.slots.push(Slot { generation: 0, location: None, allocated: false });
                (self.slots.len() - 1) as u32
            }
        };
        let slot = &mut self.slots[index as usize];
        slot.allocated = true;
        Id { index, generation: slot.generation, _type: PhantomData }
    }

    pub fn free(&mut self, id: Id<T>) {
        assert!(self.is_allocated(id), "freeing a dead {id:?}");
        let slot = &mut self.slots[id.index as usize];
        slot.generation = slot.generation.wrapping_add(1);
        slot.location = None;
        slot.allocated = false;
        self.free.push(id.index);
        self.allocated -= 1;
    }

    pub fn is_allocated(&self, id: Id<T>) -> bool {
        self.slots
            .get(id.index as usize)
            .is_some_and(|slot| slot.allocated && slot.generation == id.generation)
    }

    /// The location of a live, inserted entity.
    pub fn get(&self, id: Id<T>) -> Option<Location> {
        let slot = self.slots.get(id.index as usize)?;
        if slot.allocated && slot.generation == id.generation {
            slot.location
        } else {
            None
        }
    }

    pub fn set_location(&mut self, id: Id<T>, location: Location) {
        debug_assert!(self.is_allocated(id));
        self.slots[id.index as usize].location = Some(location);
    }

    /// The number of allocated IDs, including ones not inserted yet.
    pub fn allocated(&self) -> u32 {
        self.allocated
    }
}

impl<T> StateHash for Ids<T> {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write_u64(self.slots.len() as u64);
        for slot in &self.slots {
            h.write_u64((slot.generation as u64) << 1 | slot.allocated as u64);
            match slot.location {
                Some(location) => h.write_u64((location.chunk as u64) << 32 | location.slot as u64),
                None => h.write_u64(u64::MAX),
            }
        }
        h.write_u64(self.free.len() as u64);
        for &index in &self.free {
            h.write_u64(index as u64);
        }
    }
}
