//! The light grid (M8.6 of the Sacred Tree depths, L6): what the town's
//! built lanterns light, per tile and floor.
//!
//! Each built lantern (a building kind with a `light` range) lights the
//! floors of the tiles round it that it sees on its own level, out to its
//! range (6 m in the town). A tile is on the lantern's level when it has a
//! floor within a layer of the lantern's (`Volume::floor_near`: a step up or
//! down a ramp is the same level, a tunnel two meters under is not), so
//! stacked levels never share light. It sees a tile when the straight line
//! from its tile's middle to the tile's middle crosses no solid cell at the
//! lantern's head height (`HEAD`: the cells 1 to 2 m over its floor), sampled
//! every quarter tile in whole numbers, so rock, a tunnel's wall or a pillar
//! stops it. Its light falls off softly with the distance between tile
//! middles, `1 - (d / range)²`, as a byte (`FULL` at the lantern, 0 at its
//! range); where two lanterns reach, the brighter wins. All of it is integer
//! and fixed-point arithmetic over the volume's cells, so every peer gets
//! the same grid.
//!
//! **Derived, not saved.** The grid is a function of the buildings and the
//! volume, both in the world's state, saves and hash, so it is left out of
//! all three (`#[serde(skip)]`) and recomputed: on load
//! (`World::from_state`), and on each tick when its key changed, the
//! terrain's revision (a dig, a blueprint put down) or the built lanterns
//! (`LightGrid::refresh`). The save stays small, and a loaded world's grid is
//! the one it had. `LightGrid::revision` counts the recomputes that changed
//! the grid, for the view (`sight::Key::light`).
//!
//! **What it is for (M8.7).** Underground, sprites walk and work only where
//! it is lit (`sprite_lit`, in scenes it `governs`: those with a lantern
//! kind), and ghouls pay more for lantern light (`lantern`,
//! `chase::shun`). The query is
//! [`LightGrid::at`] (a tile's floor, 0..1) and [`LightGrid::lit`] (a body's
//! feet); both count ground under open sky as lit (the sun or the moon, L3),
//! and every scene without a volume too. The elder's lamp is view only (L5):
//! it isn't in the grid.

use std::collections::BTreeMap;

use fx::{Fx, FxVec2};

use super::{BuildingKind, BuildingState};
use crate::terrain::Terrain;
use crate::volume::{self, Volume};

/// A tile's light at a lantern, the most there is.
pub const FULL: u8 = 255;
/// The layers over a lantern's floor its light travels at: from 1 m (its
/// post) to 2 m (its head and a tunnel's roof). A cell solid at either on
/// the way stops it.
pub const HEAD: [i32; 2] = [2, 3];
/// Samples per tile along a ray.
const SAMPLES: i64 = 4;

/// A built lantern, as the grid reads it: its tile, its floor's layer and
/// its range in meters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Lamp {
    pub tile: (i32, i32),
    pub floor: i32,
    pub range: Fx,
}

/// The light on every lit floor: by tile and floor layer, a byte (`FULL` at
/// a lantern). Derived state (see the module doc): two grids compare by
/// their light only.
#[derive(Clone, Debug, Default)]
pub struct LightGrid {
    /// What it was computed from: the terrain's revision and the lamps.
    key: Option<(u64, Vec<Lamp>)>,
    /// Goes up by one each time a recompute changes the light.
    pub revision: u64,
    cells: BTreeMap<((i32, i32), i32), u8>,
    /// Whether the scene can light its depths at all: it has a building
    /// kind that gives light (a lantern). Only then do sprites keep to
    /// light (M8.7, L3); a volume scene with no lanterns to build (the
    /// Tunnel Lab) leaves its sprites free, as they were.
    pub governs: bool,
}

impl PartialEq for LightGrid {
    fn eq(&self, other: &LightGrid) -> bool {
        self.cells == other.cells
    }
}

impl Eq for LightGrid {}

impl LightGrid {
    /// The grid's own light on `tile`'s floor at `layer`, 0 to `FULL`: what
    /// lanterns give it, whatever the sky.
    pub fn level(&self, tile: (i32, i32), layer: i32) -> u8 {
        self.cells.get(&(tile, layer)).copied().unwrap_or(0)
    }

    /// Every lit floor and its light, by tile then layer.
    pub fn cells(&self) -> impl Iterator<Item = (((i32, i32), i32), u8)> + '_ {
        self.cells.iter().map(|(&k, &v)| (k, v))
    }

    /// How lit `tile`'s floor at `layer` is, 0 to 1, for M8.7: the
    /// lanterns' light (`level`), or 1 under open sky (nothing over the
    /// floor: the sun or the moon) and in a scene without a volume.
    pub fn at(&self, terrain: &Terrain, tile: (i32, i32), layer: i32) -> Fx {
        match &terrain.volume {
            Some(volume) if volume.ceiling(tile, layer).is_some() => Fx::from_ratio(self.level(tile, layer) as i64, FULL as i64),
            _ => Fx::ONE,
        }
    }

    /// Whether a body at `p` whose feet are at `z` stands in light: its
    /// floor's `at` is more than 0. A body on no floor (in the air) reads
    /// the layer its feet are in.
    pub fn lit(&self, terrain: &Terrain, p: FxVec2, z: Fx) -> bool {
        self.at(terrain, Terrain::pit_tile(p), feet_layer(terrain, p, z)) > Fx::ZERO
    }

    /// Whether a sprite at `p`, feet at `z`, stands where it may walk and
    /// work (M8.7, L3): lit (`lit`), or anywhere in a scene the light
    /// doesn't govern.
    pub fn sprite_lit(&self, terrain: &Terrain, p: FxVec2, z: Fx) -> bool {
        !self.governs || self.lit(terrain, p, z)
    }

    /// `at` as a sprite reads it (`sprite_lit`): whether `tile`'s floor at
    /// `layer` is one it may walk.
    pub fn sprite_at(&self, terrain: &Terrain, tile: (i32, i32), layer: i32) -> bool {
        !self.governs || self.at(terrain, tile, layer) > Fx::ZERO
    }

    /// The lanterns' own light (`level`) on the floor of a body at `p`
    /// whose feet are at `z`, whatever the sky: what a ghoul shuns (M8.7,
    /// L4). Open sky alone gives none.
    pub fn lantern(&self, terrain: &Terrain, p: FxVec2, z: Fx) -> u8 {
        if self.cells.is_empty() {
            return 0;
        }
        self.level(Terrain::pit_tile(p), feet_layer(terrain, p, z))
    }

    /// A grid lit by `lamps` over `terrain`'s volume, keyed as if no
    /// building gave light: for tests of what reads the grid, in scenes
    /// with no lantern kind. It stays until the ground changes.
    #[cfg(test)]
    pub fn of_lamps(terrain: &Terrain, lamps: &[Lamp]) -> LightGrid {
        let cells = terrain.volume.as_ref().map_or_else(BTreeMap::new, |volume| compute(volume, lamps));
        LightGrid { key: Some((terrain.revision, Vec::new())), revision: 1, cells, governs: true }
    }
}

/// The floor layer a body at `p` with its feet at `z` stands on, or the
/// layer its feet are in when on none (in the air).
fn feet_layer(terrain: &Terrain, p: FxVec2, z: Fx) -> i32 {
    terrain.volume.as_ref().and_then(|v| v.floor_at(p, z)).map_or_else(|| volume::layer_at(z), |(floor, _)| floor)
}

impl LightGrid {

    /// Recomputes the grid if the terrain's revision or the built lanterns
    /// changed since it was computed; returns whether its light changed
    /// (and its revision went up).
    pub fn refresh(&mut self, terrain: &Terrain, buildings: &[BuildingState], kinds: &[BuildingKind]) -> bool {
        self.governs = kinds.iter().any(|k| k.light.is_some());
        let lamps = lamps(terrain, buildings, kinds);
        if self.key.as_ref().is_some_and(|(revision, old)| *revision == terrain.revision && *old == lamps) {
            return false;
        }
        let cells = terrain.volume.as_ref().map_or_else(BTreeMap::new, |volume| compute(volume, &lamps));
        self.key = Some((terrain.revision, lamps));
        if cells == self.cells {
            return false;
        }
        self.cells = cells;
        self.revision += 1;
        true
    }
}

impl super::Settlement {
    /// Brings the light grid up to date with `terrain` and the buildings
    /// (`LightGrid::refresh`): the world calls it before and after every
    /// tick and on load. Returns whether the light changed.
    pub fn refresh_light(&mut self, terrain: &Terrain) -> bool {
        self.light.refresh(terrain, &self.buildings, &self.building_kinds)
    }

    /// How lit `tile`'s floor at `layer` is, 0 to 1 (`LightGrid::at`): the
    /// query M8.7's sprites and ghouls read. Ground under open sky is lit.
    pub fn light_at(&self, terrain: &Terrain, tile: (i32, i32), layer: i32) -> Fx {
        self.light.at(terrain, tile, layer)
    }

    /// Whether a body at `p` with its feet at `z` stands in light
    /// (`LightGrid::lit`).
    pub fn lit(&self, terrain: &Terrain, p: FxVec2, z: Fx) -> bool {
        self.light.lit(terrain, p, z)
    }
}

/// The built lanterns, as lamps: each on its floor (`BuildingState::height`,
/// as the volume's floor under it). None outside a volume.
pub fn lamps(terrain: &Terrain, buildings: &[BuildingState], kinds: &[BuildingKind]) -> Vec<Lamp> {
    let Some(volume) = &terrain.volume else { return Vec::new() };
    buildings
        .iter()
        .filter(|b| b.built())
        .filter_map(|b| {
            let range = kinds.get(b.kind as usize)?.light?;
            let (floor, _) = volume.floor_at(b.position, b.height)?;
            Some(Lamp { tile: Terrain::pit_tile(b.position), floor, range })
        })
        .collect()
}

/// The light `lamps` give, by tile and floor (see the module doc).
pub fn compute(volume: &Volume, lamps: &[Lamp]) -> BTreeMap<((i32, i32), i32), u8> {
    let mut cells: BTreeMap<((i32, i32), i32), u8> = BTreeMap::new();
    for lamp in lamps {
        let reach = lamp.range.ceil().floor_to_int() as i32;
        let r2 = lamp.range * lamp.range;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let d2 = Fx::from_int(dx * dx + dy * dy);
                if d2 >= r2 {
                    continue;
                }
                let tile = (lamp.tile.0 + dx, lamp.tile.1 + dy);
                let Some(floor) = volume.floor_near(tile, lamp.floor) else { continue };
                if !sees(volume, lamp, (dx, dy)) {
                    continue;
                }
                let share = (r2 - d2) / r2;
                let level = (share * Fx::from_int(FULL as i32)).floor_to_int().clamp(0, FULL as i64) as u8;
                if level == 0 {
                    continue;
                }
                let cell = cells.entry((tile, floor)).or_insert(0);
                *cell = (*cell).max(level);
            }
        }
    }
    cells
}

/// Whether `lamp` sees the tile `delta` tiles from its own: no solid cell at
/// its `HEAD` layers in any tile the line between the two tiles' middles
/// crosses, sampled `SAMPLES` times a tile, the far tile included (its own
/// is clear, or it stands in rock). Integer only: sample `i` of `n` is at
/// `delta * i / n` tiles, its tile the nearest whole one.
fn sees(volume: &Volume, lamp: &Lamp, delta: (i32, i32)) -> bool {
    let (dx, dy) = (delta.0 as i64, delta.1 as i64);
    let n = SAMPLES * dx.abs().max(dy.abs()).max(1);
    let clear = |t: (i32, i32)| HEAD.iter().all(|&h| !volume.is_solid(t, lamp.floor + h));
    (1..=n).all(|i| {
        // round(d * i / n) = floor((2 d i + n) / 2n), exact in integers.
        let step = |d: i64| (2 * d * i + n).div_euclid(2 * n) as i32;
        clear((lamp.tile.0 + step(dx), lamp.tile.1 + step(dy)))
    })
}
