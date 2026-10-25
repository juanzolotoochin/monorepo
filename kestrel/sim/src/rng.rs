//! Counter-based randomness.
//!
//! An [`Rng`] is keyed on (seed, tick, entity, stream), so every entity draws
//! its own sequence each tick no matter which thread runs it or in what order.
//! Nothing about the RNG needs saving except the world seed.

use fx::Fx;

/// Stream keys, one per independent use of randomness.
pub mod stream {
    pub const CHUNK_ORDER: u64 = 1;
    pub const COMMAND: u64 = 2;
    pub const SELF_UPDATE: u64 = 3;
    pub const READ: u64 = 4;
    pub const SPAWN: u64 = 5;
}

#[inline]
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[derive(Clone, Debug)]
pub struct Rng {
    key: u64,
    counter: u64,
}

impl Rng {
    pub fn new(seed: u64, tick: u64, entity: u64, stream: u64) -> Rng {
        let key = mix(mix(mix(mix(seed) ^ tick) ^ entity) ^ stream);
        Rng { key, counter: 0 }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.counter += 1;
        mix(self.key.wrapping_add(self.counter.wrapping_mul(0x9e37_79b9_7f4a_7c15)))
    }

    /// A value in `[0, n)`. Uses a multiply instead of rejection, so it has a
    /// bias of at most n / 2^64, which is irrelevant for gameplay.
    #[inline]
    pub fn below(&mut self, n: u64) -> u64 {
        ((self.next_u64() as u128 * n as u128) >> 64) as u64
    }

    /// True with probability `numerator / denominator`.
    #[inline]
    pub fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.below(denominator) < numerator
    }

    /// A value in `[0, 1)`.
    #[inline]
    pub fn unit(&mut self) -> Fx {
        Fx::from_bits((self.next_u64() >> 32) as i64)
    }

    /// A value in `[low, high)`.
    #[inline]
    pub fn range(&mut self, low: Fx, high: Fx) -> Fx {
        low + (high - low) * self.unit()
    }

    /// Shuffles `items` in place (Fisher-Yates).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}
