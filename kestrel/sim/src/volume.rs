//! The volume: ground as solid and empty cells, so tunnels, caves and
//! bridges put floors over each other (V1, V2 and V4 of the verticality
//! design, `kestrel/VERTICALITY.md`).
//!
//! A cell is a 1 m × 1 m tile (as [`Terrain::pit_tile`] numbers them) by a
//! [`LAYER`]: layer `k` fills heights `k * LAYER` to `(k + 1) * LAYER`.
//! Each tile's column keeps its runs of solid cells (spans), so a tunnel
//! splits a span in two. Floors are derived: the top of a span with
//! [`HEADROOM`] empty layers above it, and always the top of the top span.
//! A floor slopes by M6's corner rule, taking each neighbour's height from
//! the floor within a layer of it in that column, so tunnel floors ramp as
//! pits do. Ore is per cell. The volume is part of the terrain's state
//! ([`Terrain::volume`]).

use fx::{Fx, FxVec2};
use serde::{Deserialize, Serialize};

use crate::hash::{StateHash, StateHasher};
use crate::terrain::{Patch, Terrain, LAYER};

/// The empty layers a floor needs above it: 2 m, a body's height.
pub const HEADROOM: i32 = 4;
/// How far above a body's feet a floor may be and still be its ground
/// ([`Volume::floor`]): a layer, the most a walk climbs per tile on sloped
/// ground ([`crate::terrain::MAX_SLOPE`]). Floors in one column are at
/// least [`HEADROOM`] plus one layers apart, so this never reaches the
/// floor above.
pub const STEP_UP: i32 = 1;
/// A body's height, feet to head: 2 m, the [`HEADROOM`] a floor needs. A
/// jump or a jetpack stops with the head at a ceiling
/// ([`Terrain::ceiling_at`]).
pub const BODY_HEIGHT: Fx = Fx::from_bits(2 << 32);

/// A run of solid cells in a column: layers `bottom` up to, not including,
/// `top`. Its top face, the floor it may make, is at `top * LAYER`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub bottom: i32,
    pub top: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Volume {
    /// Solid spans by tile, sorted, without empty columns. A column's spans
    /// are sorted, never empty and never touching (touching spans merge).
    columns: Vec<((i32, i32), Vec<Span>)>,
    /// The mineral in a solid cell, by tile and layer, sorted; plain ground
    /// has none.
    ore: Vec<(((i32, i32), i32), u8)>,
}

/// The height of layer `layer`'s bottom face (the top of the layer below).
pub fn height(layer: i32) -> Fx {
    LAYER.mul_int(layer as i64)
}

/// The layer holding height `z`.
pub fn layer_at(z: Fx) -> i32 {
    z.to_bits().div_euclid(LAYER.to_bits()).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

impl Volume {
    /// The same cells, `by` tiles over.
    pub fn shifted(&self, by: (i32, i32)) -> Volume {
        let shift = |(x, y): (i32, i32)| (x + by.0, y + by.1);
        Volume {
            columns: self.columns.iter().map(|(tile, spans)| (shift(*tile), spans.clone())).collect(),
            ore: self.ore.iter().map(|&((tile, layer), mineral)| ((shift(tile), layer), mineral)).collect(),
        }
    }

    /// Every column with solid cells, by tile.
    pub fn columns(&self) -> impl Iterator<Item = ((i32, i32), &[Span])> {
        self.columns.iter().map(|(tile, spans)| (*tile, spans.as_slice()))
    }

    /// The solid spans of `tile`'s column, bottom up.
    pub fn spans(&self, tile: (i32, i32)) -> &[Span] {
        self.columns.binary_search_by_key(&tile, |&(t, _)| t).map_or(&[], |at| &self.columns[at].1)
    }

    pub fn is_solid(&self, tile: (i32, i32), layer: i32) -> bool {
        let spans = self.spans(tile);
        let at = spans.partition_point(|s| s.top <= layer);
        spans.get(at).is_some_and(|s| s.bottom <= layer)
    }

    /// Makes a cell solid, merging it into the spans it touches. Returns
    /// whether it was empty.
    pub fn fill(&mut self, tile: (i32, i32), layer: i32) -> bool {
        let column = match self.columns.binary_search_by_key(&tile, |&(t, _)| t) {
            Ok(at) => at,
            Err(at) => {
                self.columns.insert(at, (tile, Vec::new()));
                at
            }
        };
        let spans = &mut self.columns[column].1;
        // Spans before `at` end at or below the cell.
        let at = spans.partition_point(|s| s.top <= layer);
        if spans.get(at).is_some_and(|s| s.bottom <= layer) {
            return false;
        }
        let below = at > 0 && spans[at - 1].top == layer;
        let above = spans.get(at).is_some_and(|s| s.bottom == layer + 1);
        match (below, above) {
            (true, true) => {
                spans[at - 1].top = spans[at].top;
                spans.remove(at);
            }
            (true, false) => spans[at - 1].top = layer + 1,
            (false, true) => spans[at].bottom = layer,
            (false, false) => spans.insert(at, Span { bottom: layer, top: layer + 1 }),
        }
        true
    }

    /// Makes a cell empty, splitting its span if it was inside one, and
    /// drops its ore. Returns whether it was solid.
    pub fn clear(&mut self, tile: (i32, i32), layer: i32) -> bool {
        let Ok(column) = self.columns.binary_search_by_key(&tile, |&(t, _)| t) else { return false };
        let spans = &mut self.columns[column].1;
        let at = spans.partition_point(|s| s.top <= layer);
        let Some(&span) = spans.get(at).filter(|s| s.bottom <= layer) else { return false };
        match (span.bottom == layer, span.top == layer + 1) {
            (true, true) => {
                spans.remove(at);
            }
            (true, false) => spans[at].bottom = layer + 1,
            (false, true) => spans[at].top = layer,
            (false, false) => {
                spans[at].top = layer;
                spans.insert(at + 1, Span { bottom: layer + 1, top: span.top });
            }
        }
        if spans.is_empty() {
            self.columns.remove(column);
        }
        if let Ok(at) = self.ore.binary_search_by_key(&(tile, layer), |&(cell, _)| cell) {
            self.ore.remove(at);
        }
        true
    }

    /// Fills layers `bottom` up to `top` of every tile from `min` to `max`
    /// (both included).
    pub fn fill_box(&mut self, min: (i32, i32), max: (i32, i32), bottom: i32, top: i32) {
        for x in min.0..=max.0 {
            for y in min.1..=max.1 {
                for layer in bottom..top {
                    self.fill((x, y), layer);
                }
            }
        }
    }

    /// Clears layers `bottom` up to `top` of every tile from `min` to `max`
    /// (both included).
    pub fn clear_box(&mut self, min: (i32, i32), max: (i32, i32), bottom: i32, top: i32) {
        for x in min.0..=max.0 {
            for y in min.1..=max.1 {
                for layer in bottom..top {
                    self.clear((x, y), layer);
                }
            }
        }
    }

    /// The mineral in a cell, if any (numbered as [`crate::mine`] numbers
    /// them).
    pub fn ore(&self, tile: (i32, i32), layer: i32) -> Option<u8> {
        self.ore.binary_search_by_key(&(tile, layer), |&(cell, _)| cell).ok().map(|at| self.ore[at].1)
    }

    /// Puts `mineral` in a solid cell, or takes its ore out with `None`.
    /// Returns whether the cell is solid (an empty one holds no ore).
    pub fn set_ore(&mut self, tile: (i32, i32), layer: i32, mineral: Option<u8>) -> bool {
        if !self.is_solid(tile, layer) {
            return false;
        }
        match (self.ore.binary_search_by_key(&(tile, layer), |&(cell, _)| cell), mineral) {
            (Ok(at), Some(m)) => self.ore[at].1 = m,
            (Ok(at), None) => {
                self.ore.remove(at);
            }
            (Err(at), Some(m)) => self.ore.insert(at, ((tile, layer), m)),
            (Err(_), None) => {}
        }
        true
    }

    /// The floors of `tile`'s column, bottom up, as the layer above each
    /// (its height is [`height`] of it): the tops of spans with at least
    /// [`HEADROOM`] empty layers above them, and the top of the top span.
    pub fn floors(&self, tile: (i32, i32)) -> impl Iterator<Item = i32> + '_ {
        let spans = self.spans(tile);
        spans
            .iter()
            .enumerate()
            .filter(move |&(i, s)| spans.get(i + 1).is_none_or(|next| next.bottom - s.top >= HEADROOM))
            .map(|(_, s)| s.top)
    }

    /// The top floor of `tile`'s column, if it has solid cells.
    pub fn top(&self, tile: (i32, i32)) -> Option<i32> {
        self.spans(tile).last().map(|s| s.top)
    }

    /// The ceiling over a floor (or any layer) of `tile`: the bottom of the
    /// first span above `floor`, as a layer. Nothing under open sky.
    pub fn ceiling(&self, tile: (i32, i32), floor: i32) -> Option<i32> {
        let spans = self.spans(tile);
        spans.get(spans.partition_point(|s| s.bottom < floor)).map(|s| s.bottom)
    }

    /// The empty layers between `floor` and its [`Volume::ceiling`], or
    /// nothing under open sky.
    pub fn headroom(&self, tile: (i32, i32), floor: i32) -> Option<i32> {
        self.ceiling(tile, floor).map(|ceiling| ceiling - floor)
    }

    /// The floor of `tile` within a layer of `layer`, if any. Floors in a
    /// column are more than two layers apart, so there is at most one.
    pub fn floor_near(&self, tile: (i32, i32), layer: i32) -> Option<i32> {
        self.floors(tile).find(|f| (f - layer).abs() <= 1)
    }

    /// The floor a body at `p` whose feet are at height `z` stands on, as
    /// its layer and its sloped height there ([`Volume::patch`]): the
    /// highest of `p`'s tile whose sloped height is at or below `z` plus
    /// [`STEP_UP`] layers. Nothing if no floor is that low: in a wall, or
    /// in a passage lower than [`HEADROOM`] (whose bottom is no floor).
    /// A floor's ramps down to its neighbours a layer below rise smoothly
    /// along a walk, so a walk up a ramp always reaches the next tile's
    /// floor; a floor two layers up is a wall's edge, level, a meter up,
    /// and out of reach.
    pub fn floor_at(&self, p: FxVec2, z: Fx) -> Option<(i32, Fx)> {
        let reach = z + height(STEP_UP);
        // A floor's sloped height is within a layer below it, and floors are
        // further apart than that, so the heights rise with the floors.
        self.floors(Terrain::pit_tile(p)).map(|floor| (floor, self.patch(p, floor).height())).take_while(|&(_, h)| h <= reach).last()
    }

    /// The ground under a body at `p` whose feet are at height `z`: the
    /// height of its [`Volume::floor_at`]. Nothing if no floor is that low.
    pub fn floor(&self, p: FxVec2, z: Fx) -> Option<Fx> {
        self.floor_at(p, z).map(|(_, h)| h)
    }

    /// The ceiling over a body at `p` whose feet are at `z`: the bottom of
    /// the first span over its [`Volume::floor_at`], as a height. Nothing
    /// under open sky, or without a floor.
    pub fn ceiling_over(&self, p: FxVec2, z: Fx) -> Option<Fx> {
        let (floor, _) = self.floor_at(p, z)?;
        self.ceiling(Terrain::pit_tile(p), floor).map(height)
    }

    /// The top floor at `p`, sloped: what [`Terrain::ground`] is in a volume
    /// scene. Nothing over an empty column.
    pub fn ground(&self, p: FxVec2) -> Option<Fx> {
        self.top_patch(p).map(|patch| patch.height())
    }

    /// The sloped top floor over `p`'s tile, as [`Terrain::ground`] has it.
    pub(crate) fn top_patch(&self, p: FxVec2) -> Option<Patch> {
        let floor = self.top(Terrain::pit_tile(p))?;
        Some(self.patch(p, floor))
    }

    /// The sloped floor `floor` over `p`'s tile: M6's corner rule, each
    /// neighbour's height from its [`Volume::floor_near`] this floor. A
    /// neighbour with none counts as this floor's own height, as a wall
    /// or a drop does, so the floor stays level along that edge.
    pub(crate) fn patch(&self, p: FxVec2, floor: i32) -> Patch {
        Patch::over(p, height(floor), |t| self.floor_near(t, floor).map(height))
    }
}

/// The fewest tiles a volume scene reaches from its center, and the most.
pub const LAYOUT_EXTENTS: std::ops::RangeInclusive<i32> = 8..=128;
/// The most boxes a [`VolumeLayout`] has.
pub const MAX_BOXES: usize = 256;
/// The layers a box may reach: from 16 m below the scene's zero to 32 m
/// above it.
pub const LAYOUT_LAYERS: std::ops::RangeInclusive<i32> = -32..=64;
/// The most cells all of a layout's boxes together cover (a 64 m map
/// filled 16 m deep is about 133 000).
pub const MAX_LAYOUT_CELLS: u64 = 1 << 22;

/// A scene's volume as authored (V10 of the verticality design): solid
/// and empty boxes applied in order, so a later empty box carves an
/// earlier solid one, and a later solid box fills it back or puts ore in
/// it. A scene's `volume:` section.
///
/// Units are the volume's cells, not meters: tiles on the ground (1 m,
/// tile `(x, z)` spans scene meters `x - 0.5` to `x + 0.5` and `z - 0.5` to
/// `z + 0.5`, so the scene's origin is the middle of tile `(0, 0)`) and
/// [`LAYER`]s up (0.5 m, layer `k` from `k * 0.5` to `(k + 1) * 0.5` m, so
/// a box with `top: 0` makes a floor at the scene's zero).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeLayout {
    /// Half-width of the square map, in tiles: tiles `-extent` to `extent`
    /// on both axes. Every box stays inside it.
    pub extent: i32,
    /// In order: each applies over the ones before.
    pub boxes: Vec<VolumeBox>,
}

/// One box of a [`VolumeLayout`]: the tiles from `min` to `max` (both
/// included, as `(x, z)`), layers `bottom` up to, not including, `top`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum VolumeBox {
    /// Fills its cells with `material` (stone if unset): stone takes out
    /// any ore an earlier box put there.
    Solid {
        min: [i32; 2],
        max: [i32; 2],
        bottom: i32,
        top: i32,
        #[serde(default)]
        material: VolumeMaterial,
    },
    /// Clears its cells, and their ore: a tunnel or a cave.
    Empty { min: [i32; 2], max: [i32; 2], bottom: i32, top: i32 },
}

/// What a solid cell is: plain stone, or an ore mineral of the mine's
/// (`crate::mine`'s numbering, [`VolumeMaterial::mineral`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VolumeMaterial {
    #[default]
    Stone,
    AmberOre,
    BlueCrystal,
    GreenMalachite,
}

impl VolumeMaterial {
    /// The cell's mineral as [`Volume::ore`] keeps it; none for stone.
    pub fn mineral(self) -> Option<u8> {
        match self {
            VolumeMaterial::Stone => None,
            VolumeMaterial::AmberOre => Some(crate::mine::AMBER),
            VolumeMaterial::BlueCrystal => Some(crate::mine::BLUE),
            VolumeMaterial::GreenMalachite => Some(crate::mine::GREEN),
        }
    }
}

impl VolumeBox {
    /// Its tiles' corners and its layers.
    fn bounds(&self) -> ([i32; 2], [i32; 2], i32, i32) {
        match *self {
            VolumeBox::Solid { min, max, bottom, top, .. } | VolumeBox::Empty { min, max, bottom, top } => (min, max, bottom, top),
        }
    }

    /// How many cells it covers.
    fn cells(&self) -> u64 {
        let (min, max, bottom, top) = self.bounds();
        let side = |a: i32, b: i32| (b as i64 - a as i64 + 1).max(0) as u64;
        side(min[0], max[0]) * side(min[1], max[1]) * (top as i64 - bottom as i64).max(0) as u64
    }
}

impl VolumeLayout {
    /// Refuses a layout that isn't a map: an extent out of
    /// [`LAYOUT_EXTENTS`], no boxes or more than [`MAX_BOXES`], a box
    /// inside out or empty, out of the map or out of [`LAYOUT_LAYERS`], or
    /// more than [`MAX_LAYOUT_CELLS`] in all. The error names the box.
    pub fn validate(&self) -> Result<(), String> {
        if !LAYOUT_EXTENTS.contains(&self.extent) {
            return Err(format!(
                "a volume's extent is {} tiles, but must be from {} to {}",
                self.extent,
                LAYOUT_EXTENTS.start(),
                LAYOUT_EXTENTS.end()
            ));
        }
        if self.boxes.is_empty() || self.boxes.len() > MAX_BOXES {
            return Err(format!("a volume has {} boxes, but needs 1 to {MAX_BOXES}", self.boxes.len()));
        }
        let mut cells = 0u64;
        for (i, b) in self.boxes.iter().enumerate() {
            let (min, max, bottom, top) = b.bounds();
            let at = |m: [i32; 2]| format!("({}, {})", m[0], m[1]);
            if min[0] > max[0] || min[1] > max[1] {
                return Err(format!("volume box {i}: its min {} is past its max {}", at(min), at(max)));
            }
            if [min, max].iter().flatten().any(|v| v.abs() > self.extent) {
                return Err(format!(
                    "volume box {i}: tiles {} to {} leave the map (tiles -{e} to {e})",
                    at(min),
                    at(max),
                    e = self.extent
                ));
            }
            if bottom >= top {
                return Err(format!("volume box {i}: its bottom layer {bottom} is not below its top {top}"));
            }
            if !LAYOUT_LAYERS.contains(&bottom) || !LAYOUT_LAYERS.contains(&top) {
                return Err(format!(
                    "volume box {i}: layers {bottom} to {top} leave layers {} to {} (-16 m to 32 m)",
                    LAYOUT_LAYERS.start(),
                    LAYOUT_LAYERS.end()
                ));
            }
            cells += b.cells();
        }
        if cells > MAX_LAYOUT_CELLS {
            return Err(format!("a volume's boxes cover {cells} cells, more than {MAX_LAYOUT_CELLS}"));
        }
        Ok(())
    }

    /// The volume its boxes make, in tiles from the scene's origin (as
    /// [`crate::settlement::SceneSetup::volume`] takes it).
    pub fn build(&self) -> Volume {
        let mut volume = Volume::default();
        for b in &self.boxes {
            match *b {
                VolumeBox::Solid { min, max, bottom, top, material } => {
                    for x in min[0]..=max[0] {
                        for y in min[1]..=max[1] {
                            for layer in bottom..top {
                                volume.fill((x, y), layer);
                                volume.set_ore((x, y), layer, material.mineral());
                            }
                        }
                    }
                }
                VolumeBox::Empty { min, max, bottom, top } => volume.clear_box((min[0], min[1]), (max[0], max[1]), bottom, top),
            }
        }
        volume
    }
}

impl StateHash for Volume {
    fn hash_into(&self, h: &mut StateHasher) {
        h.write_u64(self.columns.len() as u64);
        for ((x, y), spans) in &self.columns {
            h.write_u64((*x as u32 as u64) << 32 | *y as u32 as u64);
            h.write_u64(spans.len() as u64);
            for span in spans {
                h.write_u64((span.bottom as u32 as u64) << 32 | span.top as u32 as u64);
            }
        }
        h.write_u64(self.ore.len() as u64);
        for &(((x, y), layer), mineral) in &self.ore {
            h.write_u64((x as u32 as u64) << 32 | y as u32 as u64);
            h.write_u64((layer as u32 as u64) << 8 | mineral as u64);
        }
    }
}

