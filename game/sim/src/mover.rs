//! `Mover`, a throwaway test entity for Milestone 0. It wanders, pushes
//! neighbors apart, falls asleep, attacks, eats from its chunk's food supply
//! and spawns children, so every tick stage has real work to do.

use fx::{Fx, FxVec3};

use crate::id::Id;
use crate::soa::soa_table;

/// Marker type for mover IDs.
pub struct Mover;

soa_table! {
    pub struct Movers / MoverRow {
        pub id: Id<Mover>,
        pub pos: FxVec3,
        pub vel: FxVec3,
        pub radius: Fx,
        pub hp: i32,
        pub hunger: i32,
        pub kills: u32,
        /// 0 while awake, otherwise the tick the mover wakes on.
        pub sleep_until: u64,
    }
}

/// Tiles per tick.
pub const MAX_SPEED: Fx = Fx::from_bits(1 << 30); // 0.25
pub const MAX_HP: i32 = 20;
/// How far beyond the two bodies' edges an attack reaches, in tiles.
pub const REACH: Fx = Fx::ONE;
pub const HUNGRY_AT: i32 = 90;

/// Body radii for size classes 1, 2 and 4.
pub const RADII: [Fx; 3] = [Fx::HALF, Fx::ONE, Fx::from_int(2)];

impl MoverRow {
    pub fn awake(&self) -> bool {
        self.sleep_until == 0
    }
}
