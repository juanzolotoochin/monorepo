//! Generated mining ground: terraced hills a quarter meter per step, a flat
//! landing around the spawn, raised mesas, and ore veins in the diggable
//! layers. Integer math on a seed, so every machine generates the same
//! ground; only the [`MineSpec`] is state (see [`crate::terrain::Terrain::mine`]).

use fx::Fx;
use serde::{Deserialize, Serialize};

use crate::terrain::MAX_LAYERS;

/// One terrace step: low enough to walk up.
pub const STEP: Fx = Fx::from_bits(1 << 30); // 0.25
/// Minerals, as the pile lab numbers them.
pub const AMBER: u8 = 0;
pub const BLUE: u8 = 1;
pub const GREEN: u8 = 2;
pub const MINERALS: usize = 3;
/// The flat landing's half-width around the spawn, in tiles.
const LANDING: i32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MineSpec {
    pub seed: u64,
    /// The first tile (as [`crate::terrain::Terrain::pit_tile`] numbers them).
    pub min: (i32, i32),
    /// Tiles per side.
    pub size: u32,
    /// The spawn's tile, at the middle of the landing.
    pub landing: (i32, i32),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Mine {
    pub spec: MineSpec,
    /// Terrace steps per tile, row by row.
    heights: Vec<u8>,
    /// Per tile and layer: 0 for plain earth, else the mineral plus one.
    ore: Vec<u8>,
}

impl Mine {
    pub fn generate(spec: MineSpec) -> Mine {
        let size = spec.size as usize;
        let mut heights = vec![0; size * size];
        let mut ore = vec![0; size * size * MAX_LAYERS as usize];
        for row in 0..size {
            for column in 0..size {
                let (x, y) = (spec.min.0 + column as i32, spec.min.1 + row as i32);
                let i = row * size + column;
                heights[i] = steps(spec, x, y);
                for layer in 0..MAX_LAYERS {
                    ore[i * MAX_LAYERS as usize + layer as usize] = vein(spec.seed, x, y, layer).map_or(0, |m| m + 1);
                }
            }
        }
        Mine { spec, heights, ore }
    }

    fn index(&self, tile: (i32, i32)) -> Option<usize> {
        let (column, row) = (tile.0 - self.spec.min.0, tile.1 - self.spec.min.1);
        let size = self.spec.size as i32;
        (column >= 0 && row >= 0 && column < size && row < size).then(|| (row * size + column) as usize)
    }

    /// The tile's terrace steps, before digging.
    pub fn steps(&self, tile: (i32, i32)) -> Option<u8> {
        self.index(tile).map(|i| self.heights[i])
    }

    /// The tile's ground height before digging.
    pub fn height(&self, tile: (i32, i32)) -> Option<Fx> {
        self.steps(tile).map(|s| STEP.mul_int(s as i64))
    }

    /// The mineral in a tile's `layer` (0 is the top), if any.
    pub fn ore(&self, tile: (i32, i32), layer: u8) -> Option<u8> {
        let i = self.index(tile)?;
        if layer >= MAX_LAYERS {
            return None;
        }
        self.ore[i * MAX_LAYERS as usize + layer as usize].checked_sub(1)
    }

    /// How many units digging out an ore layer yields: 3 to 6.
    pub fn yield_of(&self, tile: (i32, i32), layer: u8) -> u32 {
        3 + hash(self.spec.seed, tile.0, tile.1, 90 + layer as u32) % 4
    }
}

/// Terrace steps at a tile: two octaves of hills, mesas a meter up, and the
/// landing flat, a round clearing that ramps up a step every three tiles
/// past its edge.
fn steps(spec: MineSpec, x: i32, y: i32) -> u8 {
    let seed = spec.seed;
    let hills = (noise(seed, x, y, 40, 1) * 2 + noise(seed, x, y, 14, 2)) / 3;
    let mut steps = (hills.saturating_sub(64) * 10 / 128).min(9);
    if noise(seed, x, y, 22, 3) > 178 {
        steps += 4;
    }
    let (dx, dy) = (x - spec.landing.0, y - spec.landing.1);
    let from_landing = ((dx * dx + dy * dy) as u32).isqrt() as i32;
    let cap = ((from_landing - LANDING).max(0) / 3) as u32;
    steps.min(cap) as u8
}

/// The mineral in a tile's layer: blotchy veins, richer deeper down, with
/// green malachite only below the top layer.
fn vein(seed: u64, x: i32, y: i32, layer: u8) -> Option<u8> {
    let k = layer as u32;
    let veins = [(GREEN, 6, 214 - 8 * k, 1), (BLUE, 8, 206 - 8 * k, 0), (AMBER, 10, 194 - 8 * k, 0)];
    veins.into_iter().find_map(|(mineral, cell, threshold, from)| {
        let salt = 10 + mineral as u32 * 8 + k;
        let present = k >= from && noise(seed, x, y, cell, salt) > threshold && hash(seed, x, y, salt + 50) % 5 != 0;
        present.then_some(mineral)
    })
}

fn hash(seed: u64, x: i32, y: i32, salt: u32) -> u32 {
    let mut h = seed
        ^ (x as u32 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u32 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (salt as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    h as u32
}

/// Value noise in 0..=255 that varies smoothly over `cell` tiles.
fn noise(seed: u64, x: i32, y: i32, cell: i32, salt: u32) -> u32 {
    let (cx, cy) = (x.div_euclid(cell), y.div_euclid(cell));
    let (fx, fy, c) = (x.rem_euclid(cell) as u32, y.rem_euclid(cell) as u32, cell as u32);
    let v = |dx: i32, dy: i32| hash(seed, cx + dx, cy + dy, salt) & 255;
    let top = v(0, 0) * (c - fx) + v(1, 0) * fx;
    let bottom = v(0, 1) * (c - fx) + v(1, 1) * fx;
    (top * (c - fy) + bottom * fy) / (c * c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(seed: u64) -> MineSpec {
        MineSpec { seed, min: (-128, -128), size: 257, landing: (0, 2) }
    }

    #[test]
    fn the_same_seed_makes_the_same_mine() {
        assert_eq!(Mine::generate(spec(7)), Mine::generate(spec(7)));
        assert_ne!(Mine::generate(spec(7)).heights, Mine::generate(spec(8)).heights);
    }

    /// The landing is flat; the hills are mostly walkable, with some cliffs;
    /// each mineral turns up, rarer ones less often.
    #[test]
    fn mines_have_a_landing_hills_and_every_mineral() {
        let mine = Mine::generate(spec(7));
        for x in -8..=8 {
            for y in -6..=10 {
                if x * x + (y - 2) * (y - 2) <= 64 {
                    assert_eq!(mine.steps((x, y)), Some(0));
                }
            }
        }
        let (mut steep, mut edges, mut highest) = (0, 0, 0);
        let mut found = [0u32; MINERALS];
        for x in -128..128 {
            for y in -128..128 {
                let here = mine.steps((x, y)).unwrap();
                highest = highest.max(here);
                let right = mine.steps((x + 1, y)).unwrap();
                edges += 1;
                if here.abs_diff(right) > 1 {
                    steep += 1;
                }
                for layer in 0..MAX_LAYERS {
                    if let Some(m) = mine.ore((x, y), layer) {
                        assert!(layer > 0 || m != GREEN, "no green in the top layer");
                        found[m as usize] += 1;
                    }
                }
            }
        }
        assert!(highest >= 8, "hills and mesas rise ({highest} steps)");
        assert!(steep * 50 < edges, "mostly walkable: {steep} of {edges} steps are cliffs");
        assert!(steep > 0, "some cliffs need the jetpack");
        let tiles = 256 * 256 * MAX_LAYERS as u32;
        assert!(found.iter().all(|&n| n * 200 > tiles), "every mineral turns up: {found:?}");
        assert!(found[GREEN as usize] < found[AMBER as usize], "green is rarer: {found:?}");
        assert!(found.iter().sum::<u32>() * 4 < tiles, "most ground is plain: {found:?}");
        eprintln!("ore by mineral: {found:?} of {tiles}");
    }
}
