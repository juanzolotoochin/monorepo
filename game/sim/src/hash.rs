//! State hashing for desync detection.

use fx::{Fx, FxVec2, FxVec3};

/// A fast, platform-independent 64-bit hasher. Not cryptographic; it only has
/// to make accidental collisions between diverged states unlikely.
pub struct StateHasher(u64);

impl Default for StateHasher {
    fn default() -> Self {
        StateHasher(0x243f_6a88_85a3_08d3)
    }
}

impl StateHasher {
    #[inline]
    pub fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x51_7cc1_b727_220a_95);
    }

    #[inline]
    pub fn write<T: StateHash + ?Sized>(&mut self, value: &T) {
        value.hash_into(self);
    }

    /// The final value, with the bits mixed so similar states hash far apart.
    pub fn finish(&self) -> u64 {
        let mut z = self.0;
        z = (z ^ (z >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
        z = (z ^ (z >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        z ^ (z >> 33)
    }
}

/// Types that contribute to the state hash.
pub trait StateHash {
    fn hash_into(&self, h: &mut StateHasher);
}

macro_rules! hash_as_u64 {
    ($($ty:ty),+) => {
        $(impl StateHash for $ty {
            #[inline]
            fn hash_into(&self, h: &mut StateHasher) {
                h.write_u64(*self as u64);
            }
        })+
    };
}

hash_as_u64!(u8, u32, u64, i32, i64, bool);

impl StateHash for Fx {
    #[inline]
    fn hash_into(&self, h: &mut StateHasher) {
        h.write_u64(self.to_bits() as u64);
    }
}

impl StateHash for FxVec2 {
    #[inline]
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.x);
        h.write(&self.y);
    }
}

impl StateHash for FxVec3 {
    #[inline]
    fn hash_into(&self, h: &mut StateHasher) {
        h.write(&self.x);
        h.write(&self.y);
        h.write(&self.z);
    }
}

impl<T: StateHash> StateHash for [T] {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write_u64(self.len() as u64);
        for item in self {
            item.hash_into(h);
        }
    }
}

impl<T: StateHash> StateHash for Vec<T> {
    fn hash_into(&self, h: &mut StateHasher) {
        self.as_slice().hash_into(h);
    }
}

impl<A: StateHash, B: StateHash> StateHash for (A, B) {
    fn hash_into(&self, h: &mut StateHasher) {
        self.0.hash_into(h);
        self.1.hash_into(h);
    }
}

pub fn hash_of<T: StateHash + ?Sized>(value: &T) -> u64 {
    let mut h = StateHasher::default();
    value.hash_into(&mut h);
    h.finish()
}

/// The state hash after one tick, split so a desync can be traced to a
/// subsystem and a chunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickHash {
    /// The tick that just ran.
    pub tick: u64,
    pub total: u64,
    /// Per-subsystem hashes, in a fixed order.
    pub subsystems: Vec<(&'static str, u64)>,
    /// Per-chunk hashes, by chunk index.
    pub chunks: Vec<u64>,
}

impl TickHash {
    /// The first subsystem that differs from `other`, then the first chunk.
    pub fn first_difference(&self, other: &TickHash) -> Option<String> {
        if self == other {
            return None;
        }
        if self.tick != other.tick {
            return Some(format!("tick {} vs {}", self.tick, other.tick));
        }
        for ((name, a), (_, b)) in self.subsystems.iter().zip(&other.subsystems) {
            if a != b {
                return Some(format!("tick {}: subsystem {name}", self.tick));
            }
        }
        for (index, (a, b)) in self.chunks.iter().zip(&other.chunks).enumerate() {
            if a != b {
                return Some(format!("tick {}: chunk {index}", self.tick));
            }
        }
        Some(format!("tick {}: total", self.tick))
    }
}
