//! Generated mining ground: terraced hills a quarter meter per step, a flat
//! landing around the spawn, raised mesas, and ore veins in the diggable
//! layers: the dig lab's three minerals, or a town's stone ([`Veins`]).
//! A town's mine can have an authored [`Hill`] in the middle instead of the
//! landing. Integer math on a seed, so every machine generates the same
//! ground; only the [`MineSpec`] is state (see
//! [`crate::terrain::Terrain::mine`]).

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
/// Each mineral's item ID, in mineral order. A scene with piles adds them to
/// the settlement's items, after the scenario's (`Settlement::from_scene`).
pub const MINERAL_IDS: [&str; MINERALS] = ["amber_ore", "blue_crystal", "green_malachite"];
/// A stone mine's one mineral.
pub const STONE: u8 = 0;
/// The scenario item a town's stone veins yield (`Settlement::from_scene`).
pub const STONE_ID: &str = "stone";
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
    /// How many diggable layers each tile has ([`crate::terrain::Terrain::max_layers`]).
    #[serde(default = "layers")]
    pub layers: u8,
    /// What the veins hold.
    #[serde(default)]
    pub veins: Veins,
    /// The authored hill in the middle, if any, in place of the landing.
    #[serde(default)]
    pub hill: Option<Hill>,
}

/// An authored hill (the Sacred Tree's, M7 of the mining design): a square
/// plateau `steps` up from the ground around it, its sides cliffs, with
/// two ramps down from the middle of its -x and +x sides. Round its foot
/// the generated terraces stay flat for `foot` tiles, then rise at most a
/// step every three tiles, as round the landing. The plateau's top is the
/// mine's datum: [`Mine::height`] measures from it, so a town on it stands
/// at zero as on flat ground, and the ground around lies the hill's height
/// below. Nobody digs the plateau or the ramps ([`Hill::ranges`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hill {
    /// The plateau's middle tile (the tree's).
    pub center: (i32, i32),
    /// The plateau's half-width in tiles: it spans `2 * plateau + 1`.
    pub plateau: u8,
    /// How high it stands, in terrace steps ([`STEP`]).
    pub steps: u8,
    /// Each ramp's width in tiles, centered on the plateau's (an even width
    /// rounds up to odd).
    pub ramp_width: u8,
    /// Each ramp's length in tiles, out from the plateau's side. It falls
    /// evenly to the foot, so it is at least half `steps` long to stay
    /// walkable (a layer, two steps, per tile).
    pub ramp_length: u8,
    /// How far the flat ground reaches from the plateau, in tiles.
    pub foot: u8,
}

/// How many stone veins a hill guarantees at the surface
/// ([`Hill::exposed_veins`]).
pub const EXPOSED_VEINS: usize = 4;
/// How many layers deep a guaranteed vein goes.
const EXPOSED_VEIN_LAYERS: u8 = 2;

impl Hill {
    /// Why this hill can't be raised, if it can't: a plateau or ramps too
    /// small to stand on, or ramps too steep to walk.
    pub fn validate(&self) -> Result<(), String> {
        if self.plateau < 2 || self.steps == 0 || self.ramp_width < 3 || self.ramp_length == 0 {
            return Err(format!("a hill needs a plateau, a height and ramps: {self:?}"));
        }
        if self.ramp_width / 2 >= self.plateau {
            return Err(format!("a hill's ramps are wider than its plateau: {self:?}"));
        }
        if self.steps as u32 > 2 * self.ramp_length as u32 {
            return Err(format!("a hill's ramps are too steep to walk: {self:?}"));
        }
        Ok(())
    }

    /// The tile ranges nobody digs, each its first and last tile on both
    /// axes (as [`crate::terrain::Terrain::undiggable`] has them): the
    /// plateau and its two ramps.
    pub fn ranges(&self) -> [((i32, i32), (i32, i32)); 3] {
        let (x, y) = self.center;
        let (p, w, l) = (self.plateau as i32, (self.ramp_width / 2) as i32, self.ramp_length as i32);
        [((x - p, y - p), (x + p, y + p)), ((x - p - l, y - w), (x - p - 1, y + w)), ((x + p + 1, y - w), (x + p + l, y + w))]
    }

    /// The hill's steps at a tile on the plateau or a ramp. A ramp tile
    /// `k` tiles out from the plateau has `steps * (length - k) / length`,
    /// so its last tile is down at the foot.
    fn steps_at(&self, x: i32, y: i32) -> Option<u8> {
        let (dx, dy) = ((x - self.center.0).abs(), (y - self.center.1).abs());
        let (p, w, l) = (self.plateau as i32, (self.ramp_width / 2) as i32, self.ramp_length as i32);
        if dx <= p && dy <= p {
            return Some(self.steps);
        }
        let out = dx - p;
        (dy <= w && out <= l).then(|| (self.steps as i32 * (l - out) / l) as u8)
    }

    /// Whether a tile is on the plateau (not a ramp).
    pub fn on_plateau(&self, tile: (i32, i32)) -> bool {
        let p = self.plateau as i32;
        (tile.0 - self.center.0).abs() <= p && (tile.1 - self.center.1).abs() <= p
    }

    /// How far a tile is from the plateau, in whole tiles (0 on it).
    fn from_plateau(&self, x: i32, y: i32) -> i32 {
        let p = self.plateau as i32;
        let dx = ((x - self.center.0).abs() - p).max(0);
        let dy = ((y - self.center.1).abs() - p).max(0);
        ((dx * dx + dy * dy) as u32).isqrt() as i32
    }

    /// The middle tiles of the stone veins a town's hill guarantees at the
    /// surface (M5 of the mining design), from the seed: one in each
    /// quarter round it, one axis 2 to 13 tiles past the plateau's side and
    /// the other 3 to 21 past a ramp's, so each is off the plateau and
    /// beside the ramps; round the Sacred Tree's hill, 18 to 37 tiles from
    /// the tree. Each is a plus of five tiles, stone two layers down.
    pub fn exposed_veins(&self, seed: u64) -> [(i32, i32); EXPOSED_VEINS] {
        let (p, w) = (self.plateau as i32, (self.ramp_width / 2) as i32);
        std::array::from_fn(|quarter| {
            let h = hash(seed, self.center.0, self.center.1, 80 + quarter as u32);
            let (far, near) = (p + 2 + (h % 12) as i32, w + 3 + ((h >> 8) % 19) as i32);
            let (ax, ay) = if h & (1 << 16) == 0 { (far, near) } else { (near, far) };
            let (sx, sy) = [(1, 1), (-1, 1), (-1, -1), (1, -1)][quarter];
            (self.center.0 + sx * ax, self.center.1 + sy * ay)
        })
    }
}

/// What a mine's veins hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Veins {
    /// The dig lab's amber, blue and green ([`MINERAL_IDS`]).
    #[default]
    Ore,
    /// A town's stone ([`STONE`]): its pits yield the scenario's
    /// [`STONE_ID`] item.
    Stone,
}

fn layers() -> u8 {
    MAX_LAYERS
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
        let layers = spec.layers as usize;
        let mut ore = vec![0; size * size * layers];
        // A town's hill guarantees a few stone veins at the surface.
        let exposed = match (spec.veins, spec.hill) {
            (Veins::Stone, Some(hill)) => hill.exposed_veins(spec.seed).to_vec(),
            _ => Vec::new(),
        };
        for row in 0..size {
            for column in 0..size {
                let (x, y) = (spec.min.0 + column as i32, spec.min.1 + row as i32);
                let i = row * size + column;
                heights[i] = steps(spec, x, y);
                let vein_top = exposed.iter().any(|&(vx, vy)| (x - vx).abs() + (y - vy).abs() <= 1);
                for layer in 0..spec.layers {
                    let mineral = match spec.veins {
                        Veins::Ore => vein(spec.seed, x, y, layer),
                        Veins::Stone => (stone(spec.seed, x, y, layer) || (vein_top && layer < EXPOSED_VEIN_LAYERS)).then_some(STONE),
                    };
                    ore[i * layers + layer as usize] = mineral.map_or(0, |m| m + 1);
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

    /// The tile's ground height before digging: from a hill's plateau, if
    /// the mine has one ([`Hill`]), else from the lowest ground.
    pub fn height(&self, tile: (i32, i32)) -> Option<Fx> {
        let datum = self.spec.hill.map_or(0, |hill| hill.steps as i64);
        self.steps(tile).map(|s| STEP.mul_int(s as i64 - datum))
    }

    /// The mineral in a tile's `layer` (0 is the top), if any.
    pub fn ore(&self, tile: (i32, i32), layer: u8) -> Option<u8> {
        let i = self.index(tile)?;
        if layer >= self.spec.layers {
            return None;
        }
        self.ore[i * self.spec.layers as usize + layer as usize].checked_sub(1)
    }

    /// How many units digging out an ore (or stone) layer yields: 3 to 6.
    pub fn yield_of(&self, tile: (i32, i32), layer: u8) -> u32 {
        3 + hash(self.spec.seed, tile.0, tile.1, 90 + layer as u32) % 4
    }
}

/// How many units digging out a volume's solid cell yields (V.4 of the
/// verticality design): 3 to 6, as [`Mine::yield_of`] hashes them, from
/// the cell alone (a volume has no seed: seed 0).
pub fn cell_yield(tile: (i32, i32), layer: i32) -> u32 {
    3 + hash(0, tile.0, tile.1, 90u32.wrapping_add(layer as u32)) % 4
}

/// Terrace steps at a tile: two octaves of hills, mesas a meter up, and the
/// landing flat, a round clearing that ramps up a step every three tiles
/// past its edge. With a hill, the hill instead, and the flat round its
/// foot ([`Hill`]).
fn steps(spec: MineSpec, x: i32, y: i32) -> u8 {
    let seed = spec.seed;
    if let Some(steps) = spec.hill.and_then(|hill| hill.steps_at(x, y)) {
        return steps;
    }
    let hills = (noise(seed, x, y, 40, 1) * 2 + noise(seed, x, y, 14, 2)) / 3;
    let mut steps = (hills.saturating_sub(64) * 10 / 128).min(9);
    if noise(seed, x, y, 22, 3) > 178 {
        steps += 4;
    }
    let cap = match spec.hill {
        Some(hill) => ((hill.from_plateau(x, y) - hill.foot as i32).max(0) / 3) as u32,
        None => {
            let (dx, dy) = (x - spec.landing.0, y - spec.landing.1);
            let from_landing = ((dx * dx + dy * dy) as u32).isqrt() as i32;
            ((from_landing - LANDING).max(0) / 3) as u32
        }
    };
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

/// Whether a tile's layer is stone: broad veins, a few reaching the surface,
/// richer each layer down.
fn stone(seed: u64, x: i32, y: i32, layer: u8) -> bool {
    let k = layer as u32;
    noise(seed, x, y, 9, 70) > 184 - 9 * k && hash(seed, x, y, 71 + k) % 5 != 0
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
        MineSpec { seed, min: (-128, -128), size: 257, landing: (0, 2), layers: MAX_LAYERS, veins: Veins::Ore, hill: None }
    }

    /// A town's mine has stone and nothing else: some at the surface, more
    /// each layer down, and most ground plain earth.
    #[test]
    fn stone_mines_have_stone_richer_deeper() {
        let mine = Mine::generate(MineSpec { layers: 6, veins: Veins::Stone, ..spec(7) });
        let mut by_layer = [0u32; 6];
        for x in -128..128 {
            for y in -128..128 {
                for layer in 0..6 {
                    if let Some(m) = mine.ore((x, y), layer) {
                        assert_eq!(m, STONE);
                        by_layer[layer as usize] += 1;
                    }
                }
            }
        }
        let tiles = 256 * 256;
        assert!(by_layer[0] * 50 > tiles, "stone at the surface: {by_layer:?}");
        assert!(by_layer.windows(2).all(|w| w[0] < w[1]), "richer deeper: {by_layer:?}");
        assert!(by_layer[5] * 2 < tiles, "most ground is earth: {by_layer:?}");
        eprintln!("stone by layer: {by_layer:?} of {tiles}");
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
