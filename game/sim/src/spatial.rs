//! Per-chunk spatial index for neighbor queries.
//!
//! Each chunk is divided into 8 × 8 cells of 4 × 4 tiles. The index is rebuilt
//! in housekeeping for chunks that changed, so during a tick it describes the
//! end of the previous tick: exactly the snapshot that self update reads.
//! Queries in the read phase widen their radius by `MAX_SPEED` and test the
//! current position, so they see this tick's movement too.

use fx::{Fx, FxVec2, FxVec3};

use crate::id::Id;
use crate::mover::{Mover, Movers};
use crate::world::{WorldConfig, CHUNK_TILES};

pub const CELL_TILES: i64 = 4;
pub const CELLS_PER_SIDE: i64 = CHUNK_TILES / CELL_TILES;
const CELLS: usize = (CELLS_PER_SIDE * CELLS_PER_SIDE) as usize;

/// A mover as of the last index rebuild.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub id: Id<Mover>,
    /// The mover's slot in its chunk's table. Valid until the next housekeeping.
    pub slot: u32,
    pub pos: FxVec3,
    pub radius: Fx,
}

#[derive(Clone, Debug)]
pub struct SpatialIndex {
    /// Bodies sorted by cell, then by slot.
    bodies: Vec<Body>,
    /// `bodies[cell_start[c]..cell_start[c + 1]]` are the bodies in cell `c`.
    cell_start: [u32; CELLS + 1],
}

impl Default for SpatialIndex {
    fn default() -> Self {
        SpatialIndex { bodies: Vec::new(), cell_start: [0; CELLS + 1] }
    }
}

impl SpatialIndex {
    /// Rebuilds from a chunk's movers with a counting sort, which keeps slot order within cells.
    pub fn rebuild(&mut self, origin: (i64, i64), movers: &Movers) {
        let cells: Vec<usize> = movers.pos.iter().map(|pos| local_cell(origin, *pos)).collect();
        let mut counts = [0u32; CELLS + 1];
        for &cell in &cells {
            counts[cell + 1] += 1;
        }
        for c in 0..CELLS {
            counts[c + 1] += counts[c];
        }
        self.cell_start = counts;
        let mut next = counts;
        let mut order = vec![0u32; movers.len()];
        for (slot, &cell) in cells.iter().enumerate() {
            order[next[cell] as usize] = slot as u32;
            next[cell] += 1;
        }
        self.bodies.clear();
        self.bodies.extend(order.iter().map(|&slot| {
            let s = slot as usize;
            Body { id: movers.id[s], slot, pos: movers.pos[s], radius: movers.radius[s] }
        }));
    }

    #[inline]
    fn cell(&self, cell: usize) -> &[Body] {
        &self.bodies[self.cell_start[cell] as usize..self.cell_start[cell + 1] as usize]
    }
}

fn local_cell(origin: (i64, i64), pos: FxVec3) -> usize {
    let x = (pos.x.floor_to_int() - origin.0).clamp(0, CHUNK_TILES - 1) / CELL_TILES;
    let y = (pos.y.floor_to_int() - origin.1).clamp(0, CHUNK_TILES - 1) / CELL_TILES;
    (y * CELLS_PER_SIDE + x) as usize
}

/// Calls `f(chunk, body)` for every body in a cell overlapping the square of
/// half-size `radius` around `center`. Callers do the exact distance test.
/// The order is fixed: by cell row, then cell column, then slot.
pub fn for_each_near(
    indices: &[SpatialIndex],
    config: &WorldConfig,
    center: FxVec2,
    radius: Fx,
    mut f: impl FnMut(usize, &Body),
) {
    let cells_x = config.chunks_x as i64 * CELLS_PER_SIDE;
    let cells_y = config.chunks_y as i64 * CELLS_PER_SIDE;
    let cell_of = |v: Fx, cells: i64| ((v.floor_to_int()).div_euclid(CELL_TILES)).clamp(0, cells - 1);
    let (x0, x1) = (cell_of(center.x - radius, cells_x), cell_of(center.x + radius, cells_x));
    let (y0, y1) = (cell_of(center.y - radius, cells_y), cell_of(center.y + radius, cells_y));
    for gy in y0..=y1 {
        for gx in x0..=x1 {
            let chunk = ((gy / CELLS_PER_SIDE) * config.chunks_x as i64 + gx / CELLS_PER_SIDE) as usize;
            let local = ((gy % CELLS_PER_SIDE) * CELLS_PER_SIDE + gx % CELLS_PER_SIDE) as usize;
            for body in indices[chunk].cell(local) {
                f(chunk, body);
            }
        }
    }
}
