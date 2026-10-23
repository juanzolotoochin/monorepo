//! The view's pile quantities, mirrored from the simulation
//! (`game/sim::settlement::piles`, which owns the rules), and the mound
//! packing used to draw them.
use std::collections::BTreeMap;
use voxel_assets::{Mineral, PileLayout, PILE_CAPACITY};

pub type PileId = u32;
#[derive(Clone, Debug)]
pub struct Pile {
    pub material: Mineral,
    pub position: [f32; 2],
    pub units: u32,
    pub source: bool,
    /// The ground under the pile.
    pub height: f32,
}
#[derive(Clone, Debug)]
pub struct Cargo {
    pub material: Option<Mineral>,
    pub units: u32,
    pub capacity: u32,
}
#[derive(Clone, Debug)]
pub struct Stock {
    pub piles: BTreeMap<PileId, Pile>,
    pub cargo: Cargo,
    pub created: u64,
}
impl Stock {
    /// The layout's starting piles, before the first simulated tick.
    pub fn new(layout: &PileLayout) -> Self {
        let mut piles = BTreeMap::new();
        for (id, d) in layout.dispensers.iter().enumerate() {
            piles.insert(id as PileId, Pile { material: d.material, position: [d.output[0], d.output[2]], units: 0, source: true, height: 0.0 });
        }
        let first = layout.dispensers.len() as PileId;
        for (i, p) in layout.piles.iter().enumerate() {
            piles.insert(first + i as PileId, Pile { material: p.material, position: [p.position[0], p.position[2]], units: p.units, source: false, height: 0.0 });
        }
        let created = layout.piles.iter().map(|p| u64::from(p.units)).sum();
        Self { piles, cargo: Cargo { material: None, units: 0, capacity: layout.capacity }, created }
    }
    pub fn total(&self) -> u64 {
        self.piles.values().map(|p| u64::from(p.units)).sum::<u64>() + u64::from(self.cargo.units)
    }
}

/// Stable, supported mound packing: every chunk has a chunk directly below it.
/// Filling a prefix grows the same mound instead of reshuffling it each transfer.
pub fn unit_positions() -> &'static Vec<[f32; 3]> {
    static POSITIONS: std::sync::OnceLock<Vec<[f32; 3]>> = std::sync::OnceLock::new();
    POSITIONS.get_or_init(|| {
        let mut cells: Vec<(i32, i32, i32)> = (0..6)
            .flat_map(|y| (-3_i32..=3).flat_map(move |x| (-3_i32..=3).map(move |z| (x, y, z))))
            .collect();
        cells.sort_by_key(|&(x, y, z)| (x.abs().max(z.abs()) + y, y, x * x + z * z, x, z));
        cells
            .into_iter()
            .take(PILE_CAPACITY as usize)
            .map(|(x, y, z)| [x as f32 * 0.28, y as f32 * 0.23 + 0.14, z as f32 * 0.28])
            .collect()
    })
}
pub fn pile_radius(units: u32) -> f32 {
    unit_positions()
        .iter()
        .take(units as usize)
        .map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt() + 0.18)
        .fold(0.35, f32::max)
}

#[cfg(test)]
mod packing_tests {
    use super::*;
    #[test]
    fn all_visual_slots_are_unique_and_supported_in_every_prefix() {
        let positions = unit_positions();
        assert_eq!(positions.len(), PILE_CAPACITY as usize);
        let mut occupied = std::collections::BTreeSet::new();
        for p in positions {
            let cell = (
                (p[0] / 0.28).round() as i32,
                ((p[1] - 0.14) / 0.23).round() as i32,
                (p[2] / 0.28).round() as i32,
            );
            if cell.1 > 0 {
                assert!(occupied.contains(&(cell.0, cell.1 - 1, cell.2)));
            }
            assert!(occupied.insert(cell));
        }
    }
}
