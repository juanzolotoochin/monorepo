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

    /// The floor a body whose feet are at height `z` stands on in `tile`:
    /// the highest at or below `z` plus [`STEP_UP`] layers.
    pub fn floor_layer(&self, tile: (i32, i32), z: Fx) -> Option<i32> {
        let reach = layer_at(z).saturating_add(STEP_UP);
        self.floors(tile).take_while(|&f| f <= reach).last()
    }

    /// The ground under a body at `p` whose feet are at height `z`: its
    /// tile's [`Volume::floor_layer`], sloped by M6's corner rule
    /// ([`Volume::patch`]). Nothing if no floor is that low.
    pub fn floor(&self, p: FxVec2, z: Fx) -> Option<Fx> {
        let tile = Terrain::pit_tile(p);
        self.floor_layer(tile, z).map(|floor| self.patch(p, floor).height())
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

